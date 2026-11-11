use crate::config::{self, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub path: String,
    pub exists: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildRecord {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub mode: String,
    pub version_name: String,
    pub version_code: String,
    pub branch: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub duration_ms: Option<u64>,
    pub status: String,
    pub summary: String,
    pub artifacts: Vec<Artifact>,
}

const DEFAULT_RETENTION_LIMIT: u32 = 100;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySettings {
    pub retention_limit: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub retention_limit: u32,
    pub removed_count: usize,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryDocument {
    schema_version: u32,
    retention_limit: u32,
    records: Vec<BuildRecord>,
}

impl Default for HistoryDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            retention_limit: DEFAULT_RETENTION_LIMIT,
            records: vec![],
        }
    }
}

#[derive(Default)]
pub struct Store {
    lock: Mutex<()>,
}

impl Store {
    pub fn settings(&self, dir: &Path) -> Result<HistorySettings> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        Ok(HistorySettings {
            retention_limit: read(dir)?.retention_limit,
        })
    }

    pub fn set_retention_limit(&self, dir: &Path, limit: u32) -> Result<SettingsUpdate> {
        validate_retention_limit(limit)?;
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        let mut document = read(dir)?;
        let original_count = document.records.len();
        document.retention_limit = limit;
        trim(&mut document.records, limit);
        // 设置与清理作为同一个文档提交，写入失败不会改变后续读取的策略。
        write(dir, &document)?;
        Ok(SettingsUpdate {
            retention_limit: limit,
            removed_count: original_count - document.records.len(),
        })
    }

    pub fn load(&self, dir: &Path) -> Result<()> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        read(dir).map(|_| ())
    }

    pub fn list(&self, dir: &Path, project_id: Option<&str>) -> Result<Vec<BuildRecord>> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        let mut records = read(dir)?.records;
        records.retain(|r| project_id.is_none_or(|id| r.project_id == id));
        for record in &mut records {
            for artifact in &mut record.artifacts {
                // 不把异平台路径当作当前工作目录下的相对文件，也不把目录当作产物。
                artifact.exists =
                    native_absolute(&artifact.path) && Path::new(&artifact.path).is_file();
            }
        }
        records.reverse();
        records.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(records)
    }

    pub fn start(&self, dir: &Path, mut record: BuildRecord) -> Result<()> {
        if !["apk", "wgt-android", "wgt-ios"].contains(&record.mode.as_str()) {
            return Err("构建历史模式无效".into());
        }
        record.status = "running".into();
        record.summary = summary("running")?.into();
        record.finished_at = None;
        record.duration_ms = None;
        record.artifacts.clear();
        self.update(dir, |records| {
            if records.iter().any(|r| r.id == record.id) {
                return Err("构建历史 ID 已存在".into());
            }
            records.push(record);
            Ok(())
        })
    }

    pub fn artifact(&self, dir: &Path, id: &str, path: &Path) -> Result<()> {
        self.update(dir, |records| {
            let record = running(records, id)?;
            let path = path.to_string_lossy().into_owned();
            if !record.artifacts.iter().any(|a| a.path == path) {
                // exists 是查询时的动态值，不持久化文件系统快照。
                record.artifacts.push(Artifact {
                    path,
                    exists: false,
                });
            }
            Ok(())
        })
    }

    pub fn finish(&self, dir: &Path, id: &str, status: &str) -> Result<()> {
        if !["success", "error", "cancelled"].contains(&status) {
            return Err("构建历史结束状态无效".into());
        }
        self.update(dir, |records| {
            complete(running(records, id)?, status, now_ms())
        })
    }

    // 必须在取得应用单实例锁后、接受新构建前调用；遗留 running 只能标记中断。
    pub fn recover(&self, dir: &Path) -> Result<usize> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        let mut document = read(dir)?;
        let mut count = 0;
        let now = now_ms();
        for record in &mut document.records {
            if record.status == "running" {
                complete(record, "interrupted", now)?;
                count += 1;
            }
        }
        if count > 0 {
            trim(&mut document.records, document.retention_limit);
            write(dir, &document)?;
        }
        Ok(count)
    }

    pub fn delete(&self, dir: &Path, id: &str) -> Result<()> {
        self.update(dir, |records| {
            let index = records
                .iter()
                .position(|r| r.id == id)
                .ok_or("构建历史不存在")?;
            if records[index].status == "running" {
                return Err("不能删除运行中的构建历史".into());
            }
            records.remove(index);
            Ok(())
        })
    }

    fn update(
        &self,
        dir: &Path,
        change: impl FnOnce(&mut Vec<BuildRecord>) -> Result<()>,
    ) -> Result<()> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        // 每次在同一临界区重新读取及原子写入；读取失败绝不以空历史覆盖原文件。
        let mut document = read(dir)?;
        change(&mut document.records)?;
        trim(&mut document.records, document.retention_limit);
        write(dir, &document)
    }
}

