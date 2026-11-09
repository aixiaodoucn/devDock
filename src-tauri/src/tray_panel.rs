use std::{cell::RefCell, ptr::NonNull};

use block2::RcBlock;
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyClass, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSEvent, NSEventMask, NSEventType, NSPanel, NSScreen, NSStatusWindowLevel,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use tauri::{AppHandle, Manager};

use crate::{
    config::Result,
    tray_geometry::{self, Rect},
};

fn panel_style() -> NSWindowStyleMask {
    NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel
}

fn dismiss_on_pointer(visible: bool, over_tray: bool, inside_panel: bool) -> bool {
    visible && !over_tray && !inside_panel
}

define_class!(
    // SAFETY: 从 NSPanel 正常分配，保持其布局和非激活键盘焦点机制；所有方法均在主线程。
    #[unsafe(super = NSPanel)]
    #[thread_kind = MainThreadOnly]
    #[name = "DevDockQuickPanel"]
    struct QuickPanel;

    impl QuickPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool { true }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool { false }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key_window(&self) {
            // SAFETY: 使用 NSWindow 原方法签名，先释放非激活 panel 的键盘焦点。
            unsafe { let _: () = msg_send![super(self), resignKeyWindow]; }
            // AppKit 可能在 show/hide/退出时同步重入；隐藏前不持有 HOST 借用。
            if !pointer_over_tray() {
                hide_panel(self, "resign-key", None);
            }
        }

        #[unsafe(method(cancelOperation:))]
        fn cancel_operation(&self, _sender: Option<&objc2::runtime::AnyObject>) {
            hide_panel(self, "cancel-operation", None);
        }
    }
);

struct Host {
    panel: Retained<QuickPanel>,
    wrapper: Retained<NSWindow>,
    content: Retained<NSView>,
    webview: Retained<NSView>,
    monitors: Vec<Retained<objc2::runtime::AnyObject>>,
}

impl Drop for Host {
    fn drop(&mut self) {
        // 先移除事件回调，再释放键盘焦点、归还 view，最后释放 NSPanel。
        for monitor in self.monitors.drain(..) {
            // SAFETY: token 只来自 NSEvent 的 monitor 注册，每个恰好移除一次。
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        self.panel.orderOut(None);
        self.panel.makeFirstResponder(None);
        self.panel.setContentView(None);
        self.wrapper.setContentView(Some(&self.content));
    }
}

thread_local! {
    // AppKit 对象不跨线程，也不通过 unsafe Send/Sync 放入 Tauri state。
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static APP: RefCell<Option<AppHandle>> = const { RefCell::new(None) };
}

fn panel() -> Result<Retained<QuickPanel>> {
    HOST.with(|host| host.borrow().as_ref().map(|host| host.panel.clone()))
        .ok_or_else(|| "原生快捷面板尚未初始化".into())
}

fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

fn current_screen(mtm: MainThreadMarker) -> Option<Retained<NSScreen>> {
    let mouse = NSEvent::mouseLocation();
    NSScreen::screens(mtm)
        .iter()
        .find(|screen| contains(screen.frame(), mouse))
        .or_else(|| NSScreen::mainScreen(mtm))
}

fn primary_height(mtm: MainThreadMarker) -> Option<f64> {
    // screens[0] 是菜单栏主屏；mainScreen 是当前键盘窗口所在屏，不能用于翻转全局原点。
    NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
}

fn tray_pointer_geometry() -> Option<(NSPoint, Rect, NSRect, f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    APP.with(|app| {
        let app = app.borrow();
        let tray = app.as_ref()?.tray_by_id(super::tray::TRAY_ID)?;
        let rect = super::tray::physical_rect(tray.rect().ok()??);
        let screen = current_screen(mtm)?;
        let height = primary_height(mtm)?;
        let scale = screen.backingScaleFactor();
        let native = NSRect::new(
            NSPoint::new(rect.x / scale, height - (rect.y + rect.height) / scale),
            NSSize::new(rect.width / scale, rect.height / scale),
        );
        Some((NSEvent::mouseLocation(), rect, native, scale, height))
    })
}

fn pointer_over_tray() -> bool {
    tray_pointer_geometry().is_some_and(|(mouse, _, native, _, _)| contains(native, mouse))
}

fn find_webview(view: &NSView) -> Option<Retained<NSView>> {
    let class = AnyClass::get(c"WKWebView")?;
    // SAFETY: isKindOfClass: 是 NSObject 的标准方法，参数为存活的 Class。
    if unsafe { msg_send![view, isKindOfClass: class] } {
        return Some(Retained::from(view));
    }
    view.subviews()
        .iter()
        .find_map(|child| find_webview(&child))
}

pub(crate) fn init(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| std::io::Error::other("NSPanel 必须在主线程初始化"))?;
    // SAFETY: setup 主线程中获取仍由 Tauri 持有的窗口，额外 retain 直到宿主归还 contentView。
    let wrapper = unsafe { Retained::retain(window.ns_window()?.cast::<NSWindow>()) }
        .ok_or_else(|| std::io::Error::other("quick-panel NSWindow 不存在"))?;
    let content = wrapper
        .contentView()
        .ok_or_else(|| std::io::Error::other("quick-panel contentView 不存在"))?;
    let webview = find_webview(&content)
        .ok_or_else(|| std::io::Error::other("quick-panel WKWebView 不存在"))?;
    // SAFETY: 真正分配 NSPanel 子类，没有变更已存在窗口的 class；初始化参数与 AppKit 一致。
    let native: Retained<QuickPanel> = unsafe {
        msg_send![QuickPanel::alloc(mtm), initWithContentRect: NSRect::new(NSPoint::new(0., 0.), NSSize::new(390., 560.)),
            styleMask: panel_style(), backing: NSBackingStoreType::Buffered, defer: false]
    };
    // SAFETY: 由 Host 唯一管理释放，不允许 close 再额外 release。
    unsafe {
        native.setReleasedWhenClosed(false);
    }
    native.setTitle(&objc2_foundation::NSString::from_str("DevDock 快捷操作"));
    native.setCollectionBehavior(super::tray::quick_panel_spaces(native.collectionBehavior()));
    native.setHidesOnDeactivate(false);
    native.setFloatingPanel(true);
    native.setBecomesKeyOnlyIfNeeded(false);
    native.setLevel(NSStatusWindowLevel);
    native.setHasShadow(true);
    wrapper.makeFirstResponder(None);
    wrapper.setContentView(None);
    native.setContentView(Some(&content));
    APP.with(|app| *app.borrow_mut() = Some(window.app_handle().clone()));
    HOST.with(|host| {
        *host.borrow_mut() = Some(Host {
            panel: native,
            wrapper,
            content,
            webview,
            monitors: Vec::new(),
        })
    });

