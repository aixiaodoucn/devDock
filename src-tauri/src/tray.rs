use crate::{config::Result, tray_geometry::Rect};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(not(target_os = "macos"))]
use {crate::tray_geometry, tauri::PhysicalSize};

#[cfg(all(test, target_os = "macos"))]
mod spaces_tests {
    use objc2_app_kit::NSWindowCollectionBehavior as Behavior;

    #[test]
    fn quick_panel_joins_desktop_and_fullscreen_spaces() {
        let behavior = super::quick_panel_spaces(Behavior::Default);
        assert!(behavior.contains(Behavior::CanJoinAllSpaces | Behavior::FullScreenAuxiliary));
    }

    #[test]
    fn quick_panel_removes_conflicts_and_preserves_unrelated_behavior() {
        let behavior = super::quick_panel_spaces(
            Behavior::MoveToActiveSpace
                | Behavior::FullScreenPrimary
                | Behavior::FullScreenNone
                | Behavior::IgnoresCycle,
        );
        assert!(!behavior.intersects(
            Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary | Behavior::FullScreenNone,
        ));
        assert!(behavior.contains(Behavior::IgnoresCycle));
        assert_eq!(super::quick_panel_spaces(behavior), behavior);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn quick_panel_spaces(
    mut behavior: objc2_app_kit::NSWindowCollectionBehavior,
) -> objc2_app_kit::NSWindowCollectionBehavior {
    use objc2_app_kit::NSWindowCollectionBehavior as Behavior;
    // AllSpaces 与 MoveToActiveSpace 互斥；快捷窗不能被当作全屏主窗口。
    behavior.remove(
        Behavior::MoveToActiveSpace | Behavior::FullScreenPrimary | Behavior::FullScreenNone,
    );
    behavior | Behavior::CanJoinAllSpaces | Behavior::FullScreenAuxiliary
}

#[cfg(target_os = "macos")]
fn on_panel_main_thread(
    app: &AppHandle,
    operation: impl FnOnce() -> Result<()> + Send + 'static,
) -> Result<()> {
    if objc2_foundation::MainThreadMarker::new().is_some() {
        return operation();
    }
    // IPC 可能来自工作线程；同步等待 hide 完成，保证打开主窗口之前先释放 panel 的焦点。
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = send.send(operation());
    })
    .map_err(|e| e.to_string())?;
    receive.recv().map_err(|e| e.to_string())?
}

pub const TRAY_ID: &str = "dev-dock-tray";
pub const QUICK_PANEL: &str = "quick-panel";

