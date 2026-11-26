//! 本机凭据独立于工作台配置与备份；桌面进程和 stdio 桥使用同一套路径规则。
use crate::config::Result;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const UNAVAILABLE: &str = "无法访问本机私有凭据文件";
const MAX_BYTES: usize = 1024 * 1024;

fn app_root(base: &Path, development: bool) -> PathBuf {
    base.join(if development {
        crate::app_identity::DEVELOPMENT_ID
    } else {
        crate::app_identity::PRODUCTION_ID
    })
}

fn root() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .filter(|p| p.is_absolute())
        .ok_or(UNAVAILABLE)?;
    Ok(app_root(&base, cfg!(debug_assertions)))
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // 也拒绝 Windows 目录联接等重解析点。
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !is_link(&meta) => Ok(Some(meta)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        _ => Err(UNAVAILABLE.into()),
    }
}

fn directory(path: &Path, create: bool, private: bool) -> Result<bool> {
    if metadata(path)?.is_none() {
        if !create {
            return Ok(false);
        }
        let mut options = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            options.mode(0o700);
        }
        // 应用首次运行时，系统用户数据目录可能尚未创建。
        options
            .recursive(true)
            .create(path)
            .map_err(|_| UNAVAILABLE)?;
    }
    let meta = metadata(path)?.ok_or(UNAVAILABLE)?;
    if !meta.is_dir() {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // 普通应用目录允许其他用户读取但不能写；凭据子目录必须仅当前用户可访问。
        if meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & (if private { 0o077 } else { 0o022 }) != 0
        {
            return Err(UNAVAILABLE.into());
        }
    }
    #[cfg(not(unix))]
    let _ = private; // Windows 使用当前用户 LocalAppData 的继承权限。
    Ok(true)
}

fn file_path(root: &Path, key: &str, create: bool) -> Result<Option<PathBuf>> {
    if !matches!(key, "scm" | "scm-prodlog" | "sqlpad" | "mcp-bridge") {
        return Err(UNAVAILABLE.into());
    }
    let dir = root.join("private-credentials");
    if !directory(root, create, false)? || !directory(&dir, create, true)? {
        return Ok(None);
    }
    Ok(Some(dir.join(format!("{key}.json"))))
}

fn read_at(root: &Path, key: &str) -> Result<Option<String>> {
    let Some(path) = file_path(root, key, false)? else {
        return Ok(None);
    };
    let Some(meta) = metadata(&path)? else {
        return Ok(None);
    };
    if !meta.is_file() {
        return Err(UNAVAILABLE.into());
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(UNAVAILABLE.into()),
    };
    let meta = file.metadata().map_err(|_| UNAVAILABLE)?;
    if !meta.is_file() || meta.len() > MAX_BYTES as u64 {
        return Err(UNAVAILABLE.into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o077 != 0 || meta.uid() != unsafe { libc::geteuid() } {
            return Err(UNAVAILABLE.into());
        }
    }
    let mut data = String::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_string(&mut data)
        .map_err(|_| UNAVAILABLE)?;
    if data.len() > MAX_BYTES {
        return Err(UNAVAILABLE.into());
    }
    Ok(Some(data))
}

fn write_at(root: &Path, key: &str, value: &str) -> Result<()> {
    if value.len() > MAX_BYTES {
        return Err(UNAVAILABLE.into());
    }
    let path = file_path(root, key, true)?.ok_or(UNAVAILABLE)?;
    if metadata(&path)?.is_some_and(|m| !m.is_file()) {
        return Err(UNAVAILABLE.into());
    }
    // 同目录私有临时文件原子替换，不让 stdio 进程读到写了一半的令牌。
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().ok_or(UNAVAILABLE)?)
        .map_err(|_| UNAVAILABLE)?;
    tmp.write_all(value.as_bytes())
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|_| UNAVAILABLE)?;
    tmp.persist(path).map_err(|_| UNAVAILABLE)?;
    Ok(())
}

fn clear_at(root: &Path, key: &str) -> Result<()> {
    let Some(path) = file_path(root, key, false)? else {
        return Ok(());
    };
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(UNAVAILABLE.into()),
    }
}

pub(crate) fn load(key: &str) -> Result<Option<String>> {
    read_at(&root()?, key)
}
pub(crate) fn save(key: &str, value: &str) -> Result<()> {
    write_at(&root()?, key, value)
}
pub(crate) fn clear(key: &str) -> Result<()> {
    clear_at(&root()?, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolates_builds_and_entries_and_only_creates_on_write() {
        let tmp = tempfile::tempdir().unwrap();
        let dev = app_root(tmp.path(), true);
        let prod = app_root(tmp.path(), false);
        assert_eq!(read_at(&dev, "sqlpad").unwrap(), None);
        clear_at(&dev, "mcp-bridge").unwrap();
        assert!(!dev.exists());
        for key in ["scm", "scm-prodlog", "sqlpad", "mcp-bridge"] {
            write_at(&dev, key, key).unwrap();
        }
        write_at(&dev, "mcp-bridge", "rotated").unwrap();
        assert_eq!(
            read_at(&dev, "mcp-bridge").unwrap().as_deref(),
            Some("rotated")
        );
        assert_eq!(read_at(&prod, "scm").unwrap(), None);
        assert!(write_at(&dev, "../config", "no").is_err());
        clear_at(&dev, "mcp-bridge").unwrap();
        assert_eq!(read_at(&dev, "mcp-bridge").unwrap(), None);
        assert_eq!(read_at(&dev, "scm").unwrap().as_deref(), Some("scm"));
    }
    #[cfg(unix)]
    #[test]
    fn owner_only_files_and_no_symlink_following() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("app");
        write_at(&root, "scm", "fixture").unwrap();
        let dir = root.join("private-credentials");
        let file = dir.join("scm.json");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        symlink(&file, dir.join("sqlpad.json")).unwrap();
        assert!(read_at(&root, "sqlpad").is_err());
        assert!(write_at(&root, "sqlpad", "other").is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_at(&root, "scm").is_err());
    }
}