    let mouse_mask =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    let global = RcBlock::new(|event: NonNull<NSEvent>| {
        if let Ok(panel) = panel() {
            if dismiss_on_pointer(panel.isVisible(), pointer_over_tray(), false) {
                // SAFETY: AppKit 在此次 monitor 回调期间保持 event 有效。
                hide_panel(&panel, "global-mouse-down", Some(unsafe { event.as_ref() }));
            }
        }
    });
    let local = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit 保证 monitor 回调期间 event 有效；原样返回或返回 null 消费 ESC。
        let event_ref = unsafe { event.as_ref() };
        if let Ok(panel) = panel() {
            let inside = event_ref.windowNumber() == panel.windowNumber();
            if event_ref.r#type() == NSEventType::KeyDown {
                if panel.isVisible() && inside && event_ref.keyCode() == 53 {
                    hide_panel(&panel, "local-escape", Some(event_ref));
                    return std::ptr::null_mut();
                }
            } else if dismiss_on_pointer(panel.isVisible(), pointer_over_tray(), inside) {
                hide_panel(&panel, "local-mouse-down", Some(event_ref));
            }
        }
        event.as_ptr()
    });
    let global = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mouse_mask, &global);
    // SAFETY: local 的返回值仅为输入事件或 null，遵守 AppKit 事件所有权约定。
    let local = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            mouse_mask | NSEventMask::KeyDown,
            &local,
        )
    };
    let ready = global.is_some() && local.is_some();
    HOST.with(|host| {
        host.borrow_mut()
            .as_mut()
            .unwrap()
            .monitors
            .extend(global.into_iter().chain(local));
    });
    if !ready {
        shutdown();
        return Err(std::io::Error::other("无法注册快捷面板的原生事件监听").into());
    }
    Ok(())
}

#[cfg(debug_assertions)]
fn debug_panel_state(panel: &QuickPanel, phase: &str, active_before: bool) {
    debug_panel_state_with_event(panel, phase, active_before, None);
}

