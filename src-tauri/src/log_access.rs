//! 在 MCP 协议层与 SCM 之间实施本机仓库映射授权，不向模型枚举整个工作台。
mod preferences;
use crate::log_bridge::{LocalStore, Record, Store};
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
    service_lease: Option<tokio::sync::watch::Sender<()>>,
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

    fn save_raw_preference(&mut self, dir: &Path, subject: &str, enabled: bool) -> Result<bool> {
        // 先撤销内存授权，即使偏好保存失败，已在途的原文查询也不能继续交付。
        if !enabled {
            self.set_raw(false)?;
        }
        let mut prefs = Preferences::load(dir)?;
        if enabled {
            prefs.raw_subject = Some(subject.to_owned());
        } else if prefs.raw_subject.as_deref() == Some(subject) {
            prefs.raw_subject = None;
        }
        prefs.save(dir)?;
        // 保存偏好不启动服务；只有运行中的同身份日志服务可以取得实时授权。
        if enabled && self.active_subject.as_deref() == Some(subject) && self.server.is_some() {
            self.set_raw(true)?;
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
        self.service_lease.take();
        if let Some(server) = self.server.take() {
            server.shutdown().await;
        }
        if self.cleanup_pending {
            store.clear()?;
            self.cleanup_pending = false;
        }
        Ok(())
    }
    pub(crate) async fn disable_and_forget(&mut self, app: &AppHandle) -> Result<()> {
        // 即使无法写入偏好，也先终止本次监听，不让旧账号继续访问。
        let saved = tasks::data_dir(app).and_then(|dir| Preferences::set_resume(&dir, false));
        self.revoke().await?;
        saved
    }
    pub async fn revoke(&mut self) -> Result<()> {
        self.stop(&LocalStore).await
    }
}
// 关闭应用只清理运行时监听和令牌，不改写用户下次启动的偏好。
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

async fn project_call(app: &AppHandle, tool: &str, args: Value) -> Result<Value> {
    let runtime = app.state::<scm::Runtime>();
    if tool == "scm_project_options" {
        return match args.get("projectId") {
            Some(id) => serde_json::to_value(
                runtime
                    .edit_options(id.as_str().ok_or("项目 ID 无效")?.into())
                    .await?,
            ),
            None => serde_json::to_value(runtime.create_options().await?),
        }
        .map_err(|_| "无法编码项目选项".into());
    }
    let binding = runtime.session_binding().await?;
    crate::scm_approval::approve(app, tool, &binding.site, &binding.account, &args).await?;
    if tool == "scm_remove_project_module" {
        let project_id = field(&args, "projectId")?.to_owned();
        let module_id = field(&args, "moduleId")?.to_owned();
        runtime
            .approved_remove_module(
                &binding,
                project_id.clone(),
                module_id.clone(),
                field(&args, "revision")?.into(),
            )
            .await?;
        return Ok(json!({"projectId":project_id,"moduleId":module_id}));
    }
    let input = serde_json::from_value(args["input"].clone()).map_err(|_| "项目参数无效")?;
    let id = match tool {
        "scm_create_project" => runtime.approved_create(&binding, input).await?,
        "scm_update_project" => {
            let id = field(&args, "projectId")?.to_owned();
            let authors =
                serde_json::from_value(args["authors"].clone()).map_err(|_| "负责人参数无效")?;
            runtime
                .approved_update(
                    &binding,
                    id.clone(),
                    input,
                    authors,
                    field(&args, "revision")?.into(),
                )
                .await?;
            id
        }
        _ => return Err("未知项目工具".into()),
    };
    Ok(json!({"projectId":id}))
}

async fn while_service_active(
    mut lease: tokio::sync::watch::Receiver<()>,
    task: impl std::future::Future<Output = Result<Value>>,
) -> Result<Value> {
    // 每个监听实例拥有独立生命周期；停用后旧连接和未决审批不能继承下次启动的授权。
    use std::{
        future::{poll_fn, Future},
        pin::pin,
        task::Poll,
    };
    let mut stopped = pin!(lease.changed());
    let mut task = pin!(task);
    poll_fn(|cx| {
        if stopped.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Err("MCP 服务已停用；已提交的写入请先核对结果".into()));
        }
        task.as_mut().poll(cx)
    })
    .await
}

