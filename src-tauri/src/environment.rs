use crate::{
    accessibility,
    config::{self, Project, Result, Settings},
    manifest, nvm,
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const ACCESS_EXECUTE: i32 = 1;
const ACCESS_WRITE: i32 = 2;
const ACCESS_READ: i32 = 4;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub project_id: String,
    pub checked_at: u64,
    pub items: Vec<CheckItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub action: Option<String>,
}

impl EnvironmentReport {
    fn add(&mut self, id: &str, label: &str, status: &str, detail: impl Into<String>) {
        self.items.push(CheckItem {
            id: id.into(),
            label: label.into(),
            status: status.into(),
            detail: detail.into(),
            action: if status == "error" {
                Some("settings".into())
            } else {
                None
            },
        });
    }
    fn path(&mut self, id: &str, label: &str, path: &Path, directory: bool) {
        let valid = path.is_absolute()
            && if directory {
                path.is_dir()
            } else {
                path.is_file()
            };
        self.add(
            id,
            label,
            if valid { "pass" } else { "error" },
            if valid {
                "路径存在且类型正确"
            } else {
                "路径未配置、不适用于当前系统、不存在或类型不符"
            },
        );
    }
}

fn fallback<'a>(project: &'a str, global: &'a str) -> &'a str {
    if project.is_empty() {
        global
    } else {
        project
    }
}

/// 只读取配置、文件和权限状态；绝不执行项目命令或触发授权。
pub fn check(project: &Project, settings: &Settings) -> Result<EnvironmentReport> {
    check_with(
        project,
        settings,
        std::env::consts::OS,
        accessibility::trusted,
    )
}

