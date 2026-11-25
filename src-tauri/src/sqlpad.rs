//! SQLPad 3.x 独立会话，数据库工具只在主窗口逐次授权后执行。
mod query;
use crate::{config::Result, ensure_backend_window};
pub(crate) use query::valid_query_args;
use reqwest::{header, redirect::Policy, Client, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};
use tauri::{State, WebviewWindow};
use tokio::sync::Mutex;

const AUTH_REQUIRED: &str = "AUTH_REQUIRED: 请重新登录生产 SQLPad";
const INVALID_RESPONSE: &str = "SQLPad 响应格式不兼容";
const BODY_LIMIT: usize = 512 * 1024;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Connection {
    #[serde(rename(deserialize = "_id"))]
    id: String,
    name: String,
    driver: String,
}

#[derive(Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Status {
    connected: bool,
    base_url: String,
    connections: Vec<Connection>,
    selected_id: Option<String>,
}

struct Session {
    generation: uuid::Uuid,
    client: Client,
    root: Url,
    status: Status,
}

#[derive(Default)]
pub(crate) struct Runtime(Mutex<Option<Session>>);

fn root_url(input: &str) -> Result<Url> {
    let url = Url::parse(input.trim()).map_err(|_| "请输入 SQLPad HTTPS 根地址")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("请输入不含凭据、查询参数或页面路径的 SQLPad HTTPS 根地址".into());
    }
    Ok(url)
}

