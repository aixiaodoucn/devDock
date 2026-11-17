use crate::config::Result;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::Command,
};

pub(crate) fn is_supported_macos(terminal: &str) -> bool {
    if terminal.contains('\0')
        || terminal.ends_with('/')
        || terminal.split('/').any(|part| matches!(part, "." | ".."))
    {
        return false;
    }
    terminal.is_empty()
        || terminal == "Terminal"
        || ((terminal.starts_with('/') || Path::new(terminal).is_absolute())
            && matches!(
                app_name(terminal),
                Some(
                    "Terminal.app"
                        | "Ghostty.app"
                        | "iTerm.app"
                        | "iTerm2.app"
                        | "WezTerm.app"
                        | "kitty.app"
                        | "Alacritty.app"
                        | "Warp.app"
                        | "Tabby.app"
                        | "cmux.app"
                )
            ))
}

fn app_name(terminal: &str) -> Option<&str> {
    Path::new(terminal).file_name()?.to_str()
}

// 仅将用户数据作为位置参数传递；不拼接进 shell 脚本。
// open 由 LaunchServices 启动 GUI，任务清理进程组不会终止终端中的业务进程。
const WARP_LAUNCH: &str = r#"
"$1" -a "$2" "$3"
status=$?
if [ "$status" -ne 0 ]; then
    /bin/rm -f -- "$4"
    printf '%s\n' 'Warp 启动失败，已清理本次新建的启动配置。' >&2
fi
exit "$status"
"#;

// CLI 只发送一次新建工作区请求，不注入现有会话，也不自动启动或修改 socket 权限。
const CMUX_LAUNCH: &str = r#"
cli=$1
cwd=$2
command=$3
title=$4
set -- new-workspace --cwd "$cwd"
if [ -n "$command" ]; then set -- "$@" --command "$command"; fi
if [ -n "$title" ]; then set -- "$@" --name "$title"; fi
"$cli" "$@"
status=$?
if [ "$status" -ne 0 ]; then
    printf '%s\n' 'cmux 请求失败：请先打开 cmux，并确认当前 socket 已允许 DevDock 访问；DevDock 不会修改权限或自动重试。若结果不明，请先检查是否已创建工作区。' >&2
fi
exit "$status"
"#;

pub(crate) fn prepare(
    terminal: &str,
    cwd: &str,
    command: &str,
    title: &str,
    home: &Path,
) -> Result<Option<Command>> {
    let native = match app_name(terminal) {
        Some("Warp.app") => "warp",
        Some("cmux.app") => "cmux",
        _ => return Ok(None),
    };
    if !is_supported_macos(terminal) {
        return Err("终端应用必须使用无 NUL、尾斜杠或点路径段的绝对路径".into());
    }
    if !(cwd.starts_with('/') || Path::new(cwd).is_absolute())
        || [terminal, cwd, command, title]
            .iter()
            .any(|s| s.contains('\0'))
    {
        return Err("终端工作目录必须为绝对路径，启动参数不能包含 NUL 字符".into());
    }
    if native == "cmux" {
        let cli = Path::new(terminal).join("Contents/Resources/bin/cmux");
        if !cli.is_file() {
            return Err(format!(
                "cmux 内置 CLI 不存在：{}，请安装包含 CLI 的 cmux 版本",
                cli.display()
            ));
        }
        let mut launch = Command::new("/bin/sh");
        launch.args(["-c", CMUX_LAUNCH, "devdock-cmux"]);
        launch.arg(cli).args([cwd, command, title]);
        return Ok(Some(launch));
    }
    if !home.is_absolute() {
        return Err("Warp 配置主目录必须为绝对路径".into());
    }
    let directory = home.join(".warp/launch_configurations");
    fs::create_dir_all(&directory).map_err(|e| format!("创建 Warp 启动配置目录失败：{e}"))?;
    let name = format!("DevDock-{}", uuid::Uuid::new_v4());
    let path = directory.join(format!("{name}.yaml"));
    let payload = warp_config(&name, cwd, command, title)?;
    write_new_config(&path, &payload)?;
    // Warp 官方 URI 文档引用的 Raycast 实现按配置 name 启动，不按文件路径。
    // name 由安全 ASCII 前缀和 UUID 组成，无需额外百分号编码；用户标题只用于 tab。
    let uri = format!("warp://launch/{name}");
    let mut launch = Command::new("/bin/sh");
    launch.args([
        "-c",
        WARP_LAUNCH,
        "devdock-warp",
        "/usr/bin/open",
        terminal,
        &uri,
    ]);
    launch.arg(path);
    // Warp 异步读取配置，成功或结果不明时必须保留，不能按时间猜测删除。
    Ok(Some(launch))
}