// 权限仅以查询闭包注入，此路径没有请求授权、打开设置或启动进程的能力。
fn check_with(
    project: &Project,
    settings: &Settings,
    platform: &str,
    trusted: impl FnOnce() -> bool,
) -> Result<EnvironmentReport> {
    let mut report = EnvironmentReport {
        project_id: project.id.clone(),
        checked_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间早于 Unix epoch")?
            .as_millis()
            .try_into()
            .map_err(|_| "系统时间超出毫秒范围")?,
        items: vec![],
    };
    let root = Path::new(&project.path);
    report.path("project-path", "项目目录", root, true);
    let terminal = fallback(&project.terminal, &settings.terminal);
    check_terminal(&mut report, terminal, platform);
    for (index, action) in project
        .commands
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind == "shell" && project.platforms.contains(&a.platform))
    {
        let env = fallback(&action.env_vars, &project.env_vars);
        let valid = nvm::validate_env_vars(env).is_ok();
        // 解析器错误可能含原始输入；仅返回命令名称/序号及语法结论。
        report.add(
            &format!("env-{}", index + 1),
            &format!("命令 {} 的环境变量", action.name),
            if valid { "pass" } else { "error" },
            format!(
                "命令位置 {}：{}",
                index + 1,
                if valid {
                    "环境变量语法有效（未执行命令）"
                } else {
                    "环境变量语法无效，请检查赋值、变量名、重复项及引号"
                }
            ),
        );
    }
    if project.project_type != "backend" {
        match nvm::resolve_project(project, settings) {
            Ok(resolved) => check_node(&mut report, &resolved, platform),
            Err(_) => report.add(
                "node",
                "Node 环境",
                "error",
                "Node 配置错误，请在偏好设置配置公共Node环境并检查精确版本和绝对数据目录",
            ),
        }
    }
    let uniapp = project.project_type == "uniapp";
    let app = uniapp && project.platforms.iter().any(|p| p == "app");
    let menu = uniapp
        && project
            .commands
            .iter()
            .any(|a| a.kind == "menu" && project.platforms.contains(&a.platform));
    let terminal_ax = platform == "macos"
        && ["/Ghostty.app", "/iTerm.app", "/iTerm2.app"]
            .iter()
            .any(|suffix| terminal.ends_with(suffix));
    if platform == "macos" && (terminal_ax || app || menu) {
        let granted = trusted();
        report.add("accessibility", "辅助功能权限", if granted { "pass" } else { "warning" }, if granted { "当前进程已获辅助功能权限；HBuilderX 本地菜单运行和 Ghostty/iTerm 终端操作需要该权限" } else { "未获辅助功能权限；Ghostty/iTerm 及 uniapp 本地 HBuilderX 菜单运行需要授权，Terminal 终端本身不需要；体检未请求权限" });
        if !granted {
            report.items.last_mut().unwrap().action = Some("request-accessibility".into());
        }
    }
    if uniapp && platform == "windows" {
        if app || menu {
            report.add(
                "hbuilderx",
                "HBuilderX 支持",
                "skipped",
                "Windows 尚未支持 HBuilderX 菜单运行",
            );
        }
        if app {
            report.add(
                "app-support",
                "App 构建支持",
                "skipped",
                "Windows 尚未支持 App 构建，仅检查基础目录、终端、Node 和环境变量",
            );
        }
        return Ok(report);
    }
    // 仅 HBuilderX 菜单操作及 App 构建需要 HX 工程文件；普通 shell 自行管理运行环境。
    if app || menu {
        let hx = if settings.hbuilderx_path.is_empty() {
            "/Applications/HBuilderX.app"
        } else {
            &settings.hbuilderx_path
        };
        report.path("hbuilderx", "HBuilderX 目录", Path::new(hx), true);
        report.path(
            "app-vue",
            "App.vue",
            &[root.join("App.vue"), root.join("src/App.vue")]
                .into_iter()
                .find(|p| p.is_file())
                .unwrap_or_else(|| root.join("App.vue")),
            false,
        );
        check_manifest(&mut report, project);
    }
    if app {
        for (id, label, value) in [
            ("android-app-id", "Android AppID", &project.android_app_id),
            ("ios-app-id", "iOS AppID", &project.ios_app_id),
        ] {
            let valid = !value.is_empty()
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            report.add(
                id,
                label,
                if valid { "pass" } else { "error" },
                if valid {
                    "平台 AppID 格式合法"
                } else {
                    "平台 AppID 缺少或格式无效"
                },
            );
        }
        let android = Path::new(&project.android_project_path);
        report.path("android-project", "APK 原生工程", android, true);
        report.path("gradlew", "Gradle wrapper", &android.join("gradlew"), false);
        let module_valid = !project.android_module.is_empty()
            && project
                .android_module
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if module_valid {
            let module = android.join(&project.android_module);
            report.path("android-module", "Android 模块目录", &module, true);
            let gradle = [module.join("build.gradle"), module.join("build.gradle.kts")]
                .into_iter()
                .find(|p| p.is_file())
                .unwrap_or_else(|| module.join("build.gradle"));
            report.path("module-gradle", "模块 Gradle 配置", &gradle, false);
        } else {
            report.add(
                "android-module",
                "Android 模块目录",
                "error",
                "模块名必须为单一目录名称，仅含字母、数字、下划线或连字符",
            );
        }
        check_jdk(
            &mut report,
            fallback(&project.jdk_path, &settings.jdk_path),
            std::env::var_os("JAVA_HOME").map(PathBuf::from),
            platform,
        );
        let release = root.join("unpackage/release");
        check_output(&mut report, "output-wgt", "WGT 输出目录", &release);
        let apk = if project.output_dir.is_empty() {
            release
        } else {
            root.join(&project.output_dir)
        };
        if !project.output_dir.is_empty() && foreign_relative(&project.output_dir) {
            report.add(
                "output-apk",
                "APK 输出目录",
                "error",
                "输出目录路径不适用于当前系统",
            );
        } else {
            check_output(&mut report, "output-apk", "APK 输出目录", &apk);
        }
    }
    Ok(report)
}

