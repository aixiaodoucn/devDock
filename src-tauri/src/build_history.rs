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

#[derive(Default)]
pub struct Store {
    lock: Mutex<()>,
}

impl Store {
    pub fn load(&self, dir: &Path) -> Result<()> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        read(dir).map(|_| ())
    }

    pub fn list(&self, dir: &Path, project_id: Option<&str>) -> Result<Vec<BuildRecord>> {
        let _guard = self.lock.lock().map_err(|_| "构建历史锁异常")?;
        let mut records = read(dir)?;
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
        let mut records = read(dir)?;
        let mut count = 0;
        let now = now_ms();
        for record in &mut records {
            if record.status == "running" {
                complete(record, "interrupted", now)?;
                count += 1;
            }
        }
        if count > 0 {
            trim(&mut records);
            write(dir, &records)?;
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
        let mut records = read(dir)?;
        change(&mut records)?;
        trim(&mut records);
        write(dir, &records)
    }
}

fn native_absolute(path: &str) -> bool {
    Path::new(path).is_absolute() && (cfg!(windows) || !path.starts_with("//"))
}

fn read(dir: &Path) -> Result<Vec<BuildRecord>> {
    let text = match fs::read_to_string(dir.join("build-history.json")) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(format!("无法读取构建历史: {error}")),
    };
    let records: Vec<BuildRecord> = serde_json::from_str(&text)
        .map_err(|_| "构建历史文件损坏，已保留原文件，请先备份并修复".to_string())?;
    let mut ids = std::collections::HashSet::new();
    for record in &records {
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
    Ok(records)
}

fn write(dir: &Path, records: &[BuildRecord]) -> Result<()> {
    let text = serde_json::to_string_pretty(records).map_err(|e| e.to_string())?;
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

fn trim(records: &mut Vec<BuildRecord>) {
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
        if *count > 100 {
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
