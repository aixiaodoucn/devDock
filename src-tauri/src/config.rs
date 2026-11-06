use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub hbuilderx_path: String,
    pub terminal: String,
    pub jdk_path: String,
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
    let temp = parent.join(format!(".ai-dev-{}.tmp", uuid::Uuid::new_v4()));
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
