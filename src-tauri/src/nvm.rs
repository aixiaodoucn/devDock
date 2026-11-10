use crate::config::{Action, Project, Result};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const NVM_SCRIPTS: [&str; 2] = ["/opt/homebrew/opt/nvm/nvm.sh", "/usr/local/opt/nvm/nvm.sh"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersion {
    pub version: String,
    pub path: String,
    pub nvm_dir: String,
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn semver(version: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = version.strip_prefix('v')?.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut numbers = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty()
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        numbers[index] = part.parse().ok()?;
    }
    Some(numbers)
}

pub fn validate_selection(version: &str, dir: &str) -> Result<()> {
    if !dir.is_empty() && (!Path::new(dir).is_absolute() || dir.contains('\0')) {
        return Err("nvmDir 必须为绝对路径".into());
    }
    if !version.is_empty() {
        if semver(version).is_none() {
            return Err("nodeVersion 必须为精确版本 vMAJOR.MINOR.PATCH".into());
        }
        if dir.is_empty() {
            return Err("选择 Node 版本时必须指定 nvmDir 绝对路径".into());
        }
    }
    Ok(())
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::{
            ffi::CString,
            os::unix::{ffi::OsStrExt, fs::PermissionsExt},
        };
        let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        // 同时检查权限位与当前进程的访问能力，不运行 Node。
        metadata.permissions().mode() & 0o111 != 0
            && unsafe { libc::access(path.as_ptr(), libc::X_OK) == 0 }
    }
    #[cfg(not(unix))]
    false
}

fn candidate_roots(
    nvm_dir: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots = vec![];
    if let Some(dir) = nvm_dir {
        roots.push(dir);
    }
    if let Some(home) = home {
        roots.push(home.join(".nvm"));
        roots.push(home.join(".config/nvm"));
    }
    if let Some(xdg) = xdg {
        roots.push(xdg.join("nvm"));
    }
    roots.retain(|root| root.is_absolute());
    roots
}

pub fn detect() -> Vec<NodeVersion> {
    scan(candidate_roots(
        std::env::var_os("NVM_DIR").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    ))
}

fn scan(roots: Vec<PathBuf>) -> Vec<NodeVersion> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    for root in roots {
        if !root.is_absolute() {
            continue;
        }
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(root.join("versions/node")) else {
            continue;
        };
        let Some(nvm_dir) = root.to_str() else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(version) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(numbers) = semver(&version) else {
                continue;
            };
            let node = entry.path().join("bin/node");
            if !executable(&node) {
                continue;
            }
            let Ok(node) = fs::canonicalize(node) else {
                continue;
            };
            let Some(path) = node.to_str() else {
                continue;
            };
            if seen.insert(node.clone()) {
                found.push((
                    numbers,
                    NodeVersion {
                        version,
                        path: path.into(),
                        nvm_dir: nvm_dir.into(),
                    },
                ));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    found.into_iter().map(|(_, version)| version).collect()
}

// 解析 KEY=VALUE;KEY2=VALUE2 或逐行输入；值保留空格和符号，仅去掉首尾空白。
pub fn parse_env_vars(text: &str) -> Result<Vec<(String, String)>> {
    let mut result: Vec<(String, String)> = vec![];
    for entry in text.split([';', '\n', '\r']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("环境变量缺少等号: {entry}"))?;
        let key = key.trim();
        if key.is_empty()
            || !key.bytes().enumerate().all(|(index, b)| {
                b == b'_' || b.is_ascii_alphabetic() || (index > 0 && b.is_ascii_digit())
            })
        {
            return Err(format!("环境变量名称无效: {key}"));
        }
        if value.contains('\0') {
            return Err(format!("环境变量 {key} 的值包含无效空字符"));
        }
        if result.iter().any(|(existing, _)| existing == key) {
            return Err(format!("环境变量重复: {key}"));
        }
        result.push((key.to_owned(), value.trim().to_owned()));
    }
    Ok(result)
}

pub fn validate_env_vars(text: &str) -> Result<()> {
    parse_env_vars(text).map(|_| ())
}

