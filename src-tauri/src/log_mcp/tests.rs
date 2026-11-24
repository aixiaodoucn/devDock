use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(server: &LogMcpServer, headers: &str, body: &str) -> (u16, String) {
    request_with_length(server, headers, body, body.len()).await
}

async fn request_with_length(
    server: &LogMcpServer,
    headers: &str,
    body: &str,
    length: usize,
) -> (u16, String) {
    let mut socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
    let wire = format!("POST /mcp HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{headers}Content-Length: {}\r\n\r\n{body}", server.addr, server.bearer_token(), length);
    socket.write_all(wire.as_bytes()).await.unwrap();
    let mut reply = String::new();
    socket.read_to_string(&mut reply).await.unwrap();
    let (head, body) = reply.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        body.into(),
    )
}

#[test]
fn loopback_protocol_and_guards() {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let dispatch = |name, args| async move { Ok(json!({"tool":name,"args":args})) };
        assert!(start_log_mcp(false,0,dispatch).await.unwrap().is_none());
        let server = start_log_mcp(true,0,dispatch).await.unwrap().unwrap();
        assert_eq!(server.addr.ip(), std::net::Ipv4Addr::LOCALHOST);
        assert!(server.bearer_token().len() >= 43);
        for (header,status) in [("Origin: null\r\n",403), ("Host: other.invalid\r\n",403), ("Authorization: Basic invalid\r\n",401), ("MCP-Protocol-Version: unknown\r\n",400)] {
            assert_eq!(request(&server,header,"{}").await.0,status);
        }
        for (method,params,key) in [("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),"serverInfo"),("tools/list",json!({}),"tools"),("tools/call",json!({"name":"resolve_log_target","arguments":{"repositoryPath":"sample"}}),"content")] {
            let (status,text)=request(&server,"",&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}).to_string()).await;
            assert_eq!(status,200);
            let reply: Value = serde_json::from_str(&text).unwrap();
            let result = &reply["result"][key];
            assert!(!result.is_null());
            if method == "tools/list" {
                assert!(result.as_array().unwrap().iter().all(|tool| tool["description"].as_str().unwrap().contains("生产环境 · SCM")));
            } else if method == "tools/call" {
                let payload: Value = serde_json::from_str(result[0]["text"].as_str().unwrap()).unwrap();
                assert_eq!(payload["environment"], "production");
                assert_eq!(payload["source"], "scm");
                assert_eq!(payload["tool"], "resolve_log_target");
            }
        }
        let ping=request(&server,"",r#"{"jsonrpc":"2.0","method":"ping","id":"p"}"#).await;
        assert_eq!(serde_json::from_str::<Value>(&ping.1).unwrap(),json!({"jsonrpc":"2.0","id":"p","result":{}}));
        assert_eq!(request(&server,"",r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await,(202,String::new()));
        server.shutdown().await;
    });
}

#[test]
fn tool_argument_allowlist() {
    let args = json!({"moduleId":"sample","targetId":"sample","logId":"sample","keyword":"sample","maxLines":50});
    assert!(valid_args("search_prod_logs", &args));
    for (key, value) in [
        ("maxLines", json!(51)),
        ("keyword", json!(null)),
        ("command", json!("sample")),
    ] {
        let mut invalid = args.clone();
        invalid[key] = value;
        assert!(!valid_args("search_prod_logs", &invalid));
    }
    let mut archive = args.clone();
    archive["archiveHour"] = json!("2032-02-29-14");
    assert!(valid_args("search_prod_logs", &archive));
    for invalid in [json!("2031-02-29-14"), json!("../history"), Value::Null] {
        archive["archiveHour"] = invalid;
        assert!(!valid_args("search_prod_logs", &archive));
    }
    assert!(!valid_args("unknown", &args));
    for (id, expected) in [("123", true), ("", false), ("sample", false)] {
        assert_eq!(
            valid_args(
                "resolve_log_target",
                &json!({"repositoryPath":"sample","moduleId":id})
            ),
            expected
        );
    }
}

#[test]
fn full_scan_uses_desktop_default_or_exactly_one_override_and_accepts_count_only() {
    let mut args = json!({"moduleId":"7","logId":"info","keyword":"example"});
    assert!(valid_args("scan_prod_logs", &args));
    args["allTargets"] = json!(true);
    assert!(valid_args("scan_prod_logs", &args));
    args["maxLines"] = json!(0);
    assert!(valid_args("scan_prod_logs", &args));
    args["targetId"] = json!("example");
    assert!(!valid_args("scan_prod_logs", &args));
    args.as_object_mut().unwrap().remove("allTargets");
    assert!(valid_args("scan_prod_logs", &args));
    args["command"] = json!("anything");
    assert!(!valid_args("scan_prod_logs", &args));
}

