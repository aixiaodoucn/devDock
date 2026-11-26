//! SQLPad 使用独立的本机私有存储条目，保存会话及所选连接，不保存密码或免审批授权。
use super::*;
use cookie_store::{Cookie, CookieStore};
use reqwest::cookie::CookieStore as HttpCookies;
use std::sync::RwLock;

pub(super) const WARNING: &str = "本机私有存储不可用，SQLPad 登录仅在本次运行有效";
pub(super) const CLEAR_WARNING: &str = "无法删除本机已记住的 SQLPad 会话";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    connection: Option<Connection>,
}
impl Jar {
    pub(super) fn snapshot(
        &self,
        root: &Url,
        username: Option<&str>,
        connection: Option<&Connection>,
    ) -> Result<String> {
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
            username: username.map(str::to_owned),
            connection: connection.cloned(),
            cookies,
        })
        .map_err(|_| WARNING.into())
    }
    pub(super) fn restore(value: &str) -> Result<Session> {
        let saved: Saved = serde_json::from_str(value).map_err(|_| INVALID_RESPONSE)?;
        let root = root_url(&saved.root)?;
        // 保留 Cookie 原始域、路径和到期时间，过期项不会被重新赋予有效期。
        let store =
            CookieStore::from_cookies(saved.cookies.into_iter().map(Ok::<_, String>), false)?;
        if store.iter_unexpired().next().is_none() {
            return Err(AUTH_REQUIRED.into());
        }
        let mut session = Session::new(root, Self(RwLock::new(store)))?;
        session.status.username = saved.username;
        session.pending_connection = saved.connection;
        Ok(session)
    }
}

const KEY: &str = "sqlpad";

pub(super) async fn load() -> Result<Option<String>> {
    tokio::task::spawn_blocking(|| crate::local_credentials::load(KEY))
        .await
        .map_err(|_| WARNING)?
        .map_err(|_| WARNING.into())
}
pub(super) async fn save(value: String) -> Result<()> {
    tokio::task::spawn_blocking(move || crate::local_credentials::save(KEY, &value))
        .await
        .map_err(|_| WARNING)?
        .map_err(|_| WARNING.into())
}
pub(super) async fn clear() -> Result<()> {
    tokio::task::spawn_blocking(|| crate::local_credentials::clear(KEY))
        .await
        .map_err(|_| CLEAR_WARNING)?
        .map_err(|_| CLEAR_WARNING.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remembered_user_and_connection_require_a_fresh_catalog() {
        let site = root_url("https://fixture.invalid").unwrap();
        let jar = Jar::default();
        let cookie = header::HeaderValue::from_static("session=test; Path=/; Secure");
        jar.set_cookies(&mut [&cookie].into_iter(), &site);
        let conn = Connection {
            id: "one".into(),
            name: "Example".into(),
            driver: "mysql".into(),
        };
        let saved = jar
            .snapshot(&site, Some("user@example.invalid"), Some(&conn))
            .unwrap();
        let mut session = Jar::restore(&saved).unwrap();
        assert_eq!(
            session.status.username.as_deref(),
            Some("user@example.invalid")
        );
        assert!(session.status.selected_id.is_none());
        session.update_connections(vec![conn.clone()]).unwrap();
        assert_eq!(session.status.selected_id.as_deref(), Some("one"));
        assert!(!session.status.auto_approve);
        for visible in [
            vec![],
            vec![Connection {
                driver: "postgres".into(),
                ..conn.clone()
            }],
        ] {
            let mut session = Jar::restore(&saved).unwrap();
            session.update_connections(visible).unwrap();
            assert!(session.status.selected_id.is_none());
        }
    }

    #[test]
    fn round_trip_preserves_scope_and_excludes_expired_cookies() {
        let root = root_url("https://fixture.invalid").unwrap();
        let jar = Jar::default();
        let cookie =
            header::HeaderValue::from_static("session=fixture; Path=/api; Secure; HttpOnly");
        let expired = header::HeaderValue::from_static("expired=no; Path=/; Max-Age=0");
        jar.set_cookies(&mut [&cookie, &expired].into_iter(), &root);
        let value = jar.snapshot(&root, None, None).unwrap();
        assert!(!value.contains("expired=no"));
        let session = Jar::restore(&value).unwrap();
        assert!(session.status.username.is_none() && session.pending_connection.is_none());
        let (site, restored) = (session.root, session.jar);
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
