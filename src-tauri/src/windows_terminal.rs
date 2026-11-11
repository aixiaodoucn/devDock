//! 终端通过临时 JSON 接收执行数据，避免长环境变量进入启动参数。
use crate::config::{Action, Project, Result};
use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchData {
    path: String,
    env: Vec<(String, String)>,
    command: String,
    node_bin: Option<String>,
    title: Option<String>,
}

fn launch_data(
    project: &Project,
    action: &Action,
    title: Option<&str>,
    node_bin: Option<PathBuf>,
) -> Result<LaunchData> {
    if project.path.contains('\0')
        || action.command.contains('\0')
        || title.is_some_and(|s| s.contains('\0'))
    {
        return Err("终端参数包含无效空字符".into());
    }
    Ok(LaunchData {
        path: project.path.clone(),
        env: crate::nvm::parse_env_vars(&action.env_vars)
            .map_err(|_| "终端环境变量格式无效，请检查 KEY=VALUE 和引号".to_owned())?,
        command: action.command.clone(),
        node_bin: node_bin.map(|p| p.to_string_lossy().into_owned()),
        title: title.map(str::to_owned),
    })
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn startup(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or("终端数据路径编码无效")?;
    if path.contains('\0') {
        return Err("终端数据路径包含无效空字符".into());
    }
    // JSON 仅作为数据读取，只有用户明确配置的原始命令会被执行。
    // 保留用户 profile 与组织策略，正常执行被拒绝时在终端中明确显示。
    Ok(format!(
        r#"& {{ try {{ $dockFile = {}; $dockData = Get-Content -Raw -LiteralPath $dockFile -Encoding UTF8 -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop; Remove-Item -LiteralPath $dockFile -ErrorAction Stop; Set-Location -LiteralPath $dockData.path -ErrorAction Stop; foreach ($dockPair in $dockData.env) {{ [Environment]::SetEnvironmentVariable([string]$dockPair[0], [string]$dockPair[1], [EnvironmentVariableTarget]::Process) }}; if ($dockData.nodeBin) {{ [Environment]::SetEnvironmentVariable('PATH', ([string]$dockData.nodeBin + ';' + [Environment]::GetEnvironmentVariable('PATH', [EnvironmentVariableTarget]::Process)), [EnvironmentVariableTarget]::Process) }}; if ($null -ne $dockData.title) {{ try {{ $Host.UI.RawUI.WindowTitle = [string]$dockData.title }} catch {{ }} }}; if (-not [string]::IsNullOrWhiteSpace($dockData.command)) {{ Invoke-Expression -Command $dockData.command }} }} catch {{ Write-Error 'DevDock 终端启动或命令失败；请检查目录、环境变量和命令。如被组织策略拒绝，请联系管理员，不要绕过策略。' }} }}"#,
        quote(path)
    ))
}

#[derive(Debug)]
struct LaunchSpec {
    program: String,
    args: Vec<String>,
    new_console: bool,
}
fn specification(terminal: &str, title: Option<&str>, path: &Path) -> Result<LaunchSpec> {
    let normalized = terminal.replace('\\', "/");
    if normalized
        .split('/')
        .any(|part| part.to_ascii_lowercase().ends_with(".app"))
    {
        return Err(
            "Windows 不能使用 macOS .app 终端路径，请选择 Windows Terminal 或 PowerShell".into(),
        );
    }
    let basename = normalized
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let canonical = matches!(terminal, "" | "PowerShell" | "WindowsTerminal");
    let absolute = normalized.starts_with("//")
        || (normalized.as_bytes().get(1) == Some(&b':')
            && normalized.as_bytes().get(2) == Some(&b'/'));
    if !canonical && !absolute {
        return Err("终端必须为 Windows 终端名称或绝对 exe 路径".into());
    }
    let wt = terminal == "WindowsTerminal"
        || matches!(basename.as_str(), "wt.exe" | "windowsterminal.exe");
    if !wt && !canonical && !matches!(basename.as_str(), "powershell.exe" | "pwsh.exe") {
        return Err("请选择 wt.exe、WindowsTerminal.exe、powershell.exe 或 pwsh.exe".into());
    }
    let program = match terminal {
        "WindowsTerminal" => "wt.exe",
        "" | "PowerShell" => "powershell.exe",
        other => other,
    }
    .to_owned();
    let mut args = vec![];
    if wt {
        args.extend(["-w".into(), "new".into(), "new-tab".into()]);
        if let Some(title) = title {
            args.extend([
                "--title".into(),
                title.into(),
                "--suppressApplicationTitle".into(),
            ]);
        }
        args.push("powershell.exe".into());
    }
    args.extend(["-NoExit".into(), "-Command".into(), startup(path)?]);
    Ok(LaunchSpec {
        program,
        args,
        new_console: !wt,
    })
}

fn write_data(dir: &Path, data: &LaunchData) -> Result<PathBuf> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "handoff-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut created = false;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        created = true;
        serde_json::to_writer(&mut file, data).map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())
    })();
    if created && result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result.map(|_| path)
}