fn native_absolute(path: &str) -> bool {
    Path::new(path).is_absolute() && (cfg!(windows) || !path.starts_with("//"))
}

fn validate_retention_limit(limit: u32) -> Result<()> {
    if !(1..=1000).contains(&limit) {
        return Err("构建历史保留上限必须为 1 到 1000 的整数".into());
    }
    Ok(())
}

fn read(dir: &Path) -> Result<HistoryDocument> {
    let text = match fs::read_to_string(dir.join("build-history.json")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(HistoryDocument::default());
        }
        Err(error) => return Err(format!("无法读取构建历史: {error}")),
    };
    // 只兼容旧数组；对象必须提供完整版本及设置，避免损坏设置被默认值覆盖。
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| "构建历史文件损坏，已保留原文件，请先备份并修复".to_string())?;
    let document: HistoryDocument = if value.is_array() {
        serde_json::from_value(value).map(|records| HistoryDocument {
            records,
            ..HistoryDocument::default()
        })
    } else {
        serde_json::from_value(value)
    }
    .map_err(|_| "构建历史文件损坏，已保留原文件，请先备份并修复".to_string())?;
    if document.schema_version != 1 {
        return Err("构建历史文件版本不支持，已保留原文件".into());
    }
    validate_retention_limit(document.retention_limit)?;
    let mut ids = std::collections::HashSet::new();
    for record in &document.records {
        if !ids.insert(&record.id)
            || !["apk", "wgt-android", "wgt-ios"].contains(&record.mode.as_str())
            || summary(&record.status).is_err()
            || (record.status == "running"
                && (record.finished_at.is_some() || record.duration_ms.is_some()))
            || (record.status != "running"
                && (record.finished_at.is_none() || record.duration_ms.is_none()))
        {
            return Err("构建历史记录无效，已保留原文件，请先备份并修复".into());
        }
    }
    Ok(document)
}

fn write(dir: &Path, document: &HistoryDocument) -> Result<()> {
    let text = serde_json::to_string_pretty(document).map_err(|e| e.to_string())?;
    config::atomic_write(&dir.join("build-history.json"), &text)
}

fn running<'a>(records: &'a mut [BuildRecord], id: &str) -> Result<&'a mut BuildRecord> {
    let record = records
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or("构建历史不存在")?;
    if record.status != "running" {
        return Err("构建历史已经结束".into());
    }
    Ok(record)
}

fn summary(status: &str) -> Result<&'static str> {
    match status {
        "running" => Ok("构建中"),
        "success" => Ok("构建完成"),
        "error" => Ok("构建失败，请在任务面板查看详情"),
        "cancelled" => Ok("构建已取消"),
        "interrupted" => Ok("应用退出或中断，构建结果未确认"),
        _ => Err("构建历史状态无效".into()),
    }
}

fn complete(record: &mut BuildRecord, status: &str, now: u64) -> Result<()> {
    record.summary = summary(status)?.into();
    record.status = status.into();
    record.finished_at = Some(now);
    record.duration_ms = Some(now.saturating_sub(record.started_at));
    Ok(())
}

