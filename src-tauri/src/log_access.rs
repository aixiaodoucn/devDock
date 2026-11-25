//! 在 MCP 协议层与 SCM 之间实施本机仓库映射授权，不向模型枚举整个工作台。
mod preferences;
use crate::log_bridge::{Record, Store, SystemStore};
use crate::{config::Result, ensure_backend_window, log_mcp, scm, tasks};
use preferences::Preferences;
use serde::Serialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::Mutex;

#[derive(Default)]
pub struct Runtime(pub Mutex<Access>, std::sync::OnceLock<Result<()>>);

#[derive(Default)]
pub struct Access {
    scan_all: bool,
    // 仅保存在当前服务内存。每次重新授权使用不同的标记，旧查询不能继承新授权。
    raw_grant: Option<std::sync::Arc<()>>,
    active_subject: Option<String>,
    server: Option<log_mcp::LogMcpServer>,
    cleanup_pending: bool,
    exiting: bool,
}
impl Access {
    fn set_raw(&mut self, enabled: bool) -> Result<bool> {
        if enabled {
            if self.server.is_none() || self.exiting {
                return Err("请先启用日志 MCP 服务".into());
            }
            self.raw_grant.get_or_insert_with(Default::default);
        } else {
            self.raw_grant = None;
        }
        Ok(enabled)
    }

    fn check_grant(&self, original: Option<&std::sync::Arc<()>>) -> Result<()> {
        if original.is_some_and(|grant| {
            !self
                .raw_grant
                .as_ref()
                .is_some_and(|current| std::sync::Arc::ptr_eq(current, grant))
        }) {
            return Err("Scope: 本次查询的原文授权已撤销".into());
        }
        Ok(())
    }

    fn publish(&mut self, server: log_mcp::LogMcpServer, store: &impl Store) -> Result<()> {
        // 写入失败时不发布监听句柄；即使凭据系统部分写入，旧记录也无法通过探测。
        self.cleanup_pending = true;
        if let Err(err) = store.save(&Record::from_server(&server)) {
            drop(server);
            self.cleanup_pending = store.clear().is_err();
            return Err(err);
        }
        self.server = Some(server);
        Ok(())
    }
    async fn stop(&mut self, store: &impl Store) -> Result<()> {
        self.raw_grant = None;
        self.active_subject = None;
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        if self.cleanup_pending {
            store.clear()?;
            self.cleanup_pending = false;
        }
        Ok(())
    }
    pub async fn revoke(&mut self) -> Result<()> {
        self.stop(&SystemStore).await
    }
}
// Tauri 正常退出时不保证执行托管状态的 Drop，只在主动启用后安装退出清理。
fn install_exit_hook(app: &AppHandle, runtime: &Runtime) -> Result<()> {
    runtime
        .1
        .get_or_init(|| {
            app.plugin(
                tauri::plugin::Builder::<_, ()>::new("log-mcp-lifecycle")
                    .on_event(|app, event| {
                        if matches!(event, tauri::RunEvent::Exit) {
                            let runtime = app.state::<Runtime>();
                            tauri::async_runtime::block_on(async {
                                let mut access = runtime.0.lock().await;
                                access.exiting = true;
                                if access.revoke().await.is_err() {
                                    eprintln!("退出时未能删除日志 MCP 专用凭据");
                                }
                            });
                        }
                    })
                    .build(),
            )
            .map_err(|_| "无法注册日志 MCP 退出清理".into())
        })
        .clone()
}

#[derive(Serialize)]
pub struct Status {
    enabled: bool,
    endpoint: String,
}

fn status(server: Option<&log_mcp::LogMcpServer>) -> Status {
    Status {
        enabled: server.is_some(),
        endpoint: server.map(|s| s.endpoint()).unwrap_or_default(),
    }
}

fn resolve_ids<'a>(
    catalog: &'a [(String, String)],
    names: &BTreeSet<&str>,
    selected: Option<&str>,
) -> Result<Vec<&'a str>> {
    // 先按每个本地名称判断是否已有精确项，再过滤调用者选择的 ID，避免手选绕过优先级。
    let exact: BTreeSet<_> = catalog
        .iter()
        .map(|(_, name)| name.as_str())
        .filter(|name| names.contains(name))
        .collect();
    let ids: BTreeSet<_> = catalog
        .iter()
        .filter_map(|(id, name)| {
            let matches = exact.contains(name.as_str())
                || name.rsplit_once('/').is_some_and(|(prefix, leaf)| {
                    !leaf.is_empty()
                        && prefix
                            .split('/')
                            .all(|part| !part.is_empty() && part != "." && part != "..")
                        && names.contains(leaf)
                        && !exact.contains(leaf)
                });
            (matches
                && !name.is_empty()
                && !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_digit())
                && selected.is_none_or(|value| value == id))
            .then_some(id.as_str())
        })
        .collect();
    if ids.is_empty() {
        return Err("日志站点没有对应的模块映射".into());
    }
    Ok(ids.into_iter().collect())
}

