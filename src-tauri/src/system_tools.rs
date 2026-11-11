use crate::config::Result;
#[cfg(any(windows, test))]
use crate::Tool;
#[cfg(windows)]
use crate::Tools;
#[cfg(any(windows, test))]
use std::{fs, path::PathBuf};
use std::{path::Path, process::Command};

// Explorer 不接受部分扩展长度路径写法，转换为等价的普通盘符或 UNC 路径。
fn explorer_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    }
}

pub fn open_command(path: &Path, is_file: bool, platform: &str) -> Result<Command> {
    match platform {
        "macos" => {
            let mut command = Command::new("/usr/bin/open");
            if is_file {
                command.arg("-R");
            }
            command.arg(path);
            Ok(command)
        }
        "windows" => {
            let mut command = Command::new("explorer.exe");
            if is_file {
                command.arg("/select,").arg(explorer_path(path));
            } else {
                command.arg(explorer_path(path));
            }
            Ok(command)
        }
        _ => Err("当前系统暂不支持打开目录或定位文件".into()),
    }
}

#[cfg(windows)]
fn first_file(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file())
}
#[cfg(windows)]
fn path_executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .and_then(|paths| first_file(std::env::split_paths(&paths).map(|dir| dir.join(name))))
}
#[cfg(any(windows, test))]
fn add_jdk(jdks: &mut Vec<Tool>, path: PathBuf) {
    if !path.join("bin/java.exe").is_file() {
        return;
    }
    if jdks
        .iter()
        .any(|jdk| jdk.path.eq_ignore_ascii_case(&path.to_string_lossy()))
    {
        return;
    }
    jdks.push(Tool {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: path.to_string_lossy().into_owned(),
    });
}

#[cfg(windows)]
pub fn windows_tools() -> Tools {
    let env_path = |key| std::env::var_os(key).map(PathBuf::from);
    let local = env_path("LOCALAPPDATA");
    let programs = env_path("ProgramFiles");
    let profile = env_path("USERPROFILE");
    let system = env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let mut terminals = vec![];
    if let Some(path) = path_executable("wt.exe").or_else(|| {
        local
            .as_ref()
            .and_then(|dir| first_file([dir.join("Microsoft/WindowsApps/wt.exe")]))
    }) {
        terminals.push(Tool {
            name: "Windows Terminal".into(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    if let Some(path) = path_executable("pwsh.exe").or_else(|| {
        programs
            .as_ref()
            .and_then(|dir| first_file([dir.join("PowerShell/7/pwsh.exe")]))
    }) {
        terminals.push(Tool {
            name: "PowerShell 7".into(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    if let Some(path) = first_file([system.join("System32/WindowsPowerShell/v1.0/powershell.exe")])
        .or_else(|| path_executable("powershell.exe"))
    {
        terminals.push(Tool {
            name: "Windows PowerShell".into(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    let mut jdks = vec![];
    if let Some(path) = env_path("JAVA_HOME") {
        add_jdk(&mut jdks, path);
    }
    let mut jdk_roots = vec![];
    if let Some(root) = programs.as_ref() {
        for vendor in [
            "Java",
            "Eclipse Adoptium",
            "Microsoft",
            "Amazon Corretto",
            "BellSoft",
            "Zulu",
        ] {
            jdk_roots.push(root.join(vendor));
        }
    }
    if let Some(root) = profile.as_ref() {
        jdk_roots.push(root.join(".jdks"));
    }
    for root in jdk_roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                add_jdk(&mut jdks, entry.path());
            }
        }
    }
    let hbuilderx_path = path_executable("HBuilderX.exe")
        .or_else(|| {
            let mut candidates = vec![];
            if let Some(root) = programs {
                candidates.push(root.join("HBuilderX/HBuilderX.exe"));
            }
            if let Some(root) = local {
                candidates.push(root.join("Programs/HBuilderX/HBuilderX.exe"));
            }
            first_file(candidates)
        })
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    Tools {
        hbuilderx_path,
        terminals,
        jdks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_reveals_files_and_opens_directories_without_shell_interpolation() {
        let path = Path::new(r"C:\项目 文件\a;$(sample).json");
        let command = open_command(path, true, "windows").unwrap();
        assert_eq!(command.get_program(), "explorer.exe");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("/select,"), path.as_os_str()]
        );
        let command = open_command(path, false, "windows").unwrap();
        assert_eq!(command.get_args().collect::<Vec<_>>(), [path.as_os_str()]);
    }
    #[test]
    fn extended_windows_paths_are_converted_without_losing_unc_share() {
        assert_eq!(
            explorer_path(Path::new(r"\\?\C:\项目\file.txt")),
            r"C:\项目\file.txt"
        );
        assert_eq!(
            explorer_path(Path::new(r"\\?\UNC\server\share\file.txt")),
            r"\\server\share\file.txt"
        );
        assert!(open_command(Path::new("/tmp"), false, "linux").is_err());
    }
    #[test]
    fn jdk_detection_requires_java_exe_and_deduplicates_candidates() {
        let root = tempfile::tempdir().unwrap();
        let jdk = root.path().join("jdk 17 中文");
        fs::create_dir_all(jdk.join("bin")).unwrap();
        let mut found = vec![];
        add_jdk(&mut found, jdk.clone());
        assert!(found.is_empty());
        fs::write(jdk.join("bin/java.exe"), b"synthetic executable").unwrap();
        add_jdk(&mut found, jdk.clone());
        add_jdk(&mut found, jdk);
        assert_eq!(found.len(), 1);
    }
}