fn check_jdk(
    report: &mut EnvironmentReport,
    configured: &str,
    java_home: Option<PathBuf>,
    platform: &str,
) {
    // 无显式配置时只读 JAVA_HOME，不运行构建过程使用的系统查询或 Java。
    let jdk = if configured.is_empty() {
        java_home
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_default()
    } else {
        configured.into()
    };
    let (status, detail) = if jdk.as_os_str().is_empty() {
        ("warning", "未显式配置 JDK；构建时会尝试系统查询，体检未运行该查询。请在项目或偏好设置中选择 JDK，以确认版本和兼容性")
    } else if jdk.is_absolute() && jdk.join("bin/java").is_file() {
        (
            "warning",
            "JDK home/bin/java 存在；未运行 Java 或 Gradle，版本及兼容性未知",
        )
    } else if jdk.is_absolute()
        && platform == "macos"
        && jdk.join("Contents/Home/bin/java").is_file()
    {
        // 当前构建接口要求 Home 路径，检测到 bundle 不能直接报为构建可用。
        ("warning", "检测到 macOS JDK 包；请将配置的 JDK 路径设为 Contents/Home 后再构建。未运行 Java 或 Gradle，版本及兼容性未知")
    } else {
        ("error", "JDK 路径无效或 home/bin/java 缺少；请检查项目/全局 JDK 或 JAVA_HOME，体检未运行系统查询工具")
    };
    report.add("jdk", "JDK / Gradle 兼容性", status, detail);
}

fn check_terminal(report: &mut EnvironmentReport, terminal: &str, platform: &str) {
    match (platform, terminal) {
        ("macos", "" | "Terminal") => report.path(
            "terminal",
            "终端",
            Path::new("/System/Applications/Utilities/Terminal.app"),
            true,
        ),
        ("windows", "" | "WindowsTerminal" | "PowerShell") => report.add(
            "terminal",
            "终端",
            "warning",
            "使用 Windows 系统终端别名；未启动终端，无法确认其可用性及终端环境",
        ),
        (_, "Terminal" | "WindowsTerminal" | "PowerShell") => {
            report.add("terminal", "终端", "error", "所选终端不适用于当前系统")
        }
        _ => {
            let path = Path::new(terminal);
            report.path("terminal", "终端", path, platform == "macos");
        }
    }
}

fn check_node(report: &mut EnvironmentReport, project: &Project, platform: &str) {
    if project.project_type != "nodejs" {
        report.add(
            "node",
            "Node 环境",
            "warning",
            "使用终端默认环境，未检测运行中的 Node 版本",
        );
        return;
    }
    if nvm::validate_project(project).is_err() {
        report.add(
            "node",
            "Node 环境",
            "error",
            "Node manager、精确版本或对应绝对数据目录配置无效",
        );
        return;
    }
    let manager = nvm::effective_manager(project).unwrap();
    if manager == "system" {
        report.add(
            "node",
            "Node 环境",
            "warning",
            "使用终端默认环境，无法保证与应用 PATH 一致，未检测运行中的 Node 版本",
        );
        return;
    }
    let root = Path::new(if manager == "fnm" {
        &project.fnm_dir
    } else {
        &project.nvm_dir
    });
    report.path(
        &format!("{manager}-dir"),
        &format!("{manager} 安装目录"),
        root,
        true,
    );
    let installed = nvm::selected_node(project, platform == "windows").is_ok();
    report.add(
        "node",
        "Node 环境",
        if installed { "pass" } else { "error" },
        if installed {
            "所选精确版本的安装文件存在（目录版本标识）；未执行 Node，未检测二进制实际版本"
        } else {
            "所选精确 Node 版本未安装、文件缺少或不可执行；未以其他版本代替"
        },
    );
    if manager == "nvm" && platform != "windows" {
        let script = nvm::nvm_script(root).is_some_and(|path| accessible(&path, ACCESS_READ));
        report.add(
            "nvm-script",
            "nvm 激活脚本",
            if script { "pass" } else { "error" },
            if script {
                "nvm.sh 存在且可读；未加载或执行"
            } else {
                "找不到可读的 nvm.sh，无法在终端激活所选版本"
            },
        );
    }
}

