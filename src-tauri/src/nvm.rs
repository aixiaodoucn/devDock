#[cfg(any(not(windows), test))]
use crate::config::Action;
use crate::config::{Project, Result, Settings};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[cfg(any(not(windows), test))]
const NVM_SCRIPTS: [&str; 2] = ["/opt/homebrew/opt/nvm/nvm.sh", "/usr/local/opt/nvm/nvm.sh"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersion {
    pub version: String,
    pub path: String,
    pub nvm_dir: String,
    pub manager: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fnm_dir: String,
}

#[cfg(any(not(windows), test))]
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
    if !dir.is_empty() && !crate::paths::is_absolute(dir) {
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

/// 旧配置有版本时继续使用 nvm，否则沿用终端默认环境。
pub(crate) fn effective_manager(project: &Project) -> Result<&str> {
    match project.node_manager.as_str() {
        "" => Ok(if project.node_version.is_empty() {
            "system"
        } else {
            "nvm"
        }),
        "nvm" | "fnm" if project.node_version.is_empty() => Ok("system"),
        manager @ ("system" | "nvm" | "fnm") => Ok(manager),
        "inherit" if project.node_version.is_empty() => Ok("system"),
        "inherit" => Err("请在偏好设置配置公共Node环境".into()),
        _ => Err("nodeManager 必须为 inherit、system、nvm 或 fnm".into()),
    }
}

fn validate_roots(nvm_dir: &str, fnm_dir: &str) -> Result<()> {
    for (name, dir) in [("nvmDir", nvm_dir), ("fnmDir", fnm_dir)] {
        if !dir.is_empty() && !crate::paths::is_absolute(dir) {
            return Err(format!("{name} 必须为绝对路径"));
        }
    }
    Ok(())
}

/// 公共配置仅验证格式，允许安装目录尚不存在及跨系统导入。
pub fn validate_settings(settings: &Settings) -> Result<()> {
    validate_roots(&settings.nvm_dir, &settings.fnm_dir)?;
    let root = match settings.node_manager.as_str() {
        "" | "system" => return Ok(()),
        "nvm" => &settings.nvm_dir,
        "fnm" => &settings.fnm_dir,
        _ => return Err("公共 nodeManager 必须为 system、nvm 或 fnm".into()),
    };
    if root.is_empty() {
        return Err("公共 Node 环境必须指定对应绝对数据目录".into());
    }
    Ok(())
}

fn validate_project_format(project: &Project) -> Result<()> {
    validate_roots(&project.nvm_dir, &project.fnm_dir)?;
    if !["", "inherit", "system", "nvm", "fnm"].contains(&project.node_manager.as_str()) {
        return Err("nodeManager 必须为 inherit、system、nvm 或 fnm".into());
    }
    if !project.node_version.is_empty() && semver(&project.node_version).is_none() {
        return Err("nodeVersion 必须为精确版本 vMAJOR.MINOR.PATCH".into());
    }
    Ok(())
}

/// 配置保存与导入不检查安装，也允许尚未配置公共环境的 inherit 项目。
pub fn validate_project_config(project: &Project, settings: &Settings) -> Result<()> {
    if project.project_type != "nodejs" {
        return Ok(());
    }
    validate_settings(settings)?;
    validate_project_format(project)?;
    if settings.node_manager.is_empty() {
        validate_project(project)?;
    }
    Ok(())
}

/// 仅生成本次运行/体检的快照，公共选择不会写回兼容字段。
pub fn resolve_project(project: &Project, settings: &Settings) -> Result<Project> {
    if project.project_type != "nodejs" {
        return Ok(project.clone());
    }
    validate_project_config(project, settings)?;
    let mut resolved = project.clone();
    if project.node_version.is_empty() || settings.node_manager == "system" {
        resolved.node_manager = "system".into();
        resolved.node_version.clear();
        resolved.nvm_dir.clear();
        resolved.fnm_dir.clear();
    } else if !settings.node_manager.is_empty() {
        resolved.node_manager = settings.node_manager.clone();
        resolved.nvm_dir = settings.nvm_dir.clone();
        resolved.fnm_dir = settings.fnm_dir.clone();
    } else if project.node_manager == "inherit" {
        return Err("请在偏好设置配置公共Node环境".into());
    }
    Ok(resolved)
}

pub fn validate_project(project: &Project) -> Result<()> {
    validate_project_format(project)?;
    if project.node_manager == "inherit" {
        return Ok(());
    }
    let manager = effective_manager(project)?;
    // 导入只检查格式，不依赖当前机器的安装；旧空版本可保留合法 nvmDir。
    if manager == "system" {
        if !project.node_version.is_empty() {
            return Err("终端默认环境不能指定 nodeVersion".into());
        }
        return Ok(());
    }
    if semver(&project.node_version).is_none() {
        return Err("nodeVersion 必须为精确版本 vMAJOR.MINOR.PATCH".into());
    }
    let dir = if manager == "fnm" {
        &project.fnm_dir
    } else {
        &project.nvm_dir
    };
    if dir.is_empty() {
        return Err(format!("选择 {manager} 时必须指定对应绝对数据目录"));
    }
    Ok(())
}

/// 统一解析精确安装路径，供终端执行和只读体检共用。
pub(crate) fn selected_node(project: &Project, windows: bool) -> Result<Option<PathBuf>> {
    validate_project(project)?;
    let manager = effective_manager(project)?;
    if manager == "system" {
        return Ok(None);
    }
    let root = Path::new(if manager == "fnm" {
        &project.fnm_dir
    } else {
        &project.nvm_dir
    });
    if !root.is_absolute() {
        return Err("Node 数据目录不是当前系统可用的绝对路径".into());
    }
    let version = &project.node_version;
    let candidates = if manager == "fnm" {
        vec![root
            .join("node-versions")
            .join(version)
            .join("installation")
            .join(if windows { "node.exe" } else { "bin/node" })]
    } else if windows {
        vec![
            root.join(version).join("node.exe"),
            root.join(version.strip_prefix('v').unwrap())
                .join("node.exe"),
        ]
    } else {
        vec![root.join("versions/node").join(version).join("bin/node")]
    };
    candidates
        .into_iter()
        .find(|node| {
            if windows {
                node.is_file()
            } else {
                executable(node)
            }
        })
        .map(Some)
        .ok_or_else(|| "所选精确 Node 版本未安装、文件缺少或不可执行；未以其他版本代替".into())
}

pub(crate) fn nvm_script(root: &Path) -> Option<PathBuf> {
    std::iter::once(root.join("nvm.sh"))
        .chain([
            PathBuf::from("/opt/homebrew/opt/nvm/nvm.sh"),
            PathBuf::from("/usr/local/opt/nvm/nvm.sh"),
        ])
        .find(|path| path.is_absolute() && path.is_file())
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
    #[cfg(windows)]
    {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(any(unix, windows)))]
    false
}

#[cfg(any(not(windows), test))]
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
    #[cfg(windows)]
    {
        scan_windows(windows_roots(
            std::env::var_os("NVM_HOME").map(PathBuf::from),
            std::env::var_os("APPDATA").map(PathBuf::from),
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from),
            std::env::var_os("ProgramFiles").map(PathBuf::from),
        ))
    }
    #[cfg(not(windows))]
    {
        scan(candidate_roots(
            std::env::var_os("NVM_DIR").map(PathBuf::from),
            std::env::var_os("HOME").map(PathBuf::from),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        ))
    }
}

