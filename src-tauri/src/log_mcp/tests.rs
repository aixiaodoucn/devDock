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
                assert_eq!(result.as_array().unwrap().len(), 13);
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
fn live_match_limit_controls_schema_validation_and_default() {
    sqlpad_test_runtime().block_on(async {
        let server = start_log_mcp(true, 0, |_, args| async move { Ok(args) }).await.unwrap().unwrap();
        for cap in [7, 1200] {
            server.set_match_limit(cap);
            let list = request(&server, "", r#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#).await.1;
            let list: Value = serde_json::from_str(&list).unwrap();
            for name in ["search_prod_logs", "scan_prod_logs"] {
                let schema = &list["result"]["tools"].as_array().unwrap().iter().find(|v| v["name"] == name).unwrap()["inputSchema"]["properties"]["maxLines"];
                assert_eq!(schema["maximum"], cap);
                let mut args = json!({"moduleId":"1","targetId":"one","logId":"info","keyword":"sample"});
                for limit in [None, Some(cap), Some(cap + 1)] {
                    if let Some(n) = limit { args["maxLines"] = json!(n); }
                    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}});
                    let reply: Value = serde_json::from_str(&request(&server,"",&body.to_string()).await.1).unwrap();
                    if limit == Some(cap + 1) { assert_eq!(reply["error"]["code"], -32602); } else {
                        let dispatched: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
                        assert_eq!(dispatched["maxLines"], json!(limit.or_else(|| (name == "search_prod_logs").then_some(cap.min(20)))));
                    }
                }
            }
        }
        server.shutdown().await;
    });
}

#[test]
fn catalog_matches_tools_list_and_documents_each_parameter() {
    let request = br#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#;
    let response = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let reply = protocol::rpc(request, 500, &|_, _| async {
                panic!("目录不能执行工具")
            })
            .await;
            reply.into_body().collect().await.unwrap().to_bytes()
        });
    let rpc: Value = serde_json::from_slice(&response).unwrap();
    let tools = rpc["result"]["tools"].as_array().unwrap();
    let catalog = serde_json::to_value(tool_catalog(500)).unwrap();
    let entries = catalog.as_array().unwrap();
    let expected = [
        ("resolve_log_target", "scm", "SCM"),
        ("search_prod_logs", "scm", "SCM"),
        ("scan_prod_logs", "scm", "SCM"),
        ("prod_db_describe_table", "sqlpad", "SQLPad"),
        ("prod_db_query_readonly", "sqlpad", "SQLPad"),
        ("scm_project_options", "scm-project", "项目 SCM"),
        ("scm_project_modules", "scm-project", "项目 SCM"),
        ("scm_create_project", "scm-project", "项目 SCM"),
        ("scm_update_project", "scm-project", "项目 SCM"),
        ("scm_remove_project_module", "scm-project", "项目 SCM"),
        ("backend_git_prepare", "backend-git", "后端项目 Git"),
        ("backend_git_execute", "backend-git", "后端项目 Git"),
        ("backend_git_state", "backend-git", "后端项目 Git"),
    ];
    assert_eq!(entries.len(), expected.len());
    assert_eq!(tools.len(), entries.len());
    for ((entry, tool), (name, source, label)) in entries.iter().zip(tools).zip(expected) {
        assert_eq!(entry.as_object().unwrap().len(), 7);
        assert_eq!(entry["name"], name);
        assert_eq!(entry["source"], source);
        assert!(tool["description"].as_str().unwrap().contains(label));
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        if name == "prod_db_query_readonly" {
            let properties = &tool["inputSchema"]["properties"];
            assert_eq!(properties["limit"]["minimum"], 1);
            assert!(properties["limit"].get("maximum").is_none());
            assert!(properties["limit"].get("default").is_none());
            assert_eq!(tool["inputSchema"]["required"], json!(["table"]));
            assert_eq!(
                tool["inputSchema"]["anyOf"],
                json!([
                    {"required":["columns"]}, {"required":["groupBy"]}, {"required":["aggregates"]}
                ])
            );
            assert_eq!(properties["columns"]["maxItems"], 10);
            assert_eq!(properties["groupBy"]["uniqueItems"], true);
            for name in ["groupBy", "aggregates"] {
                assert_eq!(properties[name]["minItems"], 1);
                assert_eq!(properties[name]["maxItems"], 10);
            }
            let aggregate = &properties["aggregates"]["items"];
            assert_eq!(aggregate["required"], json!(["fn", "alias"]));
            assert_eq!(
                aggregate["properties"]["fn"]["enum"],
                json!(["count", "sum", "avg", "min", "max"])
            );
            assert_eq!(aggregate["additionalProperties"], false);
        }
        assert_eq!(
            entry["environment"],
            match source {
                "scm-project" => "project",
                "backend-git" => "local",
                _ => "production",
            }
        );
        assert!(!entry["title"].as_str().unwrap().is_empty());
        for key in ["name", "description", "inputSchema"] {
            assert_eq!(entry[key], tool[key]);
        }
        assert!(!entry["example"].as_str().unwrap().trim().is_empty());
        for (key, property) in entry["inputSchema"]["properties"].as_object().unwrap() {
            assert!(
                property["description"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{key}"
            );
        }
    }
}