fn check_manifest(report: &mut EnvironmentReport, project: &Project) {
    match config::manifest_path(project)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
    {
        None => report.add(
            "manifest",
            "manifest.json",
            "error",
            "manifest.json 缺少、不可读或路径不适用于当前系统",
        ),
        Some(text) => match manifest::info(&text) {
            Err(_) => report.add(
                "manifest",
                "manifest.json",
                "error",
                "manifest JSONC 格式或根字段无效；未回传文件内容",
            ),
            Ok(info) => {
                let valid =
                    manifest::validate_version(&info.version_name, &info.version_code).is_ok();
                report.add(
                    "manifest",
                    "manifest.json",
                    if valid { "pass" } else { "warning" },
                    if valid {
                        "manifest JSONC 格式及版本字段有效"
                    } else {
                        "manifest JSONC 可解析，但版本字段不符合构建格式"
                    },
                );
                let valid = !info.appid.is_empty()
                    && info
                        .appid
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
                report.add(
                    "manifest-app-id",
                    "manifest AppID",
                    if valid { "pass" } else { "warning" },
                    if valid {
                        "manifest AppID 格式合法"
                    } else {
                        "manifest AppID 缺少或格式无效"
                    },
                );
            }
        },
    }
}

fn foreign_relative(value: &str) -> bool {
    let path = Path::new(value);
    value.contains('\0')
        || (!path.is_absolute()
            && (crate::paths::is_absolute(value)
                || value.starts_with('\\')
                || value.as_bytes().get(1) == Some(&b':')))
}

fn check_output(report: &mut EnvironmentReport, id: &str, label: &str, path: &Path) {
    if !path.is_absolute() {
        report.add(id, label, "error", "输出目录必须能解析为当前系统绝对路径");
        return;
    }
    let mut ancestor = path;
    while !ancestor.exists() {
        let Some(parent) = ancestor.parent() else {
            break;
        };
        ancestor = parent;
    }
    if !ancestor.is_dir() || !accessible(ancestor, ACCESS_WRITE | ACCESS_EXECUTE) {
        report.add(
            id,
            label,
            "error",
            "输出目录或最近现存父目录不可访问、非目录或不可写；未创建文件探测",
        );
    } else {
        report.add(id, label, "warning", "输出目录或最近现存父目录可访问；仅静态检查，未创建文件，无法保证磁盘空间、ACL 和实际构建写入成功");
    }
}

