//! 显式启用的无状态 MCP 端点。由桌面层持有句柄，禁止记录令牌。
use http_body_util::{BodyExt, Full, Limited};
use hyper::{
    body::{Body, Bytes, Incoming},
    header::HeaderMap,
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{
    convert::Infallible,
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tokio::{
    net::TcpListener,
    task::{JoinHandle, JoinSet},
};

#[path = "log_mcp/protocol.rs"]
mod protocol;
pub(crate) use protocol::sqlpad_error;
pub(crate) use protocol::{tool_catalog, CatalogEntry};
#[cfg(test)]
#[path = "log_mcp/tests.rs"]
mod tests;
#[cfg(test)]
fn valid_args(name: &str, args: &Value) -> bool {
    protocol::valid_args(name, args, crate::scm::prodlog::DEFAULT_MATCH_LIMIT)
}

pub(crate) const REQUEST_LIMIT: usize = 32 * 1024;
pub(crate) const RESPONSE_LIMIT: usize = 256 * 1024;
const CONCURRENCY: usize = 8;
const PER_MINUTE: usize = 120;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(165);
const TOOL_TIMEOUT: Duration = Duration::from_secs(25);
const BODY_TIMEOUT: Duration = Duration::from_secs(5);
type Reply = Response<Full<Bytes>>;

pub struct LogMcpServer {
    match_limit: Arc<AtomicUsize>,
    addr: SocketAddr,
    token: String,
    pub(crate) identity: crate::log_bridge::Identity,
    task: JoinHandle<()>,
}
impl LogMcpServer {
    pub(crate) fn set_match_limit(&self, limit: usize) {
        self.match_limit.store(limit, Ordering::Relaxed);
    }
    pub(crate) fn port(&self) -> u16 {
        self.addr.port()
    }
    pub fn endpoint(&self) -> String {
        format!("http://{}/mcp", self.addr)
    }
    pub fn bearer_token(&self) -> &str {
        &self.token
    }
    pub async fn shutdown(mut self) {
        self.task.abort();
        let _ = (&mut self.task).await;
    }
}
impl Drop for LogMcpServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct State<F> {
    match_limit: Arc<AtomicUsize>,
    host: String,
    origin: String,
    authorization: String,
    identity: crate::log_bridge::Identity,
    window: Mutex<(Instant, usize)>,
    dispatch: F,
}
impl<F> State<F> {
    fn admit(&self) -> bool {
        let mut w = self.window.lock().unwrap();
        if w.0.elapsed() >= Duration::from_secs(60) {
            *w = (Instant::now(), 0);
        }
        if w.1 >= PER_MINUTE {
            false
        } else {
            w.1 += 1;
            true
        }
    }
}

/// 未启用时不监听；端口 0 表示由系统分配。回调须验证仓库与目标授权，
/// 异步执行 I/O，并在 future 被丢弃时取消后端工作。
pub async fn start_log_mcp<F, Fut>(
    enabled: bool,
    port: u16,
    dispatch: F,
) -> io::Result<Option<LogMcpServer>>
where
    F: Fn(&'static str, Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, String>> + Send + 'static,
{
    if !enabled {
        return Ok(None);
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;
    // 每次启动生成新凭据，两枚独立 UUIDv4 含有 244 个随机位。
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let identity = crate::log_bridge::Identity::new();
    let match_limit = Arc::new(AtomicUsize::new(crate::scm::prodlog::DEFAULT_MATCH_LIMIT));
    let state = Arc::new(State {
        match_limit: match_limit.clone(),
        identity: identity.clone(),
        host: addr.to_string(),
        origin: format!("http://{addr}"),
        authorization: format!("Bearer {token}"),
        window: Mutex::new((Instant::now(), 0)),
        dispatch,
    });
    let task = tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        loop {
            // 接收连接前限制任务数。监听任务退出时同时取消所有子任务。
            while tasks.try_join_next().is_some() {}
            if tasks.len() >= CONCURRENCY {
                tasks.join_next().await;
                continue;
            }
            let (socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let state = state.clone();
            tasks.spawn(async move {
                let service = service_fn(move |request| {
                    let state = state.clone();
                    async move { Ok::<_, Infallible>(handle(request, &state).await) }
                });
                let mut http = http1::Builder::new();
                http.timer(hyper_util::rt::TokioTimer::new())
                    .header_read_timeout(BODY_TIMEOUT)
                    .max_buf_size(16 * 1024)
                    .max_headers(64);
                let connection = http.serve_connection(TokioIo::new(socket), service);
                let _ = tokio::time::timeout(CONNECTION_TIMEOUT, connection).await;
            });
        }
    });
    Ok(Some(LogMcpServer {
        match_limit,
        addr,
        token,
        identity,
        task,
    }))
}
fn response(status: u16, body: Vec<u8>) -> Reply {
    let mut builder = Response::builder()
        .status(status)
        .header("Cache-Control", "no-store")
        .header("Connection", "close");
    if !body.is_empty() {
        builder = builder.header("Content-Type", "application/json");
    }
    if status == 401 {
        builder = builder.header("WWW-Authenticate", "Bearer");
    }
    if status == 405 {
        builder = builder.header("Allow", "POST");
    }
    builder.body(Full::new(Bytes::from(body))).unwrap()
}
// 区分可选头缺失与安全头非法或重复。
fn single<'a>(headers: &'a HeaderMap, key: &str) -> Result<Option<&'a str>, ()> {
    let mut all = headers.get_all(key).iter();
    let value = all.next().map(|v| v.to_str()).transpose().map_err(|_| ())?;
    if all.next().is_some() {
        Err(())
    } else {
        Ok(value)
    }
}
async fn handle<F, Fut>(request: Request<Incoming>, state: &State<F>) -> Reply
where
    F: Fn(&'static str, Value) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    let reject = |status| response(status, vec![]);
    if !state.admit() {
        return reject(429);
    }
    let h = request.headers();
    if single(h, "host") != Ok(Some(state.host.as_str()))
        || !matches!(single(h, "origin"), Ok(None))
            && single(h, "origin") != Ok(Some(state.origin.as_str()))
    {
        return reject(403);
    }
    // 仅持有本次桥接探测密钥的桥可取得实例标记；此步不携带令牌或日志请求。
    if request.method() == hyper::Method::GET
        && request.uri() == "/log-mcp-identity"
        && single(h, "origin") == Ok(None)
        && matches!(single(h,"x-devdock-probe"), Ok(Some(v)) if bool::from(v.as_bytes().ct_eq(state.identity.probe.as_bytes())))
    {
        return Response::builder()
            .header("Cache-Control", "no-store")
            .body(Full::new(Bytes::copy_from_slice(
                state.identity.marker.as_bytes(),
            )))
            .unwrap();
    }
    if !matches!(single(h,"authorization"),Ok(Some(v)) if bool::from(v.as_bytes().ct_eq(state.authorization.as_bytes())))
    {
        return reject(401);
    }
    if request.uri().authority().is_some()
        || request.uri().path_and_query().map(|p| p.as_str()) != Some("/mcp")
    {
        return reject(404);
    }
    if request.method() != hyper::Method::POST {
        return reject(405);
    }
    if !matches!(single(h, "mcp-protocol-version"), Ok(None))
        && !matches!(single(h,"mcp-protocol-version"),Ok(Some(v)) if protocol::VERSIONS.contains(&v))
    {
        return reject(400);
    }
    if !matches!(single(h,"content-type"),Ok(Some(v)) if v.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"))
    {
        return reject(415);
    }
    let accepts_json = h
        .get_all("accept")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|v| matches!(v.trim(), "application/json" | "*/*"));
    if !accepts_json {
        return reject(406);
    }
    if request.body().size_hint().lower() > REQUEST_LIMIT as u64 {
        return reject(413);
    }
    let body = match tokio::time::timeout(
        BODY_TIMEOUT,
        Limited::new(request.into_body(), REQUEST_LIMIT).collect(),
    )
    .await
    {
        Ok(Ok(b)) => b.to_bytes(),
        Ok(Err(_)) => return reject(413),
        Err(_) => return reject(408),
    };
    protocol::rpc(
        &body,
        state.match_limit.load(Ordering::Relaxed),
        &state.dispatch,
    )
    .await
}
