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

// PowerShell -EncodedCommand 使用 UTF-16LE Base64，避免多层 Windows 参数解析破坏引号。
fn encode_powershell(code: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = code.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        encoded.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        encoded.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        encoded.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    encoded
}

pub(crate) fn is_supported(terminal: &str) -> bool {
    terminal_basename(terminal).is_ok()
}

#[derive(Debug)]
struct LaunchSpec {
    program: String,
    args: Vec<String>,
    new_console: bool,
    confirmation_required: bool,
    hide_console: bool,
}
fn terminal_basename(terminal: &str) -> Result<String> {
    if terminal.contains('\0') {
        return Err("终端路径不能包含 NUL 字符".into());
    }
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
    if !canonical
        && !matches!(
            basename.as_str(),
            "wt.exe"
                | "windowsterminal.exe"
                | "powershell.exe"
                | "pwsh.exe"
                | "wezterm.exe"
                | "wezterm-gui.exe"
                | "alacritty.exe"
                | "tabby.exe"
                | "warp.exe"
        )
    {
        return Err(
            "请选择 Windows Terminal、PowerShell、WezTerm、Alacritty、Tabby 或 Warp 的可执行文件"
                .into(),
        );
    }
    Ok(basename)
}

fn specification(terminal: &str, title: Option<&str>, path: &Path) -> Result<LaunchSpec> {
    let basename = terminal_basename(terminal)?;
    if basename == "warp.exe" {
        return Err("Warp 必须通过启动配置和已注册的 URI 协议启动".into());
    }
    let wt = terminal == "WindowsTerminal"
        || matches!(basename.as_str(), "wt.exe" | "windowsterminal.exe");
    let native = matches!(
        basename.as_str(),
        "wezterm.exe" | "wezterm-gui.exe" | "alacritty.exe" | "tabby.exe"
    );
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
    if native {
        match basename.as_str() {
            "wezterm.exe" | "wezterm-gui.exe" => {
                args.extend(["start".into(), "--always-new-process".into(), "--".into()])
            }
            "alacritty.exe" => args.push("-e".into()),
            // Tabby 使用 yargs，-- 保留后面的 PowerShell 选项，执行前仍有 Run/Cancel 确认。
            "tabby.exe" => args.extend(["run".into(), "--".into()]),
            _ => unreachable!(),
        }
        args.extend([
            "powershell.exe".into(),
            "-NoExit".into(),
            "-EncodedCommand".into(),
            encode_powershell(&startup(path)?),
        ]);
    } else {
        args.extend(["-NoExit".into(), "-Command".into(), startup(path)?]);
    }
    Ok(LaunchSpec {
        program,
        args,
        new_console: !wt && !native,
        confirmation_required: basename == "tabby.exe",
        hide_console: false,
    })
}