fn fnm_roots() -> Vec<PathBuf> {
    fnm_candidates(
        std::env::var_os("FNM_DIR").map(PathBuf::from),
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from),
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
    )
}

fn fnm_candidates(
    custom: Option<PathBuf>,
    home: Option<PathBuf>,
    xdg: Option<PathBuf>,
    appdata: Option<PathBuf>,
    local: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots: Vec<_> = custom.into_iter().collect();
    if let Some(home) = home {
        roots.extend([
            home.join("Library/Application Support/fnm"),
            home.join(".local/share/fnm"),
            home.join(".fnm"),
            home.join("AppData/Roaming/fnm"),
            home.join("AppData/Local/fnm"),
        ]);
    }
    for base in [xdg, appdata, local].into_iter().flatten() {
        roots.push(base.join("fnm"));
    }
    roots.retain(|root| root.is_absolute());
    roots
}

fn scan_fnm(roots: Vec<PathBuf>, windows: bool) -> Vec<NodeVersion> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    for root in roots {
        if !root.is_absolute() {
            continue;
        }
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        let Some(dir) = root.to_str() else {
            continue;
        };
        let Ok(entries) = fs::read_dir(root.join("node-versions")) else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(version) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if semver(&version).is_none() {
                continue;
            }
            let node = entry.path().join("installation").join(if windows {
                "node.exe"
            } else {
                "bin/node"
            });
            if !(if windows {
                node.is_file()
            } else {
                executable(&node)
            }) {
                continue;
            }
            let Ok(node) = fs::canonicalize(node) else {
                continue;
            };
            let Some(path) = node.to_str() else {
                continue;
            };
            if seen.insert(crate::paths::key(path)) {
                found.push(NodeVersion {
                    version,
                    path: path.into(),
                    manager: "fnm".into(),
                    nvm_dir: String::new(),
                    fnm_dir: dir.into(),
                });
            }
        }
    }
    sort_versions(&mut found);
    found
}

fn sort_versions(versions: &mut [NodeVersion]) {
    versions.sort_by(|a, b| {
        semver(&b.version)
            .cmp(&semver(&a.version))
            .then_with(|| a.manager.cmp(&b.manager))
            .then_with(|| a.path.cmp(&b.path))
    });
}

