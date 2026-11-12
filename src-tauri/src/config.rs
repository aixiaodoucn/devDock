use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub hbuilderx_path: String,
    pub terminal: String,
    pub jdk_path: String,
    // 仅新增字段提供默认值，保留旧字段的严格必填校验。
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_open_output_after_build")]
    pub open_output_after_build: bool,
    // 保留 aiTools/defaultAiToolId 字段兼容旧配置，现承载通用终端命令。
    #[serde(default)]
    pub ai_tools: Vec<AiTool>,
    #[serde(default)]
    pub default_ai_tool_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiTool {
    pub id: String,
    pub name: String,
    pub command: String,
    pub enabled: bool,
}
impl AiTool {
    pub fn validate(&self) -> Result<()> {
        if [&self.id, &self.name, &self.command]
            .iter()
            .any(|value| value.trim().is_empty() || value.contains('\0'))
        {
            return Err("终端命令 id/name/command 不能为空或含 NUL".into());
        }
        Ok(())
    }
}
pub fn validate_ai_tools(tools: &[AiTool]) -> Result<()> {
    let mut ids = HashSet::new();
    for tool in tools {
        tool.validate()?;
        if !ids.insert(&tool.id) {
            return Err("终端命令 id 重复".into());
        }
    }
    Ok(())
}
fn default_theme() -> String {
    "lavender".into()
}
fn default_open_output_after_build() -> bool {
    true
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hbuilderx_path: String::new(),
            terminal: String::new(),
            jdk_path: String::new(),
            theme: default_theme(),
            open_output_after_build: default_open_output_after_build(),
            ai_tools: vec![],
            default_ai_tool_id: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub menu_path: Vec<String>,
    pub command: String,
    // 每条终端命令可独立配置环境变量；旧配置默认为空。
    #[serde(default)]
    pub env_vars: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub project_type: String,
    pub path: String,
    pub platforms: Vec<String>,
    pub manifest_path: String,
    pub android_app_id: String,
    pub ios_app_id: String,
    pub android_project_path: String,
    pub android_module: String,
    pub jdk_path: String,
    pub gradle_task: String,
    pub output_dir: String,
    pub sync_main_version: bool,
    pub terminal: String,
    // 保留 aiToolId 兼容字段，选择通用终端命令；空值跟随全局默认，允许悬空引用。
    #[serde(default)]
    pub ai_tool_id: String,
    // 老配置和备份缺少选择时，继续使用终端自己的默认 Node。
    #[serde(default)]
    pub node_version: String,
    #[serde(default)]
    pub nvm_dir: String,
    // 每个项目可配置终端命令使用的环境变量，旧配置默认为空。
    #[serde(default)]
    pub env_vars: String,
    pub commands: Vec<Action>,
    // 后端项目组只保存仓库元数据，旧配置默认不包含子项目或 SCM 映射。
    #[serde(default)]
    pub sub_projects: Vec<crate::backend_git::Repository>,
    #[serde(default)]
    pub scm_development_project_id: String,
    #[serde(default)]
    pub scm_test_project_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppConfig {
    pub schema_version: u32,
    pub projects: Vec<Project>,
    pub settings: Settings,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            projects: vec![],
            settings: Settings {
                terminal: if cfg!(windows) {
                    "PowerShell"
                } else {
                    "Terminal"
                }
                .into(),
                ..Default::default()
            },
        }
    }
}
fn terminal(t: &str) -> Result<()> {
    if !["", "Terminal", "WindowsTerminal", "PowerShell"].contains(&t)
        && !crate::paths::is_absolute(t)
    {
        return Err("终端必须是 Terminal、WindowsTerminal、PowerShell 或应用绝对路径".into());
    }
    Ok(())
}
fn canonical_platform(platform: &str) -> &str {
    match platform {
        "pc" => "web",
        "weixin" | "dingtalk" => "miniapp",
        _ => platform,
    }
}
impl AppConfig {
    // 返回通过校验的规范化副本，兼容旧对象且不修改调用方数据。
    pub fn validate(&self) -> Result<Self> {
        let mut normalized = self.clone();
        for project in &mut normalized.projects {
            // 先拒绝重复原始选项，再合并旧分类；不能用去重掩盖无效输入。
            let mut originals = HashSet::new();
            if project
                .platforms
                .iter()
                .any(|platform| !originals.insert(platform))
            {
                return Err("项目平台为空、重复或不适用于当前项目类型".into());
            }
            let mut platforms = Vec::new();
            for platform in &project.platforms {
                let canonical = canonical_platform(platform).to_owned();
                if !platforms.contains(&canonical) {
                    platforms.push(canonical);
                }
            }
            project.platforms = platforms;
            // 旧版项目级环境变量迁移到该项目每条终端命令，避免升级后丢失。
            let legacy_env = project.env_vars.trim().to_owned();
            if !legacy_env.is_empty()
                && project.commands.iter().any(|action| action.kind == "shell")
            {
                for action in &mut project.commands {
                    if action.kind == "shell" && action.env_vars.is_empty() {
                        action.env_vars = legacy_env.clone();
                    }
                }
                project.env_vars.clear();
            }
            // 动作只映射所属分类，不合并动作，也不改写名称、命令或菜单。
            for action in &mut project.commands {
                action.platform = canonical_platform(&action.platform).to_owned();
            }
        }
        normalized.validate_canonical()?;
        Ok(normalized)
    }

