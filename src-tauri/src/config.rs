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
    pub commands: Vec<Action>,
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
                terminal: "Terminal".into(),
                ..Default::default()
            },
        }
    }
}
fn terminal(t: &str) -> Result<()> {
    if !t.is_empty() && t != "Terminal" && !Path::new(t).is_absolute() {
        return Err("终端必须是 Terminal 或应用绝对路径".into());
    }
    Ok(())
}
impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err("不支持的 schemaVersion".into());
        }
        if !["lavender", "blue", "cyan", "rose", "amber", "slate"]
            .contains(&self.settings.theme.as_str())
        {
            return Err("无效的 theme：仅支持 lavender、blue、cyan、rose、amber 或 slate".into());
        }
        terminal(&self.settings.terminal)?;
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for p in &self.projects {
            if p.id.trim().is_empty() || p.name.trim().is_empty() || !ids.insert(&p.id) {
                return Err("项目 id/name 为空或 id 重复".into());
            }
            if !["uniapp", "nodejs", "backend"].contains(&p.project_type.as_str())
                || !Path::new(&p.path).is_absolute()
            {
                return Err("项目类型或绝对路径无效".into());
            }
            // components 去除尾斜杠与多余分隔符，不要求备份中的目录当前存在。
            let normalized: PathBuf = Path::new(&p.path).components().collect();
            if !paths.insert(normalized) {
                return Err("项目目录重复".into());
            }
            let allowed: &[&str] = match p.project_type.as_str() {
                "uniapp" => &["app", "weixin", "dingtalk"],
                "nodejs" => &["pc", "weixin"],
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
            terminal(&p.terminal)?;
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
            }
        }
        Ok(())
    }
}
pub fn parse(text: &str) -> Result<AppConfig> {
    let c: AppConfig = serde_json::from_str(text).map_err(|e| e.to_string())?;
    c.validate()?;
    Ok(c)
}
pub fn atomic_write(path: &Path, data: &str) -> Result<()> {
    let parent = path.parent().ok_or("路径无父目录")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".dev-dock-{}.tmp", uuid::Uuid::new_v4()));
    use std::io::Write;
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
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
    parse(&text).map_err(|e| format!("旧版配置无效，未迁移: {e}"))?;
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
            if !Path::new(&record.path).is_absolute() {
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
        assert_eq!(text, legacy);
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
        invalid["projects"][0]["commands"][0]["platform"] = "weixin".into();
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
