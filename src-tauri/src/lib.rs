mod accessibility;
mod ai;
mod config;
mod git;
mod hbuilder_owner;
mod manifest;
mod nvm;
mod paths;
mod system_tools;
mod tasks;
mod tray;
mod tray_geometry;
#[cfg(target_os = "macos")]
mod tray_panel;
mod uniapp_scripts;
#[cfg(windows)]
mod windows_job;
#[cfg(any(windows, test))]
mod windows_terminal;
use config::{Action, AppConfig, Project, Result, Settings};
use serde::Serialize;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
use std::{fs, path::Path, process::Command, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};
#[derive(Default)]
struct StartupLogs(Mutex<Vec<String>>);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Tool {
    name: String,
    path: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Tools {
    hbuilderx_path: String,
    terminals: Vec<Tool>,
    jdks: Vec<Tool>,
}
#[tauri::command]
fn load_config(app: AppHandle) -> Result<AppConfig> {
    tasks::load(&app)
}
#[tauri::command]
fn save_config(app: AppHandle, config: AppConfig) -> Result<()> {
    let text = config::serialize(&config)?;
    config::atomic_write(&tasks::config_path(&app)?, &text)?;
    // 文件已保存，通知失败不能将成功写入误报为保存失败。
    let _ = app.emit("config-updated", ());
    Ok(())
}
#[tauri::command]
fn read_manifest(project: Project) -> Result<manifest::ManifestInfo> {
    manifest::info(
        &fs::read_to_string(config::manifest_path(&project)?).map_err(|e| e.to_string())?,
    )
}
#[derive(Serialize)]
struct PackageScript {
    name: String,
    command: String,
}
fn package_scripts(root: &Path) -> Result<Vec<PackageScript>> {
    let text = fs::read_to_string(root.join("package.json"))
        .map_err(|e| format!("无法读取 package.json: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Some(scripts) = value.get("scripts") else {
        return Ok(vec![]);
    };
    let scripts = scripts
        .as_object()
        .ok_or("package.json scripts 必须为对象")?;
    let manager = if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else {
        "npm"
    };
    let mut result = vec![];
    for (name, source) in scripts {
        if !source.is_string() || name.is_empty() || name.contains('\0') {
            return Err("package.json scripts 名称或命令无效".into());
        }
        // 脚本名作为 shell 数据安全引用，发现脚本时绝不执行内容。
        let quoted = if cfg!(windows) {
            format!("'{}'", name.replace('\'', "''"))
        } else {
            format!("'{}'", name.replace('\'', "'\\''"))
        };
        // Windows 使用工具官方 .cmd 入口，不依赖 PowerShell 脚本执行策略。
        let manager = if cfg!(windows) {
            format!("{manager}.cmd")
        } else {
            manager.to_owned()
        };
        result.push(PackageScript {
            name: name.clone(),
            command: format!("{manager} run {quoted}"),
        });
    }
    Ok(result)
}
#[tauri::command]
fn read_package_scripts(project: Project) -> Result<Vec<PackageScript>> {
    package_scripts(Path::new(&project.path))
}
#[tauri::command]
fn read_uniapp_scripts(project: Project) -> Result<Vec<uniapp_scripts::UniappScript>> {
    uniapp_scripts::read(Path::new(&project.path))
}
#[tauri::command]
async fn read_git_branch(path: String) -> Result<String> {
    tauri::async_runtime::spawn_blocking(move || git::read_branch(Path::new(&path)))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn read_backup(path: String) -> Result<AppConfig> {
    config::parse(&fs::read_to_string(path).map_err(|e| e.to_string())?)
}
#[tauri::command]
fn write_backup(path: String, config: AppConfig) -> Result<()> {
    let text = config::serialize(&config)?;
    if !Path::new(&path).is_absolute() {
        return Err("备份需使用绝对路径".into());
    }
    config::atomic_write(Path::new(&path), &text)
}
fn path_open_command(path: &Path) -> Result<Command> {
    let resolved = fs::canonicalize(path)
        .map_err(|e| format!("路径不存在或无法访问（{}）: {e}", path.display()))?;
    // 产物文件只在文件管理器中选中，目录正常打开，不启动产物的关联应用。
    system_tools::open_command(&resolved, resolved.is_file(), std::env::consts::OS)
}
#[tauri::command]
async fn open_path(path: String) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut command = path_open_command(Path::new(&path))?;
        #[cfg(windows)]
        {
            // Explorer 可能复用已有进程，启动成功后无需等待其退出码。
            command
                .spawn()
                .map_err(|e| format!("无法打开资源管理器: {e}"))?;
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let output = command.output().map_err(|e| e.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(format!(
                    "打开目录或定位文件失败: {} {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn detect_node_versions() -> Vec<nvm::NodeVersion> {
    nvm::detect()
}
#[tauri::command]
fn detect_tools() -> Result<Tools> {
    #[cfg(windows)]
    {
        Ok(system_tools::windows_tools())
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos_tools()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("当前系统暂不支持自动检测开发工具".into())
    }
}
#[cfg(target_os = "macos")]
fn detect_macos_tools() -> Result<Tools> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut terminals = vec![];
    for (name, path) in [
        ("Terminal", "/System/Applications/Utilities/Terminal.app"),
        ("Ghostty", "/Applications/Ghostty.app"),
        ("iTerm", "/Applications/iTerm.app"),
    ] {
        if Path::new(path).is_dir() {
            terminals.push(Tool {
                name: name.into(),
                path: path.into(),
            })
        }
    }
    for name in ["Ghostty", "iTerm"] {
        let p = Path::new(&home).join(format!("Applications/{name}.app"));
        if p.is_dir() {
            terminals.push(Tool {
                name: name.into(),
                path: p.to_string_lossy().into(),
            })
        }
    }
    let mut jdks = vec![];
    for dir in [
        PathBuf::from("/Library/Java/JavaVirtualMachines"),
        Path::new(&home).join("Library/Java/JavaVirtualMachines"),
    ] {
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path().join("Contents/Home");
                if p.join("bin/java").exists() {
                    jdks.push(Tool {
                        name: e.file_name().to_string_lossy().into(),
                        path: p.to_string_lossy().into(),
                    })
                }
            }
        }
    }
    let hbuilderx_path = [
        PathBuf::from("/Applications/HBuilderX.app"),
        Path::new(&home).join("Applications/HBuilderX.app"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .map(|p| p.to_string_lossy().into())
    .unwrap_or_default();
    Ok(Tools {
        hbuilderx_path,
        terminals,
        jdks,
    })
}
#[tauri::command]
fn start_build(
    app: AppHandle,
    project: Project,
    mode: String,
    version_name: String,
    version_code: String,
) -> Result<String> {
    tasks::build(app, project, mode, version_name, version_code)
}
#[tauri::command]
fn run_action(
    app: AppHandle,
    project: Project,
    action: Action,
    settings: Settings,
) -> Result<String> {
    // 不把环境变量和激活前缀写进标题；过滤控制字符，避免干扰终端标题序列。
    let title = format!("{} · {}", project.name, action.command.trim())
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    tasks::action_with_title(app, project, action, settings, Some(title))
}
#[tauri::command]
fn open_project_terminal(
    app: AppHandle,
    project: Project,
    tool_id: Option<String>,
) -> Result<String> {
    // 从持久化配置选择工具；未选择工具时仅打开项目终端。
    let settings = tasks::load(&app)?.settings;
    let (project, action) = ai::prepare_action(&project, &settings, tool_id.as_deref())?;
    tasks::action(app, project, action, settings)
}
#[tauri::command]
fn open_project_command(app: AppHandle, project: Project) -> Result<String> {
    // 仅使用快照中的 ID，偏好和项目路径均以当前持久化配置为准。
    let config = tasks::load(&app)?;
    let (project, action) = ai::prepare_saved_action(&config, &project.id)?;
    tasks::action(app, project, action, config.settings)
}
#[tauri::command]
fn cancel_task(app: AppHandle, task_id: String) -> Result<()> {
    tasks::cancel(&app, &task_id)
}
#[tauri::command]
fn recover_pending(app: AppHandle) -> Result<Vec<String>> {
    let mut logs = tasks::recover(&app)?;
    logs.splice(0..0, app.state::<StartupLogs>().0.lock().unwrap().drain(..));
    Ok(logs)
}
#[tauri::command]
async fn list_menus(app: AppHandle, hbuilderx_path: String) -> Result<Vec<Vec<String>>> {
    tasks::mac_only()?;
    if !hbuilderx_path.is_empty() && !Path::new(&hbuilderx_path).is_dir() {
        return Err("HBuilderX 路径不存在".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut permission_prompt = accessibility::PermissionPrompt::default();
        permission_prompt.ensure()?;
        let runtime = app.state::<tasks::Runtime>();
        if runtime.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return Err("HBuilderX 正忙".into());
        }
        let result = (|| {
            let script = tasks::scripts(&app)?.join("hbuilderx.applescript");
            let mut child = Command::new("/usr/bin/osascript")
                .arg(script)
                .arg("list")
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            // 并发读取输出管道，避免菜单树过大时填满缓冲区而阻塞。
            let stdout = child.stdout.take().unwrap();
            let stderr = child.stderr.take().unwrap();
            let reader = std::thread::spawn(move || {
                use std::io::Read;
                let mut s = String::new();
                let _ = std::io::BufReader::new(stdout).read_to_string(&mut s);
                s
            });
            let errors = std::thread::spawn(move || {
                use std::io::Read;
                let mut s = String::new();
                let _ = std::io::BufReader::new(stderr).read_to_string(&mut s);
                s
            });
            let start = std::time::Instant::now();
            let status = loop {
                if start.elapsed().as_secs() > 20 {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    let _ = errors.join();
                    return Err("读取菜单超时，请检查辅助功能权限".into());
                }
                if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
                    break s;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            };
            let out = reader.join().map_err(|_| "读取菜单失败")?;
            let err = errors.join().map_err(|_| "读取菜单错误失败")?;
            if !status.success() {
                if accessibility::is_denied(&err) {
                    return Err(permission_prompt.denied());
                }
                return Err(err);
            }
            Ok(out
                .lines()
                .filter(|s| !s.is_empty())
                .map(|line| line.split('\u{1f}').map(str::to_owned).collect())
                .collect())
        })();
        runtime
            .busy
            .store(false, std::sync::atomic::Ordering::SeqCst);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg(test)]
mod package_tests {
    use super::*;
    #[test]
    fn ai_tools_are_exported_and_imported_with_commands_and_enabled_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("ai-backup.json")
            .to_str()
            .unwrap()
            .to_owned();
        let mut config = AppConfig::default();
        config.settings.ai_tools = vec![
            config::AiTool {
                id: "custom".into(),
                name: "自定义助手".into(),
                command: "custom-ai --profile 'local work'".into(),
                enabled: true,
            },
            config::AiTool {
                id: "disabled".into(),
                name: "停用助手".into(),
                command: "other-ai".into(),
                enabled: false,
            },
        ];
        write_backup(path.clone(), config.clone()).unwrap();
        let exported: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            exported["settings"]["aiTools"],
            serde_json::to_value(&config.settings.ai_tools).unwrap()
        );
        let restored = read_backup(path).unwrap();
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(config).unwrap()
        );
    }
    #[test]
    fn legacy_backup_roundtrip_merges_platforms_and_preserves_every_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup.json").to_str().unwrap().to_owned();
        let legacy = serde_json::json!({"schemaVersion":1,"projects":[{
            "id":"uni","name":"旧项目","type":"uniapp","path":"/tmp/backup-platform",
            "platforms":["weixin","dingtalk"],"manifestPath":"","androidAppId":"","iosAppId":"",
            "androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"",
            "syncMainVersion":false,"terminal":"Terminal","commands":[
                {"id":"wx","name":"微信原名","platform":"weixin","kind":"menu","menuPath":["运行","微信/开发"],"command":"kept"},
                {"id":"ding","name":"钉钉原名","platform":"dingtalk","kind":"menu","menuPath":["发行","自定义发行","钉钉生产"],"command":""}
            ]}],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        fs::write(&path, legacy.to_string()).unwrap();
        let imported = read_backup(path.clone()).unwrap();
        assert_eq!(imported.projects[0].platforms, ["miniapp"]);
        // 直接传入旧对象，覆盖绕过 parse 的保存/导出调用。
        write_backup(
            path.clone(),
            serde_json::from_value(legacy.clone()).unwrap(),
        )
        .unwrap();
        let exported: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(exported["schemaVersion"], 1);
        assert_eq!(
            exported["projects"][0]["platforms"],
            serde_json::json!(["miniapp"])
        );
        let mut expected = legacy["projects"][0]["commands"].clone();
        for action in expected.as_array_mut().unwrap() {
            action["platform"] = "miniapp".into();
            action["envVars"] = "".into();
        }
        assert_eq!(exported["projects"][0]["commands"], expected);
        assert_eq!(
            serde_json::to_value(read_backup(path.clone()).unwrap()).unwrap(),
            exported
        );
        let before = fs::read(&path).unwrap();
        let mut invalid = imported;
        invalid.projects[0].platforms = vec!["miniapp".into(), "miniapp".into()];
        assert!(write_backup(path.clone(), invalid).is_err());
        assert_eq!(fs::read(path).unwrap(), before);
    }

    #[test]
    fn node_backup_import_defaults_and_selection_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("node-backup.json")
            .to_str()
            .unwrap()
            .to_owned();
        let legacy = serde_json::json!({"schemaVersion":1,"projects":[{"id":"node","name":"Node","type":"nodejs","path":"/tmp/node-project","platforms":["pc"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]}],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        fs::write(&path, legacy.to_string()).unwrap();
        let mut config = read_backup(path.clone()).unwrap();
        assert!(config.projects[0].node_version.is_empty());
        assert!(config.projects[0].nvm_dir.is_empty());
        assert!(config.settings.default_ai_tool_id.is_empty());
        assert!(config.projects[0].ai_tool_id.is_empty());
        // 备份保留已删除或停用工具的引用，不强制其存在。
        config.settings.default_ai_tool_id = "removed-global".into();
        config.projects[0].ai_tool_id = "removed-project".into();
        config.projects[0].node_version = "v22.10.0".into();
        config.projects[0].nvm_dir = "/portable backup/nvm's data".into();
        write_backup(path.clone(), config).unwrap();
        let imported = read_backup(path.clone()).unwrap();
        assert_eq!(imported.settings.default_ai_tool_id, "removed-global");
        assert_eq!(imported.projects[0].ai_tool_id, "removed-project");
        assert_eq!(imported.projects[0].node_version, "v22.10.0");
        assert_eq!(imported.projects[0].nvm_dir, "/portable backup/nvm's data");
        let original = fs::read_to_string(&path).unwrap();
        let mut invalid = imported;
        invalid.projects[0].node_version = "default".into();
        assert!(write_backup(path.clone(), invalid).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
    }
    #[test]
    fn theme_backup_roundtrip_and_legacy_import() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup.json").to_str().unwrap().to_owned();
        let legacy = r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}}"#;
        fs::write(&path, legacy).unwrap();
        let mut config = read_backup(path.clone()).unwrap();
        assert_eq!(config.settings.theme, "lavender");
        assert!(config.settings.open_output_after_build);
        for (theme, enabled) in [
            ("lavender", true),
            ("blue", false),
            ("cyan", true),
            ("rose", false),
            ("amber", true),
            ("slate", false),
        ] {
            config.settings.theme = theme.into();
            config.settings.open_output_after_build = enabled;
            write_backup(path.clone(), config.clone()).unwrap();
            assert_eq!(read_backup(path.clone()).unwrap().settings.theme, theme);
            let value: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(value["settings"]["theme"], theme);
            assert_eq!(value["settings"]["openOutputAfterBuild"], enabled);
            assert_eq!(
                read_backup(path.clone())
                    .unwrap()
                    .settings
                    .open_output_after_build,
                enabled
            );
            assert_eq!(value["schemaVersion"], 1);
        }
        let original = fs::read_to_string(&path).unwrap();
        config.settings.theme = "dark".into();
        assert!(write_backup(path.clone(), config.clone())
            .unwrap_err()
            .contains("theme"));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::write(&path, serde_json::to_string(&config).unwrap()).unwrap();
        assert!(read_backup(path).unwrap_err().contains("theme"));
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn artifacts_are_revealed_in_finder_instead_of_launched() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["3.8.8.wgt", "3.8.8-ios.wgt", "门店 App.apk"] {
            let path = dir.path().join(name);
            fs::write(&path, "test artifact").unwrap();
            let command = path_open_command(&path).unwrap();
            let args: Vec<_> = command.get_args().collect();
            assert_eq!(command.get_program(), "/usr/bin/open");
            assert_eq!(
                args,
                vec![
                    std::ffi::OsStr::new("-R"),
                    fs::canonicalize(&path).unwrap().as_os_str()
                ]
            );
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn project_directories_are_opened_normally() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("项目目录 with spaces");
        fs::create_dir(&path).unwrap();
        let command = path_open_command(&path).unwrap();
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![fs::canonicalize(path).unwrap().as_os_str()]
        );
    }
    #[test]
    fn missing_path_is_rejected_before_launching_finder() {
        let dir = tempfile::tempdir().unwrap();
        assert!(path_open_command(&dir.path().join("missing.wgt")).is_err());
    }
    #[test]
    fn scripts_are_suggestions_and_script_names_are_quoted() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"vite build","odd' name":"echo authorized"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();
        let scripts = package_scripts(dir.path()).unwrap();
        if cfg!(windows) {
            assert_eq!(scripts[0].command, "pnpm.cmd run 'build'");
            assert_eq!(scripts[1].command, "pnpm.cmd run 'odd'' name'");
        } else {
            assert_eq!(scripts[0].command, "pnpm run 'build'");
            assert_eq!(scripts[1].command, "pnpm run 'odd'\\'' name'");
        }
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":false}}"#,
        )
        .unwrap();
        assert!(package_scripts(dir.path()).is_err());
    }
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(tasks::Runtime::default())
        .manage(StartupLogs::default())
        .setup(|app| {
            // 崩溃时系统自动释放锁，防止另一个应用实例误恢复正在构建的 AppID。
            let dir = app.path().app_data_dir()?;
            fs::create_dir_all(&dir)?;
            let lock = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(dir.join("instance.lock"))?;
            lock.try_lock()
                .map_err(|e| std::io::Error::other(format!("DevDock 已在运行: {e}")))?;
            let old_dir = dir.with_file_name("com.aidev.desktop");
            let legacy_lock =
                config::migrate_legacy(&old_dir, &dir).map_err(std::io::Error::other)?;
            // 同时持有新旧实例锁，迁移后的恢复与构建仍受跨版本互斥保护。
            app.manage((lock, legacy_lock));
            let logs =
                tasks::recover(app.handle()).unwrap_or_else(|e| vec![format!("启动恢复失败: {e}")]);
            *app.state::<StartupLogs>().0.lock().unwrap() = logs;
            // tray 初始化失败明确终止启动，避免主窗口隐藏后应用失联。
            tray::init(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            read_manifest,
            read_package_scripts,
            read_uniapp_scripts,
            read_git_branch,
            detect_tools,
            detect_node_versions,
            read_backup,
            write_backup,
            open_path,
            start_build,
            run_action,
            open_project_terminal,
            open_project_command,
            cancel_task,
            list_menus,
            recover_pending,
            tray::show_main_window,
            tray::hide_quick_panel
        ])
        .build(tauri::generate_context!())
        .expect("无法初始化 DevDock");
    app.run(|handle, event| match event {
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            // 点击程序坞图标时恢复被关闭隐藏的主窗口，不依赖快捷弹框的可见状态。
            if !handle
                .state::<tasks::Runtime>()
                .shutting_down
                .load(std::sync::atomic::Ordering::SeqCst)
            {
                if let Err(error) = tray::show_main_window(handle.clone(), None) {
                    eprintln!("无法重新打开 DevDock 主窗口: {error}");
                }
            }
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } => match label.as_str() {
            tray::QUICK_PANEL => {
                api.prevent_close();
                let _ = tray::hide_quick_panel(handle.clone());
            }
            "main" => {
                // 保留窗口直到退出清理完成，避免恢复期间界面已被销毁。
                api.prevent_close();
                if handle.tray_by_id(tray::TRAY_ID).is_some() {
                    if let Some(main) = handle.get_webview_window("main") {
                        let _ = main.hide();
                    }
                } else {
                    handle.exit(0);
                }
            }
            _ => {}
        },
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            let runtime = handle.state::<tasks::Runtime>();
            // 与任务注册使用同一把锁，避免检查为空后又注册新任务。
            let guard = runtime.recovery_lock.lock().unwrap();
            let already_shutting_down = runtime
                .shutting_down
                .swap(true, std::sync::atomic::Ordering::SeqCst);
            let active = !runtime.tasks.lock().unwrap().is_empty();
            drop(guard);
            if active {
                api.prevent_exit();
                if !already_shutting_down {
                    tasks::cancel_all(handle);
                    let app = handle.clone();
                    let exit_code = code.unwrap_or(0);
                    std::thread::spawn(move || {
                        // 工作线程在恢复 AppID 后才移出任务表；等待清空，不能提前终止恢复。
                        while !app
                            .state::<tasks::Runtime>()
                            .tasks
                            .lock()
                            .unwrap()
                            .is_empty()
                        {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        // 再次进入 ExitRequested 时任务已为空，可直接退出，不再创建等待线程。
                        app.exit(exit_code);
                    });
                }
            }
        }
        tauri::RunEvent::Exit => {
            #[cfg(target_os = "macos")]
            tray_panel::shutdown();
            tasks::cancel_all(handle);
        }
        _ => {}
    });
}
