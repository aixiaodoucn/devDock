use crate::config::Result;
use std::{path::Path, process::Command, sync::OnceLock};

const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[cfg(target_os = "macos")]
fn is_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    // 仅查询本进程的 AX 权限；不在启动时请求，也不修改 TCC 数据库。
    unsafe { AXIsProcessTrusted() != 0 }
}

#[cfg(not(target_os = "macos"))]
fn is_trusted() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn request_permission() {
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }

    // 静态 CFString/CFBoolean 的生命周期覆盖 options，无需字典 retain/release 回调。
    unsafe {
        let keys = [kAXTrustedCheckOptionPrompt];
        let values = [kCFBooleanTrue];
        let options = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            1,
            std::ptr::null(),
            std::ptr::null(),
        );
        if !options.is_null() {
            // 仅请求系统原生提示；授权状态由下次 ensure 重新查询。
            AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn request_permission() {}

fn open_settings() -> Result<()> {
    let output = Command::new("/usr/bin/open")
        .arg(SETTINGS_URL)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{} {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[derive(Default)]
pub(crate) struct PermissionPrompt {
    denial: Option<String>,
}

impl PermissionPrompt {
    pub(crate) fn ensure(&mut self) -> Result<()> {
        self.ensure_with(is_trusted, request_permission, open_settings)
    }

    pub(crate) fn denied(&mut self) -> String {
        self.denied_with(request_permission, open_settings)
    }

    fn ensure_with(
        &mut self,
        trusted: impl FnOnce() -> bool,
        request: impl FnOnce(),
        open: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        // 拒绝缓存仅用于同次操作内去重，不能遮蔽用户刚授予或撤销的权限。
        if trusted() {
            self.denial = None;
            Ok(())
        } else {
            Err(self.denied_with(request, open))
        }
    }

    fn denied_with(&mut self, request: impl FnOnce(), open: impl FnOnce() -> Result<()>) -> String {
        // 子进程仍可能被拒绝；不以父进程的权限覆盖拒绝，同次操作只请求并打开一次。
        self.denial
            .get_or_insert_with(|| {
                request();
                let outcome = match open() {
                    Ok(()) => "已打开系统设置。".to_owned(),
                    Err(error) => format!("打开系统设置失败: {error}。"),
                };
                format!("辅助功能权限不足，已暂停本次操作。{outcome}请在系统设置 → 隐私与安全性 → 辅助功能中手动为 DevDock 开启权限，开启后返回 DevDock 重试。")
            })
            .clone()
    }
}

pub(crate) fn is_denied(message: &str) -> bool {
    static DENIAL: OnceLock<regex::Regex> = OnceLock::new();
    // 只识别 AX 拒绝及系统的明确措辞；Apple Events 授权失败属于另一种权限。
    DENIAL
        .get_or_init(|| {
            regex::Regex::new(
                r#"(?i)(?:^|[^0-9])-25211(?:$|[^0-9])|\bnot allowed (?:assistive access|to send keystrokes)\b|不允许(?:\s*[“"']?(?:osascript|System Events)[”"']?\s*)?(?:辅助访问|发送按键)"#,
            )
            .unwrap()
        })
        .is_match(message)
}

pub(crate) fn is_automation_command(command: &Command) -> bool {
    let program = Path::new(command.get_program()).file_name();
    program == Some(std::ffi::OsStr::new("osascript"))
        || (matches!(program.and_then(|s| s.to_str()), Some("bash" | "sh"))
            && command.get_args().next().is_some_and(|arg| {
                Path::new(arg).file_name() == Some(std::ffi::OsStr::new("build-app.sh"))
            }))
}

#[cfg(any(not(windows), test))]
pub(crate) fn terminal_needs_accessibility(terminal: &str) -> bool {
    ["/Ghostty.app", "/iTerm.app", "/iTerm2.app"]
        .iter()
        .any(|suffix| terminal.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn recognizes_only_accessibility_denials() {
        for message in [
            "execution error: System Events got an error: osascript is not allowed assistive access. (-25211)",
            "execution error: -25211",
            "System Events got an error: osascript is not allowed assistive access.",
            "osascript is not allowed to send keystrokes. (1002)",
            "不允许“osascript”辅助访问。(-25211)",
            "osascript不允许辅助访问。",
            "不允许 osascript 发送按键。",
            "osascript不允许发送按键。",
        ] {
            assert!(is_denied(message), "{message}");
        }
        for message in [
            "build failed",
            "Accessibility permission denied in fixture",
            "not authorized to send Apple events (-1743)",
            "error -252110",
            "version 25211",
            "辅助功能",
            "发送按键失败",
            "assistive access granted",
            "keystrokes failed",
        ] {
            assert!(!is_denied(message), "{message}");
        }
    }

    #[test]
    fn missing_permission_stops_work_and_opens_settings_once() {
        let mut prompt = PermissionPrompt::default();
        let opened = Cell::new(0);
        let requested = Cell::new(0);
        let mut work_started = false;
        for _ in 0..3 {
            let result = prompt.ensure_with(
                || false,
                || requested.set(requested.get() + 1),
                || {
                    assert_eq!(requested.get(), 1, "应先请求原生提示再打开设置");
                    opened.set(opened.get() + 1);
                    Ok(())
                },
            );
            if result.is_ok() {
                work_started = true;
            }
            let error = result.unwrap_err();
            assert!(error.contains("已打开"));
            assert!(error.contains("DevDock"));
            assert!(error.contains("开启后返回 DevDock 重试"));
            assert!(!error.contains("重启"));
            assert!(!error.contains("退出重开"));
        }
        assert!(!work_started);
        assert_eq!(requested.get(), 1);
        assert_eq!(opened.get(), 1);
    }

    #[test]
    fn cached_denial_is_cleared_after_grant_and_prompts_again_after_revocation() {
        let mut prompt = PermissionPrompt::default();
        let trusted = Cell::new(false);
        let queried = Cell::new(0);
        let requested = Cell::new(0);
        let opened = Cell::new(0);
        let mut ensure = || {
            prompt.ensure_with(
                || {
                    queried.set(queried.get() + 1);
                    trusted.get()
                },
                || requested.set(requested.get() + 1),
                || {
                    opened.set(opened.get() + 1);
                    Ok(())
                },
            )
        };

        assert!(ensure().is_err());
        assert!(ensure().is_err());
        assert_eq!(opened.get(), 1);
        trusted.set(true);
        assert!(
            ensure().is_ok(),
            "授权后应能使用同一个 PermissionPrompt 重试"
        );
        trusted.set(false);
        assert!(ensure().is_err());
        assert!(ensure().is_err());
        assert_eq!(queried.get(), 5, "每次 ensure 都应重新查询实际权限");
        assert_eq!(requested.get(), 2, "授权被撤销后应重新请求原生提示");
        assert_eq!(opened.get(), 2, "授权被撤销后应重新引导授权");
    }

    #[test]
    fn trusted_process_does_not_open_settings() {
        let mut prompt = PermissionPrompt::default();
        assert!(prompt
            .ensure_with(
                || true,
                || panic!("不应请求提示"),
                || panic!("不应打开设置")
            )
            .is_ok());
    }

    #[test]
    fn repeated_child_denials_open_settings_once_even_when_parent_is_trusted() {
        let mut prompt = PermissionPrompt::default();
        prompt
            .ensure_with(
                || true,
                || panic!("不应请求提示"),
                || panic!("不应打开设置"),
            )
            .unwrap();
        let opened = Cell::new(0);
        let requested = Cell::new(0);
        for _ in 0..3 {
            let error = prompt.denied_with(
                || requested.set(requested.get() + 1),
                || {
                    opened.set(opened.get() + 1);
                    Ok(())
                },
            );
            assert!(error.contains("手动"));
            assert!(error.contains("开启后返回 DevDock 重试"));
        }
        assert_eq!(requested.get(), 1);
        assert_eq!(opened.get(), 1);
    }

    #[test]
    fn failed_settings_open_preserves_manual_instructions() {
        let requested = Cell::new(false);
        let error = PermissionPrompt::default()
            .ensure_with(
                || false,
                || requested.set(true),
                || Err("exit status: 1".into()),
            )
            .unwrap_err();
        assert!(requested.get());
        assert!(error.contains("打开系统设置失败: exit status: 1"));
        assert!(error.contains("系统设置 → 隐私与安全性 → 辅助功能"));
        assert!(error.contains("DevDock"));
        assert!(!error.contains("已打开"));
    }

    #[test]
    fn fallback_is_limited_to_automation_commands() {
        let mut osascript = Command::new("/usr/bin/osascript");
        osascript.arg("terminal.applescript");
        assert!(is_automation_command(&osascript));
        let mut hbuilder = Command::new("/bin/bash");
        hbuilder.arg("/tmp/scripts/build-app.sh").arg("apk");
        assert!(is_automation_command(&hbuilder));
        for program in ["/bin/bash", "/tmp/gradlew", "/usr/bin/unzip"] {
            let mut command = Command::new(program);
            command.arg("/tmp/scripts/build.sh").arg("build-app.sh");
            assert!(!is_automation_command(&command));
        }
    }

    #[test]
    fn only_supported_ui_terminals_need_accessibility() {
        for terminal in [
            "/Applications/Ghostty.app",
            "/Applications/iTerm.app",
            "/Applications/iTerm2.app",
        ] {
            assert!(terminal_needs_accessibility(terminal));
        }
        for terminal in [
            "Terminal",
            "/System/Applications/Utilities/Terminal.app",
            "/Applications/Other.app",
        ] {
            assert!(!terminal_needs_accessibility(terminal));
        }
    }
}