fn warp_config(name: &str, cwd: &str, command: &str, title: &str) -> Result<Vec<u8>> {
    // JSON 是合法 YAML，避免引号、换行、冒号或 Unicode 改变配置结构。
    // https://docs.warp.dev/terminal/sessions/launch-configurations
    let title = if title.is_empty() { "DevDock" } else { title };
    let mut layout = json!({"cwd": cwd});
    if !command.is_empty() {
        layout["commands"] = json!([{"exec": command}]);
    }
    serde_json::to_vec_pretty(&json!({
        "name": name,
        "windows": [{"tabs": [{"title": title, "layout": layout}]}]
    }))
    .map_err(|e| format!("序列化 Warp 启动配置失败：{e}"))
}

fn write_new_config(path: &Path, payload: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("创建 Warp 启动配置失败：{e}"))?;
    if let Err(error) = file.write_all(payload).and_then(|_| file.sync_all()) {
        drop(file);
        // 仅清理本函数成功创建的唯一文件，绝不触碰既有配置。
        let _ = fs::remove_file(path);
        return Err(format!("写入 Warp 启动配置失败：{error}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recognizes_only_supported_app_names() {
        for name in [
            "Terminal",
            "Ghostty",
            "iTerm",
            "iTerm2",
            "WezTerm",
            "kitty",
            "Alacritty",
            "Warp",
            "Tabby",
            "cmux",
        ] {
            assert!(is_supported_macos(&format!("/Applications/{name}.app")));
        }
        assert!(is_supported_macos(""));
        assert!(is_supported_macos("Terminal"));
        for name in [
            "Warp.app",
            "Warp",
            "/Applications/Other.app",
            "/Applications/NotWarp.app",
            "/Applications/Warp.app/",
            "/Applications/Warp.app/.",
            "/Applications/../Warp.app",
            "/Applications/\0Warp.app",
        ] {
            assert!(!is_supported_macos(name));
        }
    }

    #[test]
    fn other_terminals_fall_back_without_writing_files() {
        let home = tempdir().unwrap();
        assert!(prepare(
            "/Applications/Ghostty.app",
            "/tmp",
            "pwd",
            "test",
            home.path()
        )
        .unwrap()
        .is_none());
        assert_eq!(fs::read_dir(home.path()).unwrap().count(), 0);
    }

    #[test]
    fn warp_preserves_special_characters_in_legacy_layout() {
        let cwd = "/tmp/项目 '\" #&?";
        let command = "printf '%s\\n' \"a:b\"\nprintf '\\u4e2d'";
        let title = "任务\n: \"quoted\"";
        let value: serde_json::Value =
            serde_json::from_slice(&warp_config("DevDock-test", cwd, command, title).unwrap())
                .unwrap();
        assert_eq!(value["name"], "DevDock-test");
        let tab = &value["windows"][0]["tabs"][0];
        assert_eq!(tab["title"], title);
        assert_eq!(tab["layout"]["cwd"], cwd);
        assert_eq!(tab["layout"]["commands"][0]["exec"], command);
    }

    #[test]
    fn warp_configs_are_unique_private_and_preserve_existing_files() {
        let home = tempdir().unwrap();
        let directory = home.path().join(".warp/launch_configurations");
        fs::create_dir_all(&directory).unwrap();
        let existing = directory.join("user.yaml");
        fs::write(&existing, "user content").unwrap();
        let first = prepare("/Applications/Warp.app", "/tmp", "pwd", "same", home.path())
            .unwrap()
            .unwrap();
        let second = prepare("/Applications/Warp.app", "/tmp", "pwd", "same", home.path())
            .unwrap()
            .unwrap();
        assert_ne!(first.get_args().last(), second.get_args().last());
        assert_eq!(fs::read_to_string(existing).unwrap(), "user content");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 3);
        for launch in [first, second] {
            let path = Path::new(launch.get_args().last().unwrap());
            assert!(path.is_file());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            let args: Vec<_> = launch.get_args().collect();
            assert_eq!(args[3], "/usr/bin/open");
            assert_eq!(
                args[5],
                format!(
                    "warp://launch/{}",
                    path.file_stem().unwrap().to_str().unwrap()
                )
                .as_str()
            );
        }
    }

    #[test]
    fn empty_warp_command_opens_directory_without_empty_exec() {
        let value: serde_json::Value =
            serde_json::from_slice(&warp_config("DevDock-test", "/tmp", "", "").unwrap()).unwrap();
        assert_eq!(value["name"], "DevDock-test");
        let tab = &value["windows"][0]["tabs"][0];
        assert_eq!(tab["title"], "DevDock");
        assert_eq!(tab["layout"]["cwd"], "/tmp");
        assert!(tab["layout"].get("commands").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn cmux_sends_once_and_omits_empty_command() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempdir().unwrap();
        let cli = directory.path().join("fake-cmux");
        let calls = directory.path().join("calls");
        fs::write(
            &cli,
            "#!/bin/sh\nprintf '%s\\n' call >> \"$CALLS\"\nprintf '%s\\n' \"$@\"\nexit 7\n",
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", CMUX_LAUNCH, "test"])
            .arg(&cli)
            .args(["/tmp/a b", "", "a 'quoted' title"])
            .env("CALLS", &calls)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "new-workspace\n--cwd\n/tmp/a b\n--name\na 'quoted' title\n"
        );
        assert_eq!(fs::read_to_string(calls).unwrap(), "call\n");
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("请先打开 cmux"));
    }

    #[test]
    fn creation_never_overwrites_existing_file() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("existing.yaml");
        fs::write(&path, "original").unwrap();
        assert!(write_new_config(&path, b"replacement").is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn warp_cleans_only_its_config_on_explicit_launch_failure() {
        let directory = tempdir().unwrap();
        let owned = directory.path().join("owned.yaml");
        let existing = directory.path().join("user.yaml");
        fs::write(&owned, "config").unwrap();
        fs::write(&existing, "user").unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", WARP_LAUNCH, "test", "/usr/bin/false", "app", "uri"])
            .arg(&owned)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!owned.exists());
        assert_eq!(fs::read_to_string(existing).unwrap(), "user");
        fs::write(&owned, "config").unwrap();
        let status = Command::new("/bin/sh")
            .args(["-c", WARP_LAUNCH, "test", "/usr/bin/true", "app", "uri"])
            .arg(&owned)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(owned.exists());
    }

    #[test]
    fn cmux_requires_real_bundle_cli_and_keeps_command_as_one_argument() {
        let home = tempdir().unwrap();
        let app = home.path().join("cmux.app");
        let terminal = app.to_str().unwrap();
        assert!(prepare(terminal, "/tmp", "pwd", "title", home.path())
            .unwrap_err()
            .contains("CLI"));
        let cli = app.join("Contents/Resources/bin/cmux");
        fs::create_dir_all(cli.parent().unwrap()).unwrap();
        fs::write(&cli, "").unwrap();
        let command = "printf '%s' \"$(date)\"\necho '任务'";
        let launch = prepare(terminal, "/tmp/a b", command, "title", home.path())
            .unwrap()
            .unwrap();
        let args: Vec<_> = launch.get_args().collect();
        assert_eq!(args[3], cli.as_os_str());
        assert_eq!(args[4], "/tmp/a b");
        assert_eq!(args[5], command);
        assert!(!home.path().join(".warp").exists());
    }
}
