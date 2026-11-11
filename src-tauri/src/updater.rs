use crate::{config::Result, tasks::Runtime};
use serde::Serialize;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState(tauri::async_runtime::Mutex<Session>);

#[derive(Default)]
struct Session {
    update: Option<Update>,
    bytes: Option<Arc<Vec<u8>>>,
}

#[derive(Clone, Serialize)]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
    downloaded: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    downloaded: u64,
    total: Option<u64>,
}

impl Session {
    fn info(&self) -> Option<UpdateInfo> {
        self.update.as_ref().map(|update| UpdateInfo {
            version: update.version.clone(),
            notes: update.body.clone(),
            downloaded: self.bytes.is_some(),
        })
    }

    fn cache_download(&mut self, bytes: Vec<u8>) -> Result<()> {
        let update = self.update.as_ref().ok_or("请先检查新版本")?;
        // 校验成功后才进入待安装状态；网络下载结束并不代表签名验证成功。
        update
            .verify(&bytes)
            .map_err(|error| format!("更新包签名校验失败：{error}"))?;
        self.bytes = Some(Arc::new(bytes));
        Ok(())
    }
}

fn ensure_main(window: &WebviewWindow) -> Result<()> {
    if window.label() != "main" {
        return Err("请在 DevDock 主窗口操作应用更新".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn check_update(app: AppHandle, window: WebviewWindow) -> Result<Option<UpdateInfo>> {
    ensure_main(&window)?;
    // 开发进程没有可替换的安装包，避免覆盖本地调试程序。
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    let state = app.state::<UpdateState>();
    let mut session = state
        .0
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    if session.bytes.is_some() {
        return Ok(session.info());
    }
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("无法初始化更新检查：{error}"))?;
    let candidate = updater.check().await.map_err(|error| match error {
        tauri_plugin_updater::Error::ReleaseNotFound => {
            "尚未发布可用的自动更新清单，请稍后重试或从 Releases 手动下载安装".to_owned()
        }
        other => format!("检查更新失败，请检查网络后重试：{other}"),
    })?;
    session.update = candidate;
    Ok(session.info())
}

#[tauri::command]
pub async fn download_update(
    app: AppHandle,
    window: WebviewWindow,
    on_progress: Channel<DownloadProgress>,
) -> Result<()> {
    ensure_main(&window)?;
    if cfg!(debug_assertions) {
        return Err("开发模式不下载应用更新，请使用安装版 DevDock".into());
    }
    let state = app.state::<UpdateState>();
    let mut session = state
        .0
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    if session.bytes.is_some() {
        return Ok(());
    }
    let update = session.update.as_mut().ok_or("请先检查新版本")?;
    update.timeout = Some(Duration::from_secs(20 * 60));
    let mut downloaded = 0_u64;
    let bytes = update
        .download(
            |length, total| {
                downloaded = downloaded.saturating_add(length as u64);
                let _ = on_progress.send(DownloadProgress { downloaded, total });
            },
            || {},
        )
        .await
        .map_err(|error| format!("下载或校验更新包失败，请重试：{error}"))?;
    session.cache_download(bytes)
}

fn validate_install_state(active: bool, shutting_down: bool) -> Result<()> {
    if shutting_down {
        return Err("应用正在退出或更新，请稍后重试".into());
    }
    if active {
        return Err("仍有任务正在运行，请等待任务完成后再重启更新".into());
    }
    Ok(())
}

fn begin_install(runtime: &Runtime) -> Result<()> {
    // 与任务注册及退出共用门锁，检查通过后不再接受新任务。
    let _guard = runtime.recovery_lock.lock().unwrap();
    validate_install_state(
        !runtime.tasks.lock().unwrap().is_empty(),
        runtime.shutting_down.load(Ordering::SeqCst),
    )?;
    runtime.shutting_down.store(true, Ordering::SeqCst);
    Ok(())
}

fn unlock_instance_files(current: &std::fs::File, legacy: Option<&std::fs::File>) -> Result<()> {
    // Tauri 重启会先拉起新进程，再结束旧进程；需提前释放两个发布身份的实例锁。
    if let Some(legacy) = legacy {
        legacy
            .unlock()
            .map_err(|error| format!("释放旧版实例锁失败：{error}"))?;
    }
    current
        .unlock()
        .map_err(|error| format!("释放实例锁失败：{error}"))
}

fn finish_failed_install(runtime: &Runtime) {
    let _guard = runtime.recovery_lock.lock().unwrap();
    runtime.shutting_down.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub async fn install_update(app: AppHandle, window: WebviewWindow) -> Result<()> {
    ensure_main(&window)?;
    if cfg!(debug_assertions) {
        return Err("开发模式不安装应用更新，请使用安装版 DevDock".into());
    }
    let state = app.state::<UpdateState>();
    let session = state
        .0
        .try_lock()
        .map_err(|_| "更新操作正在进行，请稍后重试")?;
    let update = session.update.clone().ok_or("请先检查新版本")?;
    if session.bytes.is_none() {
        return Err("请先下载并校验更新包".into());
    }
    begin_install(&app.state::<Runtime>())?;
    let bytes = session.bytes.as_ref().unwrap().clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        // 官方更新器安装前再次验证签名；Windows 安装器负责退出和重新启动。
        update.install(bytes.as_slice())
    })
    .await;
    match outcome {
        Ok(Err(error)) => {
            finish_failed_install(&app.state::<Runtime>());
            Err(format!(
                "安装更新失败，可重试；请确认应用位于可写的安装目录：{error}"
            ))
        }
        Err(error) => {
            finish_failed_install(&app.state::<Runtime>());
            Err(format!("更新任务异常，请重试：{error}"))
        }
        Ok(Ok(())) => {
            let locks = app.state::<(std::fs::File, Option<std::fs::File>)>();
            let (current, legacy) = locks.inner();
            if let Err(error) = unlock_instance_files(current, legacy.as_ref()) {
                finish_failed_install(&app.state::<Runtime>());
                return Err(format!(
                    "更新已安装，但{error}；请手动退出并重新打开 DevDock"
                ));
            }
            app.restart();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use tauri_plugin_updater::UpdaterExt;

    #[test]
    fn update_installation_reserves_task_gate_and_failure_releases_it() {
        let runtime = crate::tasks::Runtime::default();
        begin_install(&runtime).unwrap();
        assert!(runtime.shutting_down.load(Ordering::SeqCst));
        assert!(begin_install(&runtime).is_err());
        finish_failed_install(&runtime);
        assert!(!runtime.shutting_down.load(Ordering::SeqCst));
        begin_install(&runtime).unwrap();
    }

    #[test]
    fn update_installation_does_not_interrupt_active_tasks() {
        let runtime = crate::tasks::Runtime::default();
        runtime.tasks.lock().unwrap().insert(
            "synthetic-running-task".into(),
            Arc::new(crate::tasks::Control::default()),
        );
        assert!(begin_install(&runtime).is_err());
        assert!(!runtime.shutting_down.load(Ordering::SeqCst));
        runtime.tasks.lock().unwrap().clear();
        begin_install(&runtime).unwrap();
    }

    // 来自 tauri-plugin-updater 2.14.1 的公开测试向量，仅验证字节 test，不含私钥。
    const TEST_PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEE0OUVGNDc0NkZBNkU2RDcKUldUWDVxWnZkUFNlcERHV2cyYno4bjZBclFuQk13Z1FLMWdlVm9zb3JjYkx6Y3VuRCtLSitjWWU=";
    const TEST_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUWDVxWnZkUFNlcFBGMnpzanZQL0wvVFF0QkxSbldSdmFxKzRvbUsxVUV1Z3RRcC9VU3JHaUkxdjc2MnNEeTNpZDNhb3I3YlJDOUVUeWVvdDZsTUhIcldvWjVoalVNMVFjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNjQ5MDUxCWZpbGU6cGF5bG9hZAl2ZXJzaW9uOjIuMC4wCkREcTJDTE9FbDVjUW1DMmVKUkhsblpQVlZpT1pzdTFDanBQS1FvZWk0QS9YQmZrRHFHTzBCK1FNcmNySGdFUTVxVjNtbmU3NXRkRldoSnBxVVhoOUFBPT0=";

    fn candidate() -> Update {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({"pubkey": TEST_PUBLIC_KEY, "endpoints": ["https://example.invalid/latest.json"]}),
        );
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap();
        app.updater_builder()
            .target("test-target")
            .build()
            .unwrap()
            .restore_update(serde_json::json!({
                "version": "2.0.0",
                "platforms": {"test-target": {
                    "url": "https://example.invalid/synthetic-update.exe",
                    "signature": TEST_SIGNATURE
                }}
            }))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn valid_signature_becomes_ready_only_after_verification() {
        let mut session = Session {
            update: Some(candidate()),
            bytes: None,
        };
        assert!(!session.info().unwrap().downloaded);
        session.cache_download(b"test".to_vec()).unwrap();
        assert!(session.info().unwrap().downloaded);
    }

    #[test]
    fn invalid_signature_or_version_never_becomes_a_ready_update() {
        for mode in ["bytes", "signature", "version"] {
            let mut update = candidate();
            let mut bytes = b"test".to_vec();
            match mode {
                "bytes" => bytes.push(0),
                "signature" => update.signature = "invalid-signature".into(),
                _ => update.version = "2.0.1".into(),
            }
            let mut session = Session {
                update: Some(update),
                bytes: None,
            };
            assert!(session.cache_download(bytes).is_err(), "{mode}");
            assert!(session.bytes.is_none());
            assert!(!session.info().unwrap().downloaded);
        }
    }

    #[test]
    fn instance_locks_are_released_before_starting_the_new_process() {
        let dir = tempfile::tempdir().unwrap();
        let open = |name| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(dir.path().join(name))
                .unwrap()
        };
        let current = open("current.lock");
        let legacy = open("legacy.lock");
        current.try_lock().unwrap();
        legacy.try_lock().unwrap();
        let next_current = open("current.lock");
        let next_legacy = open("legacy.lock");
        assert!(next_current.try_lock().is_err());
        assert!(next_legacy.try_lock().is_err());
        unlock_instance_files(&current, Some(&legacy)).unwrap();
        next_current.try_lock().unwrap();
        next_legacy.try_lock().unwrap();
    }

    #[test]
    fn downloaded_bytes_require_a_checked_release() {
        let mut session = Session::default();
        assert!(session.cache_download(vec![1, 2, 3]).is_err());
        assert!(session.bytes.is_none());
    }
}