#[tauri::command]
pub fn hide_quick_panel(app: AppHandle) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        on_panel_main_thread(&app, crate::tray_panel::hide)
    }
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(panel) = app.get_webview_window(QUICK_PANEL) {
            panel.hide().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[tauri::command]
pub fn show_main_window(app: AppHandle, project_id: Option<String>) -> Result<()> {
    hide_quick_panel(app.clone())?;
    let main = app
        .get_webview_window("main")
        .ok_or("DevDock 主窗口不存在")?;
    main.show().map_err(|e| e.to_string())?;
    main.unminimize().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    if let Some(project_id) = project_id {
        app.emit_to("main", "open-project", project_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(crate) fn physical_rect(rect: tauri::Rect) -> Rect {
    // tray-icon 返回物理坐标；转换只为解开 Tauri 的 Position/Size 枚举。
    let position = rect.position.to_physical::<f64>(1.);
    let size = rect.size.to_physical::<f64>(1.);
    Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

#[cfg(target_os = "macos")]
fn toggle_panel(
    app: &AppHandle,
    rect: tauri::Rect,
    _position: PhysicalPosition<f64>,
) -> Result<()> {
    let handle = app.clone();
    let anchor = physical_rect(rect);
    on_panel_main_thread(app, move || crate::tray_panel::toggle(&handle, anchor))
}

#[cfg(not(target_os = "macos"))]
fn toggle_panel(app: &AppHandle, rect: tauri::Rect, position: PhysicalPosition<f64>) -> Result<()> {
    let panel = app
        .get_webview_window(QUICK_PANEL)
        .ok_or("快捷窗口不存在")?;
    if panel.is_visible().map_err(|e| e.to_string())? {
        return hide_quick_panel(app.clone());
    }
    let monitor = app
        .monitor_from_point(position.x, position.y)
        .map_err(|e| e.to_string())?
        .or(app.primary_monitor().map_err(|e| e.to_string())?)
        .ok_or("无法确定快捷窗口所在屏幕")?;
    let work = monitor.work_area();
    let placement = tray_geometry::place(
        physical_rect(rect),
        Rect {
            x: work.position.x as f64,
            y: work.position.y as f64,
            width: work.size.width as f64,
            height: work.size.height as f64,
        },
        monitor.scale_factor(),
    );
    panel
        .set_size(PhysicalSize::new(
            placement.width.round() as u32,
            placement.height.round() as u32,
        ))
        .map_err(|e| e.to_string())?;
    panel
        .set_position(PhysicalPosition::new(
            placement.x.round() as i32,
            placement.y.round() as i32,
        ))
        .map_err(|e| e.to_string())?;
    panel.show().map_err(|e| e.to_string())?;
    panel.set_focus().map_err(|e| e.to_string())?;
    app.emit_to(QUICK_PANEL, "quick-panel-opened", ())
        .map_err(|e| e.to_string())
}

// 点击状态栏图标会先令窗口失焦；此时让左键回调处理切换，避免隐藏后又立即打开。
#[cfg(not(target_os = "macos"))]
fn cursor_over_tray(app: &AppHandle) -> bool {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    let (Ok(Some(rect)), Ok(cursor)) = (tray.rect(), app.cursor_position()) else {
        return false;
    };
    let rect = physical_rect(rect);
    cursor.x >= rect.x
        && cursor.x <= rect.x + rect.width
        && cursor.y >= rect.y
        && cursor.y <= rect.y + rect.height
}

fn report_error(app: &AppHandle, error: String) {
    app.state::<crate::StartupLogs>()
        .0
        .lock()
        .unwrap()
        .push(format!("状态栏操作失败: {error}"));
    // 错误沿用现有日志入口展示，不自动激活主窗口，避免把用户拉离当前 Space。
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    // 提前隐藏创建，前端可在首次点击前完成事件监听；主窗口继续默认显示。
    let panel = WebviewWindowBuilder::new(
        app,
        QUICK_PANEL,
        WebviewUrl::App("index.html?view=quick".into()),
    )
    .title("DevDock 快捷操作")
    .inner_size(390., 560.)
    .visible(false)
    .focused(false)
    // 非激活弹框需要把首次点击直接交给按钮，而非只用来激活 WebView。
    .accept_first_mouse(true)
    .decorations(false)
    .resizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(true)
    .build()?;
    // macOS 仅借用隐藏 wrapper 的 webview；之后绝不通过 Tauri 显示或聚焦这个普通窗口。
    #[cfg(target_os = "macos")]
    crate::tray_panel::init(&panel)?;
    #[cfg(not(target_os = "macos"))]
    {
        let handle = app.clone();
        panel.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Focused(false)) && !cursor_over_tray(&handle) {
                if let Err(error) = hide_quick_panel(handle.clone()) {
                    report_error(&handle, error);
                }
            }
        });
    }
    let open = MenuItem::with_id(app, "tray-open-main", "打开 DevDock", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "tray-quit", "退出 DevDock", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    // Windows 任务栏不应用 macOS 模板着色，使用彩色图标兼容深浅任务栏。
    let icon: &[u8] = if cfg!(target_os = "macos") {
        include_bytes!("../icons/tray-icon.png")
    } else {
        include_bytes!("../icons/icon.png")
    };
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(icon)?)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("DevDock")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray-open-main" => {
                if let Err(error) = show_main_window(app.clone(), None) {
                    report_error(app, error);
                }
            }
            // 统一经过现有 ExitRequested 的任务取消与 AppID 恢复流程。
            "tray-quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state: MouseButtonState::Up,
                rect,
                position,
                ..
            } = event
            {
                let app = tray.app_handle();
                let result = match button {
                    MouseButton::Left => toggle_panel(app, rect, position),
                    MouseButton::Right => hide_quick_panel(app.clone()),
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    report_error(app, error);
                }
            }
        })
        .build(app)?;
    Ok(())
}