fn with_data(
    dir: &Path,
    data: &LaunchData,
    handoff: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let path = write_data(dir, data)?;
    let result = handoff(&path);
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

fn handoff_ready(exit_success: Option<bool>, consumed: bool) -> Result<bool> {
    if exit_success == Some(false) {
        return Err("终端启动进程退出失败".into());
    }
    Ok(consumed)
}

fn cleanup(dir: &Path, now: SystemTime, owner_alive: impl Fn(u32) -> bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name
            .strip_prefix("handoff-")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue;
        };
        let Some((pid, id)) = rest.split_once('-') else {
            continue;
        };
        let (Ok(pid), Ok(_)) = (pid.parse::<u32>(), uuid::Uuid::parse_str(id)) else {
            continue;
        };
        let Ok(meta) = entry.file_type() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let expired = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > Duration::from_secs(86400));
        // 活跃进程的交接文件始终保留，避免终端启动较慢时误删。
        if expired && !owner_alive(pid) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(windows)]
fn owner_alive(pid: u32) -> bool {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(process: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    unsafe {
        let handle = OpenProcess(0x1000, 0, pid);
        if handle.is_null() {
            return std::io::Error::last_os_error().raw_os_error() != Some(87);
        }
        let mut code = 259;
        let queried = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        queried == 0 || code == 259
    }
}

#[cfg(windows)]
pub fn launch(
    project: &Project,
    action: &Action,
    terminal: &str,
    title: Option<&str>,
    data_dir: &Path,
) -> Result<()> {
    use std::{os::windows::process::CommandExt, process::Command, time::Instant};
    let data = launch_data(
        project,
        action,
        title,
        crate::nvm::windows_node_bin(project)?,
    )?;
    let dir = data_dir.join("terminal-handoffs");
    cleanup(&dir, SystemTime::now(), owner_alive);
    // 写入执行数据前先验证终端，再生成仅引用数据路径的启动代码。
    specification(terminal, title, &dir.join("validation.json"))?;
    with_data(&dir, &data, |path| {
        let spec = specification(terminal, title, path)?;
        let mut command = Command::new(spec.program);
        // 保留控制台输入输出，不重定向到 NUL，以免隐藏启动错误。
        command.args(spec.args);
        if spec.new_console {
            command.creation_flags(0x10);
        } // CREATE_NEW_CONSOLE
        let mut child = command.spawn().map_err(|e| format!("终端启动失败: {e}"))?;
        let start = Instant::now();
        loop {
            // 启动器非零退出时不能误报命令交接成功。
            let status = child.try_wait().map_err(|e| e.to_string())?;
            if handoff_ready(status.map(|status| status.success()), !path.exists())? {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(15) {
                return Err(
                    "终端未读取启动数据；请检查终端、PowerShell profile 或组织策略。未尝试绕过策略"
                        .into(),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_arguments_preserve_paths_titles_and_profiles() {
        let path = Path::new("C:\\用户 空间\\a'b$`\\data.json");
        for terminal in [
            "WindowsTerminal",
            "C:\\Program Files\\wt.exe",
            "C:\\工具\\WindowsTerminal.exe",
        ] {
            let spec = specification(terminal, Some("项目 '$` 中文"), path).unwrap();
            assert_eq!(
                &spec.args[..6],
                [
                    "-w",
                    "new",
                    "new-tab",
                    "--title",
                    "项目 '$` 中文",
                    "--suppressApplicationTitle"
                ]
            );
            assert!(!spec.new_console);
            assert_eq!(spec.args[6], "powershell.exe");
            assert_eq!(
                spec.program,
                if terminal == "WindowsTerminal" {
                    "wt.exe"
                } else {
                    terminal
                }
            );
        }
        for terminal in [
            "",
            "PowerShell",
            "C:\\Program Files\\PowerShell\\pwsh.exe",
            "C:\\Windows\\powershell.exe",
        ] {
            let spec = specification(terminal, None, path).unwrap();
            assert!(spec.new_console);
            assert_eq!(&spec.args[..2], ["-NoExit", "-Command"]);
            assert!(!spec
                .args
                .iter()
                .any(|s| s.contains("NoProfile") || s.contains("ExecutionPolicy")));
        }
        let spec = specification("WindowsTerminal", None, path).unwrap();
        assert!(!spec.args.iter().any(|s| s == "--title"));
        for bad in [
            "/Applications/Terminal.app",
            "C:\\工具.app\\wt.exe",
            "relative\\wt.exe",
            "C:\\cmd.exe",
        ] {
            assert!(specification(bad, None, path).is_err());
        }
    }
    #[test]
    fn payload_is_json_data_and_startup_only_interpolates_quoted_filename() {
        let dir = tempfile::tempdir().unwrap();
        let data = LaunchData {
            path: "C:\\中文 空格\\' $`".into(),
            env: vec![("CONFIG".into(), "{\"v\":\"$(`x);'中文\"}".into())],
            command: "Write-Output '$`'; missing-command".into(),
            node_bin: Some("C:\\Node 空格".into()),
            title: None,
        };
        let path = write_data(dir.path(), &data).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["command"], data.command);
        assert_eq!(value["env"][0][1], data.env[0].1);
        assert_eq!(value["path"], data.path);
        let code = startup(Path::new("C:\\a'b$`\\data.json")).unwrap();
        assert!(code.contains("'C:\\a''b$`\\data.json'"));
        assert!(code.contains("Get-Content -Raw -LiteralPath"));
        assert!(code.find("Remove-Item").unwrap() < code.find("Set-Location").unwrap());
        assert!(code.find("Set-Location").unwrap() < code.find("Invoke-Expression").unwrap());
        assert!(code.contains("Set-Location -LiteralPath $dockData.path -ErrorAction Stop"));
        assert!(code.contains("[Environment]::SetEnvironmentVariable"));
        assert!(!code.contains(&data.command));
        assert!(!code.contains(&data.env[0].1));
    }
    #[test]
    fn launch_payload_preserves_original_command_and_parsed_env() {
        let project: Project = serde_json::from_value(serde_json::json!({
            "id":"p", "name":"项目", "type":"nodejs", "path":"C:\\中文 空格\\项目",
            "platforms":[], "manifestPath":"", "androidAppId":"", "iosAppId":"",
            "androidProjectPath":"", "androidModule":"", "jdkPath":"", "gradleTask":"",
            "outputDir":"", "syncMainVersion":false, "terminal":"", "commands":[]
        }))
        .unwrap();
        let mut action: Action = serde_json::from_value(serde_json::json!({
            "id":"dev", "name":"Dev", "platform":"web", "kind":"shell", "menuPath":[],
            "command":"Write-Output \"$x\"; npm run dev", "envVars":"CONFIG='{\"a\":\"$`;中文\"}';EMPTY="
        })).unwrap();
        let node = PathBuf::from("C:\\Node 空间\\v22.0.0");
        let data = launch_data(&project, &action, None, Some(node.clone())).unwrap();
        assert_eq!(data.command, action.command);
        assert_eq!(
            data.env,
            [
                ("CONFIG".into(), "{\"a\":\"$`;中文\"}".into()),
                ("EMPTY".into(), "".into())
            ]
        );
        assert_eq!(data.node_bin.as_deref(), node.to_str());
        action.command.clear();
        let data = launch_data(&project, &action, None, None).unwrap();
        assert!(data.command.is_empty());
        assert!(data.title.is_none());
        action.command = "bad\0".into();
        assert!(launch_data(&project, &action, None, None).is_err());
    }
    #[test]
    fn failed_handoff_cleans_data_and_never_reports_nonzero_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let data = LaunchData {
            path: "p".into(),
            env: vec![],
            command: String::new(),
            node_bin: None,
            title: None,
        };
        for consumed in [false, true] {
            assert!(handoff_ready(Some(false), consumed).is_err());
            assert_eq!(handoff_ready(Some(true), consumed).unwrap(), consumed);
            assert_eq!(handoff_ready(None, consumed).unwrap(), consumed);
        }
        for consumed in [false, true] {
            let result = with_data(dir.path(), &data, |path| {
                assert!(path.is_file());
                if consumed {
                    fs::remove_file(path).unwrap();
                }
                handoff_ready(Some(false), consumed).map(|_| ())
            });
            assert!(result.is_err());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
        assert!(with_data(dir.path(), &data, |_| Err("spawn failed".into())).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        with_data(dir.path(), &data, |path| {
            fs::remove_file(path).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn cleanup_keeps_recent_live_and_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let data = LaunchData {
            path: "p".into(),
            env: vec![],
            command: String::new(),
            node_bin: None,
            title: None,
        };
        let path = write_data(dir.path(), &data).unwrap();
        let unrelated = dir.path().join("config.json");
        fs::write(&unrelated, "keep").unwrap();
        cleanup(dir.path(), SystemTime::now(), |_| false);
        assert!(path.exists());
        let future = SystemTime::now() + Duration::from_secs(172800);
        cleanup(dir.path(), future, |_| true);
        assert!(path.exists());
        cleanup(dir.path(), future, |_| false);
        assert!(!path.exists());
        assert!(unrelated.exists());
    }
}