fn field<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| "工具参数不完整".into())
}

async fn dispatch(app: AppHandle, tool: &str, args: Value) -> Result<Value> {
    if matches!(tool, "prod_db_describe_table" | "prod_db_query_readonly") {
        let db = app.state::<crate::sqlpad::Runtime>();
        let prepared = db.prepare(tool, &args).await?;
        crate::db_approval::approve(&app, &prepared.connection, &prepared.preview).await?;
        return db.execute(prepared).await;
    }
    let cfg = tasks::load(&app).map_err(|_| "无法读取工作台映射".to_owned())?;
    let repos = cfg
        .projects
        .iter()
        .filter(|p| p.project_type == "backend")
        .flat_map(|p| p.sub_projects.iter());
    // MCP 专用会话；禁止回退到后端工作台 Cookie 或直接复用其他站点数字 ID。
    let scm = app.state::<scm::LogRuntime>();
    match tool {
        "resolve_log_target" => {
            let path = Path::new(field(&args, "repositoryPath")?);
            if !path.is_absolute() {
                return Err("请提供工作台中已配置的仓库绝对路径".into());
            }
            let names: BTreeSet<_> = repos
                .filter(|r| Path::new(&r.path) == path)
                .map(|r| r.scm_module_name.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if names.is_empty() {
                return Err(
                    "未找到已保存的仓库与 SCM 模块映射，请在 Dev Dock 后端工作台配置".into(),
                );
            }
            let catalog = scm.log_modules().await?;
            let ids = resolve_ids(
                &catalog,
                &names,
                args.get("moduleId").and_then(Value::as_str),
            )?;
            if ids.len() > 1 {
                let candidates: Vec<_> = catalog
                    .iter()
                    .filter(|(id, _)| ids.binary_search(&id.as_str()).is_ok())
                    .map(|(id, name)| json!({"moduleId":id,"moduleName":name}))
                    .collect();
                return Ok(
                    json!({"status":"ambiguous", "moduleIds": ids, "candidates":candidates, "message":"匹配到多个日志模块，请根据完整模块名选择 moduleId 再定位"}),
                );
            }
            serde_json::to_value(scm.resolve_prodlog(ids[0].to_owned()).await?)
                .map_err(|_| "无法编码查询目标".into())
        }
        "search_prod_logs" | "scan_prod_logs" => {
            let runtime = app.state::<Runtime>();
            let (scope, grant) = {
                let access = runtime.0.lock().await;
                (access.scan_all, access.raw_grant.clone())
            };
            let module = field(&args, "moduleId")?;
            // 直接调用 search 也必须重新检查当前映射；删除配置后不能继续查询。
            let names = repos
                .map(|r| r.scm_module_name.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if resolve_ids(&scm.log_modules().await?, &names, Some(module)).is_err() {
                return Err("模块不在当前已保存的后端仓库映射内".into());
            }
            if tool == "scan_prod_logs" {
                let targets = scan_targets(scope, &args)?;
                let result = scm
                    .scan_prodlog(
                        module.into(),
                        targets,
                        field(&args, "logId")?.into(),
                        field(&args, "keyword")?.into(),
                        args.get("maxLines").and_then(Value::as_u64).unwrap_or(0) as usize,
                        args.get("archiveHour")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        grant.is_some(),
                    )
                    .await?;
                runtime.0.lock().await.check_grant(grant.as_ref())?;
                return serde_json::to_value(result).map_err(|_| "无法编码查询结果".into());
            }
            let result = scm
                .search_prodlog(
                    module.into(),
                    field(&args, "targetId")?.into(),
                    field(&args, "logId")?.into(),
                    field(&args, "keyword")?.into(),
                    args.get("maxLines").and_then(Value::as_u64).unwrap_or(20) as usize,
                    args.get("archiveHour")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    grant.is_some(),
                )
                .await?;
            runtime.0.lock().await.check_grant(grant.as_ref())?;
            serde_json::to_value(result).map_err(|_| "无法编码查询结果".into())
        }
        _ => Err("未知日志工具".into()),
    }
}

fn scan_targets(all_allowed: bool, args: &Value) -> Result<scm::prodlog::ScanTargets> {
    use scm::prodlog::ScanTargets;
    if args.get("allTargets").and_then(Value::as_bool) == Some(true) && !all_allowed {
        return Err("Scope: 当前设置只允许单实例查询".into());
    }
    Ok(match args.get("targetId").and_then(Value::as_str) {
        Some(target) => ScanTargets::One(target.into()),
        None if all_allowed => ScanTargets::All,
        None => ScanTargets::First,
    })
}

#[tauri::command]
pub(crate) fn mcp_tool_catalog(window: WebviewWindow) -> Result<Vec<log_mcp::CatalogEntry>> {
    ensure_backend_window(&window)?;
    Ok(log_mcp::tool_catalog())
}

#[tauri::command]
pub async fn log_mcp_scan_scope(app: AppHandle, window: WebviewWindow) -> Result<&'static str> {
    ensure_backend_window(&window)?;
    let all = Preferences::load(&tasks::data_dir(&app)?)?.scan_all;
    Ok(if all { "all" } else { "first" })
}

#[tauri::command]
pub async fn log_mcp_set_scan_scope(
    app: AppHandle,
    window: WebviewWindow,
    scope: String,
) -> Result<String> {
    ensure_backend_window(&window)?;
    let all = match scope.as_str() {
        "first" => false,
        "all" => true,
        _ => return Err("无效的实例范围".into()),
    };
    let runtime = app.state::<Runtime>();
    let mut access = runtime.0.lock().await;
    let dir = tasks::data_dir(&app)?;
    let mut prefs = Preferences::load(&dir)?;
    prefs.scan_all = all;
    prefs.save(&dir)?;
    access.scan_all = all;
    Ok(scope)
}

#[tauri::command]
pub async fn log_mcp_raw_status(app: AppHandle, window: WebviewWindow) -> Result<bool> {
    ensure_backend_window(&window)?;
    let enabled = app.state::<Runtime>().0.lock().await.raw_grant.is_some();
    Ok(enabled)
}

#[tauri::command]
pub async fn log_mcp_set_raw(app: AppHandle, window: WebviewWindow, enabled: bool) -> Result<bool> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let mut access = runtime.0.lock().await;
    if !enabled {
        // 即使磁盘写入失败，也立即终止当前授权；向界面报告持久化失败，以便重试。
        access.set_raw(false)?;
    }
    let subject = if enabled {
        Some(
            access
                .active_subject
                .clone()
                .filter(|_| access.server.is_some() && !access.exiting)
                .ok_or("请先启用日志 MCP 服务")?,
        )
    } else {
        None
    };
    let dir = tasks::data_dir(&app)?;
    let mut prefs = Preferences::load(&dir)?;
    prefs.raw_subject = subject;
    prefs.save(&dir)?;
    access.set_raw(enabled)
}