fn with_warp_config(
    appdata: &Path,
    data: &LaunchData,
    handoff_path: &Path,
    handoff: impl FnOnce(LaunchSpec) -> Result<()>,
) -> Result<()> {
    if !appdata.is_absolute() {
        return Err("无法定位 Windows APPDATA，不能生成 Warp 启动配置".into());
    }
    let directory = appdata.join("warp/Warp/data/launch_configurations");
    fs::create_dir_all(&directory).map_err(|e| format!("创建 Warp 配置目录失败：{e}"))?;
    let name = format!("DevDock-{}", uuid::Uuid::new_v4());
    let config_path = directory.join(format!("{name}.yaml"));
    let bootstrap = format!(
        "powershell.exe -NoExit -EncodedCommand {}",
        encode_powershell(&startup(handoff_path)?)
    );
    let payload = crate::terminal_launch::warp_config(
        &name,
        &data.path,
        &bootstrap,
        data.title.as_deref().unwrap_or(""),
    )?;
    crate::terminal_launch::write_new_config(&config_path, &payload)?;
    // Windows 官方入口是注册 URI；不假定 warp.exe 接受 URI 参数，也不使用 cmd /c。
    let request = format!("try {{ Start-Process -FilePath 'warp://launch/{name}' -ErrorAction Stop }} catch {{ Write-Error 'Warp URI 启动失败，请检查 Warp 的安装和协议注册。'; exit 1 }}");
    let spec = LaunchSpec {
        program: "powershell.exe".into(),
        // 此进程只打开 URI；业务终端仍加载用户 profile，且不修改 ExecutionPolicy。
        args: vec![
            "-NoProfile".into(),
            "-EncodedCommand".into(),
            encode_powershell(&request),
        ],
        new_console: false,
        confirmation_required: false,
        hide_console: true,
    };
    let result = handoff(spec);
    // 成功表示终端已消费 JSON，此时配置必已读取；失败不留下失效的交接配置。
    let _ = fs::remove_file(config_path);
    result
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

fn observe_confirmation_launch(
    mut observe: impl FnMut() -> Result<(Option<bool>, bool)>,
    mut pause: impl FnMut(),
) -> Result<()> {
    // 观察约两秒的早期启动失败；仍运行且未消费只代表待确认，不代表命令已执行。
    for _ in 0..40 {
        let (exit_success, consumed) = observe()?;
        if handoff_ready(exit_success, consumed)? || exit_success == Some(true) {
            return Ok(());
        }
        pause();
    }
    Ok(())
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
    let data = launch_data(
        project,
        action,
        title,
        crate::nvm::windows_node_bin(project)?,
    )?;
    let dir = data_dir.join("terminal-handoffs");
    cleanup(&dir, SystemTime::now(), owner_alive);
    // 写入执行数据前先验证终端，再生成仅引用数据路径的启动代码。
    let basename = terminal_basename(terminal)?;
    if basename == "warp.exe" && !Path::new(terminal).is_file() {
        return Err("所选 Warp EXE 不存在，请重新选择已安装的 Warp".into());
    }
    with_data(&dir, &data, |path| {
        if basename == "warp.exe" {
            let appdata = std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .ok_or("缺少 Windows APPDATA，无法创建 Warp 配置")?;
            with_warp_config(&appdata, &data, path, |spec| launch_process(spec, path))
                .map_err(|error| format!("Warp 交接失败：{error}。请确认 Warp 已安装并注册 warp:// 协议，且支持 Launch Configurations。"))
        } else {
            launch_process(specification(terminal, title, path)?, path)
        }
    })
}

#[cfg(windows)]
fn launch_process(spec: LaunchSpec, path: &Path) -> Result<()> {
    use std::{os::windows::process::CommandExt, process::Command, time::Instant};
    let mut command = Command::new(spec.program);
    // 业务终端保留控制台输入输出，只有 Warp 的 URI 启动器隐藏额外控制台。
    command.args(spec.args);
    if spec.new_console {
        command.creation_flags(0x10); // CREATE_NEW_CONSOLE
    } else if spec.hide_console {
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| format!("终端启动失败: {e}"))?;
    if spec.confirmation_required {
        // 用户可能长时间停留在确认框，保留 JSON 等待 Run，不能按普通启动超时删除。
        // 未消费的文件由既有的过期交接清理处理；这里只报告请求已发出。
        return observe_confirmation_launch(
            || {
                let status = child.try_wait().map_err(|e| e.to_string())?;
                Ok((status.map(|status| status.success()), !path.exists()))
            },
            || std::thread::sleep(Duration::from_millis(50)),
        );
    }
    let start = Instant::now();
    let timeout = Duration::from_secs(if spec.hide_console { 30 } else { 15 });
    loop {
        // 启动器非零退出时不能误报命令交接成功。
        let status = child.try_wait().map_err(|e| e.to_string())?;
        if handoff_ready(status.map(|status| status.success()), !path.exists())? {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err("终端未读取启动数据；请检查终端、PowerShell profile 或组织策略；Warp 还需已注册 URI 协议。未自动重试。".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
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
    fn native_windows_terminals_keep_bootstrap_options_and_confirmation_separate() {
        let path = Path::new(r"C:\用户 空间\a'b$`\data.json");
        for (terminal, prefix, confirmation) in [
            (
                r"C:\Tools\wezterm-gui.exe",
                vec!["start", "--always-new-process", "--"],
                false,
            ),
            (r"C:\Tools\alacritty.exe", vec!["-e"], false),
            (r"C:\Tools\Tabby.exe", vec!["run", "--"], true),
        ] {
            let spec = specification(terminal, Some("中文标题"), path).unwrap();
            assert_eq!(&spec.args[..prefix.len()], prefix.as_slice());
            assert_eq!(spec.args[prefix.len()], "powershell.exe");
            assert_eq!(
                &spec.args[prefix.len() + 1..spec.args.len() - 1],
                ["-NoExit", "-EncodedCommand"]
            );
            assert!(spec
                .args
                .last()
                .unwrap()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b)));
            assert!(!spec.new_console);
            assert_eq!(spec.confirmation_required, confirmation);
        }
        assert!(is_supported(r"C:\Tools\WEZTERM.EXE"));
        assert!(!is_supported(r"C:\Tools\kitty.exe"));
        assert!(!is_supported("alacritty.exe"));
    }

    #[test]
    fn powershell_bootstrap_encoding_uses_utf16le() {
        assert_eq!(encode_powershell("A"), "QQA=");
        assert_eq!(encode_powershell("😀"), "PdgA3g==");
        assert_eq!(encode_powershell(""), "");
    }

    #[test]
    fn warp_uses_roaming_config_and_registered_uri_then_cleans_owned_file() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("warp/Warp/data/launch_configurations");
        fs::create_dir_all(&directory).unwrap();
        let existing = directory.join("user.yaml");
        fs::write(&existing, "keep").unwrap();
        let data = LaunchData {
            path: r"C:\中文 '目录".into(),
            env: vec![],
            command: "user-command-secret".into(),
            node_bin: None,
            title: Some("标题".into()),
        };
        let payload = Path::new(r"C:\用户\handoff.json");
        let mut names = vec![];
        for fail in [false, true] {
            let result = with_warp_config(root.path(), &data, payload, |spec| {
                assert_eq!(spec.program, "powershell.exe");
                assert!(spec.hide_console);
                assert!(!spec.new_console);
                assert_eq!(&spec.args[..2], ["-NoProfile", "-EncodedCommand"]);
                let file = fs::read_dir(&directory)
                    .unwrap()
                    .flatten()
                    .map(|e| e.path())
                    .find(|p| p != &existing)
                    .unwrap();
                let value: serde_json::Value =
                    serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                let name = value["name"].as_str().unwrap();
                assert_eq!(file.file_stem().unwrap().to_str().unwrap(), name);
                names.push(name.to_owned());
                let tab = &value["windows"][0]["tabs"][0];
                assert_eq!(tab["layout"]["cwd"], data.path);
                assert_eq!(tab["title"], "标题");
                let code = tab["layout"]["commands"][0]["exec"].as_str().unwrap();
                assert_eq!(
                    code,
                    format!(
                        "powershell.exe -NoExit -EncodedCommand {}",
                        encode_powershell(&startup(payload).unwrap())
                    )
                );
                assert!(!code.contains(&data.command));
                if fail {
                    Err("synthetic launch failure".into())
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
            assert_eq!(fs::read_to_string(&existing).unwrap(), "keep");
        }
        assert_ne!(names[0], names[1]);
        assert!(is_supported(r"C:\Programs\Warp\warp.exe"));
        assert!(with_warp_config(Path::new("relative"), &data, payload, |_| Ok(())).is_err());
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
    fn confirmation_launch_reports_delayed_startup_failure_without_expiring_pending_data() {
        let mut statuses = [(None, false), (Some(false), false)].into_iter();
        assert!(observe_confirmation_launch(|| Ok(statuses.next().unwrap()), || {}).is_err());
        let mut polls = 0;
        observe_confirmation_launch(
            || {
                polls += 1;
                Ok((None, false))
            },
            || {},
        )
        .unwrap();
        assert_eq!(polls, 40);
        observe_confirmation_launch(
            || Ok((Some(true), false)),
            || panic!("启动器正常转交后无需等待GUI退出"),
        )
        .unwrap();
        observe_confirmation_launch(|| Ok((None, true)), || panic!("已消费数据无需继续等待"))
            .unwrap();
    }

    #[test]
    fn confirmation_request_keeps_unconsumed_data_for_late_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let data = LaunchData {
            path: r"C:\项目".into(),
            env: vec![],
            command: "npm run dev".into(),
            node_bin: None,
            title: None,
        };
        with_data(dir.path(), &data, |path| {
            let spec = specification(r"C:\Tools\Tabby.exe", None, path)?;
            assert!(spec.confirmation_required);
            assert!(!handoff_ready(Some(true), false)?);
            Ok(())
        })
        .unwrap();
        let files: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(files.len(), 1);
        cleanup(
            dir.path(),
            SystemTime::now() + Duration::from_secs(172800),
            |_| true,
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
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
