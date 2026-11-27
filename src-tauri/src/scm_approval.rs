//! 项目 SCM 和工作台 Git 写操作逐次审批，只向主窗口发送待审批详情。
use crate::{config::Result, ensure_backend_window};
use serde::Serialize;
use std::{
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, EventTarget, Manager, State, WebviewWindow};
use tokio::{sync::oneshot, time::Instant};

const APPROVAL_TTL: Duration = Duration::from_secs(60);
const EVENT: &str = "scm-project-approval-requested";
const EXPIRED: &str = "项目操作审批已过期或取消";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingApproval {
    request_id: String,
    tool: String,
    site: String,
    account: String,
    arguments: serde_json::Value,
    expires_at: u64,
}

struct Pending {
    details: PendingApproval,
    deadline: Instant,
    reply: oneshot::Sender<bool>,
}

#[derive(Default)]
pub(crate) struct Runtime(Mutex<Option<Pending>>);

impl Runtime {
    fn live(&self) -> Result<MutexGuard<'_, Option<Pending>>> {
        let mut slot = self.0.lock().map_err(|_| "项目操作审批状态不可用")?;
        if slot
            .as_ref()
            .is_some_and(|p| p.deadline <= Instant::now() || p.reply.is_closed())
        {
            slot.take();
        }
        Ok(slot)
    }

    fn decide(&self, request_id: &str, approved: bool) -> Result<()> {
        let mut slot = self.live()?;
        let pending = slot
            .take_if(|p| p.details.request_id == request_id)
            .ok_or(EXPIRED)?;
        pending.reply.send(approved).map_err(|_| EXPIRED.into())
    }

    async fn request(
        &self,
        tool: &str,
        site: &str,
        account: &str,
        arguments: &serde_json::Value,
        notify: impl Fn(&PendingApproval) -> Result<()>,
    ) -> Result<()> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let deadline = Instant::now() + APPROVAL_TTL;
        let expires_at = (SystemTime::now() + APPROVAL_TTL)
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间异常")?
            .as_millis() as u64;
        let (reply, decision) = oneshot::channel();
        let details = PendingApproval {
            request_id: request_id.clone(),
            tool: tool.into(),
            site: site.into(),
            account: account.into(),
            arguments: arguments.clone(),
            expires_at,
        };
        {
            let mut slot = self.live()?;
            if slot.is_some() {
                return Err("已有项目操作等待审批".into());
            }
            *slot = Some(Pending {
                details: details.clone(),
                deadline,
                reply,
            });
        }
        // Future 被调用方取消时也清除请求。旧请求的清理不能影响刚创建的新请求。
        let _cleanup = Cleanup(|| {
            if let Ok(mut slot) = self.0.lock() {
                slot.take_if(|p| p.details.request_id == request_id);
            }
        });
        notify(&details)?;
        match tokio::time::timeout_at(deadline, decision).await {
            Ok(Ok(true)) => Ok(()),
            Ok(Ok(false)) => Err("用户已拒绝本次项目操作请求".into()),
            _ => Err(EXPIRED.into()),
        }
    }
}

struct Cleanup<F: Fn()>(F);
impl<F: Fn()> Drop for Cleanup<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

pub(crate) async fn approve(
    app: &AppHandle,
    tool: &str,
    site: &str,
    account: &str,
    arguments: &serde_json::Value,
) -> Result<()> {
    let runtime = app.try_state::<Runtime>().ok_or("项目操作审批尚未初始化")?;
    let window = app
        .get_webview_window("main")
        .ok_or("DevDock 主窗口不存在")?;
    runtime
        .request(tool, site, account, arguments, |details| {
            window
                .emit_to(EventTarget::webview_window("main"), EVENT, details)
                .map_err(|_| "无法通知项目操作审批窗口".into())
        })
        .await
}

#[tauri::command]
pub(crate) fn list_scm_project_approvals(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
) -> Result<Vec<PendingApproval>> {
    ensure_backend_window(&window)?;
    Ok(runtime
        .live()?
        .as_ref()
        .map(|p| p.details.clone())
        .into_iter()
        .collect())
}

#[tauri::command]
pub(crate) fn resolve_scm_project_approval(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    request_id: String,
    approved: bool,
) -> Result<()> {
    ensure_backend_window(&window)?;
    runtime.decide(&request_id, approved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };

    fn start<'a>(
        runtime: &'a Runtime,
    ) -> (
        std::pin::Pin<Box<impl Future<Output = Result<()>> + 'a>>,
        String,
    ) {
        let mut task =
            Box::pin(
                runtime.request("create", "site", "user", &serde_json::Value::Null, |_| {
                    Ok(())
                }),
            );
        assert!(matches!(
            task.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        let details = runtime.live().unwrap().as_ref().unwrap().details.clone();
        assert_eq!(details.site, "site");
        assert_eq!(details.account, "user");
        (task, details.request_id)
    }

    fn run(test: impl Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(test);
    }

    #[test]
    fn decisions_are_explicit_bound_to_one_request_and_single_use() {
        run(async {
            let runtime = Runtime::default();
            let (first, id) = start(&runtime);
            assert!(runtime
                .request("update", "site", "user", &serde_json::Value::Null, |_| Ok(
                    ()
                ))
                .await
                .is_err());
            assert!(runtime.decide("wrong", true).is_err());
            runtime.decide(&id, false).unwrap();
            assert!(first.await.is_err());
            let (second, next) = start(&runtime);
            assert_ne!(id, next);
            assert!(runtime.decide(&id, true).is_err());
            runtime.decide(&next, true).unwrap();
            assert!(runtime.decide(&next, true).is_err());
            assert!(second.await.is_ok());
        });
    }

    #[test]
    fn cancellation_timeout_and_notification_failure_release_slot() {
        run(async {
            let runtime = Runtime::default();
            let (first, id) = start(&runtime);
            drop(first);
            assert!(runtime.decide(&id, true).is_err());
            let (expired, id) = start(&runtime);
            tokio::time::advance(APPROVAL_TTL).await;
            assert!(runtime.decide(&id, true).is_err());
            let (next, next_id) = start(&runtime);
            assert!(expired.await.is_err());
            assert_eq!(
                runtime.live().unwrap().as_ref().unwrap().details.request_id,
                next_id
            );
            drop(next);
            assert!(runtime
                .request(
                    "create",
                    "site",
                    "account",
                    &serde_json::Value::Null,
                    |_| Err("通知失败".into())
                )
                .await
                .is_err());
            assert!(runtime.live().unwrap().is_none());
            let (unanswered, _) = start(&runtime);
            assert!(unanswered.await.is_err());
            assert!(runtime.live().unwrap().is_none());
        });
    }
}