#[test]
fn tool_argument_allowlist() {
    let args = json!({"moduleId":"sample","targetId":"sample","logId":"sample","keyword":"sample","maxLines":500});
    assert!(valid_args("search_prod_logs", &args));
    assert!(valid_args("scan_prod_logs", &args));
    let mut over_limit = args.clone();
    over_limit["maxLines"] = json!(501);
    assert!(!valid_args("scan_prod_logs", &over_limit));
    for (key, value) in [
        ("maxLines", json!(501)),
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

fn sqlpad_test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
}

async fn db_call<F, Fut>(name: &str, args: Value, dispatch: F) -> Value
where
    F: Fn(&'static str, Value) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    let msg = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":args}});
    let reply = protocol::rpc(&serde_json::to_vec(&msg).unwrap(), 500, &dispatch).await;
    serde_json::from_slice(&reply.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[test]
fn sqlpad_calls_use_approved_shapes_and_a_separate_timeout() {
    sqlpad_test_runtime().block_on(async {
    for (name, args) in [
        ("prod_db_describe_table", json!({"table":"orders"})),
        ("prod_db_query_readonly", json!({"table":"orders", "groupBy":["status"]})),
        ("prod_db_query_readonly", json!({"table":"orders", "aggregates":[{"fn":"count","alias":"n"}]})),
        ("prod_db_query_readonly", json!({"table":"orders", "groupBy":["region","status"], "columns":["status","region"], "aggregates":[{"fn":"sum","column":"amount","alias":"total"}]})),
        ("prod_db_query_readonly", json!({"table":"orders", "groupBy":(0..9).map(|n| format!("field_{n}")).collect::<Vec<_>>(), "aggregates":[{"fn":"count","column":"id","alias":"n"}]})),
        ("prod_db_query_readonly", json!({"table":"orders", "columns":(0..10).map(|n| format!("field_{n}")).collect::<Vec<_>>() })),
        ("prod_db_query_readonly", json!({"schema":"public", "table":"orders", "columns":["id"],"filter":{"column":"status","value":"待处理"},"limit":501})),
    ] {
        let reply = db_call(name, args.clone(), |tool, args| async move {
            Ok(json!({"tool":tool,"args":args}))
        }).await;
        assert_eq!(reply["result"]["isError"], false);
        let payload: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload, json!({"source":"sqlpad","environment":"production","tool":name,"args":args}));
        let began = tokio::time::Instant::now();
        let timed_out = db_call(name, args, |_, _| std::future::pending()).await;
        assert_eq!(began.elapsed(), Duration::from_secs(100));
        assert_eq!(timed_out["result"]["isError"], true);
        assert!(timed_out["result"]["content"][0]["text"].as_str().unwrap().contains("勿自动重试"));
    }
    });
}

#[test]
fn sqlpad_describe_transports_wide_schemas_within_response_capacity() {
    sqlpad_test_runtime().block_on(async {
        let rows: Vec<_> = (0..75)
            .map(|n| json!([format!("field_{n}"), "int"]))
            .collect();
        let args = json!({"table":"orders"});
        let reply = db_call("prod_db_describe_table", args.clone(), |_, _| async {
            Ok(json!({"rows":rows}))
        })
        .await;
        let payload: Value =
            serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(payload["rows"], json!(rows));
        let reply = db_call("prod_db_describe_table", args, |_, _| async {
            Ok(json!({"rows":[["x".repeat(RESPONSE_LIMIT), "int"]]}))
        })
        .await;
        assert_eq!(reply["error"]["code"], -32603);
    });
}

#[test]
fn sqlpad_rejects_invalid_parameters_before_dispatch() {
    sqlpad_test_runtime().block_on(async {
        for (key, value) in [
            ("sql", json!("SELECT 1")),
            ("connectionId", json!("other")),
            ("table", json!("public.orders")),
            ("schema", Value::Null),
            ("columns", json!([])),
            ("columns", json!(["*"])),
            ("columns", json!(["id", "ID"])),
            ("limit", json!(0)),
            ("limit", json!(-1)),
            ("limit", json!(1.5)),
            ("limit", json!("1")),
            ("filter", json!({"column":"id","value":"1","operator":"="})),
            ("filter", json!({"column":"id","value":1})),
            ("filter", json!({"value":"1"})),
            ("filter", json!({"column":"id","value":"名".repeat(43)})),
        ] {
            let mut args = json!({"table":"orders","columns":["id"]});
            args[key] = value;
            let reply = db_call("prod_db_query_readonly", args, |_, _| async {
                panic!("无效参数不得进入审批")
            })
            .await;
            assert_eq!(reply["error"]["code"], -32602, "{key}");
        }
        for mut args in [
            json!({"groupBy":[]}),
            json!({"groupBy":["status","STATUS"]}),
            json!({"groupBy":["orders.status"]}),
            json!({"groupBy":["status"], "columns":["id"]}),
            json!({"aggregates":[]}),
            json!({"aggregates":[{"fn":"sum","alias":"total"}]}),
            json!({"aggregates":[{"fn":"count","alias":"n"}],"columns":["id"]}),
            json!({"aggregates":[{"fn":"count","column":"*","alias":"n"}]}),
            json!({"aggregates":[{"fn":"median","column":"amount","alias":"m"}]}),
            json!({"aggregates":[{"fn":"count"}]}),
            json!({"aggregates":[{"fn":"count","alias":"order count"}]}),
            json!({"groupBy":(0..10).map(|n| format!("field_{n}")).collect::<Vec<_>>(), "aggregates":[{"fn":"count","alias":"n"}]}),
        ] {
            args["table"] = json!("orders");
            let reply = db_call("prod_db_query_readonly", args.clone(), |_, _| async {
                panic!("无效分组或聚合不得进入审批")
            }).await;
            assert_eq!(reply["error"]["code"], -32602, "{args}");
        }
        assert!(!valid_args(
            "prod_db_query_readonly",
            &json!({"table":"orders"})
        ));
        assert!(!valid_args(
            "prod_db_describe_table",
            &json!({"table":"orders","columns":["id"]})
        ));
        assert!(valid_args(
            "prod_db_query_readonly",
            &json!({"table":"orders","columns":["id"]})
        ));
    });
}

#[test]
fn sqlpad_callback_errors_expose_only_fixed_categories() {
    sqlpad_test_runtime().block_on(async {
        for (error, expected) in [
            ("用户已拒绝本次生产 SQLPad 请求", "桌面用户已拒绝审批"),
            ("SQLPad 审批已过期或取消", "审批已过期或取消"),
            ("已有生产 SQLPad 请求等待审批", "已有请求等待审批"),
            ("无法通知 SQLPad 审批窗口", "桌面审批不可用"),
        ] {
            let reply = db_call(
                "prod_db_describe_table",
                json!({"table":"orders"}),
                |_, _| async { Err(error.into()) },
            )
            .await;
            assert_eq!(reply["result"]["isError"], true);
            assert!(reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
        for code in [
            "DB_INVALID",
            "DB_DRIVER",
            "DB_STALE",
            "DB_NO_CONNECTION",
            "DB_UNKNOWN",
            "DB_RESPONSE_LIMIT",
            "DB_INCOMPLETE",
            "DB_FORMAT",
            "AUTH_REQUIRED: 请重新登录生产 SQLPad",
            "DB_UNKNOWN: private detail",
        ] {
            let reply = db_call(
                "prod_db_describe_table",
                json!({"table":"orders"}),
                |_, _| async { Err(code.to_owned()) },
            )
            .await;
            assert_eq!(reply["result"]["isError"], true);
            let text = reply["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("生产环境 · SQLPad"));
            if code.contains("private") {
                assert!(!text.contains("private") && !text.contains("DB_UNKNOWN"));
            } else {
                assert!(text.contains(code.split(':').next().unwrap()));
            }
        }
    });
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
            let reply = protocol::rpc(CALL.as_bytes(), 500, &|_, _| std::future::pending()).await;
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

#[test]
fn project_tools_route_to_the_selected_channel_and_allow_time_for_approval() {
    let input = json!({"name":"项目","branchSuffix":"","projectType":"t","user":"u","details":"","modules":[]});
    sqlpad_test_runtime().block_on(async {
        for (name, args, source, environment, timeout) in [
            ("scm_project_modules",json!({"projectId":"1"}),"scm-project","project",25),
            ("scm_project_options",json!({}),"scm-project","project",25),
            ("scm_project_options",json!({"projectId":"1"}),"scm-project","project",25),
            ("scm_create_project",json!({"input":input}),"scm-project","project",150),
            ("scm_update_project",json!({"projectId":"1","authors":["u"],"revision":"a".repeat(64),"input":input}),"scm-project","project",150),
            ("scm_remove_project_module",json!({"projectId":"1","moduleId":"2","revision":"a".repeat(64)}),"scm-project","project",150),
            ("backend_git_prepare",json!({"projectName":"项目","operation":"merge","repositoryIds":["one"],"push":true,"keepTestPom":true}),"backend-git","local",150),
            ("backend_git_execute",json!({"projectName":"项目","planId":"p"}),"backend-git","local",150),
            ("backend_git_state",json!({"projectName":"项目","planId":"p"}),"backend-git","local",25),
        ] {
            let reply = db_call(name,args.clone(),|tool,args| async move { Ok(json!({"tool":tool,"args":args})) }).await;
            let result:Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
            assert_eq!(result,json!({"source":source,"environment":environment,"tool":name,"args":args}));
            let began = tokio::time::Instant::now();
            let reply = db_call(name,args.clone(),|_,_| std::future::pending()).await;
            assert_eq!(began.elapsed(),Duration::from_secs(timeout));
            assert_eq!(reply["result"]["isError"],true);
            if timeout == 150 { assert!(reply["result"]["content"][0]["text"].as_str().unwrap().contains("勿自动重试")); }
            let mut extra = args;
            extra["approved"] = json!(true);
            let reply = db_call(name,extra,|_,_| async {panic!("未知字段不得派发")}).await;
            assert_eq!(reply["error"]["code"],-32602);
        }
    });
}

#[test]
fn project_write_errors_only_report_known_outcomes() {
    let args = json!({"projectId":"1","moduleId":"2","revision":"a".repeat(64)});
    sqlpad_test_runtime().block_on(async {
        for (error, expected) in [
            ("用户已拒绝本次项目操作请求", "未执行操作"),
            ("项目操作审批已过期或取消", "未执行操作"),
            ("Conflict: 项目已变化，请重新读取编辑选项", "重新读取表单"),
            ("SCM unknown private detail", "勿自动重试"),
        ] {
            let reply = db_call("scm_remove_project_module", args.clone(), |_, _| async {
                Err(error.into())
            })
            .await;
            assert_eq!(reply["result"]["isError"], true);
            let text = reply["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains(expected) && !text.contains("private"));
        }
    });
}

#[test]
fn project_tools_reject_invalid_forms_and_git_scope() {
    let input = json!({"name":"项目","branchSuffix":"","projectType":"t","user":"u","details":"","modules":[]});
    for (key, value) in [
        ("name", json!(" ")),
        ("details", json!("字".repeat(4097))),
        ("branchSuffix", json!("/")),
        ("modules", json!([7])),
        ("modules", json!(["m", "m"])),
        (
            "modules",
            json!((0..129).map(|i| i.to_string()).collect::<Vec<_>>()),
        ),
        ("author", json!("other")),
    ] {
        let mut changed = input.clone();
        changed[key] = value;
        assert!(
            !valid_args("scm_create_project", &json!({"input":changed})),
            "{key}"
        );
    }
    for id in [json!(null), json!("../target"), json!("1".repeat(21))] {
        assert!(!valid_args("scm_project_options", &json!({"projectId":id})));
        assert!(!valid_args("scm_project_modules", &json!({"projectId":id})));
    }
    for (key, value) in [
        ("revision", json!("short")),
        ("authors", json!([])),
        ("authors", json!(["other"])),
    ] {
        let mut args =
            json!({"projectId":"1","authors":["u"],"revision":"a".repeat(64),"input":input});
        args[key] = value;
        assert!(!valid_args("scm_update_project", &args));
    }
    for args in [
        json!({"operation":"merge"}),
        json!({"projectName":" ","operation":"merge"}),
        json!({"projectName":"p","operation":"shell"}),
        json!({"projectName":"p","operation":"merge","scmProjectId":"1"}),
        json!({"projectName":"p","operation":"switch-test","keepTestPom":true}),
        json!({"projectName":"p","operation":"push-dev"}),
        json!({"projectName":"p","operation":"pull-current","push":true}),
        json!({"projectName":"p","operation":"merge","repositoryIds":[]}),
    ] {
        assert!(!valid_args("backend_git_prepare", &args));
    }
    for (key, value) in [
        ("moduleId", json!("../2")),
        ("revision", json!("")),
        ("mode", json!("other")),
    ] {
        let mut args = json!({"projectId":"1","moduleId":"2","revision":"a".repeat(64)});
        args[key] = value;
        assert!(!valid_args("scm_remove_project_module", &args));
    }
    for name in ["backend_git_execute", "backend_git_state"] {
        assert!(!valid_args(name, &json!({"planId":"p"})));
    }
}