fn trim(records: &mut Vec<BuildRecord>, retention_limit: u32) {
    // 同毫秒完成时以插入顺序决定新旧，每个项目仅裁掉最旧的完成记录。
    let mut completed: Vec<_> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.status != "running")
        .map(|(i, r)| (i, r.finished_at.unwrap_or(0), r.project_id.clone()))
        .collect();
    completed.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    let mut counts = std::collections::HashMap::new();
    let mut remove = std::collections::HashSet::new();
    for (index, _, project) in completed {
        let count = counts.entry(project).or_insert(0);
        *count += 1;
        if *count > retention_limit {
            remove.insert(index);
        }
    }
    let mut index = 0;
    records.retain(|_| {
        let keep = !remove.contains(&index);
        index += 1;
        keep
    });
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, project: &str) -> BuildRecord {
        BuildRecord {
            id: id.into(),
            project_id: project.into(),
            project_name: "项目".into(),
            project_path: "/project".into(),
            mode: "apk".into(),
            version_name: "1.0.0".into(),
            version_code: "100".into(),
            branch: Some("main".into()),
            started_at: now_ms(),
            finished_at: None,
            duration_ms: None,
            status: "running".into(),
            summary: "构建中".into(),
            artifacts: vec![],
        }
    }
    fn completed(id: &str, project: &str, finished_at: u64) -> BuildRecord {
        let mut record = record(id, project);
        complete(&mut record, "success", finished_at).unwrap();
        record
    }

    fn seed(dir: &Path, limit: u32, records: Vec<BuildRecord>) {
        let value = serde_json::json!({
            "schemaVersion": 1,
            "retentionLimit": limit,
            "records": records,
        });
        fs::write(dir.join("build-history.json"), value.to_string()).unwrap();
    }

    #[test]
    fn settings_default_and_legacy_array_migrate_only_on_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let path = dir.path().join("build-history.json");
        assert_eq!(store.settings(dir.path()).unwrap().retention_limit, 100);
        assert!(!path.exists());
        let legacy = serde_json::to_string(&vec![completed("old", "p", 1)]).unwrap();
        fs::write(&path, &legacy).unwrap();
        store.load(dir.path()).unwrap();
        assert_eq!(store.settings(dir.path()).unwrap().retention_limit, 100);
        assert_eq!(store.list(dir.path(), None).unwrap().len(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), legacy);
        store.start(dir.path(), record("new", "p")).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["retentionLimit"], 100);
        assert_eq!(value["records"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn raised_limit_persists_across_stores_and_all_later_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let stale = Store::default();
        stale.load(dir.path()).unwrap();
        let update = store.set_retention_limit(dir.path(), 1000).unwrap();
        assert_eq!(
            serde_json::to_value(update).unwrap(),
            serde_json::json!({"retentionLimit": 1000, "removedCount": 0})
        );
        assert_eq!(
            serde_json::to_value(stale.settings(dir.path()).unwrap()).unwrap(),
            serde_json::json!({"retentionLimit": 1000})
        );
        seed(
            dir.path(),
            1000,
            (0..105)
                .map(|i| completed(&format!("done-{i}"), "p", i))
                .collect(),
        );
        stale.start(dir.path(), record("new", "p")).unwrap();
        stale
            .artifact(dir.path(), "new", &dir.path().join("app.apk"))
            .unwrap();
        stale.finish(dir.path(), "new", "success").unwrap();
        stale.delete(dir.path(), "done-0").unwrap();
        assert_eq!(store.list(dir.path(), None).unwrap().len(), 105);
        assert_eq!(
            Store::default()
                .settings(dir.path())
                .unwrap()
                .retention_limit,
            1000
        );
    }

    #[test]
    fn lowering_limit_prunes_oldest_per_project_preserves_running_and_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let artifact = dir.path().join("old.apk");
        fs::write(&artifact, "apk").unwrap();
        let mut records = vec![];
        for project in ["p", "q"] {
            for i in [3, 1, 2] {
                let mut item = completed(&format!("{project}-{i}"), project, i);
                item.artifacts.push(Artifact {
                    path: artifact.to_string_lossy().into_owned(),
                    exists: false,
                });
                records.push(item);
            }
            records.push(record(&format!("{project}-running"), project));
        }
        seed(dir.path(), 100, records);
        let update = store.set_retention_limit(dir.path(), 1).unwrap();
        assert_eq!(update.retention_limit, 1);
        assert_eq!(update.removed_count, 4);
        for project in ["p", "q"] {
            let records = store.list(dir.path(), Some(project)).unwrap();
            assert_eq!(records.len(), 2);
            assert!(records.iter().any(|r| r.id == format!("{project}-3")));
            assert!(records.iter().any(|r| r.status == "running"));
        }
        assert_eq!(fs::read_to_string(artifact).unwrap(), "apk");
        assert_eq!(
            store
                .set_retention_limit(dir.path(), 1)
                .unwrap()
                .removed_count,
            0
        );
    }

    #[test]
    fn recovery_uses_persisted_limit() {
        let dir = tempfile::tempdir().unwrap();
        seed(
            dir.path(),
            1,
            vec![
                completed("old", "p", 1),
                record("a", "p"),
                record("b", "p"),
                record("q", "q"),
            ],
        );
        let store = Store::default();
        assert_eq!(store.recover(dir.path()).unwrap(), 3);
        let records = store.list(dir.path(), Some("p")).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, "b");
        assert_eq!(records[0].status, "interrupted");
        assert_eq!(store.list(dir.path(), Some("q")).unwrap().len(), 1);
        assert_eq!(store.settings(dir.path()).unwrap().retention_limit, 1);
    }

    #[test]
    fn invalid_settings_or_documents_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let path = dir.path().join("build-history.json");
        store.set_retention_limit(dir.path(), 2).unwrap();
        let original = fs::read(&path).unwrap();
        for limit in [0, 1001, u32::MAX] {
            assert!(store.set_retention_limit(dir.path(), limit).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
        for bad in [
            "{broken",
            "{}",
            "null",
            r#"{"schemaVersion":2,"retentionLimit":100,"records":[]}"#,
            r#"{"schemaVersion":1,"retentionLimit":0,"records":[]}"#,
            r#"{"schemaVersion":1,"retentionLimit":1001,"records":[]}"#,
            r#"{"schemaVersion":1,"retentionLimit":1.5,"records":[]}"#,
            r#"{"schemaVersion":1,"retentionLimit":"100","records":[]}"#,
            r#"{"schemaVersion":1,"records":[]}"#,
        ] {
            fs::write(&path, bad).unwrap();
            assert!(store.settings(dir.path()).is_err());
            assert!(store.set_retention_limit(dir.path(), 1).is_err());
            assert!(store.load(dir.path()).is_err());
            assert!(store.start(dir.path(), record("a", "p")).is_err());
            assert!(store
                .artifact(dir.path(), "a", Path::new("/app.apk"))
                .is_err());
            assert!(store.finish(dir.path(), "a", "success").is_err());
            assert!(store.recover(dir.path()).is_err());
            assert!(store.delete(dir.path(), "a").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), bad);
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_settings_write_preserves_document_and_future_policy() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        seed(
            dir.path(),
            3,
            vec![
                completed("a", "p", 1),
                completed("b", "p", 2),
                completed("c", "p", 3),
            ],
        );
        let path = dir.path().join("build-history.json");
        let original = fs::read(&path).unwrap();
        let permissions = fs::metadata(dir.path()).unwrap().permissions();
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let result = store.set_retention_limit(dir.path(), 1);
        fs::set_permissions(dir.path(), permissions).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(store.settings(dir.path()).unwrap().retention_limit, 3);
        store.start(dir.path(), record("new", "p")).unwrap();
        store.finish(dir.path(), "new", "success").unwrap();
        assert_eq!(store.list(dir.path(), None).unwrap().len(), 3);
        assert_eq!(store.settings(dir.path()).unwrap().retention_limit, 3);
    }

    #[test]
    fn round_trip_filter_and_all_terminal_statuses() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        for status in ["success", "error", "cancelled"] {
            store.start(dir.path(), record(status, "p1")).unwrap();
            store.finish(dir.path(), status, status).unwrap();
        }
        store.start(dir.path(), record("pending", "p2")).unwrap();
        let reboot = Store::default();
        reboot.load(dir.path()).unwrap();
        assert_eq!(reboot.list(dir.path(), Some("p1")).unwrap().len(), 3);
        assert_eq!(reboot.recover(dir.path()).unwrap(), 1);
        assert_eq!(reboot.recover(dir.path()).unwrap(), 0);
        let all = reboot.list(dir.path(), None).unwrap();
        assert_eq!(all.len(), 4);
        for r in all {
            assert!(r.finished_at.is_some());
            assert!(r.duration_ms.is_some());
            assert_eq!(
                r.status,
                if r.id == "pending" {
                    "interrupted"
                } else {
                    &r.id
                }
            );
        }
    }
    #[test]
    fn artifacts_are_dynamic_and_metadata_delete_preserves_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let path = dir.path().join("test.apk");
        fs::write(&path, "apk").unwrap();
        store.start(dir.path(), record("a", "p")).unwrap();
        store.artifact(dir.path(), "a", &path).unwrap();
        store.artifact(dir.path(), "a", &path).unwrap();
        assert!(store.delete(dir.path(), "a").is_err());
        let records = store.list(dir.path(), None).unwrap();
        assert_eq!(records[0].artifacts.len(), 1);
        assert!(records[0].artifacts[0].exists);
        fs::remove_file(&path).unwrap();
        assert!(!store.list(dir.path(), None).unwrap()[0].artifacts[0].exists);
        fs::write(&path, "apk").unwrap();
        store.finish(dir.path(), "a", "success").unwrap();
        store.delete(dir.path(), "a").unwrap();
        assert!(path.exists());
        assert!(store.list(dir.path(), None).unwrap().is_empty());
    }
    #[test]
    fn corrupt_history_is_never_overwritten_even_after_successful_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        store.load(dir.path()).unwrap();
        let path = dir.path().join("build-history.json");
        for bad in ["{broken", "{}", "null"] {
            fs::write(&path, bad).unwrap();
            assert!(store.load(dir.path()).is_err());
            assert!(store.start(dir.path(), record("a", "p")).is_err());
            assert!(store.recover(dir.path()).is_err());
            assert!(store.delete(dir.path(), "a").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), bad);
        }
    }
    #[test]
    fn foreign_and_relative_paths_are_never_reported_as_existing() {
        assert!(!native_absolute("relative.apk"));
        #[cfg(not(windows))]
        for path in [
            r"C:\build\app.apk",
            r"\\server\build\app.apk",
            "//server/build/app.apk",
        ] {
            assert!(!native_absolute(path));
        }
        #[cfg(windows)]
        assert!(!native_absolute("/tmp/app.apk"));
        let dir = tempfile::tempdir().unwrap();
        assert!(native_absolute(dir.path().to_str().unwrap()));
    }
    #[test]
    fn write_failure_does_not_commit_and_summaries_never_include_raw_errors() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        store.start(dir.path(), record("a", "p")).unwrap();
        let path = dir.path().join("build-history.json");
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(store.finish(dir.path(), "a", "success").is_err());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, original).unwrap();
        assert_eq!(store.list(dir.path(), None).unwrap()[0].status, "running");
        assert!(store.finish(dir.path(), "a", "SECRET_TOKEN=value").is_err());
        store.finish(dir.path(), "a", "error").unwrap();
        assert_eq!(
            store.list(dir.path(), None).unwrap()[0].summary,
            "构建失败，请在任务面板查看详情"
        );
        assert!(!fs::read_to_string(path).unwrap().contains("SECRET_TOKEN"));
    }
    #[test]
    fn concurrent_writes_keep_all_records() {
        let dir = tempfile::tempdir().unwrap();
        let store = std::sync::Arc::new(Store::default());
        std::thread::scope(|scope| {
            for i in 0..24 {
                let store = store.clone();
                let path = dir.path();
                scope.spawn(move || {
                    let id = i.to_string();
                    store.start(path, record(&id, "p")).unwrap();
                    store.finish(path, &id, "success").unwrap();
                });
            }
        });
        assert_eq!(Store::default().list(dir.path(), None).unwrap().len(), 24);
    }
    #[test]
    fn limit_is_per_project_and_never_removes_running() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        store.start(dir.path(), record("running", "p")).unwrap();
        for i in 0..105 {
            let id = format!("done-{i}");
            store.start(dir.path(), record(&id, "p")).unwrap();
            store.finish(dir.path(), &id, "success").unwrap();
        }
        store.start(dir.path(), record("other", "q")).unwrap();
        store.finish(dir.path(), "other", "error").unwrap();
        let records = store.list(dir.path(), Some("p")).unwrap();
        assert_eq!(records.len(), 101);
        assert!(records.iter().any(|r| r.id == "running"));
        assert!(!records.iter().any(|r| r.id == "done-0"));
        assert_eq!(store.list(dir.path(), Some("q")).unwrap().len(), 1);
    }
}
