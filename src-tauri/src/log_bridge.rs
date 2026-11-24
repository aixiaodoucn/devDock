//! stdio 客户端不启动桌面工作台，也不接触 SCM 会话。
use crate::{
    config::Result,
    log_mcp::{LogMcpServer, REQUEST_LIMIT, RESPONSE_LIMIT},
};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Bytes, client::conn::http1, Request};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use std::{
    io::{self, BufRead, Read, Write},
    net::{Ipv4Addr, SocketAddrV4},
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::net::TcpStream;

mod credentials;
#[cfg(test)]
mod tests;
pub(crate) use credentials::{Store, SystemStore};
const OFFLINE: &str = "Dev Dock 日志 MCP 不可用，请打开工作台并重新启用";
const IO_ERROR: &str = "日志 MCP 标准输入输出已断开";

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Identity {
    pub(crate) probe: String,
    pub(crate) marker: String,
}
impl Identity {
    pub(crate) fn new() -> Self {
        let random = || uuid::Uuid::new_v4().simple().to_string();
        Self {
            probe: random(),
            marker: random(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Record {
    port: u16,
    token: String,
    identity: Identity,
}
impl Record {
    pub(crate) fn from_server(server: &LogMcpServer) -> Self {
        Self {
            port: server.port(),
            token: server.bearer_token().into(),
            identity: server.identity.clone(),
        }
    }
}

async fn forward(record: &Record, line: Vec<u8>) -> Result<Option<serde_json::Value>> {
    // 使用同一条 TCP 连接探测并转发；不走代理、DNS、重试或重定向，避免端口重绑定竞态。
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, record.port);
    let stream = TcpStream::connect(addr).await.map_err(|_| OFFLINE)?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| OFFLINE)?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let probe = Request::get("/log-mcp-identity")
        .header("Host", addr.to_string())
        .header("X-DevDock-Probe", &record.identity.probe)
        .body(Full::new(Bytes::new()))
        .map_err(|_| OFFLINE)?;
    let reply = sender.send_request(probe).await.map_err(|_| OFFLINE)?;
    if reply.status() != 200 {
        return Err(OFFLINE.into());
    }
    let proof = Limited::new(reply.into_body(), 32)
        .collect()
        .await
        .map_err(|_| OFFLINE)?
        .to_bytes();
    if proof.len() != 32 || !bool::from(proof.as_ref().ct_eq(record.identity.marker.as_bytes())) {
        return Err(OFFLINE.into());
    }
    let request = Request::post("/mcp")
        .header("Host", addr.to_string())
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Authorization", format!("Bearer {}", record.token))
        .body(Full::new(Bytes::from(line)))
        .map_err(|_| OFFLINE)?;
    sender.ready().await.map_err(|_| OFFLINE)?;
    let reply = sender.send_request(request).await.map_err(|_| OFFLINE)?;
    match reply.status().as_u16() {
        202 => Ok(None),
        200 => {
            let bytes = Limited::new(reply.into_body(), RESPONSE_LIMIT)
                .collect()
                .await
                .map_err(|_| OFFLINE)?
                .to_bytes();
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| OFFLINE.into())
        }
        429 => Err("日志 MCP 请求过于频繁，请稍后重试".into()),
        _ => Err(OFFLINE.into()),
    }
}

fn relay(store: &impl Store, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    // 固定一次 stdio 会话的授权，不让旧客户端随重新启用或账号切换取得新令牌。
    let record = store.load()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| OFFLINE)?;
    loop {
        let mut line = Vec::new();
        if input
            .by_ref()
            .take(REQUEST_LIMIT as u64 + 2)
            .read_until(b'\n', &mut line)
            .map_err(|_| IO_ERROR)?
            == 0
        {
            return Ok(());
        }
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        if line.len() > REQUEST_LIMIT {
            return Err("日志 MCP 请求过大".into());
        }
        let reply = runtime
            .block_on(async {
                tokio::time::timeout(Duration::from_secs(170), forward(&record, line)).await
            })
            .map_err(|_| OFFLINE)??;
        if let Some(value) = reply {
            serde_json::to_writer(&mut output, &value).map_err(|_| IO_ERROR)?;
            output
                .write_all(b"\n")
                .and_then(|_| output.flush())
                .map_err(|_| IO_ERROR)?;
        }
    }
}

pub fn run_stdio() -> Result<()> {
    relay(&SystemStore, io::stdin().lock(), io::stdout().lock())
}