/// 只读扫描；显式目录必须同时明确提供者，避免意外扫描其他安装。
pub fn detect_versions(manager: Option<&str>, dir: Option<&str>) -> Result<Vec<NodeVersion>> {
    if !matches!(manager, None | Some("nvm" | "fnm" | "system")) {
        return Err("nodeManager 必须为 system、nvm 或 fnm".into());
    }
    if let Some(dir) = dir {
        if !crate::paths::is_absolute(dir) {
            return Err("Node 数据目录必须为绝对路径".into());
        }
        if !matches!(manager, Some("nvm" | "fnm")) {
            return Err("手动目录必须指定 nvm 或 fnm".into());
        }
    }
    if manager == Some("system") {
        return Ok(vec![]);
    }
    let mut found = vec![];
    if manager.is_none() || manager == Some("nvm") {
        if let Some(dir) = dir {
            #[cfg(windows)]
            found.extend(scan_windows(vec![PathBuf::from(dir)]));
            #[cfg(not(windows))]
            found.extend(scan(vec![PathBuf::from(dir)]));
        } else {
            found.extend(detect());
        }
    }
    if manager.is_none() || manager == Some("fnm") {
        found.extend(scan_fnm(
            dir.map(|dir| vec![PathBuf::from(dir)])
                .unwrap_or_else(fnm_roots),
            cfg!(windows),
        ));
    }
    sort_versions(&mut found);
    Ok(found)
}

#[cfg(any(windows, test))]
fn windows_roots(
    nvm_home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
    home: Option<PathBuf>,
    program_files: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut roots: Vec<_> = nvm_home.into_iter().collect();
    for base in [appdata, local_appdata, program_files]
        .into_iter()
        .flatten()
    {
        roots.push(base.join("nvm"));
    }
    if let Some(home) = home {
        roots.push(home.join("AppData/Roaming/nvm"));
        roots.push(home.join("scoop/apps/nvm/current"));
    }
    roots.push(PathBuf::from(r"C:\nvm"));
    let mut seen = HashSet::new();
    roots.retain(|root| {
        root.is_absolute() && seen.insert(crate::paths::key(&root.to_string_lossy()))
    });
    roots
}

// nvm-windows 的 Node 位于版本目录根下，不使用 Unix 的 versions/node/bin 布局。
#[cfg(any(windows, test))]
fn windows_node_file(dir: &Path) -> bool {
    let node = dir.join("node.exe");
    #[cfg(windows)]
    {
        executable(&node)
    }
    // Mac 测试用普通假 exe 文件构造 Windows 安装，不执行它，也不要求 Unix 执行位。
    #[cfg(not(windows))]
    {
        node.is_file()
    }
}

#[cfg(any(windows, test))]
fn scan_windows(roots: Vec<PathBuf>) -> Vec<NodeVersion> {
    let mut found = vec![];
    let mut seen = HashSet::new();
    for root in roots {
        if !root.is_absolute() {
            continue;
        }
        let Ok(root) = fs::canonicalize(root) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let Some(nvm_dir) = root.to_str() else {
            continue;
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let version = if name.starts_with('v') {
                name
            } else {
                format!("v{name}")
            };
            let Some(numbers) = semver(&version) else {
                continue;
            };
            if !windows_node_file(&entry.path()) {
                continue;
            }
            let Ok(node) = fs::canonicalize(entry.path().join("node.exe")) else {
                continue;
            };
            let Some(path) = node.to_str() else {
                continue;
            };
            if seen.insert(crate::paths::key(path)) {
                found.push((
                    numbers,
                    NodeVersion {
                        version,
                        path: path.into(),
                        nvm_dir: nvm_dir.into(),
                        manager: "nvm".into(),
                        fnm_dir: String::new(),
                    },
                ));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    found.into_iter().map(|(_, version)| version).collect()
}

// 只为新会话提供 PATH 前置目录，不运行 Node 或改变 nvm 全局链接。
#[cfg(any(windows, test))]
pub fn windows_node_bin(project: &Project) -> Result<Option<PathBuf>> {
    if project.project_type != "nodejs" {
        return Ok(None);
    }
    Ok(selected_node(project, true)?.and_then(|node| node.parent().map(Path::to_path_buf)))
}

#[cfg(not(windows))]
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
                        manager: "nvm".into(),
                        fnm_dir: String::new(),
                    },
                ));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    found.into_iter().map(|(_, version)| version).collect()
}