async fn json_response(mut response: Response) -> Result<Value> {
    let code = response.status();
    if code.as_u16() == 401
        || (code.is_redirection()
            && response
                .headers()
                .get(header::LOCATION)
                .and_then(|s| s.to_str().ok())
                .and_then(|s| response.url().join(s).ok())
                .is_some_and(|u| u.origin() == response.url().origin() && u.path() == "/signin"))
    {
        return Err(AUTH_REQUIRED.into());
    }
    if !code.is_success() {
        return Err(format!("SQLPad 请求失败（HTTP {}）", code.as_u16()));
    }
    if !response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|s| s.to_str().ok())
        .is_some_and(|s| {
            s.split(';')
                .next()
                .is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json"))
        })
    {
        return Err(INVALID_RESPONSE.into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "SQLPad 响应读取失败")? {
        if chunk.len() > BODY_LIMIT - bytes.len() {
            return Err("SQLPad 响应过大".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let json: Value = serde_json::from_slice(&bytes).map_err(|_| INVALID_RESPONSE)?;
    if json
        .get("error")
        .is_some_and(|e| !e.is_null() && e != false && e != "")
    {
        return Err("SQLPad 拒绝请求，请检查登录信息和权限".into());
    }
    Ok(json)
}

impl Session {
    async fn refresh(&mut self) -> Result<()> {
        let response = self
            .client
            .get(
                self.root
                    .join("api/connections/")
                    .map_err(|_| INVALID_RESPONSE)?,
            )
            .send()
            .await
            .map_err(|_| "无法连接 SQLPad")?;
        let mut data = json_response(response).await?;
        // 明确使用字段白名单。上游可能附带主机和连接参数，不转发、不持久化。
        let rows = data.get_mut("connections").ok_or(INVALID_RESPONSE)?.take();
        let connections: Vec<Connection> =
            serde_json::from_value(rows).map_err(|_| INVALID_RESPONSE)?;
        let mut ids = HashSet::new();
        if connections
            .iter()
            .any(|c| c.id.is_empty() || c.name.is_empty() || !ids.insert(&c.id))
        {
            return Err(INVALID_RESPONSE.into());
        }
        self.status.selected_id = self.status.selected_id.take().filter(|id| ids.contains(id));
        self.status.connections = connections;
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn sqlpad_login(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    base_url: String,
    email: String,
    password: String,
) -> Result<Status> {
    ensure_backend_window(&window)?;
    let mut slot = runtime.0.lock().await;
    *slot = None;
    let root = root_url(&base_url)?;
    if email.trim().is_empty() || password.is_empty() {
        return Err("请输入 SQLPad 邮箱和密码".into());
    }
    let client = Client::builder()
        .cookie_store(true)
        .retry(reqwest::retry::never())
        .redirect(Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "无法创建 SQLPad 会话")?;
    let body = serde_json::to_vec(&serde_json::json!({"email":email.trim(),"password":password}))
        .map_err(|_| INVALID_RESPONSE)?;
    let response = client
        .post(root.join("api/signin").map_err(|_| INVALID_RESPONSE)?)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| "无法连接 SQLPad")?;
    json_response(response).await?;
    let mut session = Session {
        generation: uuid::Uuid::new_v4(),
        status: Status {
            connected: true,
            base_url: root.to_string(),
            ..Default::default()
        },
        root,
        client,
    };
    session.refresh().await?;
    let status = session.status.clone();
    *slot = Some(session);
    Ok(status)
}

#[tauri::command]
pub(crate) async fn sqlpad_status(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
) -> Result<Status> {
    ensure_backend_window(&window)?;
    let mut slot = runtime.0.lock().await;
    let Some(session) = slot.as_mut() else {
        return Ok(Status::default());
    };
    match session.refresh().await {
        Ok(()) => Ok(session.status.clone()),
        Err(e) => {
            if e == AUTH_REQUIRED {
                *slot = None;
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub(crate) async fn sqlpad_select_connection(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
    connection_id: String,
) -> Result<Status> {
    ensure_backend_window(&window)?;
    let mut slot = runtime.0.lock().await;
    let session = slot.as_mut().ok_or(AUTH_REQUIRED)?;
    if let Err(e) = session.refresh().await {
        if e == AUTH_REQUIRED {
            *slot = None;
        }
        return Err(e);
    }
    if !session
        .status
        .connections
        .iter()
        .any(|c| c.id == connection_id)
    {
        return Err("请选择当前账号可见的连接".into());
    }
    session.generation = uuid::Uuid::new_v4();
    session.status.selected_id = Some(connection_id);
    Ok(session.status.clone())
}

#[tauri::command]
pub(crate) async fn sqlpad_logout(
    window: WebviewWindow,
    runtime: State<'_, Runtime>,
) -> Result<Status> {
    ensure_backend_window(&window)?;
    // 本地退出，释放整个 CookieJar，不影响用户的浏览器会话。
    *runtime.0.lock().await = None;
    Ok(Status::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_only_https_origins() {
        assert!(root_url("https://example.invalid:1443").is_ok());
        for s in [
            "http://example.invalid",
            "https://name:secret@example.invalid/",
            "https://example.invalid/queries/new",
            "https://example.invalid/?q=1",
        ] {
            assert!(root_url(s).is_err());
        }
    }
    #[test]
    fn local_connection_list_rejects_expiry_without_following_redirect() {
        use std::io::{BufRead, BufReader, Write};
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root = Url::parse(&format!("http://{}/", server.local_addr().unwrap())).unwrap();
        let worker = std::thread::spawn(move || {
            let data = r#"{"connections":[{"_id":"test","driver":"postgres","name":"Example"}]}"#;
            for response in [format!("200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len()), "302 Found\r\nLocation: /signin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()] {
                let (mut stream, _) = server.accept().unwrap();
                let mut input = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                input.read_line(&mut first).unwrap();
                assert_eq!(first.trim_end(), "GET /api/connections/ HTTP/1.1");
                for line in input.lines() { if line.unwrap().is_empty() { break; } }
                write!(stream, "HTTP/1.1 {response}").unwrap();
            }
        });
        tauri::async_runtime::block_on(async {
            let mut session = Session {
                generation: uuid::Uuid::new_v4(),
                client: Client::builder()
                    .no_proxy()
                    .redirect(Policy::none())
                    .build()
                    .unwrap(),
                root,
                status: Status::default(),
            };
            session.refresh().await.unwrap();
            assert_eq!(session.status.connections[0].id, "test");
            assert!(session.status.selected_id.is_none());
            assert_eq!(session.refresh().await.unwrap_err(), AUTH_REQUIRED);
        });
        worker.join().unwrap();
    }
    #[test]
    fn exposes_only_connection_display_fields() {
        let c:Connection=serde_json::from_value(serde_json::json!({"_id":"demo","name":"example","driver":"mysql","password":"fixture","host":"example.invalid"})).unwrap();
        assert_eq!(
            serde_json::to_value(c).unwrap(),
            serde_json::json!({"id":"demo","name":"example","driver":"mysql"})
        );
    }
}