fn env_prefix(text: &str) -> Result<String> {
    let vars = parse_env_vars(text)?;
    Ok(vars
        .iter()
        .map(|(key, value)| format!("export {key}={}", quote(value)))
        .collect::<Vec<_>>()
        .join("; "))
}

pub fn command_for(project: &Project, action: &Action) -> Result<String> {
    if action.kind != "shell" {
        return Ok(action.command.clone());
    }
    let env = env_prefix(&project.env_vars)?;
    let base = if project.project_type == "nodejs" && !project.node_version.is_empty() {
        activation(
            &project.nvm_dir,
            &project.node_version,
            &action.command,
            &NVM_SCRIPTS.map(PathBuf::from),
        )?
    } else {
        action.command.clone()
    };
    // 环境变量先于 Node 激活和原命令导出，失败时以 && 阻止继续执行。
    Ok(if env.is_empty() {
        base
    } else {
        format!("{env} && {base}")
    })
}

fn activation(dir: &str, version: &str, command: &str, scripts: &[PathBuf]) -> Result<String> {
    validate_selection(version, dir)?;
    if version.is_empty() {
        return Err("未选择 Node 版本".into());
    }
    if command.contains('\0') {
        return Err("终端命令包含无效空字符".into());
    }
    let node = Path::new(dir)
        .join("versions/node")
        .join(version)
        .join("bin/node");
    if !executable(&node) {
        return Err(format!(
            "所选 Node 未安装或 bin/node 不可执行: {}",
            node.display()
        ));
    }
    let script = std::iter::once(Path::new(dir).join("nvm.sh"))
        .chain(scripts.iter().cloned())
        .find(|path| path.is_absolute() && path.is_file())
        .ok_or("找不到 nvm.sh；请检查 nvm 数据目录或 Homebrew nvm 安装")?;
    let script = script.to_str().ok_or("nvm.sh 路径编码无效")?;
    let node = node.to_str().ok_or("Node 路径编码无效")?;
    // AppleScript 已先 cd；所有数据使用单引号引用。eval 将整段用户命令放在
    // && 的同一个分支中，避免用户命令含分号/换行时绕过激活失败保护。
    Ok(format!(
        "test -x {} && test -r {} && export NVM_DIR={} && source {} --no-use && nvm use --silent {} && eval {}",
        quote(node), quote(script), quote(dir), quote(script), quote(version), quote(command),
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::fs::{symlink, PermissionsExt},
        process::Command,
    };

    fn installed(root: &Path, version: &str) -> PathBuf {
        let node = root.join("versions/node").join(version).join("bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        // 只验证文件与可执行权限，测试和检测均不执行 fake node。
        fs::write(&node, "fake node must never execute").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        node
    }

    #[test]
    fn roots_cover_environment_and_common_data_directories() {
        let temp = tempfile::tempdir().unwrap();
        let env = temp.path().join("custom-nvm");
        let home = temp.path().join("home");
        let xdg = temp.path().join("xdg");
        let roots = candidate_roots(Some(env.clone()), Some(home.clone()), Some(xdg.clone()));
        assert_eq!(
            roots,
            [
                env,
                home.join(".nvm"),
                home.join(".config/nvm"),
                xdg.join("nvm")
            ]
        );
        for (index, root) in roots.iter().enumerate() {
            installed(root, &format!("v20.0.{index}"));
        }
        assert_eq!(scan(roots).len(), 4);
        assert!(candidate_roots(Some("relative".into()), None, Some("relative".into())).is_empty());
    }

    #[test]
    fn env_vars_parse_values_with_spaces_and_symbols() {
        assert_eq!(parse_env_vars("").unwrap(), vec![]);
        assert_eq!(
            parse_env_vars(
                "AI_ALERT_EMAIL_ENABLED=false;ALERT_EMAIL_FROM=艾小逗 <1144755431@qq.com>;ALERT_EMAIL_SMTP_HOST=smtp.qq.com;"
            )
            .unwrap(),
            vec![
                ("AI_ALERT_EMAIL_ENABLED".into(), "false".into()),
                ("ALERT_EMAIL_FROM".into(), "艾小逗 <1144755431@qq.com>".into()),
                ("ALERT_EMAIL_SMTP_HOST".into(), "smtp.qq.com".into()),
            ]
        );
        assert_eq!(
            parse_env_vars("A=1\nB=two words\n\nC=\nD=a'b; E=x=y").unwrap(),
            vec![
                ("A".into(), "1".into()),
                ("B".into(), "two words".into()),
                ("C".into(), "".into()),
                ("D".into(), "a'b".into()),
                ("E".into(), "x=y".into()),
            ]
        );
        for invalid in ["NO_EQUALS", "=value", "1BAD=x", "A=1;A=2", "A=bad\0"] {
            assert!(validate_env_vars(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn env_vars_are_exported_before_node_activation_and_command() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("nvm data");
        installed(&root, "v22.10.0");
        fs::write(root.join("nvm.sh"), "# fake").unwrap();
        let mut project: Project = serde_json::from_value(serde_json::json!({"id":"node","name":"Node","type":"nodejs","path":"/tmp/node-project","platforms":["web"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]})).unwrap();
        let mut action: Action = serde_json::from_value(serde_json::json!({"id":"dev","name":"Dev","platform":"web","kind":"shell","menuPath":[],"command":"npm run dev"})).unwrap();
        project.node_version = "v22.10.0".into();
        project.nvm_dir = root.to_string_lossy().into();
        project.env_vars = "ALERT_EMAIL_FROM=艾小逗 <1144755431@qq.com>;TOKEN=a'b".into();
        let command = command_for(&project, &action).unwrap();
        assert!(command.starts_with(
            "export ALERT_EMAIL_FROM='艾小逗 <1144755431@qq.com>'; export TOKEN='a'\\''b' && test -x "
        ));
        assert!(command.contains("nvm use --silent 'v22.10.0' && eval 'npm run dev'"));
        // 未配置环境变量时保持原命令；菜单动作不注入环境变量。
        project.env_vars.clear();
        assert!(command_for(&project, &action)
            .unwrap()
            .starts_with("test -x "));
        project.env_vars = "A=1".into();
        action.kind = "menu".into();
        assert_eq!(command_for(&project, &action).unwrap(), "npm run dev");
        // 非 Node 项目的 shell 命令同样使用项目环境变量。
        project.project_type = "backend".into();
        action.kind = "shell".into();
        assert_eq!(
            command_for(&project, &action).unwrap(),
            "export A='1' && npm run dev"
        );
    }

    #[test]
    fn unselected_non_node_and_menu_actions_keep_original_command() {
        let mut project: Project = serde_json::from_value(serde_json::json!({"id":"node","name":"Node","type":"nodejs","path":"/tmp/node-project","platforms":["pc"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]})).unwrap();
        let mut action: Action = serde_json::from_value(serde_json::json!({"id":"dev","name":"Dev","platform":"pc","kind":"shell","menuPath":[],"command":"printf 'original'; false\nprintf next"})).unwrap();
        assert_eq!(command_for(&project, &action).unwrap(), action.command);
        project.node_version = "v22.10.0".into();
        project.nvm_dir = "/missing/.nvm".into();
        assert!(command_for(&project, &action).is_err());
        for project_type in ["uniapp", "backend"] {
            project.project_type = project_type.into();
            assert_eq!(command_for(&project, &action).unwrap(), action.command);
        }
        project.project_type = "nodejs".into();
        action.kind = "menu".into();
        assert_eq!(command_for(&project, &action).unwrap(), action.command);
    }

    #[test]
    fn detection_sorts_numeric_versions_deduplicates_paths_and_checks_executable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("nvm data");
        for version in [
            "v9.9.9",
            "v22.2.0",
            "v22.10.0",
            "v22.10.1",
            "v01.0.0",
            "default",
            "v23.0.0-beta",
        ] {
            installed(&root, version);
        }
        let disabled = installed(&root, "v30.0.0");
        fs::set_permissions(disabled, fs::Permissions::from_mode(0o644)).unwrap();
        fs::create_dir_all(root.join("versions/node/v31.0.0/bin/node")).unwrap();
        let alias = temp.path().join("nvm alias");
        symlink(&root, &alias).unwrap();
        let versions = scan(vec![root.clone(), alias, root]);
        assert_eq!(
            versions
                .iter()
                .map(|v| v.version.as_str())
                .collect::<Vec<_>>(),
            ["v22.10.1", "v22.10.0", "v22.2.0", "v9.9.9"]
        );
        assert!(versions
            .iter()
            .all(|v| Path::new(&v.path).is_absolute() && Path::new(&v.nvm_dir).is_absolute()));
        let value = serde_json::to_value(&versions[0]).unwrap();
        assert!(value["nvmDir"].is_string());
    }

    #[test]
    fn activation_quotes_paths_and_runs_whole_command_only_after_success() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("nvm space ' $(false)");
        installed(&root, "v22.10.0");
        let script = root.join("nvm.sh");
        let marker = temp.path().join("ran");
        let user = format!(
            "printf first; printf second > {}",
            quote(marker.to_str().unwrap())
        );
        for shell in ["/bin/bash", "/bin/zsh"] {
            fs::write(&script, "[ \"$1\" = --no-use ] || return 7\nnvm() { [ \"$1\" = use ] && [ \"$2\" = --silent ] && [ \"$3\" = v22.10.0 ] && [ -x \"$NVM_DIR/versions/node/$3/bin/node\" ]; }\n").unwrap();
            let command = activation(root.to_str().unwrap(), "v22.10.0", &user, &[]).unwrap();
            let output = Command::new(shell).args(["-c", &command]).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(&marker).unwrap(), "second");
            fs::remove_file(&marker).unwrap();
            for failure in ["return 8\n", "nvm() { return 9; }\n"] {
                fs::write(&script, failure).unwrap();
                let output = Command::new(shell).args(["-c", &command]).output().unwrap();
                assert!(!output.status.success());
                assert!(!marker.exists());
            }
        }
    }

    #[test]
    fn activation_rejects_missing_files_permissions_and_injection() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let node = installed(root, "v22.10.0");
        let script = root.join("nvm.sh");
        assert!(activation(root.to_str().unwrap(), "v22.10.0", "true", &[])
            .unwrap_err()
            .contains("nvm.sh"));
        fs::write(&script, "nvm() { return 0; }\n").unwrap();
        for version in [
            "default",
            "v22",
            "v22.10.0; false",
            "../../tmp",
            "v22.10.0\nfalse",
        ] {
            assert!(activation(root.to_str().unwrap(), version, "true", &[]).is_err());
        }
        assert!(activation("relative", "v22.10.0", "true", &[]).is_err());
        let marker = root.join("must-not-run");
        let user = format!("printf ran > {}", quote(marker.to_str().unwrap()));
        let command = activation(root.to_str().unwrap(), "v22.10.0", &user, &[]).unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(activation(root.to_str().unwrap(), "v22.10.0", "true", &[]).is_err());
        assert!(!Command::new("/bin/bash")
            .args(["-c", &command])
            .status()
            .unwrap()
            .success());
        assert!(!marker.exists());
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_file(script).unwrap();
        assert!(!Command::new("/bin/bash")
            .args(["-c", &command])
            .status()
            .unwrap()
            .success());
        assert!(!marker.exists());
        fs::remove_file(node).unwrap();
        assert!(activation(root.to_str().unwrap(), "v22.10.0", "true", &[]).is_err());
    }

    #[test]
    fn activation_supports_separate_homebrew_script_and_data_directory() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        installed(&data, "v20.18.0");
        let script = temp.path().join("homebrew nvm.sh");
        fs::write(&script, "nvm() { [ \"$NVM_DIR\" = \"$EXPECTED_NVM\" ]; }\n").unwrap();
        let command = activation(data.to_str().unwrap(), "v20.18.0", "true", &[script]).unwrap();
        assert!(Command::new("/bin/bash")
            .args(["-c", &command])
            .env("EXPECTED_NVM", &data)
            .status()
            .unwrap()
            .success());
    }
}