// 按分隔符切割，但值以引号开头时保留引号内的分号与换行。
fn split_env_entries(text: &str) -> Result<Vec<String>> {
    let mut entries = Vec::new();
    let mut current = String::new();
    let mut after_equals = false;
    let mut value_started = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in text.chars() {
        if let Some(active) = quote {
            current.push(ch);
            if escaped {
                escaped = false;
            } else if active == '"' && ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
                value_started = true;
            }
            continue;
        }
        if ch == ';' || ch == '\n' || ch == '\r' {
            entries.push(std::mem::take(&mut current));
            after_equals = false;
            value_started = false;
            escaped = false;
            continue;
        }
        current.push(ch);
        if !after_equals {
            if ch == '=' {
                after_equals = true;
            }
        } else if !value_started {
            // 仅值开头的引号表示包裹；值中间的引号按普通字符处理。
            if ch == '"' || ch == '\'' {
                quote = Some(ch);
            } else if ch != ' ' && ch != '\t' {
                value_started = true;
            }
        }
    }
    if quote.is_some() {
        return Err("环境变量引号未闭合".into());
    }
    entries.push(current);
    Ok(entries)
}

// 兼容 .env 风格外层引号：双引号内还原 \" 与 \\，单引号内容原样保留。
fn unquote_env_value(raw: &str) -> Result<String> {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let mut out = String::new();
        let mut chars = raw[1..raw.len() - 1].chars();
        while let Some(ch) = chars.next() {
            if ch != '\\' {
                out.push(ch);
                continue;
            }
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        Ok(out)
    } else if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        Ok(raw[1..raw.len() - 1].to_owned())
    } else {
        Ok(raw.to_owned())
    }
}

// 解析 KEY=VALUE;KEY2=VALUE2 或逐行输入；支持引号包裹的 JSON 等复杂值。
pub fn parse_env_vars(text: &str) -> Result<Vec<(String, String)>> {
    let mut result: Vec<(String, String)> = vec![];
    for entry in split_env_entries(text)? {
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
        let value = unquote_env_value(value.trim())?;
        if value.contains('\0') {
            return Err(format!("环境变量 {key} 的值包含无效空字符"));
        }
        if result.iter().any(|(existing, _)| existing == key) {
            return Err(format!("环境变量重复: {key}"));
        }
        result.push((key.to_owned(), value));
    }
    Ok(result)
}

pub fn validate_env_vars(text: &str) -> Result<()> {
    parse_env_vars(text).map(|_| ())
}

#[cfg(any(not(windows), test))]
fn env_prefix(text: &str) -> Result<String> {
    let vars = parse_env_vars(text)?;
    Ok(vars
        .iter()
        .map(|(key, value)| format!("export {key}={}", quote(value)))
        .collect::<Vec<_>>()
        .join(" && "))
}

