use super::super::{log_runtime::parse_log_modules, LoginPage};
use super::*;
use std::{cmp::Reverse, collections::BinaryHeap, sync::Arc, time::Duration};
use tokio::{
    sync::Semaphore,
    task::JoinSet,
    time::{timeout_at, Instant},
};

const SCAN_BUDGET: Duration = Duration::from_secs(90);
const INSTANCE_BUDGET: Duration = Duration::from_secs(55);
const CONCURRENCY: usize = 3;
const SAMPLE_BYTES: usize = 24 * 1024;
// 预留 MCP 的二次 JSON 编码空间；超大的清单整体拒绝，不返回伪完整结果。
const MAX_TARGETS: usize = 512;
const SCAN_TIMEOUT: &str = "Timeout: SCM 全文件扫描超时";
static SCAN_JOBS: Semaphore = Semaphore::const_new(1);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdlogScan {
    pub module_id: String,
    pub log_id: String,
    pub archive_hour: Option<String>,
    pub total_matches: u64,
    pub complete: bool,
    pub truncated: bool,
    pub target_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub targets: Vec<InstanceScan>,
    pub latest_matches: Vec<LatestMatch>,
    /// 最新前缀因样本字节预算提前结束；不包括行数限制或单行缩短。
    pub byte_limit_reached: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestMatch {
    pub timestamp: String,
    pub target_id: String,
    pub line: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceScan {
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_matches: Option<u64>,
    pub lines: Vec<String>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

fn controlled_error(error: &str) -> &'static str {
    match error.split_once(':').map(|(category, _)| category) {
        Some("Network") => "Network",
        Some("ScmBusiness") => "ScmBusiness",
        Some("Timeout") => "Timeout",
        Some("Limit") => "Limit",
        _ => "Protocol",
    }
}

pub(crate) enum ScanTargets {
    First,
    All,
    One(String),
}

impl LogRuntime {
    pub(crate) async fn scan_prodlog(
        &self,
        module_id: String,
        selection: ScanTargets,
        log_id: String,
        keyword: String,
        max_lines: usize,
        archive_hour: Option<String>,
        raw: bool,
    ) -> Result<ProdlogScan> {
        full_file::validate_scan_keyword(&keyword)?;
        if !numeric_id(&module_id)
            || !matches!(log_id.as_str(), "info" | "wf")
            || max_lines > MAX_MATCHES
        {
            return Err("InvalidInput: 无效的模块 ID、日志类型或样本行数上限".into());
        }
        archive_hour
            .as_deref()
            .map(ArchiveHour::parse)
            .transpose()?;
        let _job = SCAN_JOBS
            .try_acquire()
            .map_err(|_| "Limit: 正在执行全文件扫描，请稍后重试")?;
        let deadline = Instant::now() + SCAN_BUDGET;
        let session = Arc::new(
            timeout_at(deadline, async {
                let mut state = self.0.state.lock().await;
                self.0.restore(&mut state).await?;
                state
                    .session
                    .clone()
                    .ok_or_else(|| SESSION_EXPIRED.to_owned())
            })
            .await
            .map_err(|_| SCAN_TIMEOUT)??,
        );
        let check_session = || async {
            let state = timeout_at(deadline, self.0.state.lock())
                .await
                .map_err(|_| SCAN_TIMEOUT)?;
            if state
                .session
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(&s.jar, &session.jar))
            {
                Ok(())
            } else {
                Err(SESSION_EXPIRED)
            }
        };
        let result: Result<_> = async {
            // 同一个快照同时约束模块与目标，防止跨账号复用旧 ID。
            let catalog = Arc::new(
                timeout_at(deadline, async {
                    let html = session.get(LoginPage::Prodlog.path(), &[]).await?;
                    if !parse_log_modules(&html)?
                        .iter()
                        .any(|(id, _)| id == &module_id)
                    {
                        return Err("InvalidInput: 模块不在当前日志账号授权清单中".into());
                    }
                    check_session().await?;
                    catalog(
                        &module_id,
                        &post(&session, GET_TARGETS, json!({"git_pid":module_id})).await?,
                    )
                })
                .await
                .map_err(|_| SCAN_TIMEOUT)??,
            );
            if catalog.targets.is_empty() {
                return Err("NotFound: 模块没有已授权的日志实例".into());
            }
            let indices: Vec<_> = match selection {
                ScanTargets::First => vec![0],
                ScanTargets::One(id) => vec![catalog
                    .targets
                    .iter()
                    .position(|t| t.id == id)
                    .ok_or(INVALID_TARGET)?],
                ScanTargets::All => {
                    if catalog.targets.len() > MAX_TARGETS {
                        return Err("Limit: 授权实例数超过完整报告上限".into());
                    }
                    (0..catalog.targets.len()).collect()
                }
            };
            let target_count = indices.len();
            let mut report = ProdlogScan {
                module_id,
                log_id,
                archive_hour,
                target_count,
                total_matches: 0,
                success_count: 0,
                failure_count: target_count,
                complete: false,
                truncated: false,
                latest_matches: Vec::new(),
                byte_limit_reached: false,
                targets: indices
                    .iter()
                    .map(|&i| InstanceScan {
                        target_id: catalog.targets[i].id.clone(),
                        total_matches: None,
                        lines: vec![],
                        truncated: false,
                        error: Some("Timeout"),
                    })
                    .collect(),
            };
            // 堆顶是保留项中最旧的一项；同毫秒由清单位置、单机样本顺序打破平局。
            let mut latest = BinaryHeap::with_capacity(max_lines + 1);
            let mut pending = indices.into_iter().enumerate();
            // JoinSet 的 Drop 取消全部请求；deadline 未启动/未完成的实例保留 Timeout，而不是零命中。
            let mut jobs = JoinSet::new();
            loop {
                check_session().await?;
                while jobs.len() < CONCURRENCY && Instant::now() < deadline {
                    let Some((slot, i)) = pending.next() else {
                        break;
                    };
                    let (s, c, log, hour, k) = (
                        session.clone(),
                        catalog.clone(),
                        report.log_id.clone(),
                        report.archive_hour.clone(),
                        keyword.clone(),
                    );
                    jobs.spawn(async move {
                        let end = deadline.min(Instant::now() + INSTANCE_BUDGET);
                        let result = timeout_at(
                            end,
                            full_file::scan_one(
                                &s,
                                &c,
                                &c.targets[i],
                                &log,
                                hour.as_deref(),
                                &k,
                                max_lines,
                                raw,
                            ),
                        )
                        .await
                        .unwrap_or_else(|_| Err(SCAN_TIMEOUT.into()));
                        (slot, result)
                    });
                }
                let Ok(Some(joined)) = timeout_at(deadline, jobs.join_next()).await else {
                    break;
                };
                let (slot, result) = joined.map_err(|_| "Protocol: SCM 扫描任务意外终止")?;
                let row = &mut report.targets[slot];
                match result {
                    Err(e) if e.starts_with("SessionExpired:") => return Err(e),
                    Err(e) => row.error = Some(controlled_error(&e)),
                    Ok(found) => {
                        report.total_matches = report
                            .total_matches
                            .checked_add(found.total_matches)
                            .ok_or("Limit: 命中总数超出计数上限")?;
                        row.total_matches = Some(found.total_matches);
                        row.truncated = max_lines != 0 && found.truncated;
                        for (index, (timestamp, line)) in
                            found.timestamps.into_iter().zip(found.lines).enumerate()
                        {
                            latest.push((Reverse(timestamp), slot, index, line));
                            if latest.len() > max_lines {
                                latest.pop();
                            }
                        }
                        row.error = None;
                        report.success_count += 1;
                    }
                }
            }
            let mut remaining_bytes = SAMPLE_BYTES;
            for (Reverse(timestamp), slot, _, line) in latest.into_sorted_vec() {
                let row = &mut report.targets[slot];
                // 两个视图中的正文都占预算；只交付连续的最新前缀。
                let cost = 2 * line.len() + timestamp.len() + row.target_id.len();
                if cost > remaining_bytes {
                    report.byte_limit_reached = true;
                    break;
                }
                remaining_bytes -= cost;
                row.lines.push(line.clone());
                report.latest_matches.push(LatestMatch {
                    timestamp,
                    target_id: row.target_id.clone(),
                    line,
                });
            }
            for row in &mut report.targets {
                row.truncated |= max_lines != 0
                    && row
                        .total_matches
                        .is_some_and(|total| total > row.lines.len() as u64);
                report.truncated |= row.truncated;
            }
            report.failure_count -= report.success_count;
            report.complete = report.failure_count == 0;
            Ok(report)
        }
        .await;
        // 旧扫描不可将过期状态写回新登录，也不可在账号变化后交付旧结果。
        let mut state = timeout_at(deadline, self.0.state.lock())
            .await
            .map_err(|_| SCAN_TIMEOUT)?;
        if !state
            .session
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.jar, &session.jar))
        {
            return Err(SESSION_EXPIRED.into());
        }
        if result
            .as_ref()
            .is_err_and(|e| e.starts_with("SessionExpired:"))
        {
            self.0.expire(&mut state, SESSION_EXPIRED).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scm::{new_session, LoginPage};
    use http_body_util::{BodyExt, Full};
    use hyper::{body::Bytes, server::conn::http1, service::service_fn, Response};
    use hyper_util::rt::TokioIo;
    use std::{convert::Infallible, sync::Mutex};

    #[test]
    fn mock_scm_reports_all_targets_and_revalidates_single_selection() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = reqwest::Url::parse(&format!("http://{}/", listener.local_addr().unwrap()))
                .unwrap();
            let calls = Arc::new(Mutex::new(Vec::new()));
            let observed = calls.clone();
            let (entered, mut started) = tokio::sync::mpsc::unbounded_channel();
            let gate = Arc::new(Semaphore::new(0));
            let remote_gate = gate.clone();
            let server = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let observed = observed.clone();
                    let entered = entered.clone();
                    let gate = remote_gate.clone();
                    let handler = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                        let entered = entered.clone();
                        let gate = gate.clone();
                        let observed = observed.clone();
                        async move {
                            let path = req.uri().path().to_string();
                            let mut status = 200;
                            let reply = if path == LoginPage::Prodlog.path() {
                                "<select id=git_pid><option value=7>sample</option><option value=9>empty</option></select>".into()
                            } else {
                                let bytes = req.into_body().collect().await.unwrap().to_bytes();
                                let mut form = reqwest::Url::parse("http://localhost").unwrap();
                                form.set_query(Some(std::str::from_utf8(&bytes).unwrap()));
                                let param: Value =
                                    serde_json::from_str(&form.query_pairs().next().unwrap().1)
                                        .unwrap();
                                if path == GET_TARGETS {
                                    assert!(
                                        param == json!({"git_pid":"7"})
                                            || param == json!({"git_pid":"9"})
                                    );
                                    let rows: Vec<_> = (1..=4).map(|i| json!({"key":format!("private-{i}"),"ip":format!("192.0.2.{i}"),"user":"demo","module_name":"sample","git_pid":"7"})).collect();
                                    json!({"status":{"status_code":0},"module":{"git_name":"demo/sample"},"targets":if param["git_pid"] == "9" {vec![]} else {rows}}).to_string()
                                } else {
                                    assert_eq!(path, QUERY);
                                    assert_eq!(param["custom_ips"], json!([]));
                                    let [key] = param["target_keys"].as_array().unwrap().as_slice()
                                    else {
                                        panic!("one fixed target required")
                                    };
                                    let key = key.as_str().unwrap();
                                    let command = param["command"].as_str().unwrap();
                                    let is_count = command.contains(" -c ");
                                    if command.contains("'WAIT'") {
                                        entered.send(()).unwrap();
                                        gate.acquire().await.unwrap().forget();
                                    }
                                    let output = if command.contains("'LONG'") {
                                        format!("INFO: 2025-01-01 00:00:00.001 LONG order=demo token=fixture\t{}\n", "长".repeat(400)).repeat(39) + "INFO: 2025-01-01 00:00:00.000 LONG old\n"
                                    } else if is_count {
                                        "3\n".to_owned()
                                    } else {
                                        let i: usize = key.strip_prefix("private-").unwrap().parse().unwrap();
                                        [i, i + 4, 9].into_iter().map(|ms| format!("INFO: 2025-01-01 00:00:00.{ms:03} ERR GOOD sample\n")).collect()
                                    };
                                    observed.lock().unwrap().push((key.to_owned(), is_count));
                                    match key {
                                        "private-3" if command.contains("'ERR'") => json!({"status":{"status_code":17},"message":"private diagnostic"}).to_string(),
                                        "private-4" if command.contains("'ERR'") => {status = 503; "private diagnostic".into()},
                                        _ => json!({"status":{"status_code":0},"results":[{"ip":format!("192.0.2.{}",key.strip_prefix("private-").unwrap()),"user":"demo","module_name":"sample","return_code":0,"truncated":false,"output":output}]}).to_string(),
                                    }
                                }
                            };
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(status)
                                    .body(Full::new(Bytes::from(reply)))
                                    .unwrap(),
                            )
                        }
                    });
                    tokio::spawn(
                        http1::Builder::new().serve_connection(TokioIo::new(stream), handler),
                    );
                }
            });
            let runtime = Arc::new(LogRuntime::default());
            let mut session = new_session(LoginPage::Prodlog, base, "demo".into()).unwrap();
            session.client = reqwest::Client::builder().no_proxy().build().unwrap();
            {
                let mut state = runtime.0.state.lock().await;
                state.loaded = true;
                state.session = Some(session);
            }
            let scan = |module: &str, id, limit| {
                runtime.scan_prodlog(module.into(), id, "info".into(), "ERR".into(), limit, None, false)
            };
            let all = scan("7", ScanTargets::All, 3).await.unwrap();
            assert_eq!(
                (
                    all.target_count,
                    all.success_count,
                    all.failure_count,
                    all.total_matches
                ),
                (4, 2, 2, 6)
            );
            assert!(!all.complete && all.truncated);
            assert_eq!(all.targets.len(), 4);
            assert_eq!(all.targets.iter().map(|t| t.lines.len()).sum::<usize>(), 3);
            let view = serde_json::to_value(&all).unwrap();
            for (i, (ms, slot)) in [(9, 0), (9, 1), (6, 1)].into_iter().enumerate() {
                let timestamp = format!("2025-01-01 00:00:00.{ms:03}");
                assert_eq!(view["latestMatches"][i], json!({"timestamp": timestamp, "targetId": all.targets[slot].target_id, "line": format!("INFO: {timestamp} ERR GOOD sample")}));
            }
            assert_eq!(view["latestMatches"].as_array().unwrap().len(), 3);
            // 第四台在首批任务后才调度，仍须索取样本并参与并列排序。
            let late = runtime.scan_prodlog("7".into(), ScanTargets::All, "info".into(), "GOOD".into(), 5, None, true).await.unwrap();
            assert_eq!(serde_json::to_value(&late).unwrap()["latestMatches"][4]["targetId"], late.targets[3].target_id);
            assert!(all.targets[2..]
                .iter()
                .all(|t| t.error.is_some() && t.total_matches.is_none()));
            assert!(!serde_json::to_string(&all).unwrap().contains("private"));
            let one = scan("7", ScanTargets::One(all.targets[1].target_id.clone()), 1)
                .await
                .unwrap();
            assert!(one.complete && one.target_count == 1 && one.total_matches == 3);
            assert!(scan("7", ScanTargets::One("private-1".into()), 1)
                .await
                .is_err());
            assert!(scan("8", ScanTargets::All, 0).await.is_err());
            let previous = calls.lock().unwrap().len();
            let first = scan("7", ScanTargets::First, 0).await.unwrap();
            assert_eq!(first.targets[0].target_id, all.targets[0].target_id);
            assert!(
                first.complete && first.total_matches == 3 && first.targets[0].lines.is_empty()
            );
            for selector in [ScanTargets::First, ScanTargets::All] {
                assert!(scan("9", selector, 0)
                    .await
                    .unwrap_err()
                    .starts_with("NotFound:"));
            }
            assert_eq!(
                &calls.lock().unwrap()[previous..],
                &[("private-1".into(), true)]
            );
            let large = runtime
                .scan_prodlog(
                    "7".into(),
                    ScanTargets::First,
                    "wf".into(),
                    "LONG".into(),
                    50,
                    None,
                    true,
                )
                .await
                .unwrap();
            assert!(large.complete && large.truncated && large.byte_limit_reached && large.total_matches == 40);
            assert!(large.latest_matches.iter().all(|item| item.timestamp.ends_with(".001")));
            assert_eq!(large.latest_matches.len(), large.targets[0].lines.len());
            let lines = &large.targets[0].lines;
            let expected = format!("INFO: 2025-01-01 00:00:00.001 LONG order=demo token=fixture\t{}","长".repeat(400));
            assert!(lines.iter().all(|line| expected.starts_with(line) && (1022..=1024).contains(&line.len())));
            assert!(
                !lines.is_empty()
                    && lines.len() < 40
                    && lines.iter().map(String::len).sum::<usize>() <= SAMPLE_BYTES
            );

            let complete = runtime.scan_prodlog("7".into(), ScanTargets::All, "info".into(), "GOOD".into(), 0, None, true).await.unwrap();
            assert!(complete.complete && complete.target_count == 4 && complete.success_count == 4 && complete.failure_count == 0 && complete.total_matches == 12);
            assert!(complete.targets.iter().all(|row| row.lines.is_empty() && row.total_matches == Some(3)));

            // 人工推进时钟，超过总预算时必须保留未排到的第四台机器。
            let scanning = runtime.clone();
            let expired = tokio::spawn(async move {scanning.scan_prodlog("7".into(), ScanTargets::All, "wf".into(), "WAIT".into(), 0, None, true).await});
            for _ in 0..CONCURRENCY { assert!(tokio::time::timeout(Duration::from_secs(3), started.recv()).await.unwrap().is_some()); }
            tokio::time::pause();
            tokio::time::advance(SCAN_BUDGET + Duration::from_secs(1)).await;
            let timed_out = expired.await.unwrap().unwrap();
            tokio::time::resume();
            assert!(!timed_out.complete && timed_out.failure_count == 4 && timed_out.success_count == 0);
            assert!(timed_out.targets.iter().all(|t| t.error == Some("Timeout") && t.total_matches.is_none()));

            // 三个未完成的远端请求不占用登录锁；旧快照不可排队发起第四个请求或交付结果。
            let scanning = runtime.clone();
            let task = tokio::spawn(async move {
                scanning
                    .scan_prodlog(
                        "7".into(),
                        ScanTargets::All,
                        "wf".into(),
                        "WAIT".into(),
                        0,
                        None,
                        false,
                    )
                    .await
            });
            for _ in 0..CONCURRENCY {
                assert!(tokio::time::timeout(Duration::from_secs(3), started.recv())
                    .await
                    .unwrap()
                    .is_some());
            }
            assert!(scan("7", ScanTargets::All, 0)
                .await
                .unwrap_err()
                .starts_with("Limit:"));
            runtime
                .0
                .state
                .try_lock()
                .expect("scan must not hold login lock")
                .session = None;
            gate.add_permits(CONCURRENCY);
            assert_eq!(task.await.unwrap().unwrap_err(), SESSION_EXPIRED);
            assert!(started.try_recv().is_err());
            server.abort();
        });
    }
}
