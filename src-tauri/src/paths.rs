// 配置中的路径可来自其他系统；这里只做词法判断，不代表宿主能够访问。
fn windows_format(path: &str) -> bool {
    let bytes = path.as_bytes();
    (bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\'))
        || path.starts_with("\\\\")
        || (path.starts_with("//") && unc_parts(path).is_some())
}

fn unc_parts(path: &str) -> Option<Vec<&str>> {
    let parts: Vec<_> = path[2..]
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 2 || parts[..2].iter().any(|p| matches!(*p, "." | ".." | "?")) {
        return None;
    }
    Some(parts)
}

// Windows canonicalize 返回 verbatim 路径，需与普通盘符/UNC 路径等价。
fn without_verbatim(path: &str) -> std::borrow::Cow<'_, str> {
    if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}").into();
    }
    if let Some(drive) = path.strip_prefix(r"\\?\") {
        if drive.as_bytes().get(1) == Some(&b':') {
            return drive.into();
        }
    }
    path.into()
}

pub fn is_absolute(path: &str) -> bool {
    let portable = without_verbatim(path);
    let path = portable.as_ref();
    if path.contains('\0') {
        return false;
    }
    if path.starts_with("\\\\") {
        return unc_parts(path).is_some();
    }
    path.starts_with('/') || windows_format(path)
}

pub fn key(path: &str) -> String {
    let portable = without_verbatim(path);
    let path = portable.as_ref();
    if windows_format(path) {
        let normalized = path.replace('\\', "/").to_lowercase();
        if normalized.as_bytes().get(1) == Some(&b':') {
            let parts = normalized[3..]
                .split('/')
                .filter(|p| !p.is_empty() && *p != ".")
                .collect::<Vec<_>>();
            // 盘根始终保留斜杠，避免退化为盘符相对路径。
            return format!("{}/{}", &normalized[..2], parts.join("/"));
        }
        return format!(
            "//{}",
            normalized[2..]
                .split('/')
                .filter(|p| !p.is_empty() && *p != ".")
                .collect::<Vec<_>>()
                .join("/")
        );
    }
    // Unix 大小写敏感，反斜杠属于普通文件名字符。
    let parts = path
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect::<Vec<_>>()
        .join("/");
    if path.starts_with('/') {
        format!("/{parts}")
    } else {
        parts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_absolute_paths_and_invalid_inputs() {
        for path in [
            r"C:\项目 空格",
            "C:/路径",
            r"\\server\share\project",
            "//server/share/项目 空格",
            "/项目 空格",
            "C:/",
            "/",
            r"\\?\C:\project",
            r"\\?\UNC\server\share\project",
        ] {
            assert!(is_absolute(path), "{path}");
        }
        for path in [
            "",
            "relative",
            "C:relative",
            "C:",
            r"\path",
            r"\\server",
            "C:/bad\0",
            "/bad\0",
            r"\\?\C:relative",
            r"\\?\UNC\server",
        ] {
            assert!(!is_absolute(path), "{path}");
        }
    }

    #[test]
    fn keys_compare_windows_case_and_slashes_but_preserve_unix_case() {
        assert_eq!(key(r"C:\项目 空格\Foo\\"), key("c:/项目 空格/foo/"));
        assert_eq!(
            key(r"\\SERVER\Share\Project\"),
            key("//server/share/project")
        );
        assert_eq!(key(r"\\?\C:\项目"), key("c:/项目"));
        assert_eq!(
            key(r"\\?\UNC\server\share\项目"),
            key(r"\\SERVER\SHARE\项目")
        );
        assert_eq!(key(r"C:\"), "c:/");
        assert_eq!(key("C:////"), "c:/");
        assert_eq!(key("/tmp//Project/./"), key("/tmp/Project"));
        assert_ne!(key("/tmp/Project"), key("/tmp/project"));
        assert_ne!(key(r"/tmp/a\b"), key("/tmp/a/b"));
    }
}