async fn dispatch(
    app: AppHandle,
    lease: tokio::sync::watch::Receiver<()>,
    tool: &str,
    args: Value,
) -> Result<Value> {
    if matches!(
        tool,
        "scm_project_options"
            | "scm_create_project"
            | "scm_update_project"
            | "scm_remove_project_module"
    ) {
        return while_service_active(lease, project_call(&app, tool, args)).await;
    }
    if matches!(
        tool,
        "backend_git_prepare" | "backend_git_execute" | "backend_git_state"
    ) {
        return while_service_active(lease, crate::project_git::dispatch(&app, tool, &args)).await;
    }
    if matches!(tool, "prod_db_describe_table" | "prod_db_query_readonly") {
        let db = app.state::<crate::sqlpad::Runtime>();
        let prepared = db.prepare(tool, &args).await?;
        if !prepared.auto_approve {
            crate::db_approval::approve(&app, &prepared.connection, &prepared.preview).await?;
        }
        return crate::sqlpad::history::execute_recorded(&app, tool, prepared, None).await;
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
pub(crate) fn mcp_tool_catalog(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Vec<log_mcp::CatalogEntry>> {
    ensure_backend_window(&window)?;
    Ok(log_mcp::tool_catalog(
        Preferences::load(&tasks::data_dir(&app)?)?.match_limit,
    ))
}

#[tauri::command]
pub fn log_mcp_match_limit(app: AppHandle, window: WebviewWindow) -> Result<usize> {
    ensure_backend_window(&window)?;
    Ok(Preferences::load(&tasks::data_dir(&app)?)?.match_limit)
}

#[tauri::command]
pub async fn log_mcp_set_match_limit(
    app: AppHandle,
    window: WebviewWindow,
    limit: usize,
) -> Result<usize> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let access = runtime.0.lock().await;
    let dir = tasks::data_dir(&app)?;
    let mut prefs = Preferences::load(&dir)?;
    prefs.match_limit = limit;
    prefs.save(&dir)?;
    // 持久化成功后才更新运行中的服务；已开始的查询沿用请求开始时的上限。
    if let Some(server) = &access.server {
        server.set_match_limit(limit);
    }
    Ok(limit)
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

async fn current_log_subject(app: &AppHandle) -> Result<String> {
    let (site, account) = app
        .state::<scm::LogRuntime>()
        .current_identity()
        .await
        .ok_or("请先登录日志 SCM")?;
    Ok(preferences::subject(&site, &account))
}

#[tauri::command]
pub async fn log_mcp_raw_status(app: AppHandle, window: WebviewWindow) -> Result<bool> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let _guard = runtime.0.lock().await;
    let subject = current_log_subject(&app).await?;
    Ok(Preferences::load(&tasks::data_dir(&app)?)?
        .raw_subject
        .as_ref()
        == Some(&subject))
}

#[tauri::command]
pub async fn log_mcp_set_raw(app: AppHandle, window: WebviewWindow, enabled: bool) -> Result<bool> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let mut access = runtime.0.lock().await;
    let subject = current_log_subject(&app).await?;
    access.save_raw_preference(&tasks::data_dir(&app)?, &subject, enabled)
}

#[tauri::command]
pub async fn log_mcp_status(app: AppHandle, window: WebviewWindow) -> Result<Status> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let guard = runtime.0.lock().await;
    Ok(status(guard.server.as_ref()))
}

pub(crate) async fn restore_on_startup(app: AppHandle) -> Result<()> {
    let runtime = app.state::<Runtime>();
    let mut guard = runtime.0.lock().await;
    // 与手动停用、切换账号共用锁，避免延迟到达的恢复重新打开已停用的服务。
    if guard.exiting
        || guard.server.is_some()
        || !Preferences::load(&tasks::data_dir(&app)?)?.resume_service
    {
        return Ok(());
    }
    enable_locked(&app, &mut guard, true).await.map(|_| ())
}

#[tauri::command]
pub async fn log_mcp_enable(app: AppHandle, window: WebviewWindow) -> Result<Status> {
    ensure_backend_window(&window)?;
    let runtime = app.state::<Runtime>();
    let mut guard = runtime.0.lock().await;
    enable_locked(&app, &mut guard, false).await
}

