//! 生产查询结果仅保存于本机私有目录，按开始顺序保留最近的执行记录。
use super::*;
use std::{fs, path::Path, sync::Mutex as SyncMutex};
use tauri::{AppHandle, Manager};

const LIMIT: usize = 100;
const FILE: &str = "executions.json";
const UNKNOWN: &str = "尚未取得执行结果；若调用已中断，请勿自动重试";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Execution {
    id: String,
    started_at: u64,
    tool: String,
    connection: String,
    sql: String,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Default)]
pub(crate) struct History(SyncMutex<()>);

fn read(dir: &Path) -> Result<Vec<Execution>> {
    match fs::read(dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "SQLPad 执行记录格式无效".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err("无法读取 SQLPad 执行记录".into()),
    }
}

pub(crate) fn directory(app: &AppHandle) -> Result<std::path::PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位执行记录")?
        .join("sqlpad-history"))
}

impl History {
    fn update(&self, dir: &Path, change: impl FnOnce(&mut Vec<Execution>)) -> Result<()> {
        let _guard = self.0.lock().map_err(|_| "SQLPad 执行记录不可用")?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .recursive(true)
            .create(dir)
            .map_err(|_| "无法创建执行记录目录")?;
        let mut records = read(dir)?;
        change(&mut records);
        let json = serde_json::to_string(&records).map_err(|_| "无法编码执行记录")?;
        crate::config::atomic_write(&dir.join(FILE), &json)
    }

    pub(crate) fn start(
        &self,
        dir: &Path,
        tool: &str,
        connection: &str,
        sql: &str,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.update(dir, |records| {
            records.insert(
                0,
                Execution {
                    id: id.clone(),
                    started_at: crate::build_history::now_ms(),
                    tool: tool.into(),
                    connection: connection.into(),
                    sql: sql.into(),
                    result: None,
                    error: Some(UNKNOWN.into()),
                },
            );
            records.truncate(LIMIT);
        })?;
        Ok(id)
    }

    pub(crate) fn finish(&self, dir: &Path, id: &str, outcome: &Result<Value>) -> Result<()> {
        self.update(dir, |records| {
            if let Some(record) = records.iter_mut().find(|r| r.id == id) {
                record.result = outcome.as_ref().ok().cloned();
                record.error = outcome
                    .as_ref()
                    .err()
                    .map(|e| crate::log_mcp::sqlpad_error(e).to_owned());
            }
        })
    }
}

#[tauri::command]
pub(crate) async fn sqlpad_execution_history(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<Execution>> {
    ensure_backend_window(&window)?;
    read(&directory(&app)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_corrupt_history_and_rejects_new_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        fs::write(&path, b"invalid").unwrap();
        assert!(History::default()
            .start(dir.path(), "query", "db", "SELECT 1")
            .is_err());
        assert_eq!(fs::read(path).unwrap(), b"invalid");
    }

    #[test]
    fn persists_sql_result_and_safe_error_in_start_order() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let history = History::default();
        let first = history.start(dir, "query", "db", "SELECT 1").unwrap();
        let second = history.start(dir, "describe", "db", "SELECT 2").unwrap();
        let result = serde_json::json!({"rows":[{"id":1}]});
        history
            .finish(dir, &second, &Err("private diagnostic".into()))
            .unwrap();
        history.finish(dir, &first, &Ok(result.clone())).unwrap();
        let records = read(dir).unwrap();
        assert_eq!(records[1].sql, "SELECT 1");
        assert_eq!(records[1].result, Some(result));
        assert!(records[1].error.is_none());
        assert_eq!(records[0].id, second);
        assert!(records[0].result.is_none());
        assert!(!records[0].error.as_ref().unwrap().contains("private"));
    }
}
