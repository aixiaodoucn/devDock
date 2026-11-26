//! SQLPad 使用独立系统凭据，不保存密码、已选数据库或免审批授权。
use super::*;
use cookie_store::{Cookie, CookieStore};
use reqwest::cookie::CookieStore as HttpCookies;
use std::sync::RwLock;

pub(super) const WARNING: &str = "系统凭据不可用，SQLPad 登录仅在本次运行有效";
pub(super) const CLEAR_WARNING: &str =
    "无法删除已记住的 SQLPad 会话，请在系统凭据管理中移除 SQLPad 条目";

#[derive(Default)]
pub(super) struct Jar(RwLock<CookieStore>);
impl HttpCookies for Jar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &header::HeaderValue>, url: &Url) {
        if let Ok(mut store) = self.0.write() {
            for value in headers.filter_map(|h| h.to_str().ok()) {
                let _ = store.parse(value, url);
            }
        }
    }
    fn cookies(&self, url: &Url) -> Option<header::HeaderValue> {
        let store = self.0.read().ok()?;
        let value = store
            .get_request_values(url)
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        (!value.is_empty())
            .then(|| header::HeaderValue::from_str(&value).ok())
            .flatten()
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    root: String,
    cookies: Vec<Cookie<'static>>,
}
impl Jar {
    pub(super) fn snapshot(&self, root: &Url) -> Result<String> {
        let store = self.0.read().map_err(|_| WARNING)?;
        let mut cookies: Vec<_> = store.iter_unexpired().cloned().collect();
        if cookies.is_empty() {
            return Err(WARNING.into());
        }
        cookies.sort_by(|a, b| {
            (&a.domain, a.path.as_ref(), a.name()).cmp(&(&b.domain, b.path.as_ref(), b.name()))
        });
        serde_json::to_string(&Saved {
            root: root.to_string(),
            cookies,
        })
        .map_err(|_| WARNING.into())
    }
    pub(super) fn restore(value: &str) -> Result<(Url, Self)> {
        let saved: Saved = serde_json::from_str(value).map_err(|_| INVALID_RESPONSE)?;
        let root = root_url(&saved.root)?;
        // 保留 Cookie 原始域、路径和到期时间，过期项不会被重新赋予有效期。
        let store =
            CookieStore::from_cookies(saved.cookies.into_iter().map(Ok::<_, String>), false)?;
        if store.iter_unexpired().next().is_none() {
            return Err(AUTH_REQUIRED.into());
        }
        Ok((root, Self(RwLock::new(store))))
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn entry() -> Result<keyring::Entry> {
    let service = if cfg!(debug_assertions) {
        "cn.aixiaodou.devdock.dev.sqlpad"
    } else {
        "cn.aixiaodou.devdock.sqlpad"
    };
    keyring::Entry::new(service, "default").map_err(|_| WARNING.into())
}
pub(super) async fn load() -> Result<Option<String>> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        tokio::task::spawn_blocking(|| match entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(WARNING.into()),
        })
        .await
        .map_err(|_| WARNING)?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(None)
    }
}
pub(super) async fn save(value: String) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        tokio::task::spawn_blocking(move || {
            entry()?.set_password(&value).map_err(|_| WARNING.into())
        })
        .await
        .map_err(|_| WARNING)?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = value;
        Err(WARNING.into())
    }
}
pub(super) async fn clear() -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        tokio::task::spawn_blocking(|| match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CLEAR_WARNING.into()),
        })
        .await
        .map_err(|_| CLEAR_WARNING)?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip_preserves_scope_and_excludes_expired_cookies() {
        let root = root_url("https://fixture.invalid").unwrap();
        let jar = Jar::default();
        let cookie =
            header::HeaderValue::from_static("session=fixture; Path=/api; Secure; HttpOnly");
        let expired = header::HeaderValue::from_static("expired=no; Path=/; Max-Age=0");
        jar.set_cookies(&mut [&cookie, &expired].into_iter(), &root);
        let value = jar.snapshot(&root).unwrap();
        assert!(!value.contains("expired=no"));
        let (site, restored) = Jar::restore(&value).unwrap();
        assert_eq!(site, root);
        assert!(restored.cookies(&site).is_none());
        assert_eq!(
            restored
                .cookies(&site.join("api/connections/").unwrap())
                .unwrap(),
            "session=fixture"
        );
        assert!(restored
            .cookies(&Url::parse("https://other.invalid/api").unwrap())
            .is_none());
        assert!(Jar::restore(&value.replace("https://", "http://")).is_err());
    }
}
