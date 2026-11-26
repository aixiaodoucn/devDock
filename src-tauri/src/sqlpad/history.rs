//! 生产查询结果仅保存于本机私有目录，按开始顺序保留最近的执行记录。
use super::*;
use std::{fs, path::Path, sync::Mutex as SyncMutex};
use tauri::{AppHandle, Manager};

const LIMIT: usize = 100;
const FILE: &str = "executions.json";
const UNKNOWN: &str = "尚未取得执行结果；若调用已中断，请勿自动重试";

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Replay {
    pub(super) origin: String,
    pub(super) connection_id: String,
    pub(super) driver: String,
    pub(super) args: Value,
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replay: Option<Replay>,
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
    fn update(
        &self,
        dir: &Path,
        change: impl FnOnce(&mut Vec<Execution>) -> Result<()>,
    ) -> Result<()> {
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
        change(&mut records)?;
        let json = serde_json::to_string(&records).map_err(|_| "无法编码执行记录")?;
        crate::config::atomic_write(&dir.join(FILE), &json)
    }

    fn start(
        &self,
        dir: &Path,
        tool: &str,
        connection: &str,
        sql: &str,
        replay: Option<&Replay>,
        parent: Option<&str>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.update(dir, |records| {
            let mut index = match parent {
                Some(id) => {
                    records
                        .iter()
                        .position(|r| r.id == id)
                        .ok_or("原记录已移除")?
                        + 1
                }
                None => 0,
            };
            // 展示顺序不等于时间顺序。按时间淘汰最老项，保留本次原记录和新记录。
            if records.len() >= LIMIT {
                let oldest = records
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, r)| Some(r.id.as_str()) != parent)
                    .min_by_key(|(_, r)| r.started_at)
                    .map(|(i, _)| i)
                    .ok_or("执行记录不可用")?;
                records.remove(oldest);
                index -= usize::from(oldest < index);
            }
            records.insert(
                index,
                Execution {
                    id: id.clone(),
                    started_at: crate::build_history::now_ms(),
                    tool: tool.into(),
                    connection: connection.into(),
                    sql: sql.into(),
                    replay: replay.cloned(),
                    result: None,
                    error: Some(UNKNOWN.into()),
                },
            );
            Ok(())
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
            Ok(())
        })
    }
}

pub(crate) async fn execute_recorded(
    app: &AppHandle,
    tool: &str,
    prepared: query::Prepared,
    parent: Option<&str>,
) -> Result<Value> {
    let history = app.state::<History>();
    let dir = directory(app).map_err(|_| "DB_HISTORY_START")?;
    let id = history
        .start(
            &dir,
            tool,
            &prepared.connection,
            &prepared.preview,
            Some(&prepared.replay),
            parent,
        )
        .map_err(|_| "DB_HISTORY_START")?;
    let result = app.state::<Runtime>().execute(prepared).await;
    history
        .finish(&dir, &id, &result)
        .map_err(|_| "DB_HISTORY_FINISH")?;
    result
}

#[tauri::command]
pub(crate) async fn sqlpad_rerun_execution(
    window: WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<()> {
    ensure_backend_window(&window)?;
    let source = read(&directory(&app)?)?
        .into_iter()
        .find(|row| row.id == id)
        .ok_or("找不到原执行记录，请刷新")?;
    let replay = source
        .replay
        .ok_or("旧记录缺少原始查询参数，无法重新执行")?;
    // 手动点击即授权这一次重跑；只接受记录 ID，重新编译只读参数，不执行磁盘上的 SQL 文本。
    let db = app.state::<Runtime>();
    let prepared = db.prepare_rerun(&source.tool, &replay, &source.sql).await?;
    execute_recorded(&app, &source.tool, prepared, Some(&id))
        .await
        .map(|_| ())
        .map_err(|e| crate::log_mcp::sqlpad_error(&e).into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecutionView {
    #[serde(flatten)]
    record: Execution,
    can_rerun: bool,
}

#[tauri::command]
pub(crate) async fn sqlpad_execution_history(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<ExecutionView>> {
    ensure_backend_window(&window)?;
    Ok(read(&directory(&app)?)?
        .into_iter()
        .map(|mut record| {
            let can_rerun = record.replay.take().is_some();
            ExecutionView { record, can_rerun }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rerun_is_inserted_after_source_without_changing_its_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let history = History::default();
        let dir = temp.path();
        let replay = Replay {
            origin: "https://fixture.invalid".into(),
            connection_id: "one".into(),
            driver: "mysql".into(),
            args: serde_json::json!({"table":"demo","columns":["id"]}),
        };
        let old = history
            .start(dir, "query", "db", "SELECT 1", Some(&replay), None)
            .unwrap();
        history.finish(dir, &old, &Ok(Value::from(1))).unwrap();
        let other = history
            .start(dir, "query", "db", "SELECT 2", None, None)
            .unwrap();
        let new = history
            .start(dir, "query", "db", "SELECT 1", Some(&replay), Some(&old))
            .unwrap();
        history.finish(dir, &new, &Ok(Value::from(2))).unwrap();
        let rows = read(dir).unwrap();
        assert_eq!(
            [&rows[0].id, &rows[1].id, &rows[2].id],
            [&other, &old, &new]
        );
        assert_eq!(rows[1].result, Some(Value::from(1)));
        assert_eq!(rows[2].result, Some(Value::from(2)));
        assert_eq!(rows[2].replay.as_ref().unwrap().connection_id, "one");
    }

    #[test]
    fn rerun_of_oldest_record_at_capacity_preserves_both_records() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let legacy: Vec<_> = (0..LIMIT).rev().map(|i| serde_json::json!({
            "id":i.to_string(), "startedAt":i, "tool":"query", "connection":"db", "sql":"SELECT 1", "result":i, "error":null
        })).collect();
        fs::write(dir.join(FILE), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let id = History::default()
            .start(dir, "query", "db", "SELECT 1", None, Some("0"))
            .unwrap();
        let rows = read(dir).unwrap();
        assert_eq!(rows.len(), LIMIT);
        assert_eq!(rows[LIMIT - 2].id, "0");
        assert_eq!(rows[LIMIT - 2].result, Some(Value::from(0)));
        assert_eq!(rows[LIMIT - 1].id, id);
        assert!(!rows.iter().any(|r| r.id == "1"));
    }

    #[test]
    fn preserves_corrupt_history_and_rejects_new_execution() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        fs::write(&path, b"invalid").unwrap();
        assert!(History::default()
            .start(dir.path(), "query", "db", "SELECT 1", None, None)
            .is_err());
        assert_eq!(fs::read(path).unwrap(), b"invalid");
    }

    #[test]
    fn persists_sql_result_and_safe_error_in_start_order() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let history = History::default();
        let first = history
            .start(dir, "query", "db", "SELECT 1", None, None)
            .unwrap();
        let second = history
            .start(dir, "describe", "db", "SELECT 2", None, None)
            .unwrap();
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