#[tauri::command]
pub async fn log_mcp_status(app: AppHandle, window: WebviewWindow) -> Result<Status> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let guard = runtime.0.lock().await;
    Ok(status(guard.server.as_ref()))
}

#[tauri::command]
pub async fn log_mcp_enable(app: AppHandle, window: WebviewWindow) -> Result<Status> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    install_exit_hook(&app, &runtime)?;
    let mut guard = runtime.0.lock().await;
    if guard.exiting {
        return Err("工作台正在退出".into());
    }
    if guard.server.is_none() {
        // 两种数据源共享本机传输，不共享登录。SQLPad 已选连接时不要求日志站点在线。
        let sql_ready = app.state::<crate::sqlpad::Runtime>().ready().await;
        let subject = if sql_ready && !app.state::<scm::LogRuntime>().has_live_session().await {
            None
        } else {
            app.state::<scm::LogRuntime>()
                .status()
                .await
                .ok()
                .filter(|s| s.connected)
                .and_then(|s| Some(preferences::subject(&s.base_url?, &s.username?)))
        };
        if subject.is_none() && !sql_ready {
            return Err("请先登录日志 SCM，或登录 SQLPad 并选定只读连接".into());
        }
        let prefs = Preferences::load(&tasks::data_dir(&app)?)?;
        let handle = app.clone();
        let server = log_mcp::start_log_mcp(true, 0, move |name, args| {
            dispatch(handle.clone(), name, args)
        })
        .await
        .ok()
        .flatten()
        .ok_or_else(|| "无法启动本机日志 MCP 服务".to_owned())?;
        guard.publish(server, &SystemStore)?;
        guard.scan_all = prefs.scan_all;
        guard.set_raw(subject.is_some() && prefs.raw_subject == subject)?;
        guard.active_subject = subject;
    }
    Ok(status(guard.server.as_ref()))
}

#[tauri::command]
pub async fn log_mcp_token(app: AppHandle, window: WebviewWindow) -> Result<String> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let guard = runtime.0.lock().await;
    guard
        .server
        .as_ref()
        .map(|s| s.bearer_token().to_owned())
        .ok_or_else(|| "日志 MCP 未启用".into())
}

