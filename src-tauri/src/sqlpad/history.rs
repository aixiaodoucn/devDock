//! 生产查询结果仅保存于本机私有目录，按开始顺序保留最近的执行记录。
mod store;
use super::*;
use std::{fs, path::Path, sync::Mutex as SyncMutex};
pub(crate) use store::History;
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

pub(crate) fn directory(app: &AppHandle) -> Result<std::path::PathBuf> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|_| "无法定位执行记录")?
        .join("sqlpad-history"))
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
    let source = app.state::<History>().detail(&directory(&app)?, &id)?;
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

#[tauri::command]
pub(crate) async fn sqlpad_execution_history(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<Vec<store::Summary>> {
    ensure_backend_window(&window)?;
    app.state::<History>().list(&directory(&app)?)
}

#[derive(Serialize)]
pub(crate) struct Detail {
    sql: String,
    result: Option<Value>,
    error: Option<String>,
}
#[tauri::command]
pub(crate) async fn sqlpad_execution_detail(
    window: WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<Detail> {
    ensure_backend_window(&window)?;
    let record = app.state::<History>().detail(&directory(&app)?, &id)?;
    Ok(Detail {
        sql: record.sql,
        result: record.result,
        error: record.error,
    })
}
#[tauri::command]
pub(crate) async fn sqlpad_history_limit(window: WebviewWindow, app: AppHandle) -> Result<usize> {
    ensure_backend_window(&window)?;
    app.state::<History>().limit(&directory(&app)?)
}
#[tauri::command]
pub(crate) async fn sqlpad_set_history_limit(
    window: WebviewWindow,
    app: AppHandle,
    limit: usize,
) -> Result<usize> {
    ensure_backend_window(&window)?;
    app.state::<History>().set_limit(&directory(&app)?, limit)
}
#[tauri::command]
pub(crate) async fn sqlpad_delete_execution(
    window: WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<()> {
    ensure_backend_window(&window)?;
    app.state::<History>().remove(&directory(&app)?, Some(&id))
}
#[tauri::command]
pub(crate) async fn sqlpad_clear_history(window: WebviewWindow, app: AppHandle) -> Result<()> {
    ensure_backend_window(&window)?;
    app.state::<History>().remove(&directory(&app)?, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use store::detail_path;

    fn read(dir: &Path) -> Result<Vec<Execution>> {
        let store = History::default();
        store
            .list(dir)?
            .into_iter()
            .map(|r| store.detail(dir, &r.id))
            .collect()
    }
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
    fn summaries_do_not_read_details_and_clear_keeps_retention() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let history = History::default();
        history.set_limit(dir, 2).unwrap();
        let id = history
            .start(dir, "query", "db", "SELECT private", None, None)
            .unwrap();
        let other = history
            .start(dir, "query", "db", "SELECT 2", None, None)
            .unwrap();
        fs::write(detail_path(dir, &id).unwrap(), b"corrupt").unwrap();
        let summaries = history.list(dir).unwrap();
        assert_eq!(summaries.len(), 2);
        assert!(!serde_json::to_string(&summaries)
            .unwrap()
            .contains("private"));
        assert!(history.detail(dir, &id).is_err());
        assert_eq!(history.detail(dir, &other).unwrap().sql, "SELECT 2");
        history.remove(dir, Some(&id)).unwrap();
        history.remove(dir, None).unwrap();
        history.finish(dir, &other, &Ok(Value::from(3))).unwrap();
        assert!(history.list(dir).unwrap().is_empty());
        assert_eq!(History::default().limit(dir).unwrap(), 2);
        assert!(!detail_path(dir, &other).unwrap().exists());
    }

    #[test]
    fn reducing_retention_after_migration_prunes_files_not_just_summaries() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let old: Vec<_> = [1, 3, 2].into_iter().map(|i| serde_json::json!({"id":i.to_string(),"startedAt":i,"tool":"query","connection":"db","sql":"SELECT 1","result":i,"error":null})).collect();
        fs::write(dir.join(FILE), serde_json::to_vec(&old).unwrap()).unwrap();
        let store = History::default();
        assert_eq!(store.list(dir).unwrap().len(), 3);
        assert!(!dir.join(FILE).exists());
        assert!(store.set_limit(dir, 0).is_err());
        assert!(store.set_limit(dir, 10001).is_err());
        store.set_limit(dir, 1).unwrap();
        assert_eq!(store.list(dir).unwrap()[0].id, "3");
        for id in ["1", "2"] {
            assert!(!detail_path(dir, id).unwrap().exists());
        }
        let new = store
            .start(dir, "query", "db", "SELECT 1", None, Some("3"))
            .unwrap();
        assert_eq!(store.list(dir).unwrap()[0].id, new);
        assert!(!detail_path(dir, "3").unwrap().exists());
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
