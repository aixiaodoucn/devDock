use super::*;
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub(super) const TIMEOUT: Duration = Duration::from_secs(8);
fn runnable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    meta.is_file()
}
pub(super) fn locate(client: Client, home: &Path) -> Option<PathBuf> {
    let name = match client {
        Client::Claude => "claude",
        Client::Codex => "codex",
    };
    let mut dirs: Vec<_> = env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .filter(|p| p.is_absolute())
        .collect();
    dirs.extend([
        home.join(".local/bin"),
        home.join(".volta/bin"),
        home.join(".bun/bin"),
        home.join(".npm-global/bin"),
    ]);
    #[cfg(unix)]
    {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ]);
        // Desktop launchers often omit Node version-manager paths; never source user shell scripts.
        let nvm = env::var_os("NVM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".nvm"));
        if let Ok(entries) = fs::read_dir(nvm.join("versions/node")) {
            let mut versions: Vec<_> = entries.flatten().map(|e| e.path().join("bin")).collect();
            versions.sort();
            dirs.extend(versions.into_iter().rev());
        }
    }
    #[cfg(target_os = "macos")]
    if client == Client::Codex {
        dirs.extend([
            PathBuf::from("/Applications/Codex.app/Contents/Resources"),
            PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin"),
        ]);
    }
    #[cfg(windows)]
    if let Some(roaming) = env::var_os("APPDATA") {
        dirs.push(PathBuf::from(roaming).join("npm"));
    }
    // Only native executables on Windows, not cmd/bat shims that reinterpret arguments.
    let file = format!("{}{}", name, env::consts::EXE_SUFFIX);
    dirs.into_iter()
        .filter(|d| d.is_absolute())
        .map(|d| d.join(&file))
        .find(|p| runnable(p))
}

pub(super) fn run(bin: &Path, args: &[&str], home: &Path, timeout: Duration) -> Result<(), ()> {
    let mut cmd = Command::new(bin);
    // Node-based CLIs may use /usr/bin/env node. Include their bin directory for GUI launches.
    let path = env::join_paths(bin.parent().into_iter().map(Path::to_owned).chain(
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).filter(|p| p.is_absolute()),
    ))
    .map_err(|_| ())?;
    cmd.args(args)
        .env("PATH", path)
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().map_err(|_| ())?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(()).ok_or(()),
            Ok(None) if start.elapsed() < timeout => thread::sleep(Duration::from_millis(20)),
            _ => {
                #[cfg(unix)]
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(());
            }
        }
    }
}