async fn enable_locked(app: &AppHandle, guard: &mut Access, restoring: bool) -> Result<Status> {
    if guard.exiting {
        return Err("工作台正在退出".into());
    }
    if guard.server.is_none() {
        // 三种数据源共享传输但不共享会话。保留日志授权判定，仅在其他来源均不可用时检查项目 SCM。
        let runtime = app.state::<Runtime>();
        install_exit_hook(app, &runtime)?;
        let db = app.state::<crate::sqlpad::Runtime>();
        let sql_ready = if restoring {
            db.restore_ready().await.unwrap_or(false)
        } else {
            db.ready().await
        };
        let subject = if !restoring
            && sql_ready
            && !app.state::<scm::LogRuntime>().has_live_session().await
        {
            None
        } else {
            app.state::<scm::LogRuntime>()
                .status()
                .await
                .ok()
                .filter(|s| s.connected)
                .and_then(|s| Some(preferences::subject(&s.base_url?, &s.username?)))
        };
        if subject.is_none()
            && !sql_ready
            && !app
                .state::<scm::Runtime>()
                .status()
                .await
                .is_ok_and(|s| s.connected)
        {
            return Err("请先登录项目 SCM、日志 SCM，或登录 SQLPad 并选定只读连接".into());
        }
        let prefs = Preferences::load(&tasks::data_dir(&app)?)?;
        let handle = app.clone();
        let (service, lease) = tokio::sync::watch::channel(());
        let server = log_mcp::start_log_mcp(true, 0, move |name, args| {
            dispatch(handle.clone(), lease.clone(), name, args)
        })
        .await
        .ok()
        .flatten()
        .ok_or_else(|| "无法启动本机日志 MCP 服务".to_owned())?;
        server.set_match_limit(prefs.match_limit);
        guard.publish(server, &LocalStore)?;
        guard.service_lease = Some(service);
        guard.scan_all = prefs.scan_all;
        guard.set_raw(subject.is_some() && prefs.raw_subject == subject)?;
        guard.active_subject = subject;
        if !restoring && !prefs.resume_service {
            if let Err(error) = Preferences::set_resume(&tasks::data_dir(app)?, true) {
                guard.revoke().await?;
                return Err(error);
            }
        }
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
    guard.disable_and_forget(&app).await?;
    Ok(status(None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_service_cannot_dispatch_even_after_restart() {
        tauri::async_runtime::block_on(async {
            let (old, lease) = tokio::sync::watch::channel(());
            drop(old);
            let (_active, new_lease) = tokio::sync::watch::channel(());
            assert!(
                while_service_active(lease, async { panic!("停用服务不能执行") })
                    .await
                    .is_err()
            );
            assert!(while_service_active(new_lease, async { Ok(Value::Null) })
                .await
                .is_ok());
        });
    }

    #[test]
    fn stopping_service_discards_in_flight_approval() {
        tauri::async_runtime::block_on(async {
            let (service, lease) = tokio::sync::watch::channel(());
            let (entered, waiting) = tokio::sync::oneshot::channel();
            let (reply, decision) = tokio::sync::oneshot::channel::<()>();
            let call = tokio::spawn(while_service_active(lease, async move {
                entered.send(()).unwrap();
                decision.await.unwrap();
                Ok(Value::Null)
            }));
            waiting.await.unwrap();
            drop(service);
            assert!(call.await.unwrap().is_err());
            assert!(reply.send(()).is_err());
        });
    }

    #[test]
    fn raw_preference_is_account_scoped_and_does_not_start_service() {
        let dir = tempfile::tempdir().unwrap();
        let alice = preferences::subject("https://example.invalid", "alice");
        let bob = preferences::subject("https://example.invalid", "bob");
        let mut access = Access::default();
        assert!(access
            .save_raw_preference(dir.path(), &alice, true)
            .unwrap());
        assert!(access.server.is_none() && access.raw_grant.is_none());
        assert_eq!(
            Preferences::load(dir.path())
                .unwrap()
                .raw_subject
                .as_deref(),
            Some(alice.as_str())
        );
        assert!(!access.save_raw_preference(dir.path(), &bob, false).unwrap());
        assert_eq!(
            Preferences::load(dir.path())
                .unwrap()
                .raw_subject
                .as_deref(),
            Some(alice.as_str())
        );
        assert!(!access
            .save_raw_preference(dir.path(), &alice, false)
            .unwrap());
        assert!(Preferences::load(dir.path()).unwrap().raw_subject.is_none());
    }

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