#[cfg(debug_assertions)]
fn debug_panel_state_with_event(
    panel: &QuickPanel,
    phase: &str,
    active_before: bool,
    event: Option<&NSEvent>,
) {
    use objc2::ClassType;
    use std::io::Write;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let wrapper_visible =
        HOST.with(|host| host.borrow().as_ref().map(|host| host.wrapper.isVisible()));
    // 退出归还 view 后 HOST 已移出，不再记录已销毁宿主的可见状态。
    let Some(wrapper_visible) = wrapper_visible else {
        return;
    };
    // SAFETY: 只在 AppKit 主线程查询存活 panel 的真实类，不改变对象。
    let is_panel: bool = unsafe { msg_send![panel, isKindOfClass: NSPanel::class()] };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let current_event = app.currentEvent();
    let event = event.or(current_event.as_deref());
    let event_metadata = event
        .map(|event| {
            format!(
                "eventType={:?} eventTime={} eventWindow={} eventLocation={:?}",
                event.r#type(),
                event.timestamp(),
                event.windowNumber(),
                event.locationInWindow(),
            )
        })
        .unwrap_or_else(|| "event=none".into());
    let pointer_metadata = tray_pointer_geometry().map(|(mouse, physical, native, scale, height)| format!(
        "pointerOverTray={} mouse={mouse:?} trayPhysical={physical:?} trayNative={native:?} scale={scale} primaryHeight={height}",
        contains(native, mouse),
    )).unwrap_or_else(|| format!("pointerOverTray=false mouse={:?} trayGeometry=unavailable", NSEvent::mouseLocation()));
    let line = format!(
        "[quick-panel] phase={phase} class=DevDockQuickPanel NSPanel={is_panel} nonactivating={} key={} main={} appActive={active_before}->{} wrapperVisible={wrapper_visible} visible={} onActiveSpace={} panelFrame={:?} {pointer_metadata} {event_metadata}",
        panel.styleMask().contains(NSWindowStyleMask::NonactivatingPanel),
        panel.isKeyWindow(), panel.isMainWindow(), app.isActive(),
        panel.isVisible(), panel.isOnActiveSpace(), panel.frame(),
    );
    // 仅开发诊断：追加到已忽略 target，内容不含项目、输入或用户数据；写入失败不影响面板。
    eprintln!("{line}");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/target/quick-panel-debug.log"
        ))
    {
        let _ = writeln!(file, "{line}");
    }
}

fn hide_panel(panel: &QuickPanel, reason: &str, _event: Option<&NSEvent>) {
    #[cfg(debug_assertions)]
    let active_before = objc2_app_kit::NSApplication::sharedApplication(panel.mtm()).isActive();
    #[cfg(debug_assertions)]
    debug_panel_state_with_event(
        panel,
        &format!("hide-request reason={reason}"),
        active_before,
        _event,
    );
    panel.orderOut(None);
    #[cfg(debug_assertions)]
    debug_panel_state_with_event(
        panel,
        &format!("hide reason={reason}"),
        active_before,
        _event,
    );
    #[cfg(not(debug_assertions))]
    let _ = reason;
}

pub(crate) fn hide() -> Result<()> {
    let panel = panel()?;
    hide_panel(&panel, "ipc-hide", None);
    Ok(())
}

pub(crate) fn toggle(app: &AppHandle, anchor: Rect) -> Result<()> {
    let panel = panel()?;
    if panel.isVisible() {
        hide_panel(&panel, "tray-toggle", None);
        return Ok(());
    }
    let mtm = MainThreadMarker::new().ok_or("NSPanel 必须在主线程显示")?;
    let screen = current_screen(mtm).ok_or("无法确定快捷面板所在屏幕")?;
    let visible = screen.visibleFrame();
    let placement = tray_geometry::native_frame(
        anchor,
        Rect {
            x: visible.origin.x,
            y: visible.origin.y,
            width: visible.size.width,
            height: visible.size.height,
        },
        primary_height(mtm).ok_or("无法确定主屏原点")?,
        screen.backingScaleFactor(),
    );
    #[cfg(debug_assertions)]
    let active_before = objc2_app_kit::NSApplication::sharedApplication(mtm).isActive();
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-before-frame", active_before);
    panel.setFrame_display(
        NSRect::new(
            NSPoint::new(placement.x, placement.y),
            NSSize::new(placement.width, placement.height),
        ),
        true,
    );
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-after-frame", active_before);
    // 仅使非激活 panel 成为 key，不调用 NSApp.activate，也不显示/聚焦普通 Tauri wrapper。
    panel.orderFrontRegardless();
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-after-order-front", active_before);
    panel.makeKeyWindow();
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-after-make-key", active_before);
    let webview = HOST.with(|host| host.borrow().as_ref().unwrap().webview.clone());
    panel.makeFirstResponder(Some(&webview));
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-after-first-responder", active_before);
    use tauri::Emitter;
    app.emit_to(super::tray::QUICK_PANEL, "quick-panel-opened", ())
        .map_err(|e| e.to_string())?;
    #[cfg(debug_assertions)]
    debug_panel_state(&panel, "show-event-emitted", active_before);
    Ok(())
}

pub(crate) fn shutdown() {
    // 在 Tauri 销毁窗口/webview 前归还 contentView；移出 RefCell 后才允许 AppKit 重入。
    let host = HOST.with(|host| host.borrow_mut().take());
    drop(host);
    APP.with(|app| app.borrow_mut().take());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_uses_nonactivating_borderless_style() {
        let style = panel_style();
        assert!(style.contains(NSWindowStyleMask::NonactivatingPanel));
        assert!(!style.intersects(NSWindowStyleMask::Titled | NSWindowStyleMask::Resizable));
    }

    #[test]
    fn pointer_dismissal_preserves_panel_input_and_tray_toggle() {
        assert!(dismiss_on_pointer(true, false, false));
        assert!(!dismiss_on_pointer(true, true, false));
        assert!(!dismiss_on_pointer(true, false, true));
        assert!(!dismiss_on_pointer(false, false, false));
    }
}