#[tauri::command]
pub async fn log_mcp_disable(app: AppHandle, window: WebviewWindow) -> Result<Status> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let mut guard = runtime.0.lock().await;
    // 用户显式停用也清理上一次异常退出留下的记录。
    guard.cleanup_pending = true;
    guard.revoke().await?;
    Ok(status(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_failure_stops_service_and_allows_retry() {
        use std::cell::Cell;
        struct MockStore {
            fail_clear: Cell<bool>,
            stored: Cell<bool>,
        }
        impl Store for MockStore {
            fn load(&self) -> Result<Record> {
                Err("not used".into())
            }
            fn save(&self, _: &Record) -> Result<()> {
                self.stored.set(true);
                Ok(())
            }
            fn clear(&self) -> Result<()> {
                if self.fail_clear.get() {
                    return Err("locked".into());
                }
                self.stored.set(false);
                Ok(())
            }
        }
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let store = MockStore {
                    fail_clear: Cell::new(true),
                    stored: Cell::new(false),
                };
                let mut access = Access::default();
                let server = log_mcp::start_log_mcp(true, 0, |_, _| async { Ok(Value::Null) })
                    .await
                    .unwrap()
                    .unwrap();
                let port = server.port();
                assert!(access.set_raw(true).is_err());
                access.publish(server, &store).unwrap();
                access.set_raw(true).unwrap();
                let first_grant = access.raw_grant.clone();
                access.set_raw(false).unwrap();
                access.set_raw(true).unwrap();
                assert!(access.check_grant(first_grant.as_ref()).is_err());
                let second_grant = access.raw_grant.clone();
                assert!(access.stop(&store).await.is_err());
                assert!(access.check_grant(second_grant.as_ref()).is_err());
                assert!(access.server.is_none() && access.cleanup_pending && store.stored.get());
                assert!(
                    tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                        .await
                        .is_err()
                );
                store.fail_clear.set(false);
                access.stop(&store).await.unwrap();
                assert!(!store.stored.get() && !access.cleanup_pending);
            });
    }

    #[test]
    fn scan_scope_defaults_to_first_and_client_cannot_expand_it() {
        use scm::prodlog::ScanTargets;
        assert!(matches!(
            scan_targets(false, &json!({})),
            Ok(ScanTargets::First)
        ));
        assert!(scan_targets(false, &json!({"allTargets":true})).is_err());
        assert!(matches!(
            scan_targets(true, &json!({})),
            Ok(ScanTargets::All)
        ));
        assert!(
            matches!(scan_targets(true, &json!({"targetId":"t1"})), Ok(ScanTargets::One(id)) if id == "t1")
        );
    }

    #[test]
    fn namespace_matching_never_overrides_an_exact_mapping() {
        let mut catalog = vec![
            ("1".into(), "alpha/sample".into()),
            ("2".into(), "beta/nested/sample".into()),
            ("3".into(), "alpha/sample-extra".into()),
        ];
        let names = BTreeSet::from(["sample"]);
        assert_eq!(resolve_ids(&catalog[..1], &names, None).unwrap(), ["1"]);
        assert_eq!(resolve_ids(&catalog, &names, None).unwrap(), ["1", "2"]);
        assert_eq!(resolve_ids(&catalog, &names, Some("2")).unwrap(), ["2"]);
        assert!(resolve_ids(&catalog, &BTreeSet::from(["alpha/sample"]), Some("2")).is_err());
        catalog.push(("4".into(), "sample".into()));
        assert_eq!(resolve_ids(&catalog, &names, None).unwrap(), ["4"]);
        assert!(resolve_ids(&catalog, &names, Some("1")).is_err());
        assert_eq!(
            resolve_ids(&catalog, &BTreeSet::from(["sample", "alpha/sample"]), None).unwrap(),
            ["1", "4"]
        );
    }

    #[test]
    fn resolution_requires_exact_configured_repository_and_disambiguates() {
        let catalog = vec![
            ("7".into(), "other".into()),
            ("12".into(), "example".into()),
            ("27".into(), "example".into()),
        ];
        let names = BTreeSet::from(["example"]);
        assert_eq!(
            resolve_ids(&catalog, &names, None).unwrap(),
            vec!["12", "27"]
        );
        assert_eq!(
            resolve_ids(&catalog, &names, Some("27")).unwrap(),
            vec!["27"]
        );
        // 相同数字在日志站点属于另一个模块，不能因后端 SCM 中曾配置此数字而授权。
        assert!(resolve_ids(&catalog, &names, Some("7")).is_err());
        assert!(resolve_ids(&catalog, &BTreeSet::from(["exam"]), None).is_err());
    }
}