fn accessible(path: &Path, mode: i32) -> bool {
    #[cfg(unix)]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        CString::new(path.as_os_str().as_bytes())
            .is_ok_and(|p| unsafe { libc::access(p.as_ptr(), mode) == 0 })
    }
    #[cfg(not(unix))]
    {
        let _ = mode;
        fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn unset_jdk_is_unknown_and_bundle_requires_explicit_home_configuration() {
        let mut report = EnvironmentReport {
            project_id: "example".into(),
            checked_at: 0,
            items: vec![],
        };
        check_jdk(&mut report, "", None, "macos");
        assert_eq!(status(&report, "jdk"), "warning");
        assert!(report.items[0].detail.contains("系统查询"));
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("Contents/Home");
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(home.join("bin/java"), "synthetic").unwrap();
        report.items.clear();
        check_jdk(&mut report, dir.path().to_str().unwrap(), None, "macos");
        assert!(report.items[0].detail.contains("设为 Contents/Home"));
        report.items.clear();
        check_jdk(&mut report, home.to_str().unwrap(), None, "macos");
        assert_eq!(status(&report, "jdk"), "warning");
        assert!(report.items[0].detail.contains("兼容性未知"));
    }

    fn fixture() -> (tempfile::TempDir, Project, Settings) {
        let dir = tempfile::tempdir().unwrap();
        let project = serde_json::from_value(serde_json::json!({
            "id":"fixture", "name":"Fixture", "type":"uniapp", "path":dir.path(),
            "platforms":["app"], "manifestPath":"", "androidAppId":"__UNI__ABC123",
            "iosAppId":"__UNI__DEF456", "androidProjectPath":dir.path().join("android"),
            "androidModule":"app", "jdkPath":"", "gradleTask":"", "outputDir":"",
            "syncMainVersion":false, "terminal":"Terminal", "commands":[]
        }))
        .unwrap();
        let settings = Settings {
            terminal: "Terminal".into(),
            hbuilderx_path: dir.path().to_string_lossy().into_owned(),
            jdk_path: dir
                .path()
                .join("missing-jdk")
                .to_string_lossy()
                .into_owned(),
            ..Default::default()
        };
        (dir, project, settings)
    }
    fn report(project: &Project, settings: &Settings, platform: &str) -> EnvironmentReport {
        check_with(project, settings, platform, || false).unwrap()
    }
    fn status<'a>(report: &'a EnvironmentReport, id: &str) -> &'a str {
        &report
            .items
            .iter()
            .find(|item| item.id == id)
            .unwrap()
            .status
    }
    #[test]
    fn valid_manifest_and_app_vue_are_read_without_echoing_appid() {
        let (dir, project, settings) = fixture();
        let text = r#"{ // fixture
            "appid":"PRIVATE_APP_ID", "versionName":"1.2.3", "versionCode":"123",
        }"#;
        fs::write(dir.path().join("manifest.json"), text).unwrap();
        fs::write(dir.path().join("App.vue"), "fixture").unwrap();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "manifest"), "pass");
        assert_eq!(status(&result, "app-vue"), "pass");
        assert!(!serde_json::to_string(&result)
            .unwrap()
            .contains("PRIVATE_APP_ID"));
        assert_eq!(
            fs::read_to_string(dir.path().join("manifest.json")).unwrap(),
            text
        );
        fs::write(dir.path().join("manifest.json"), "{invalid}").unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "manifest"),
            "error"
        );
    }

    #[test]
    fn global_terminal_is_used_only_without_project_override_and_output_checks_parent() {
        let (dir, mut project, mut settings) = fixture();
        project.terminal.clear();
        project.project_type = "backend".into();
        settings.terminal = dir
            .path()
            .join("Terminal.app")
            .to_string_lossy()
            .into_owned();
        fs::create_dir(&settings.terminal).unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "terminal"),
            "pass"
        );
        project.terminal = dir
            .path()
            .join("missing.app")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "terminal"),
            "error"
        );
        project.project_type = "uniapp".into();
        project.output_dir = "blocked/child".into();
        fs::write(dir.path().join("blocked"), "not a directory").unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "output-apk"),
            "error"
        );
        assert!(!dir.path().join("unpackage").exists());
    }

    #[cfg(unix)]
    #[test]
    fn exact_node_file_is_checked_without_execution() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut project, settings) = fixture();
        project.project_type = "nodejs".into();
        project.node_version = "v22.1.0".into();
        project.nvm_dir = dir.path().to_string_lossy().into_owned();
        let node = dir.path().join("versions/node/v22.1.0/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, "this is not an executable format; must never run").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(dir.path().join("nvm.sh"), "must never source").unwrap();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "node"), "pass");
        assert_eq!(status(&result, "nvm-script"), "pass");
        fs::set_permissions(&node, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
    }

    #[cfg(unix)]
    #[test]
    fn fnm_diagnostics_are_read_only_and_do_not_require_nvm_script() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut project, settings) = fixture();
        project.project_type = "nodejs".into();
        project.node_manager = "fnm".into();
        project.node_version = "v22.1.0".into();
        project.fnm_dir = dir.path().to_string_lossy().into();
        let node = dir
            .path()
            .join("node-versions/v22.1.0/installation/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, "must never execute").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        let before = serde_json::to_value(&project).unwrap();
        let result = check_with(&project, &settings, "macos", || panic!("无需请求权限")).unwrap();
        assert_eq!(status(&result, "node"), "pass");
        assert_eq!(status(&result, "fnm-dir"), "pass");
        assert!(!result.items.iter().any(|item| item.id == "nvm-script"));
        assert_eq!(fs::read_to_string(&node).unwrap(), "must never execute");
        assert_eq!(serde_json::to_value(&project).unwrap(), before);
        fs::set_permissions(&node, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
        project.node_manager = "system".into();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
        project.node_version.clear();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "warning"
        );
        project.node_manager = "unknown".into();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
    }

    #[cfg(unix)]
    #[test]
    fn shared_node_diagnostics_resolve_provider_read_only_and_report_configuration_errors() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, mut project, mut settings) = fixture();
        project.project_type = "nodejs".into();
        project.node_manager = "inherit".into();
        project.node_version = "v22.1.0".into();
        let before = serde_json::to_value(&project).unwrap();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "node"), "error");
        assert!(result
            .items
            .iter()
            .find(|item| item.id == "node")
            .unwrap()
            .detail
            .contains("偏好设置"));
        settings.node_manager = "fnm".into();
        settings.fnm_dir = dir.path().to_string_lossy().into();
        let node = dir
            .path()
            .join("node-versions/v22.1.0/installation/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, "must never execute shared Node").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "node"), "pass");
        assert_eq!(status(&result, "fnm-dir"), "pass");
        assert_eq!(serde_json::to_value(&project).unwrap(), before);
        settings.node_manager = "nvm".into();
        settings.nvm_dir = dir.path().to_string_lossy().into();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
        settings.node_manager = "system".into();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "warning"
        );
        settings.node_manager = "fnm".into();
        project.node_version.clear();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "node"), "warning");
        assert!(!result.items.iter().any(|item| item.id == "fnm-dir"));
        assert_eq!(
            fs::read_to_string(node).unwrap(),
            "must never execute shared Node"
        );
    }

    #[test]
    fn shell_only_uniapp_does_not_require_hbuilder_or_app_vue() {
        let (_dir, mut project, mut settings) = fixture();
        settings.hbuilderx_path = "/missing-fixture/HBuilderX.app".into();
        project.commands = serde_json::from_value(serde_json::json!([
            {"id":"dev", "name":"Dev", "platform":"web", "kind":"shell", "menuPath":[], "command":"never execute"}
        ])).unwrap();
        for platform in ["web", "miniapp"] {
            project.platforms = vec![platform.into()];
            project.commands[0].platform = platform.into();
            let result = report(&project, &settings, "macos");
            for id in ["hbuilderx", "app-vue", "manifest"] {
                assert!(
                    !result.items.iter().any(|item| item.id == id),
                    "{platform}/{id}"
                );
            }
        }
        project.commands[0].kind = "menu".into();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "hbuilderx"), "error");
        assert_eq!(status(&result, "app-vue"), "error");
    }

    #[test]
    fn jdk_accepts_macos_bundle_root_and_direct_home() {
        let (dir, mut project, settings) = fixture();
        let bundle = dir.path().join("fixture.jdk");
        let home = bundle.join("Contents/Home");
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(home.join("bin/java"), "must never execute").unwrap();
        for path in [&bundle, &home] {
            project.jdk_path = path.to_string_lossy().into_owned();
            assert_eq!(
                status(&report(&project, &settings, "macos"), "jdk"),
                "warning"
            );
        }
        fs::remove_file(home.join("bin/java")).unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "jdk"),
            "error"
        );
    }

    #[test]
    fn missing_manifest_wrapper_and_java_are_independent() {
        let (_dir, project, settings) = fixture();
        let result = report(&project, &settings, "macos");
        for id in ["manifest", "gradlew", "jdk"] {
            assert_eq!(status(&result, id), "error", "{id}");
        }
    }
    #[test]
    fn disabled_app_and_backend_do_not_require_build_configuration() {
        let (_dir, mut project, settings) = fixture();
        for kind in ["uniapp", "nodejs", "backend"] {
            project.project_type = kind.into();
            project.platforms = vec!["web".into()];
            let result = report(&project, &settings, "macos");
            for id in [
                "gradlew",
                "jdk",
                "android-app-id",
                "ios-app-id",
                "output-apk",
            ] {
                assert!(
                    !result.items.iter().any(|item| item.id == id),
                    "{kind}/{id}"
                );
            }
            if kind == "backend" {
                assert!(!result
                    .items
                    .iter()
                    .any(|item| item.id == "manifest" || item.id == "node"));
            }
        }
    }
    #[test]
    fn windows_skips_app_but_keeps_basic_checks() {
        let (_dir, mut project, settings) = fixture();
        project.terminal = "PowerShell".into();
        let result = report(&project, &settings, "windows");
        assert_eq!(status(&result, "app-support"), "skipped");
        assert!(result.items.iter().any(|item| item.id == "project-path"));
        assert!(result.items.iter().any(|item| item.id == "terminal"));
        assert!(!result.items.iter().any(|item| item.id == "gradlew"));
    }
    #[test]
    fn env_errors_never_echo_values_or_commands() {
        let (_dir, mut project, settings) = fixture();
        project.commands = serde_json::from_value(serde_json::json!([
            {"id":"dev", "name":"Development", "platform":"app", "kind":"shell", "menuPath":[], "command":"secret-command", "envVars":"SECRET_TOKEN_WITHOUT_EQUALS"}
        ])).unwrap();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "env-1"), "error");
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("SECRET_TOKEN"));
        assert!(!json.contains("secret-command"));
        assert!(!json.contains("__UNI__ABC123"));
        assert!(json.contains("projectId"));
        assert!(json.contains("checkedAt"));
    }
    #[test]
    fn selected_node_requires_exact_installed_version() {
        let (dir, mut project, settings) = fixture();
        project.project_type = "nodejs".into();
        project.node_version = "v22.1.0".into();
        project.nvm_dir = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
        let node = dir.path().join("versions/node/v22.1.1/bin/node");
        std::fs::create_dir_all(node.parent().unwrap()).unwrap();
        std::fs::write(node, "must never execute").unwrap();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "node"),
            "error"
        );
        project.node_version.clear();
        let result = report(&project, &settings, "macos");
        assert_eq!(status(&result, "node"), "warning");
        assert!(result
            .items
            .iter()
            .find(|i| i.id == "node")
            .unwrap()
            .detail
            .contains("PATH"));
    }
    #[test]
    fn permission_query_is_read_only_and_terminal_is_overridable() {
        let (_dir, mut project, mut settings) = fixture();
        project.project_type = "backend".into();
        project.platforms.clear();
        project.terminal = "/Applications/Ghostty.app".into();
        let queried = Cell::new(0);
        let result = check_with(&project, &settings, "macos", || {
            queried.set(queried.get() + 1);
            false
        })
        .unwrap();
        assert_eq!(queried.get(), 1);
        assert_eq!(status(&result, "accessibility"), "warning");
        assert_eq!(
            result
                .items
                .iter()
                .find(|i| i.id == "accessibility")
                .unwrap()
                .action
                .as_deref(),
            Some("request-accessibility")
        );
        settings.terminal = "/Applications/iTerm.app".into();
        project.terminal = "Terminal".into();
        let result =
            check_with(&project, &settings, "macos", || panic!("Terminal 不需 AX")).unwrap();
        assert!(!result.items.iter().any(|i| i.id == "accessibility"));
    }
    #[test]
    fn project_jdk_overrides_global_and_existing_output_is_not_modified() {
        let (dir, mut project, mut settings) = fixture();
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        std::fs::write(jdk.join("bin/java"), "must never execute").unwrap();
        settings.jdk_path = jdk.to_string_lossy().into_owned();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "jdk"),
            "warning"
        );
        project.jdk_path = dir
            .path()
            .join("missing-jdk")
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            status(&report(&project, &settings, "macos"), "jdk"),
            "error"
        );
        assert!(!dir.path().join("unpackage").exists());
    }
}