const CALL: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"resolve_log_target","arguments":{"repositoryPath":"sample"}}}"#;

#[test]
fn enforces_request_response_and_rate_limits() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let server = start_log_mcp(true, 0, |_, _| async {
            Ok(json!("x".repeat(RESPONSE_LIMIT)))
        })
        .await
        .unwrap()
        .unwrap();
        let (_, result) = request(&server, "", CALL).await;
        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["error"]["code"],
            -32603
        );
        assert!(result.len() <= RESPONSE_LIMIT);
        assert_eq!(
            request_with_length(&server, "", "", REQUEST_LIMIT + 1)
                .await
                .0,
            413
        );
        for _ in 2..PER_MINUTE {
            assert_eq!(request(&server, "", "{}").await.0, 200);
        }
        assert_eq!(request(&server, "", "{}").await.0, 429);
        let addr = server.addr;
        server.shutdown().await;
        assert!(tokio::net::TcpStream::connect(addr).await.is_err());
    });
}

#[test]
fn dispatch_timeout_is_bounded() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
        .block_on(async {
            let began = tokio::time::Instant::now();
            let reply = protocol::rpc(CALL.as_bytes(), &|_, _| std::future::pending()).await;
            let body = reply.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(
                serde_json::from_slice::<Value>(&body).unwrap()["result"]["isError"],
                true
            );
            assert_eq!(began.elapsed(), TOOL_TIMEOUT);
        });
}

#[test]
fn concurrent_calls_do_not_exceed_connection_budget() {
    use tokio::sync::{mpsc, Semaphore};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (entered, mut observed) = mpsc::channel(CONCURRENCY + 1);
        let gate = Arc::new(Semaphore::new(0));
        let blocked = gate.clone();
        let server = Arc::new(
            start_log_mcp(true, 0, move |_, _| {
                let entered = entered.clone();
                let blocked = blocked.clone();
                async move {
                    entered.send(()).await.unwrap();
                    blocked.acquire().await.unwrap().forget();
                    Ok(Value::Null)
                }
            })
            .await
            .unwrap()
            .unwrap(),
        );
        let mut calls = JoinSet::new();
        for _ in 0..=CONCURRENCY {
            let server = server.clone();
            calls.spawn(async move { request(&server, "", CALL).await });
        }
        for _ in 0..CONCURRENCY {
            tokio::time::timeout(Duration::from_secs(3), observed.recv())
                .await
                .unwrap()
                .unwrap();
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), observed.recv())
                .await
                .is_err()
        );
        gate.add_permits(CONCURRENCY + 1);
        while let Some(done) = calls.join_next().await {
            assert_eq!(done.unwrap().0, 200);
        }
        assert_eq!(observed.recv().await, Some(()));
    });
}

#[test]
fn callback_errors_only_expose_known_categories() {
    for (error, expected) in [
        (
            "SessionExpired: SCM 会话已过期，请重新登录",
            "日志 SCM 登录已失效，请在偏好设置中重新登录",
        ),
        (
            "模块不在当前已保存的后端仓库映射内",
            "未找到已配置的仓库或目标映射",
        ),
        (
            "InvalidInput: 关键词必须是 1–128 字节的非空白字面文本，不能包含控制字符",
            "关键词不符合查询规则",
        ),
        (
            "Protocol: SCM 日志响应格式或目标不匹配",
            "SCM 响应结构或目标不匹配",
        ),
        (
            "Network: SCM 返回 HTTP 403",
            "SCM 返回 HTTP 403，请核对访问权限或服务状态",
        ),
        (
            "ScmBusiness: -7",
            "SCM 拒绝请求（业务码 -7），请核对桌面登录账号的生产日志权限",
        ),
        ("ScmBusiness: 42 private detail", "日志工具调用失败"),
        (
            "Network: SCM 返回 HTTP 502 private detail",
            "日志工具调用失败",
        ),
        ("Protocol: private detail", "日志工具调用失败"),
    ] {
        assert_eq!(protocol::public_error(error), expected);
    }
}