    fn validate_canonical(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err("不支持的 schemaVersion".into());
        }
        if !["lavender", "blue", "cyan", "rose", "amber", "slate"]
            .contains(&self.settings.theme.as_str())
        {
            return Err("无效的 theme：仅支持 lavender、blue、cyan、rose、amber 或 slate".into());
        }
        terminal(&self.settings.terminal)?;
        validate_ai_tools(&self.settings.ai_tools)?;
        if self.settings.default_ai_tool_id.contains('\0') {
            return Err("defaultAiToolId 不能含 NUL".into());
        }
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for p in &self.projects {
            if p.id.trim().is_empty() || p.name.trim().is_empty() || !ids.insert(&p.id) {
                return Err("项目 id/name 为空或 id 重复".into());
            }
            if !["uniapp", "nodejs", "backend"].contains(&p.project_type.as_str())
                || !crate::paths::is_absolute(&p.path)
            {
                return Err("项目类型或绝对路径无效".into());
            }
            // 按来源平台比较路径，不要求备份中的目录在当前宿主存在。
            let normalized = crate::paths::key(&p.path);
            if !paths.insert(normalized) {
                return Err("项目目录重复".into());
            }
            let allowed: &[&str] = match p.project_type.as_str() {
                "uniapp" => &["app", "web", "miniapp"],
                "nodejs" => &["web", "miniapp"],
                _ => &[],
            };
            let mut platforms = HashSet::new();
            if (p.project_type != "backend" && p.platforms.is_empty())
                || p.platforms
                    .iter()
                    .any(|x| !allowed.contains(&x.as_str()) || !platforms.insert(x))
            {
                return Err("项目平台为空、重复或不适用于当前项目类型".into());
            }
            if p.project_type != "backend"
                && (!p.sub_projects.is_empty()
                    || !p.scm_development_project_id.is_empty()
                    || !p.scm_test_project_id.is_empty())
            {
                return Err("非后端项目不能包含子项目或 SCM 项目配置".into());
            }
            for id in [&p.scm_development_project_id, &p.scm_test_project_id] {
                if !id.bytes().all(|ch| ch.is_ascii_digit()) {
                    return Err("SCM 项目 ID 必须为空或纯数字".into());
                }
            }
            let mut repository_ids = HashSet::new();
            let mut repository_paths = HashSet::new();
            for repository in &p.sub_projects {
                if [
                    &repository.id,
                    &repository.name,
                    &repository.path,
                    &repository.remote,
                    &repository.scm_git_id,
                    &repository.scm_module_name,
                    &repository.development_branch,
                    &repository.test_branch,
                ]
                .iter()
                .any(|value| value.contains('\0'))
                {
                    return Err("子项目字段不能含 NUL".into());
                }
                if repository.id.trim().is_empty() || repository.name.trim().is_empty() {
                    return Err("子项目 id/name 不能为空".into());
                }
                if !repository_ids.insert(&repository.id) {
                    return Err("同组子项目 id 重复".into());
                }
                if !crate::paths::is_absolute(&repository.path) {
                    return Err("子项目必须使用完整的绝对路径".into());
                }
                if !repository_paths.insert(crate::paths::key(&repository.path)) {
                    return Err("同组子项目目录重复".into());
                }
            }
            terminal(&p.terminal)?;
            if p.ai_tool_id.contains('\0') {
                return Err("aiToolId 不能含 NUL".into());
            }
            crate::nvm::validate_selection(&p.node_version, &p.nvm_dir)?;
            let mut actions = HashSet::new();
            for a in &p.commands {
                if a.id.is_empty()
                    || a.name.is_empty()
                    || !actions.insert(&a.id)
                    || !["menu", "shell"].contains(&a.kind.as_str())
                    || !p.platforms.contains(&a.platform)
                    || (p.project_type == "nodejs" && a.kind != "shell")
                    || (a.kind == "menu"
                        && (a.menu_path.len() < 2
                            || !["运行", "发行"].contains(&a.menu_path[0].as_str())
                            || a.menu_path.iter().any(|s| s.trim().is_empty())))
                    || (a.kind == "shell" && a.command.trim().is_empty())
                {
                    return Err("无效的动作字段".into());
                }
                crate::nvm::validate_env_vars(&a.env_vars)?;
            }
        }
        Ok(())
    }
}
pub fn parse(text: &str) -> Result<AppConfig> {
    let c: AppConfig = serde_json::from_str(text).map_err(|e| e.to_string())?;
    c.validate()
}
// 保存和备份导出共用此入口，避免 validate 的兼容副本被误当作持久化结果。
pub fn serialize(config: &AppConfig) -> Result<String> {
    let normalized = config.validate()?;
    serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())
}
pub fn atomic_write(path: &Path, data: &str) -> Result<()> {
    let parent = path.parent().ok_or("路径无父目录")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".dev-dock-{}.tmp", uuid::Uuid::new_v4()));
    // 保留被替换文件的权限，避免临时文件默认权限改变源码或配置文件属性。
    let permissions = std::fs::metadata(path).ok().map(|m| m.permissions());
    use std::io::Write;
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        if let Some(permissions) = permissions {
            std::fs::set_permissions(&temp, permissions).map_err(|e| e.to_string())?;
        }
        // Windows 替换前关闭写入句柄；rename 失败（如目标被占用）时保留原文件，
        // 不使用先删除目标再重命名的非原子回退。
        drop(f);
        std::fs::rename(&temp, path)
            .map_err(|e| format!("无法原子替换 {}: {e}", path.display()))?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        // Windows 无法删除继承了只读属性的临时文件；只解除临时副本的只读位。
        #[cfg(windows)]
        if let Ok(metadata) = std::fs::metadata(&temp) {
            let mut permissions = metadata.permissions();
            if permissions.readonly() {
                permissions.set_readonly(false);
                let _ = std::fs::set_permissions(&temp, permissions);
            }
        }
        let _ = std::fs::remove_file(temp);
    }
    result
}
// 调用方须先取得新版实例锁；返回的旧版锁应持有到应用退出，避免两个版本同时修改项目。
pub fn migrate_legacy(old_dir: &Path, new_dir: &Path) -> Result<Option<std::fs::File>> {
    use std::fs;
    let old_config = old_dir.join("config.json");
    if !old_config.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(old_dir.join("instance.lock"))
        .map_err(|e| format!("无法取得旧版数据锁: {e}"))?;
    lock.try_lock()
        .map_err(|e| format!("请先退出旧版 AI Dev，再启动 DevDock；旧版数据正在使用: {e}"))?;
    let new_config = new_dir.join("config.json");
    if new_config.try_exists().map_err(|e| e.to_string())? {
        return Ok(Some(lock));
    }
    let text = fs::read_to_string(&old_config).map_err(|e| format!("读取旧版配置失败: {e}"))?;
    let config = parse(&text).map_err(|e| format!("旧版配置无效，未迁移: {e}"))?;
    let text = serde_json::to_string(&config).map_err(|e| e.to_string())?;
    let pending = old_dir.join("pending");
    let mut records = vec![];
    if pending.try_exists().map_err(|e| e.to_string())? {
        for entry in fs::read_dir(pending).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let data = fs::read_to_string(entry.path()).map_err(|e| e.to_string())?;
            let record: crate::manifest::Recovery = serde_json::from_str(&data)
                .map_err(|e| format!("旧版恢复记录无效，未迁移: {e}"))?;
            if !crate::paths::is_absolute(&record.path) {
                return Err("旧版恢复记录路径无效，未迁移".into());
            }
            let target = new_dir.join("pending").join(entry.file_name());
            // 中断后可重试相同记录，但不能覆盖新版已有的不同恢复记录。
            if target.try_exists().map_err(|e| e.to_string())?
                && fs::read_to_string(&target).map_err(|e| e.to_string())? != data
            {
                return Err(format!("恢复记录冲突，未迁移: {}", target.display()));
            }
            records.push((target, data));
        }
    }
    // 全部校验后复制，配置最后落盘作为完成标记；旧数据始终保留。
    for (target, data) in records {
        atomic_write(&target, &data)?;
    }
    atomic_write(&new_config, &text)?;
    Ok(Some(lock))
}
pub fn manifest_path(p: &Project) -> Result<PathBuf> {
    let root = Path::new(&p.path);
    let explicit = Path::new(&p.manifest_path);
    // 可导入的跨平台路径不能直接用于宿主 I/O，也不能被拼接成相对路径。
    if !crate::paths::is_absolute(&p.path) || !root.is_absolute() {
        return Err("项目路径不是当前系统可用的绝对路径".into());
    }
    if p.manifest_path.contains('\0')
        || (crate::paths::is_absolute(&p.manifest_path) && !explicit.is_absolute())
        || (!p.manifest_path.is_empty()
            && !explicit.is_absolute()
            && (p.manifest_path.starts_with('\\')
                || p.manifest_path.as_bytes().get(1) == Some(&b':')))
    {
        return Err("manifest 路径不适用于当前系统".into());
    }
    let paths = if p.manifest_path.is_empty() {
        vec![root.join("manifest.json"), root.join("src/manifest.json")]
    } else {
        vec![if explicit.is_absolute() {
            explicit.to_path_buf()
        } else {
            root.join(explicit)
        }]
    };
    let found = paths
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("找不到 manifest.json")?;
    std::fs::canonicalize(found).map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn backend_fixture() -> serde_json::Value {
        let mut value = platform_fixture();
        value["projects"][0]["type"] = "backend".into();
        value["projects"][0]["platforms"] = serde_json::json!([]);
        value["projects"][0]["commands"] = serde_json::json!([]);
        value
    }
    #[test]
    fn backend_legacy_defaults_and_metadata_roundtrip() {
        let old = parse(&backend_fixture().to_string()).unwrap();
        let mut value = serde_json::to_value(old).unwrap();
        assert_eq!(value["projects"][0]["subProjects"], serde_json::json!([]));
        assert_eq!(value["projects"][0]["scmDevelopmentProjectId"], "");
        assert_eq!(value["projects"][0]["scmTestProjectId"], "");
        for path in ["/tmp/repo", r"C:\repo", r"\\server\share\repo"] {
            value["projects"][0]["subProjects"] =
                serde_json::json!([{"id":"child","name":"服务","path":path}]);
            value["projects"][0]["scmDevelopmentProjectId"] = "00123".into();
            let parsed = parse(&value.to_string()).unwrap();
            let output: serde_json::Value =
                serde_json::from_str(&serialize(&parsed).unwrap()).unwrap();
            assert_eq!(output["schemaVersion"], 1);
            assert_eq!(output["projects"][0]["subProjects"][0]["remote"], "origin");
            assert_eq!(output["projects"][0]["subProjects"][0]["scmGitId"], "");
            assert_eq!(
                output,
                serde_json::to_value(parse(&output.to_string()).unwrap()).unwrap()
            );
        }
    }
    #[test]
    fn backend_rejects_invalid_metadata_duplicates_and_non_backend_config() {
        let repo = serde_json::json!({"id":"child","name":"服务","path":"C:/repo","remote":"origin","scmGitId":"","scmModuleName":"","developmentBranch":"","testBranch":""});
        let mut value = backend_fixture();
        value["projects"][0]["subProjects"] = serde_json::json!([repo.clone()]);
        assert!(parse(&value.to_string()).is_ok());
        for field in [
            "id",
            "name",
            "path",
            "remote",
            "scmGitId",
            "scmModuleName",
            "developmentBranch",
            "testBranch",
        ] {
            let mut invalid = value.clone();
            invalid["projects"][0]["subProjects"][0][field] = "bad\0value".into();
            assert!(parse(&invalid.to_string()).is_err(), "{field}");
        }
        for (field, text) in [("id", " "), ("name", ""), ("path", "relative")] {
            let mut invalid = value.clone();
            invalid["projects"][0]["subProjects"][0][field] = text.into();
            assert!(parse(&invalid.to_string()).is_err(), "{field}");
        }
        for (id, path) in [("other", "c:/REPO/"), ("child", "C:/other")] {
            let mut invalid = value.clone();
            let mut other = repo.clone();
            other["id"] = id.into();
            other["path"] = path.into();
            invalid["projects"][0]["subProjects"]
                .as_array_mut()
                .unwrap()
                .push(other);
            assert!(parse(&invalid.to_string()).is_err());
        }
        for field in ["scmDevelopmentProjectId", "scmTestProjectId"] {
            for text in ["-1", "1a", "１２", " 1", "x\0"] {
                let mut invalid = value.clone();
                invalid["projects"][0][field] = text.into();
                assert!(parse(&invalid.to_string()).is_err());
            }
        }
        value["projects"][0]["type"] = "nodejs".into();
        value["projects"][0]["platforms"] = serde_json::json!(["web"]);
        assert!(parse(&value.to_string()).is_err());
        value["projects"][0]["subProjects"] = serde_json::json!([]);
        value["projects"][0]["scmTestProjectId"] = "1".into();
        assert!(parse(&value.to_string()).is_err());
    }
    #[test]
    fn backend_does_not_echo_credentials() {
        let mut value = backend_fixture();
        value["projects"][0]["subProjects"] = serde_json::json!([{"id":"r","name":"repo","path":"/tmp/repo","password":"secret","cookie":"session=secret"}]);
        match parse(&value.to_string()) {
            Ok(config) => {
                let output = serialize(&config).unwrap();
                assert!(!output.contains("secret"));
                assert!(!output.contains("password"));
                assert!(!output.contains("cookie"));
            }
            Err(error) => assert!(!error.contains("secret")),
        }
    }
    #[test]
    fn portable_paths_terminals_and_duplicate_validation() {
        for path in [
            r"C:\项目 空格",
            "C:/项目",
            r"\\server\share\项目",
            "/tmp/项目",
        ] {
            for terminal in [
                "",
                "Terminal",
                "WindowsTerminal",
                "PowerShell",
                r"C:\Program Files\pwsh.exe",
                "/bin/zsh",
                r"\\server\share\terminal.exe",
            ] {
                let mut value = platform_fixture();
                value["projects"][0]["path"] = path.into();
                value["projects"][0]["terminal"] = terminal.into();
                value["projects"][0]["nodeVersion"] = "v22.10.0".into();
                value["projects"][0]["nvmDir"] = r"C:\nvm data".into();
                value["settings"]["terminal"] = terminal.into();
                let parsed = parse(&value.to_string()).unwrap();
                assert_eq!(parsed.projects[0].path, path);
                assert_eq!(parsed.settings.terminal, terminal);
                parse(&serialize(&parsed).unwrap()).unwrap();
            }
        }
        for invalid in [
            "wt",
            "cmd",
            "unknown",
            "C:relative",
            r"\path",
            "PowerShell\0",
            "C:/bad\0",
        ] {
            for field in ["path", "terminal"] {
                let mut value = platform_fixture();
                value["projects"][0][field] = invalid.into();
                assert!(parse(&value.to_string()).is_err(), "{field}: {invalid}");
            }
            let mut value = platform_fixture();
            value["settings"]["terminal"] = invalid.into();
            assert!(parse(&value.to_string()).is_err(), "{invalid}");
        }
        for (first, second, duplicate) in [
            (r"C:\项目\Foo", "c:/项目/foo/", true),
            (r"\\SERVER\Share\Project", "//server/share/project/", true),
            ("/tmp/Project", "/tmp/project", false),
            ("/tmp/Project", "/tmp//Project/./", true),
        ] {
            let mut value = platform_fixture();
            value["projects"][0]["path"] = first.into();
            let mut other = value["projects"][0].clone();
            other["id"] = "other".into();
            other["path"] = second.into();
            value["projects"].as_array_mut().unwrap().push(other);
            assert_eq!(
                parse(&value.to_string()).is_err(),
                duplicate,
                "{first} / {second}"
            );
        }
        assert_eq!(
            AppConfig::default().settings.terminal,
            if cfg!(windows) {
                "PowerShell"
            } else {
                "Terminal"
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn manifest_rejects_foreign_paths_before_accessing_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = parse(&platform_fixture().to_string())
            .unwrap()
            .projects
            .remove(0);
        project.path = temp.path().to_string_lossy().into_owned();
        std::fs::write(temp.path().join("manifest.json"), "{}").unwrap();
        assert!(manifest_path(&project).unwrap().is_absolute());
        for foreign in [
            r"C:\project\manifest.json",
            "C:/project/manifest.json",
            r"\\server\share\manifest.json",
            r"C:manifest.json",
            r"\manifest.json",
        ] {
            project.manifest_path = foreign.into();
            assert!(manifest_path(&project).unwrap_err().contains("当前系统"));
        }
        project.manifest_path.clear();
        project.path = r"C:\project".into();
        assert!(manifest_path(&project).unwrap_err().contains("当前系统"));
    }

    #[test]
    fn recovery_migration_accepts_portable_paths_and_rejects_invalid_paths() {
        for path in [
            r"C:\项目 空格\manifest.json",
            r"\\server\share\manifest.json",
            "/tmp/manifest.json",
            "C:relative",
            r"\path",
            "C:/bad\0",
        ] {
            let old = tempfile::tempdir().unwrap();
            let new = tempfile::tempdir().unwrap();
            let record =
                serde_json::json!({"path":path,"original":"old","temporary":"new"}).to_string();
            atomic_write(
                &old.path().join("config.json"),
                &serialize(&AppConfig::default()).unwrap(),
            )
            .unwrap();
            atomic_write(&old.path().join("pending/task.json"), &record).unwrap();
            let migrated = migrate_legacy(old.path(), new.path());
            assert_eq!(migrated.is_ok(), crate::paths::is_absolute(path), "{path}");
            if migrated.is_ok() {
                assert_eq!(
                    std::fs::read_to_string(new.path().join("pending/task.json")).unwrap(),
                    record
                );
            } else {
                assert!(!new.path().join("config.json").exists());
            }
        }
    }

    #[test]
    fn atomic_replace_existing_file_and_clean_up_after_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data.json");
        atomic_write(&path, "old").unwrap();
        atomic_write(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let directory = temp.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        assert!(atomic_write(&directory, "replacement").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn atomic_replace_readonly_file_cleans_temporary_copy() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("readonly.json");
        std::fs::write(&path, "old").unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(atomic_write(&path, "new").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn atomic_replace_occupied_file_preserves_original() {
        use std::os::windows::fs::OpenOptionsExt;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("occupied.json");
        std::fs::write(&path, "old").unwrap();
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&path)
            .unwrap();
        assert!(atomic_write(&path, "new").is_err());
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    fn platform_fixture() -> serde_json::Value {
        serde_json::json!({"schemaVersion":1,"projects":[{
            "id":"uni","name":"旧项目","type":"uniapp","path":"/tmp/platform-fixture",
            "platforms":["weixin","dingtalk"],"manifestPath":"","androidAppId":"","iosAppId":"",
            "androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"",
            "outputDir":"","syncMainVersion":false,"terminal":"Terminal",
            "commands":[
                {"id":"weixin-dev","name":"微信开发","platform":"weixin","kind":"menu","menuPath":["运行","微信/自定义环境"],"command":"retained"},
                {"id":"ding-release","name":"钉钉发行","platform":"dingtalk","kind":"menu","menuPath":["发行","自定义发行","钉钉生产"],"command":""},
                {"id":"shell","name":"脚本","platform":"dingtalk","kind":"shell","menuPath":[],"command":"custom --flag 'a b'"}
            ]
        }],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}})
    }

    #[test]
    fn legacy_platforms_merge_without_changing_actions() {
        let original = platform_fixture();
        let raw: AppConfig = serde_json::from_value(original.clone()).unwrap();
        let normalized = raw.validate().unwrap();
        assert_eq!(normalized.projects[0].platforms, ["miniapp"]);
        assert_eq!(raw.projects[0].platforms, ["weixin", "dingtalk"]);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serialize(&raw).unwrap()).unwrap(),
            serde_json::to_value(&normalized).unwrap()
        );
        let parsed = parse(&original.to_string()).unwrap();
        let value = serde_json::to_value(&parsed).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(
            value["projects"][0]["platforms"],
            serde_json::json!(["miniapp"])
        );
        let mut expected = original["projects"][0]["commands"].clone();
        for action in expected.as_array_mut().unwrap() {
            action["platform"] = "miniapp".into();
            action["envVars"] = "".into();
        }
        assert_eq!(value["projects"][0]["commands"], expected);
        assert_eq!(
            serde_json::to_value(parse(&value.to_string()).unwrap()).unwrap(),
            value
        );
    }

    #[test]
    fn canonical_and_legacy_json_platforms_can_coexist() {
        for (kind, platforms, expected) in [
            (
                "nodejs",
                serde_json::json!(["pc", "web", "weixin", "miniapp"]),
                serde_json::json!(["web", "miniapp"]),
            ),
            (
                "uniapp",
                serde_json::json!(["app", "web", "weixin", "dingtalk", "miniapp"]),
                serde_json::json!(["app", "web", "miniapp"]),
            ),
            ("backend", serde_json::json!([]), serde_json::json!([])),
        ] {
            let mut value = platform_fixture();
            value["projects"][0]["type"] = kind.into();
            value["projects"][0]["commands"] = platforms.as_array().unwrap().iter().enumerate().map(|(index, platform)| {
                serde_json::json!({"id":format!("action-{index}"),"name":"原始动作","platform":platform,
                    "kind":"shell","menuPath":[],"command":"original command"})
            }).collect::<Vec<_>>().into();
            value["projects"][0]["platforms"] = platforms;
            let parsed = parse(&value.to_string()).unwrap();
            let serialized = serde_json::to_value(parsed).unwrap();
            assert_eq!(serialized["projects"][0]["platforms"], expected);
            for (original, mapped) in value["projects"][0]["commands"]
                .as_array()
                .unwrap()
                .iter()
                .zip(serialized["projects"][0]["commands"].as_array().unwrap())
            {
                let mut expected = original.clone();
                expected["platform"] = match original["platform"].as_str().unwrap() {
                    "pc" => "web",
                    "weixin" | "dingtalk" => "miniapp",
                    platform => platform,
                }
                .into();
                expected["envVars"] = "".into();
                assert_eq!(*mapped, expected);
            }
        }
    }

    #[test]
    fn normalization_preserves_strict_platform_and_action_validation() {
        for platforms in [
            serde_json::json!(["web", "web"]),
            serde_json::json!(["miniapp", "miniapp"]),
            serde_json::json!(["weixin", "weixin"]),
            serde_json::json!(["dingtalk", "dingtalk"]),
            serde_json::json!(["weixin", "miniapp", "weixin"]),
            serde_json::json!(["unknown"]),
        ] {
            let mut value = platform_fixture();
            value["projects"][0]["platforms"] = platforms;
            assert!(parse(&value.to_string()).is_err(), "{value}");
        }
        for (kind, platforms) in [
            ("nodejs", serde_json::json!(["app"])),
            ("backend", serde_json::json!(["miniapp"])),
        ] {
            let mut value = platform_fixture();
            value["projects"][0]["type"] = kind.into();
            value["projects"][0]["platforms"] = platforms;
            value["projects"][0]["commands"] = serde_json::json!([]);
            assert!(parse(&value.to_string()).is_err());
        }
        for platform in ["app", "web", "unknown"] {
            let mut value = platform_fixture();
            value["projects"][0]["commands"][0]["platform"] = platform.into();
            assert!(parse(&value.to_string()).is_err());
        }
        let mut value = platform_fixture();
        value["projects"][0]["commands"][1]["id"] = "weixin-dev".into();
        assert!(parse(&value.to_string()).is_err());
        for invalid in [serde_json::json!(null), serde_json::json!(42)] {
            let mut value = platform_fixture();
            value["projects"][0]["platforms"][0] = invalid;
            assert!(parse(&value.to_string()).is_err());
        }
        let mut value = platform_fixture();
        value["projects"][0]["commands"][0]["extra"] = true.into();
        assert!(parse(&value.to_string())
            .unwrap_err()
            .contains("unknown field"));
    }

    #[test]
    fn legacy_file_load_and_migration_persist_canonical_platforms() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let text = platform_fixture().to_string();
        let path = old.path().join("config.json");
        atomic_write(&path, &text).unwrap();
        let loaded = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.projects[0].platforms, ["miniapp"]);
        let _lock = migrate_legacy(old.path(), new.path()).unwrap();
        let migrated: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(new.path().join("config.json")).unwrap())
                .unwrap();
        assert_eq!(
            migrated["projects"][0]["platforms"],
            serde_json::json!(["miniapp"])
        );
        assert_eq!(migrated, serde_json::to_value(loaded).unwrap());
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn command_env_vars_default_roundtrip_and_legacy_migration() {
        let mut value = serde_json::json!({"schemaVersion":1,"projects":[{"id":"p","name":"Project","type":"nodejs","path":"/tmp/project","platforms":["web"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[{"id":"dev","name":"Dev","platform":"web","kind":"shell","menuPath":[],"command":"npm run dev"},{"id":"test","name":"Test","platform":"web","kind":"shell","menuPath":[],"command":"npm test","envVars":"NODE_ENV=test"}]}],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        // 旧配置没有动作级 envVars 时默认为空，已有值保持。
        let parsed = parse(&value.to_string()).unwrap();
        assert!(parsed.projects[0].commands[0].env_vars.is_empty());
        assert_eq!(parsed.projects[0].commands[1].env_vars, "NODE_ENV=test");
        // 旧版项目级环境变量迁移到尚未配置的终端命令，不覆盖命令自身配置。
        value["projects"][0]["envVars"] =
            "ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;SMTP=smtp.qq.com".into();
        let parsed = parse(&value.to_string()).unwrap();
        assert!(parsed.projects[0].env_vars.is_empty());
        assert_eq!(
            parsed.projects[0].commands[0].env_vars,
            "ALERT_EMAIL_FROM=示例发件人 <sender@example.com>;SMTP=smtp.qq.com"
        );
        assert_eq!(parsed.projects[0].commands[1].env_vars, "NODE_ENV=test");
        let serialized = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            serde_json::to_value(parse(&serialized.to_string()).unwrap()).unwrap(),
            serialized
        );
        // 没有终端命令时保留旧值，避免迁移过程中丢数据。
        value["projects"][0]["commands"] = serde_json::json!([]);
        let parsed = parse(&value.to_string()).unwrap();
        assert_eq!(parsed.projects[0].env_vars, value["projects"][0]["envVars"]);
        for invalid in ["NO_EQUALS", "=x", "A=1;A=2", "A=bad\0"] {
            value["projects"][0]["envVars"] = serde_json::Value::Null;
            value["projects"][0]["commands"] = serde_json::json!([{"id":"dev","name":"Dev","platform":"web","kind":"shell","menuPath":[],"command":"npm run dev","envVars":invalid}]);
            assert!(parse(&value.to_string()).is_err(), "{invalid}");
        }
        value["projects"][0]["commands"] = serde_json::json!([{"id":"dev","name":"Dev","platform":"web","kind":"shell","menuPath":[],"command":"npm run dev","envVars":42}]);
        assert!(parse(&value.to_string()).is_err());
    }

    #[test]
    fn ai_preferences_default_roundtrip_and_validate() {
        let legacy = serde_json::json!({"schemaVersion":1,"projects":[{"id":"p","name":"Project","type":"backend","path":"/tmp/project","platforms":[],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]}],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        let config = parse(&legacy.to_string()).unwrap();
        let mut value = serde_json::to_value(config).unwrap();
        assert_eq!(value["settings"]["defaultAiToolId"], "");
        assert_eq!(value["projects"][0]["aiToolId"], "");
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap()["defaultAiToolId"],
            ""
        );
        // 悬空引用可保存，工具删除或禁用不应阻止配置读写。
        value["settings"]["defaultAiToolId"] = "removed-global".into();
        value["projects"][0]["aiToolId"] = "removed-project".into();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let parsed = parse(&value.to_string()).unwrap();
        atomic_write(&path, &serde_json::to_string(&parsed).unwrap()).unwrap();
        let restored = parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(serde_json::to_value(restored).unwrap(), value);
        for invalid in [
            serde_json::json!("bad\0id"),
            serde_json::json!(null),
            serde_json::json!(42),
            serde_json::json!(false),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let mut invalid_global = value.clone();
            invalid_global["settings"]["defaultAiToolId"] = invalid.clone();
            assert!(parse(&invalid_global.to_string()).is_err());
            let mut invalid_project = value.clone();
            invalid_project["projects"][0]["aiToolId"] = invalid;
            assert!(parse(&invalid_project.to_string()).is_err());
        }
    }

    #[test]
    fn ai_tools_default_and_custom_disabled_roundtrip() {
        let legacy = r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}}"#;
        assert!(Settings::default().ai_tools.is_empty());
        assert!(parse(legacy).unwrap().settings.ai_tools.is_empty());
        let mut config = AppConfig::default();
        config.settings.ai_tools = vec![
            AiTool {
                id: "custom".into(),
                name: "自定义 CLI".into(),
                command: "  custom --arg 'a b';\necho done  ".into(),
                enabled: true,
            },
            AiTool {
                id: "disabled".into(),
                name: "停用工具".into(),
                command: "other-cli".into(),
                enabled: false,
            },
        ];
        let value = serde_json::to_value(&config).unwrap();
        assert_eq!(
            value["settings"]["aiTools"][0]["command"],
            config.settings.ai_tools[0].command
        );
        let parsed = parse(&value.to_string()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    #[test]
    fn ai_tools_reject_invalid_fields_and_duplicate_ids() {
        let valid = serde_json::json!({"id":"custom","name":"Custom","command":"custom-cli","enabled":true});
        let mut config = serde_json::to_value(AppConfig::default()).unwrap();
        config["settings"]["aiTools"] = serde_json::json!([valid.clone(), valid.clone()]);
        assert!(parse(&config.to_string()).is_err());
        for field in ["id", "name", "command"] {
            for invalid in [
                serde_json::json!(""),
                serde_json::json!(" \t\n"),
                serde_json::json!("a\0b"),
                serde_json::json!(null),
                serde_json::json!(42),
                serde_json::json!(false),
            ] {
                let mut tool = valid.clone();
                tool[field] = invalid;
                config["settings"]["aiTools"] = serde_json::json!([tool]);
                assert!(parse(&config.to_string()).is_err(), "{field}: {config}");
            }
        }
        for invalid in [
            serde_json::json!("true"),
            serde_json::json!(1),
            serde_json::json!(null),
        ] {
            let mut tool = valid.clone();
            tool["enabled"] = invalid;
            config["settings"]["aiTools"] = serde_json::json!([tool]);
            assert!(parse(&config.to_string()).is_err());
        }
        for field in ["id", "name", "command", "enabled"] {
            let mut tool = valid.clone();
            tool.as_object_mut().unwrap().remove(field);
            config["settings"]["aiTools"] = serde_json::json!([tool]);
            assert!(parse(&config.to_string())
                .unwrap_err()
                .contains("missing field"));
        }
        let mut tool = valid;
        tool["extra"] = true.into();
        config["settings"]["aiTools"] = serde_json::json!([tool]);
        assert!(parse(&config.to_string())
            .unwrap_err()
            .contains("unknown field"));
        for invalid in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!("cli"),
        ] {
            config["settings"]["aiTools"] = invalid;
            assert!(parse(&config.to_string()).is_err());
        }
    }

    #[test]
    fn node_selection_defaults_roundtrips_and_remains_strict() {
        let project = serde_json::json!({"id":"node","name":"Node","type":"nodejs","path":"/tmp/node-project","platforms":["pc"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[]});
        let mut backup = serde_json::json!({"schemaVersion":1,"projects":[project],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        let legacy = parse(&backup.to_string()).unwrap();
        let value = serde_json::to_value(legacy).unwrap();
        assert_eq!(value["projects"][0]["nodeVersion"], "");
        assert_eq!(value["projects"][0]["nvmDir"], "");
        backup["projects"][0]["nodeVersion"] = "v22.12.0".into();
        backup["projects"][0]["nvmDir"] = "/not-installed/.nvm".into();
        let parsed = parse(&backup.to_string()).unwrap();
        let value = serde_json::to_value(parsed).unwrap();
        assert_eq!(value["projects"][0]["nodeVersion"], "v22.12.0");
        assert_eq!(value["projects"][0]["nvmDir"], "/not-installed/.nvm");
        for version in [
            "default",
            "node",
            "v22",
            "v22.1",
            "v22.1.0; touch /tmp/injected",
            "../v22.1.0",
            "v01.2.3",
            "v22.1.0-beta",
        ] {
            let mut invalid = backup.clone();
            invalid["projects"][0]["nodeVersion"] = version.into();
            assert!(parse(&invalid.to_string()).is_err(), "{version}");
        }
        for field in ["nodeVersion", "nvmDir"] {
            let mut invalid = backup.clone();
            invalid["projects"][0][field] = serde_json::Value::Null;
            assert!(parse(&invalid.to_string()).is_err());
        }
        backup["projects"][0]["nvmDir"] = "relative/.nvm".into();
        assert!(parse(&backup.to_string()).is_err());
        backup["projects"][0]["nvmDir"] = "/tmp/.nvm".into();
        backup["projects"][0]["extra"] = true.into();
        assert!(parse(&backup.to_string())
            .unwrap_err()
            .contains("unknown field"));
        backup["projects"][0]
            .as_object_mut()
            .unwrap()
            .remove("extra");
        backup["projects"][0]
            .as_object_mut()
            .unwrap()
            .remove("jdkPath");
        assert!(parse(&backup.to_string())
            .unwrap_err()
            .contains("missing field"));
    }
    #[test]
    fn theme_defaults_for_new_and_legacy_configs() {
        assert_eq!(Settings::default().theme, "lavender");
        assert_eq!(AppConfig::default().settings.theme, "lavender");
        let legacy = r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}}"#;
        let config = parse(legacy).unwrap();
        assert_eq!(config.settings.theme, "lavender");
        assert_eq!(config.schema_version, 1);
        let mut value: serde_json::Value = serde_json::from_str(legacy).unwrap();
        value["settings"]["unexpected"] = true.into();
        assert!(parse(&value.to_string())
            .unwrap_err()
            .contains("unknown field"));
        value["settings"]
            .as_object_mut()
            .unwrap()
            .remove("unexpected");
        value["settings"].as_object_mut().unwrap().remove("jdkPath");
        assert!(parse(&value.to_string())
            .unwrap_err()
            .contains("missing field"));
    }
    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_file_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        atomic_write(&path, "{\"appid\":\"new\"}").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // 目标不存在时仍创建新文件。
        let created = dir.path().join("new.json");
        atomic_write(&created, "{}").unwrap();
        assert!(created.is_file());
    }
    #[test]
    fn output_preference_defaults_and_roundtrips() {
        assert!(Settings::default().open_output_after_build);
        let legacy = r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}}"#;
        assert!(parse(legacy).unwrap().settings.open_output_after_build);
        for enabled in [true, false] {
            let mut config = AppConfig::default();
            config.settings.open_output_after_build = enabled;
            let text = serde_json::to_string(&config).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["settings"]["openOutputAfterBuild"], enabled);
            assert_eq!(
                parse(&text).unwrap().settings.open_output_after_build,
                enabled
            );
        }
        let mut value: serde_json::Value = serde_json::from_str(legacy).unwrap();
        value["settings"]["openOutputAfterBuild"] = "false".into();
        assert!(parse(&value.to_string()).is_err());
    }
    #[test]
    fn theme_roundtrips_through_config_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        for theme in ["lavender", "blue", "cyan", "rose", "amber", "slate"] {
            let mut config = AppConfig::default();
            config.settings.theme = theme.into();
            config.validate().unwrap();
            atomic_write(&path, &serde_json::to_string_pretty(&config).unwrap()).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["settings"]["theme"], theme);
            assert_eq!(parse(&text).unwrap().settings.theme, theme);
        }
    }
    #[test]
    fn theme_rejects_invalid_values() {
        for theme in ["", "dark", "Blue", " lavender "] {
            let mut config = AppConfig::default();
            config.settings.theme = theme.into();
            let error = config.validate().unwrap_err();
            assert!(error.contains("theme"));
            assert!(error.contains("lavender") && error.contains("blue"));
            assert!(parse(&serde_json::to_string(&config).unwrap()).is_err());
        }
        for theme in [serde_json::Value::Null, true.into(), 1.into()] {
            let mut value = serde_json::to_value(AppConfig::default()).unwrap();
            value["settings"]["theme"] = theme;
            assert!(parse(&value.to_string()).is_err());
        }
    }
    #[test]
    fn theme_defaults_after_legacy_migration() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let legacy = r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}}"#;
        atomic_write(&old.path().join("config.json"), legacy).unwrap();
        let _lock = migrate_legacy(old.path(), new.path()).unwrap();
        let text = std::fs::read_to_string(new.path().join("config.json")).unwrap();
        assert_eq!(
            serde_json::to_value(parse(&text).unwrap()).unwrap(),
            serde_json::to_value(parse(legacy).unwrap()).unwrap()
        );
        assert_eq!(parse(&text).unwrap().settings.theme, "lavender");
    }
    #[test]
    fn backup_rejects_invalid_project_relationships() {
        let project = serde_json::json!({"id":"p1","name":"示例","type":"uniapp","path":"/tmp/project","platforms":["app"],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"app","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"Terminal","commands":[{"id":"a1","name":"发行","platform":"app","kind":"menu","menuPath":["发行","App-制作应用wgt包"],"command":""}]});
        let base = serde_json::json!({"schemaVersion":1,"projects":[project],"settings":{"hbuilderxPath":"","terminal":"Terminal","jdkPath":""}});
        assert!(parse(&base.to_string()).is_ok());
        let mut invalid = base.clone();
        invalid["projects"][0]["type"] = "nodejs".into();
        assert!(parse(&invalid.to_string()).is_err());
        let mut invalid = base.clone();
        invalid["projects"][0]["platforms"] = serde_json::json!([]);
        assert!(parse(&invalid.to_string()).is_err());
        let mut invalid = base.clone();
        invalid["projects"][0]["platforms"] = serde_json::json!(["app", "app"]);
        assert!(parse(&invalid.to_string()).is_err());
        let mut invalid = base.clone();
        invalid["projects"][0]["commands"][0]["platform"] = "web".into();
        assert!(parse(&invalid.to_string()).is_err());
        for menu in [
            serde_json::json!(["发行"]),
            serde_json::json!(["编辑", "复制"]),
        ] {
            let mut invalid = base.clone();
            invalid["projects"][0]["commands"][0]["menuPath"] = menu;
            assert!(parse(&invalid.to_string()).is_err());
        }
        let mut invalid = base.clone();
        let mut duplicate = invalid["projects"][0].clone();
        duplicate["id"] = "p2".into();
        duplicate["path"] = "/tmp/project/".into();
        invalid["projects"].as_array_mut().unwrap().push(duplicate);
        assert!(parse(&invalid.to_string()).is_err());
        let mut invalid = base.clone();
        invalid["projects"][0]["type"] = "nodejs".into();
        invalid["projects"][0]["platforms"] = serde_json::json!(["weixin"]);
        invalid["projects"][0]["commands"][0]["platform"] = "weixin".into();
        assert!(parse(&invalid.to_string()).is_err());
    }
    #[test]
    fn migration_preserves_old_data_and_recovery_records() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let config = serde_json::to_string(&AppConfig::default()).unwrap();
        let record = r#"{"path":"/tmp/project/manifest.json","original":"old","temporary":"new"}"#;
        atomic_write(&old.path().join("config.json"), &config).unwrap();
        atomic_write(&old.path().join("pending/task.json"), record).unwrap();
        let lock = migrate_legacy(old.path(), new.path()).unwrap().unwrap();
        for dir in [old.path(), new.path()] {
            assert_eq!(
                std::fs::read_to_string(dir.join("config.json")).unwrap(),
                config
            );
            assert_eq!(
                std::fs::read_to_string(dir.join("pending/task.json")).unwrap(),
                record
            );
        }
        let other = std::fs::File::open(old.path().join("instance.lock")).unwrap();
        assert!(other.try_lock().is_err());
        drop(lock);
        assert!(other.try_lock().is_ok());
    }
    #[test]
    fn migration_does_not_overwrite_existing_config() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        atomic_write(&old.path().join("config.json"), "invalid old config").unwrap();
        let config = serde_json::to_string(&AppConfig::default()).unwrap();
        atomic_write(&new.path().join("config.json"), &config).unwrap();
        let _lock = migrate_legacy(old.path(), new.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(new.path().join("config.json")).unwrap(),
            config
        );
    }
    #[test]
    fn migration_rejects_invalid_config_without_writing_new_data() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        for invalid in ["invalid json".to_string(), {
            let mut config = AppConfig::default();
            config.schema_version = 2;
            serde_json::to_string(&config).unwrap()
        }] {
            atomic_write(&old.path().join("config.json"), &invalid).unwrap();
            assert!(migrate_legacy(old.path(), new.path())
                .unwrap_err()
                .contains("旧版配置无效"));
            assert!(!new.path().join("config.json").exists());
            assert_eq!(
                std::fs::read_to_string(old.path().join("config.json")).unwrap(),
                invalid
            );
        }
    }
    #[test]
    fn migration_requires_old_instance_to_exit() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        atomic_write(
            &old.path().join("config.json"),
            &serde_json::to_string(&AppConfig::default()).unwrap(),
        )
        .unwrap();
        let lock = std::fs::File::create(old.path().join("instance.lock")).unwrap();
        lock.try_lock().unwrap();
        assert!(migrate_legacy(old.path(), new.path())
            .unwrap_err()
            .contains("请先退出旧版 AI Dev"));
        assert!(!new.path().join("config.json").exists());
    }
    #[test]
    fn migration_rejects_invalid_or_conflicting_recovery_records() {
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        atomic_write(
            &old.path().join("config.json"),
            &serde_json::to_string(&AppConfig::default()).unwrap(),
        )
        .unwrap();
        atomic_write(&old.path().join("pending/task.json"), "invalid").unwrap();
        assert!(migrate_legacy(old.path(), new.path()).is_err());
        assert!(!new.path().join("config.json").exists());
        let record = r#"{"path":"/tmp/project/manifest.json","original":"old","temporary":"new"}"#;
        atomic_write(&old.path().join("pending/task.json"), record).unwrap();
        atomic_write(&new.path().join("pending/task.json"), "existing").unwrap();
        assert!(migrate_legacy(old.path(), new.path())
            .unwrap_err()
            .contains("恢复记录冲突"));
        assert_eq!(
            std::fs::read_to_string(new.path().join("pending/task.json")).unwrap(),
            "existing"
        );
        assert!(!new.path().join("config.json").exists());
        atomic_write(&new.path().join("pending/task.json"), record).unwrap();
        let _lock = migrate_legacy(old.path(), new.path()).unwrap();
        assert!(new.path().join("config.json").is_file());
    }
    #[test]
    fn backup_strict() {
        let c = AppConfig::default();
        assert!(parse(&serde_json::to_string(&c).unwrap()).is_ok());
        for s in [
            r#"{"schemaVersion":2,"projects":[],"settings":{"hbuilderxPath":"","terminal":"","jdkPath":""}}"#,
            r#"{"schemaVersion":1,"projects":[],"settings":{}}"#,
            r#"{"schemaVersion":1,"projects":[],"settings":{"hbuilderxPath":"","terminal":"","jdkPath":""},"extra":1}"#,
        ] {
            assert!(parse(s).is_err());
        }
    }
}
