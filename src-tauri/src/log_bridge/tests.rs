use super::*;
use crate::log_mcp::start_log_mcp;
use serde_json::{json, Value};
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
struct Memory(Mutex<Option<String>>);
impl Store for Memory {
    fn save(&self, value: &Record) -> Result<()> {
        *self.0.lock().unwrap() = Some(serde_json::to_string(value).unwrap());
        Ok(())
    }
    fn load(&self) -> Result<Record> {
        self.0
            .lock()
            .unwrap()
            .as_deref()
            .map(|s| serde_json::from_str(s).unwrap())
            .ok_or_else(|| OFFLINE.into())
    }
    fn clear(&self) -> Result<()> {
        self.0.lock().unwrap().take();
        Ok(())
    }
}

#[test]
fn jsonl_and_notifications_use_only_mock_credentials() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
    let server = start_log_mcp(true, 0, |_, _| async { Ok(Value::Null) }).await.unwrap().unwrap();
    let store = Memory::default();
    store.save(&Record::from_server(&server)).unwrap();
    let result = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        let input = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n";
        relay(&store, &input[..], &mut out).unwrap();
        store.clear().unwrap();
        assert!(relay(&store, &input[..], &mut out).is_err());
        out
    }).await.unwrap();
    assert_eq!(result.iter().filter(|x| **x == b'\n').count(), 1);
    assert_eq!(serde_json::from_slice::<Value>(&result).unwrap(), json!({"jsonrpc":"2.0","id":1,"result":{}}));
    server.shutdown().await;
    });
}

#[test]
fn reused_port_gets_neither_bearer_nor_request() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let server = start_log_mcp(true, 0, |_, _| async { Ok(Value::Null) })
                .await
                .unwrap()
                .unwrap();
            let record = Record::from_server(&server);
            server.shutdown().await;
            let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, record.port))
                .await
                .unwrap();
            let receiver = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                let mut peer = BufReader::new(socket);
                let mut wire = String::new();
                while !wire.ends_with("\r\n\r\n") {
                    assert_ne!(peer.read_line(&mut wire).await.unwrap(), 0);
                }
                peer.get_mut()
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\n\r\n00000000000000000000000000000000")
                    .await
                    .unwrap();
                peer.read_to_string(&mut wire).await.unwrap();
                wire
            });
            assert!(forward(&record, b"private request".to_vec()).await.is_err());
            let wire = tokio::time::timeout(Duration::from_secs(3), receiver)
                .await
                .unwrap()
                .unwrap();
            for secret in [
                &record.token[..],
                &record.identity.marker,
                "private request",
                "authorization:",
            ] {
                assert!(!wire.to_ascii_lowercase().contains(secret));
            }
        });
}