#[cfg(any(not(windows), test))]
pub fn command_for(project: &Project, action: &Action) -> Result<String> {
    if action.kind != "shell" {
        return Ok(action.command.clone());
    }
    let env = env_prefix(&action.env_vars)?;
    let base = if project.project_type == "nodejs" {
        validate_project(project)?;
        match effective_manager(project)? {
            "nvm" => activation(
                &project.nvm_dir,
                &project.node_version,
                &action.command,
                &NVM_SCRIPTS.map(PathBuf::from),
            )?,
            "fnm" => {
                let node = selected_node(project, false)?.ok_or("未选择 Node 版本")?;
                if action.command.contains('\0') {
                    return Err("终端命令包含无效空字符".into());
                }
                let bin = node
                    .parent()
                    .and_then(Path::to_str)
                    .ok_or("Node 路径编码无效")?;
                // 只影响当前命令会话，不调用 fnm、不创建链接、不改全局选择。
                format!(
                    "test -x {} && export PATH={}:\"$PATH\" && eval {}",
                    quote(node.to_str().ok_or("Node 路径编码无效")?),
                    quote(bin),
                    quote(&action.command)
                )
            }
            _ => action.command.clone(),
        }
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

#[cfg(any(not(windows), test))]
fn activation(dir: &str, version: &str, command: &str, scripts: &[PathBuf]) -> Result<String> {
    validate_selection(version, dir)?;
    if version.is_empty() {
        return Err("未选择 Node 版本".into());
    }
    if command.contains('\0') {
        return Err("终端命令包含无效空字符".into());
    }
    if !Path::new(dir).is_absolute() {
        return Err("nvmDir 不是当前系统可用的绝对路径".into());
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

#[cfg(test)]
mod portable_tests {
    use super::*;

    #[test]
    fn unix_candidates_use_host_absolute_paths_and_preserve_priority() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let xdg = temp.path().join("xdg");
        let custom = temp.path().join("custom nvm");
        assert_eq!(
            candidate_roots(Some(custom.clone()), Some(home.clone()), Some(xdg.clone())),
            [
                custom,
                home.join(".nvm"),
                home.join(".config/nvm"),
                xdg.join("nvm")
            ]
        );
        assert!(candidate_roots(Some("relative".into()), None, None).is_empty());
    }

    #[test]
    fn fnm_roots_cover_environment_and_common_locations() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path();
        let roots = fnm_candidates(
            Some(base.join("custom")),
            Some(base.join("home")),
            Some(base.join("xdg")),
            Some(base.join("roaming")),
            Some(base.join("local")),
        );
        for suffix in [
            "custom",
            "home/Library/Application Support/fnm",
            "home/.local/share/fnm",
            "home/.fnm",
            "xdg/fnm",
            "roaming/fnm",
            "local/fnm",
        ] {
            assert!(roots.contains(&base.join(suffix)), "{suffix}");
        }
        assert!(fnm_candidates(Some("relative".into()), None, None, None, None).is_empty());
    }

    #[test]
    fn shell_environment_quotes_values_without_executing_them() {
        assert_eq!(quote("a'b;$(false)"), "'a'\\''b;$(false)'");
        assert_eq!(
            env_prefix("TOKEN=a'b;CONFIG=\"x;y\"").unwrap(),
            "export TOKEN='a'\\''b' && export CONFIG='x;y'"
        );
        assert!(env_prefix("BAD NAME=value").is_err());
    }
}

#[cfg(test)]
mod windows_tests {
    use super::*;

    fn project() -> Project {
        serde_json::from_value(serde_json::json!({"id":"node","name":"Node","type":"nodejs","path":"C:/项目","platforms":["web"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"PowerShell","commands":[]})).unwrap()
    }

    fn installed(root: &Path, version: &str) -> PathBuf {
        let dir = root.join(version);
        fs::create_dir_all(&dir).unwrap();
        // 故意写入不可执行内容；扫描和选择均不得运行它。
        fs::write(dir.join("node.exe"), "fake exe must never execute").unwrap();
        dir
    }

    #[test]
    fn windows_scan_sorts_normalizes_versions_and_ignores_unix_layout() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("nvm 中文 space");
        for version in [
            "v9.9.9",
            "v22.2.0",
            "22.10.0",
            "v01.0.0",
            "default",
            "v23.0.0-beta",
        ] {
            installed(&root, version);
        }
        fs::create_dir_all(root.join("v30.0.0/node.exe")).unwrap();
        fs::create_dir_all(root.join("v31.0.0")).unwrap();
        installed(&root.join("versions/node"), "v40.0.0");
        let found = scan_windows(vec![root.clone(), root.clone(), "relative".into()]);
        assert_eq!(
            found.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(),
            ["v22.10.0", "v22.2.0", "v9.9.9"]
        );
        assert!(found
            .iter()
            .all(|v| Path::new(&v.path).is_file() && v.path.contains("nvm 中文 space")));
        assert!(serde_json::to_value(&found[0]).unwrap()["nvmDir"].is_string());
        let mut selected = project();
        selected.node_version = "v22.10.0".into();
        selected.nvm_dir = root.to_string_lossy().into_owned();
        assert_eq!(
            windows_node_bin(&selected).unwrap(),
            Some(root.join("22.10.0"))
        );
        let preferred = installed(&root, "v22.10.0");
        assert_eq!(windows_node_bin(&selected).unwrap(), Some(preferred));
        for version in ["v31.0.0", "v30.0.0", "v99.0.0", "v22", "../v22.10.0"] {
            selected.node_version = version.into();
            assert!(windows_node_bin(&selected).is_err(), "{version}");
        }
        selected.node_version = "v22.10.0".into();
        selected.nvm_dir = "relative".into();
        assert!(windows_node_bin(&selected).is_err());
    }

    #[test]
    fn fnm_windows_layout_uses_installation_directory_without_nvm() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fnm");
        let bin = installed(&root.join("node-versions/v22.10.0"), "installation");
        let mut selected = project();
        selected.node_manager = "fnm".into();
        selected.node_version = "v22.10.0".into();
        selected.fnm_dir = root.to_string_lossy().into();
        let found = scan_fnm(vec![root.clone(), root], true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manager, "fnm");
        assert!(found[0].nvm_dir.is_empty());
        assert_eq!(windows_node_bin(&selected).unwrap(), Some(bin.clone()));
        fs::remove_file(bin.join("node.exe")).unwrap();
        assert!(windows_node_bin(&selected).is_err());
        selected.node_manager = "system".into();
        selected.node_version.clear();
        assert_eq!(windows_node_bin(&selected).unwrap(), None);
    }

    #[test]
    fn unselected_and_non_node_projects_return_none() {
        let mut selected = project();
        assert_eq!(windows_node_bin(&selected).unwrap(), None);
        selected.project_type = "backend".into();
        selected.node_version = "invalid".into();
        assert_eq!(windows_node_bin(&selected).unwrap(), None);
    }

    #[test]
    fn windows_roots_cover_environment_and_common_installations() {
        let temp = tempfile::tempdir().unwrap();
        let roots = windows_roots(
            Some(temp.path().join("custom nvm")),
            Some(temp.path().join("roaming")),
            Some(temp.path().join("local")),
            Some(temp.path().join("profile")),
            Some(temp.path().join("program files")),
        );
        for suffix in [
            "custom nvm",
            "roaming/nvm",
            "local/nvm",
            "profile/AppData/Roaming/nvm",
            "profile/scoop/apps/nvm/current",
            "program files/nvm",
        ] {
            assert!(roots.contains(&temp.path().join(suffix)), "{suffix}");
        }
        assert!(
            windows_roots(Some("relative".into()), None, None, None, None)
                .iter()
                .all(|p| p.is_absolute())
        );
        for dir in [r"C:\nvm data", r"\\server\share\nvm", "/nvm data"] {
            validate_selection("v22.10.0", dir).unwrap();
        }
        for dir in [r"C:relative", r"\nvm", "C:/bad\0"] {
            assert!(validate_selection("v22.10.0", dir).is_err());
        }
    }
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

    fn selected_fnm(root: &Path) -> Project {
        serde_json::from_value(serde_json::json!({"id":"p","name":"P","type":"nodejs","path":"/tmp/p","platforms":[],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[],"nodeManager":"fnm","nodeVersion":"v22.10.0","fnmDir":root})).unwrap()
    }

    fn installed_fnm(root: &Path, version: &str) -> PathBuf {
        let node = root
            .join("node-versions")
            .join(version)
            .join("installation/bin/node");
        fs::create_dir_all(node.parent().unwrap()).unwrap();
        fs::write(&node, "fake node must never execute").unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        node
    }

    #[test]
    fn fnm_scan_manual_directory_dedup_sort_and_manager_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fnm space ' $(false)");
        for version in ["v9.9.9", "v22.10.0", "v22.2.0", "v01.0.0", "v23.0.0-beta"] {
            installed_fnm(&root, version);
        }
        let disabled = installed_fnm(&root, "v30.0.0");
        fs::set_permissions(disabled, fs::Permissions::from_mode(0o644)).unwrap();
        let alias = temp.path().join("alias");
        symlink(&root, &alias).unwrap();
        let found = scan_fnm(vec![root.clone(), alias], false);
        assert_eq!(
            found.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(),
            ["v22.10.0", "v22.2.0", "v9.9.9"]
        );
        assert!(found
            .iter()
            .all(|v| v.manager == "fnm" && v.nvm_dir.is_empty() && !v.fnm_dir.is_empty()));
        assert_eq!(
            detect_versions(Some("fnm"), root.to_str()).unwrap().len(),
            3
        );
        installed(&root, "v22.10.0");
        fs::write(root.join("nvm.sh"), "never source").unwrap();
        let mut both = detect_versions(Some("nvm"), root.to_str()).unwrap();
        both.extend(found);
        sort_versions(&mut both);
        assert_eq!(
            both.iter()
                .filter(|v| v.version == "v22.10.0")
                .map(|v| v.manager.as_str())
                .collect::<Vec<_>>(),
            ["fnm", "nvm"]
        );
        assert!(detect_versions(Some("system"), None).unwrap().is_empty());
        for (manager, dir) in [
            (Some("bad"), None),
            (Some("fnm"), Some("relative")),
            (None, root.to_str()),
        ] {
            assert!(detect_versions(manager, dir).is_err());
        }
        assert!(detect_versions(Some("fnm"), Some("/missing-synthetic-fnm"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn shared_settings_require_explicit_mode_and_portable_active_root_only() {
        let mut settings = Settings::default();
        validate_settings(&settings).unwrap();
        for manager in ["nvm", "fnm"] {
            settings.node_manager = manager.into();
            assert!(validate_settings(&settings).is_err());
            settings.nvm_dir = "/not-installed/nvm".into();
            settings.fnm_dir = r"C:\not-installed\fnm".into();
            validate_settings(&settings).unwrap();
            settings.nvm_dir.clear();
            settings.fnm_dir.clear();
        }
        settings.node_manager = "system".into();
        validate_settings(&settings).unwrap();
        settings.nvm_dir = "relative".into();
        assert!(validate_settings(&settings).is_err());
        settings.nvm_dir.clear();
        for manager in ["inherit", "unknown", " nvm"] {
            settings.node_manager = manager.into();
            assert!(validate_settings(&settings).is_err());
        }
    }

    #[test]
    fn inherit_imports_without_shared_settings_but_cannot_execute_unresolved() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = selected_fnm(temp.path());
        project.node_manager = "inherit".into();
        project.platforms = vec!["web".into()];
        project.fnm_dir.clear();
        let settings = Settings::default();
        validate_project(&project).unwrap();
        validate_project_config(&project, &settings).unwrap();
        let config = crate::config::AppConfig {
            settings: settings.clone(),
            projects: vec![project.clone()],
            ..Default::default()
        };
        crate::config::parse(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(
            resolve_project(&project, &settings).unwrap_err(),
            "请在偏好设置配置公共Node环境"
        );
        assert!(selected_node(&project, false).is_err());
        let action: Action = serde_json::from_value(serde_json::json!({"id":"a","name":"A","kind":"shell","platform":"web","menuPath":[],"command":"npm run dev"})).unwrap();
        assert!(command_for(&project, &action).is_err());
        project.node_version = "v22".into();
        assert!(validate_project_config(&project, &settings).is_err());
    }

    #[test]
    fn shared_provider_selects_same_exact_version_without_changing_legacy_project() {
        let temp = tempfile::tempdir().unwrap();
        let nvm_root = temp.path().join("nvm");
        let fnm_root = temp.path().join("fnm");
        let nvm_node = installed(&nvm_root, "v22.10.0");
        let fnm_node = installed_fnm(&fnm_root, "v22.10.0");
        let project = selected_fnm(&fnm_root);
        let before = serde_json::to_value(&project).unwrap();
        let mut settings = Settings::default();
        let legacy = resolve_project(&project, &settings).unwrap();
        assert_eq!(serde_json::to_value(legacy).unwrap(), before);
        settings.nvm_dir = nvm_root.to_string_lossy().into();
        settings.fnm_dir = fnm_root.to_string_lossy().into();
        for (manager, expected) in [("nvm", nvm_node), ("fnm", fnm_node.clone())] {
            settings.node_manager = manager.into();
            let resolved = resolve_project(&project, &settings).unwrap();
            assert_eq!(selected_node(&resolved, false).unwrap(), Some(expected));
            assert_eq!(serde_json::to_value(&project).unwrap(), before);
        }
        fs::remove_file(fnm_node).unwrap();
        let resolved = resolve_project(&project, &settings).unwrap();
        assert!(selected_node(&resolved, false).is_err()); // 不借用另一提供者同版本。
        settings.node_manager.clear();
        let mut old_nvm = project.clone();
        old_nvm.node_manager.clear();
        old_nvm.nvm_dir = nvm_root.to_string_lossy().into();
        let resolved = resolve_project(&old_nvm, &settings).unwrap();
        assert_eq!(
            selected_node(&resolved, false).unwrap(),
            Some(nvm_root.join("versions/node/v22.10.0/bin/node"))
        );
    }

    #[test]
    fn shared_defaults_and_non_node_projects_do_not_activate_node() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = selected_fnm(temp.path());
        let mut settings = Settings::default();
        for manager in ["", "system", "nvm", "fnm"] {
            settings.node_manager = manager.into();
            settings.nvm_dir = temp.path().join("missing-nvm").to_string_lossy().into();
            settings.fnm_dir = temp.path().join("missing-fnm").to_string_lossy().into();
            project.node_version.clear();
            let resolved = resolve_project(&project, &settings).unwrap();
            assert_eq!(effective_manager(&resolved).unwrap(), "system");
            assert!(selected_node(&resolved, false).unwrap().is_none());
        }
        project.node_version = "v22.10.0".into();
        settings.node_manager = "system".into();
        assert!(resolve_project(&project, &settings)
            .unwrap()
            .node_version
            .is_empty());
        project.project_type = "uniapp".into();
        settings.node_manager = "invalid".into();
        let before = serde_json::to_value(&project).unwrap();
        assert_eq!(
            serde_json::to_value(resolve_project(&project, &settings).unwrap()).unwrap(),
            before
        );
    }

    #[test]
    fn project_validation_is_portable_and_default_never_activates() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = selected_fnm(temp.path());
        validate_project(&project).unwrap(); // 不要求安装存在。
        assert!(selected_node(&project, false).is_err());
        for manager in ["unknown", "system", "nvm"] {
            project.node_manager = manager.into();
            assert!(validate_project(&project).is_err());
        }
        project.node_manager = "fnm".into();
        for version in ["v22", "v22.10.0;false", "v01.0.0", "../v22.10.0"] {
            project.node_version = version.into();
            assert!(validate_project(&project).is_err());
        }
        project.node_version.clear();
        validate_project(&project).unwrap();
        assert!(selected_node(&project, false).unwrap().is_none());
        project.node_manager.clear();
        project.fnm_dir.clear();
        project.nvm_dir = temp.path().join("not installed").to_string_lossy().into();
        validate_project(&project).unwrap();
        project.node_manager = "system".into();
        let action: Action = serde_json::from_value(serde_json::json!({"id":"a","name":"A","kind":"shell","platform":"web","menuPath":[],"command":"printf 'a';\nprintf b"})).unwrap();
        assert_eq!(command_for(&project, &action).unwrap(), action.command);
        project.node_manager = "bad".into();
        project.project_type = "backend".into();
        assert_eq!(command_for(&project, &action).unwrap(), action.command);
    }

    #[test]
    fn fnm_activation_quotes_command_and_guards_entire_command_without_running_node() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fnm ' $(false)");
        let node = installed_fnm(&root, "v22.10.0");
        let project = selected_fnm(&root);
        let marker = temp.path().join("ran");
        let mut action: Action = serde_json::from_value(serde_json::json!({"id":"a","name":"A","kind":"shell","platform":"web","menuPath":[],"command":"", "envVars":"VALUE='a;b'"})).unwrap();
        action.command = format!(
            "test \"$VALUE\" = 'a;b'; printf 'quoted' > {}\nprintf done",
            quote(marker.to_str().unwrap())
        );
        let command = command_for(&project, &action).unwrap();
        assert!(command.starts_with("export VALUE='a;b' && test -x "));
        assert!(!command.contains("fnm use"));
        for shell in ["/bin/bash", "/bin/zsh"] {
            let output = Command::new(shell).args(["-c", &command]).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(fs::read_to_string(&marker).unwrap(), "quoted");
            fs::remove_file(&marker).unwrap();
            fs::set_permissions(&node, fs::Permissions::from_mode(0o644)).unwrap();
            assert!(command_for(&project, &action).is_err());
            assert!(!Command::new(shell)
                .args(["-c", &command])
                .status()
                .unwrap()
                .success());
            assert!(!marker.exists());
            fs::set_permissions(&node, fs::Permissions::from_mode(0o755)).unwrap();
        }
        action.command = "bad\0command".into();
        assert!(command_for(&project, &action).is_err());
        fs::remove_file(node).unwrap();
        assert!(command_for(&project, &action).is_err());
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
                "AI_ALERT_EMAIL_ENABLED=false;ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;ALERT_EMAIL_SMTP_HOST=smtp.qq.com;"
            )
            .unwrap(),
            vec![
                ("AI_ALERT_EMAIL_ENABLED".into(), "false".into()),
                ("ALERT_EMAIL_FROM".into(), "示例发件人 <sender@example.com>".into()),
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
    fn env_vars_support_quoted_json_values_with_semicolons() {
        let text = r#"AI_CONFIG="[{\"name\":\"weixin\",\"apiKey\":\"a;b\"}]";OTHER=1"#;
        let vars = parse_env_vars(text).unwrap();
        assert_eq!(vars[0].0, "AI_CONFIG");
        assert_eq!(vars[0].1, r#"[{"name":"weixin","apiKey":"a;b"}]"#);
        assert_eq!(vars[1], ("OTHER".into(), "1".into()));
        let vars = parse_env_vars(r#"CONFIG='[{"a":"b;c"}]'; X=2"#).unwrap();
        assert_eq!(vars[0].1, r#"[{"a":"b;c"}]"#);
        assert_eq!(vars[1], ("X".into(), "2".into()));
        // 引号内的换行不拆分，未闭合引号明确报错。
        assert_eq!(
            parse_env_vars("A=\"line1\nline2\"").unwrap()[0].1,
            "line1\nline2"
        );
        assert!(validate_env_vars(r#"A="unterminated"#).is_err());
        assert!(validate_env_vars(r#"A='unterminated"#).is_err());
        // 引号内的分号仍作为值的一部分，执行时单引号安全引用。
        let mut action: Action = serde_json::from_value(serde_json::json!({"id":"dev","name":"Dev","platform":"web","kind":"shell","menuPath":[],"command":"node app.js"})).unwrap();
        action.env_vars = text.into();
        let project: Project = serde_json::from_value(serde_json::json!({"id":"p","name":"P","type":"backend","path":"/tmp/p","platforms":[],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]})).unwrap();
        let command = command_for(&project, &action).unwrap();
        assert!(command.contains(r#"AI_CONFIG='[{"name":"weixin","apiKey":"a;b"}]'"#));
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
        action.env_vars = "ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;TOKEN=a'b".into();
        let command = command_for(&project, &action).unwrap();
        assert!(command.starts_with(
            "export ALERT_EMAIL_FROM='示例发件人 <sender@example.com>' && export TOKEN='a'\\''b' && test -x "
        ));
        assert!(command.contains("nvm use --silent 'v22.10.0' && eval 'npm run dev'"));
        // 未配置环境变量时保持原命令；菜单动作不注入环境变量。
        action.env_vars.clear();
        assert!(command_for(&project, &action)
            .unwrap()
            .starts_with("test -x "));
        action.env_vars = "A=1".into();
        action.kind = "menu".into();
        assert_eq!(command_for(&project, &action).unwrap(), "npm run dev");
        // 不同命令各自使用自己的环境变量，非 Node 项目的 shell 命令同样生效。
        project.project_type = "backend".into();
        action.kind = "shell".into();
        assert_eq!(
            command_for(&project, &action).unwrap(),
            "export A='1' && npm run dev"
        );
        let mut other = action.clone();
        other.command = "npm test".into();
        other.env_vars = "NODE_ENV=test".into();
        assert_eq!(
            command_for(&project, &other).unwrap(),
            "export NODE_ENV='test' && npm test"
        );
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
