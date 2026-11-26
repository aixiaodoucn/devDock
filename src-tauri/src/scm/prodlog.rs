//! 生产日志查询只使用已登录 SCM 返回的目标，每个远端请求固定到一台机器。
mod coverage;
mod full_file;
use super::{
    is_login_page, is_login_path, network_error, numeric_id, LogRuntime, Result, Session,
    SESSION_EXPIRED,
};
pub(crate) use coverage::ScanTargets;
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{hash_map::RandomState, HashSet},
    hash::BuildHasher,
    net::IpAddr,
    sync::LazyLock,
};

const GET_TARGETS: &str = "/kpi/prodlog/getTargets";
const QUERY: &str = "/kpi/prodlog/query";
const MAX_BYTES: usize = 512 * 1024;
pub(crate) const DEFAULT_MATCH_LIMIT: usize = 500;
pub(crate) const MAX_MATCHES: usize = 10_000;
const BAD_RESPONSE: &str = "Protocol: SCM 日志响应格式或目标不匹配";
const INVALID_TARGET: &str = "InvalidInput: 请选择已解析的单台机器及白名单日志";
static HASHER: LazyLock<RandomState> = LazyLock::new(RandomState::new);

#[derive(Debug, Serialize)]
pub struct LogOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Serialize)]
pub struct Target {
    pub id: String,
    pub label: String,
    #[serde(skip)]
    key: String,
    #[serde(skip)]
    ip: IpAddr,
    #[serde(skip)]
    user: String,
    #[serde(skip)]
    module_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdlogResolution {
    pub module_id: String,
    pub targets: Vec<Target>,
    pub logs: Vec<LogOption>,
    #[serde(skip)]
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchScope {
    pub module_id: String,
    pub target_id: String,
    pub log_id: String,
    pub archive_hour: Option<String>,
    pub file_scope: &'static str,
    pub returned_lines: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdlogSearch {
    pub total_matches: u64,
    pub lines: Vec<String>,
    pub truncated: bool,
    pub scope: SearchScope,
}

fn validate_keyword(keyword: &str) -> Result<()> {
    if keyword.trim().is_empty() || keyword.len() > 128 || keyword.chars().any(char::is_control) {
        return Err(
            "InvalidInput: 关键词必须是 1–128 字节的非空白字面文本，不能包含控制字符".into(),
        );
    }
    Ok(())
}

fn catalog(module: &str, value: &Value) -> Result<ProdlogResolution> {
    let name = value["module"]["git_name"]
        .as_str()
        .and_then(|s| s.rsplit('/').next())
        .ok_or(BAD_RESPONSE)?;
    if name.len() > 96
        || !name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    {
        return Err(BAD_RESPONSE.into());
    }
    let rows = value["targets"].as_array().ok_or(BAD_RESPONSE)?;
    let mut keys = HashSet::new();
    let mut ids = HashSet::new();
    let mut targets = Vec::new();
    for row in rows {
        let field = |key: &str| {
            row[key]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
                .ok_or(BAD_RESPONSE)
        };
        let key = field("key")?;
        let ip = field("ip")?.parse::<IpAddr>().map_err(|_| BAD_RESPONSE)?;
        let user = field("user")?;
        let module_name = field("module_name")?;
        let pid = &row["git_pid"];
        if !(pid.as_str() == Some(module) || pid.as_u64().is_some_and(|n| n.to_string() == module))
            || !keys.insert(key)
        {
            return Err(BAD_RESPONSE.into());
        }
        // 进程内稳定且与身份元组绑定，目标重排不会改变选择；内部 key 不离开 Rust。
        let id = format!(
            "t{:016x}",
            HASHER.hash_one((module, name, key, ip, user, module_name))
        );
        if !ids.insert(id.clone()) {
            return Err(BAD_RESPONSE.into());
        }
        targets.push(Target {
            label: format!("机器 {}", &id[1..]),
            id,
            key: key.into(),
            ip,
            user: user.into(),
            module_name: module_name.into(),
        });
    }
    let logs = ["info", "wf"]
        .into_iter()
        .map(|id| LogOption {
            id: id.into(),
            label: format!("{name}.log.{id}"),
        })
        .collect();
    Ok(ProdlogResolution {
        module_id: module.into(),
        targets,
        logs,
        name: name.into(),
    })
}

/// 先限制为固定 ASCII 形状，再按公历计算月份天数；后缀不接受路径或通配符。
pub(crate) fn valid_archive_hour(hour: &str) -> bool {
    if hour.len() != 13
        || !hour.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 | 10 => b == b'-',
            _ => b.is_ascii_digit(),
        })
    {
        return false;
    }
    let number = |part: &str| part.bytes().fold(0u32, |n, b| n * 10 + u32::from(b - b'0'));
    let year = number(&hour[..4]);
    let days = match number(&hour[5..7]) {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    year > 0 && (1..=days).contains(&number(&hour[8..10])) && number(&hour[11..]) < 24
}

struct ArchiveHour<'a>(&'a str);

impl<'a> ArchiveHour<'a> {
    fn parse(hour: &'a str) -> Result<Self> {
        if valid_archive_hour(hour) {
            Ok(Self(hour))
        } else {
            Err("InvalidInput: 归档小时必须是有效的 YYYY-MM-DD-HH".into())
        }
    }
}

// 两种查询共用同一白名单路径与单机目标封装；客户端不能提供路径或命令。
fn single_file_param(
    catalog: &ProdlogResolution,
    target_id: &str,
    log_id: &str,
    archive: Option<ArchiveHour<'_>>,
    command: impl FnOnce(&str) -> String,
) -> Result<Value> {
    let target = catalog
        .targets
        .iter()
        .find(|t| t.id == target_id)
        .ok_or(INVALID_TARGET)?;
    if !matches!(log_id, "info" | "wf") {
        return Err(INVALID_TARGET.into());
    }
    let name = &catalog.name;
    let suffix = archive
        .map(|hour| format!(".{}", hour.0))
        .unwrap_or_default();
    Ok(
        json!({"git_pid":catalog.module_id,"target_keys":[target.key],"custom_ips":[],"command":command(&format!("/data/logs/{name}/{name}.log.{log_id}{suffix}"))}),
    )
}

static SENSITIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ix)authorization|bearer|basic\s|cookie|passw(?:or)?d|passwd|pwd|token|secret|api[._-]?key|private[\s_-]?key|credential|session[._-]?id|身份证|密码|密钥|手机号|1[3-9][0-9]{9}|[0-9]{17}[0-9X]|[\w.+-]+@[\w.-]+\.[a-z]{2,}|://[^\s/]+@|[a-z0-9_+/=-]{40,}|eyJ[a-z0-9_-]+\.[a-z0-9_-]+|(?:[0-9]{1,3}\.){3}[0-9]{1,3}").expect("static redaction rules")
});

fn bounded_line(line: &str, raw: bool) -> (String, bool) {
    // 未授权原文时先检查整行，避免截断边界暴露部分敏感字段。
    if !raw && (line.chars().any(char::is_control) || SENSITIVE.is_match(line)) {
        return ("[敏感内容已隐藏]".into(), false);
    }
    let mut end = line.len().min(1024);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    (line[..end].into(), end < line.len())
}

async fn post(session: &Session, path: &'static str, param: Value) -> Result<Value> {
    let response = session
        .client
        .post(session.base.join(path).map_err(|_| BAD_RESPONSE)?)
        .form(&[("param", param.to_string())])
        .send()
        .await
        .map_err(network_error)?;
    decode_response(response).await
}

async fn decode_response(mut response: reqwest::Response) -> Result<Value> {
    if response.status() == reqwest::StatusCode::UNAUTHORIZED || is_login_path(response.url()) {
        return Err(SESSION_EXPIRED.into());
    }
    if !response.status().is_success() {
        return Err(format!(
            "Network: SCM 返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_error)? {
        if chunk.len() > MAX_BYTES - bytes.len() {
            return Err("Limit: SCM 日志响应超出字节上限".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        if is_login_page(&String::from_utf8_lossy(&bytes)) {
            SESSION_EXPIRED
        } else {
            BAD_RESPONSE
        }
    })?;
    let code = scm_code(&value["status"]["status_code"]).ok_or(BAD_RESPONSE)?;
    if code != 0 {
        return Err(format!("ScmBusiness: {code}"));
    }
    Ok(value)
}

// 只接受整数或其标准十进制字符串；不转发响应文本或未校验的状态字段。
fn scm_code(value: &Value) -> Option<i32> {
    if let Some(n) = value.as_i64() {
        return i32::try_from(n).ok();
    }
    let text = value.as_str()?;
    text.parse::<i32>().ok().filter(|n| n.to_string() == text)
}

impl LogRuntime {
    pub async fn resolve_prodlog(&self, module_id: String) -> Result<ProdlogResolution> {
        if !numeric_id(&module_id) {
            return Err("InvalidInput: 需要数字模块 ID".into());
        }
        let mut state = self.0.state.lock().await;
        self.0.restore(&mut state).await?;
        let session = state.session.as_mut().ok_or(SESSION_EXPIRED)?;
        let result = post(session, GET_TARGETS, json!({"git_pid":module_id}))
            .await
            .and_then(|value| catalog(&module_id, &value));
        if let Err(ref error) = result {
            self.0.expire(&mut state, error).await;
        } else if session.remembered {
            super::persist(session, self.0.credentials.clone()).await;
        }
        result
    }

    pub async fn search_prodlog(
        &self,
        module_id: String,
        target_id: String,
        log_id: String,
        keyword: String,
        max_lines: usize,
        archive_hour: Option<String>,
        raw: bool,
    ) -> Result<ProdlogSearch> {
        archive_hour
            .as_deref()
            .map(ArchiveHour::parse)
            .transpose()?;
        full_file::validate_scan_keyword(&keyword)?;
        if !numeric_id(&module_id) || !(1..=MAX_MATCHES).contains(&max_lines) {
            return Err("InvalidInput: 无效的模块 ID 或匹配行数上限".into());
        }
        let mut state = self.0.state.lock().await;
        self.0.restore(&mut state).await?;
        let session = state.session.as_mut().ok_or(SESSION_EXPIRED)?;
        // 在同一登录会话内重新查询授权目标，不复用前端或上一次查询的清单。
        let result: Result<ProdlogSearch> = async {
            let cat = catalog(
                &module_id,
                &post(session, GET_TARGETS, json!({"git_pid":module_id})).await?,
            )?;
            let target = cat
                .targets
                .iter()
                .find(|t| t.id == target_id)
                .ok_or(INVALID_TARGET)?;
            let found = full_file::scan_one(
                session,
                &cat,
                target,
                &log_id,
                archive_hour.as_deref(),
                &keyword,
                max_lines,
                raw,
            )
            .await?;
            let scope = SearchScope {
                module_id,
                target_id,
                log_id,
                archive_hour: archive_hour.clone(),
                file_scope: "fullFile",
                returned_lines: found.lines.len(),
            };
            Ok(ProdlogSearch {
                total_matches: found.total_matches,
                lines: found.lines,
                truncated: found.truncated,
                scope,
            })
        }
        .await;
        if let Err(ref error) = result {
            self.0.expire(&mut state, error).await;
        } else if session.remembered {
            super::persist(session, self.0.credentials.clone()).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> serde_json::Value {
        serde_json::json!({"status":{"status_code":0},"module":{"git_name":"demo/sample"},"targets":[{"key":"sample-1","ip":"192.0.2.7","user":"demo","module_name":"sample","git_pid":"7"}]})
    }

    #[test]
    fn local_scm_uses_existing_cookie_and_only_sends_authorized_forms() {
        use reqwest::{Client, Url};
        use std::{
            io::{BufRead, BufReader, Read, Write},
            net::TcpListener,
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let keyword = "failure: code=7";
        let server = std::thread::spawn(move || {
            // 状态和模块清单都只访问日志首页，夹具不提供项目列表权限。
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = BufReader::new(stream.try_clone().unwrap()).lines();
                assert_eq!(
                    request.next().unwrap().unwrap(),
                    "GET /kpi/prodlog/index HTTP/1.1"
                );
                let headers: Vec<_> = request
                    .map(|line| line.unwrap())
                    .take_while(|line| !line.is_empty())
                    .collect();
                assert!(headers
                    .iter()
                    .any(|line| line.eq_ignore_ascii_case("cookie: PHPSESSID=fixture")));
                let body = "<select id=git_pid><option value=7>sample</option></select>";
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            // 当前和归档分别重验、查询；最后的过期目标只重验，不发起 query。
            for step in 0..6 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                reader.read_line(&mut request).unwrap();
                let expected_path = if matches!(step, 2 | 4) {
                    QUERY
                } else {
                    GET_TARGETS
                };
                assert_eq!(request.trim(), format!("POST {expected_path} HTTP/1.1"));
                let mut headers = std::collections::HashMap::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    let (k, v) = line.trim().split_once(':').unwrap();
                    headers.insert(k.to_lowercase(), v.trim().to_string());
                }
                assert_eq!(headers["content-type"], "application/x-www-form-urlencoded");
                assert_eq!(headers["cookie"], "PHPSESSID=fixture");
                let mut body = vec![0u8; headers["content-length"].parse().unwrap()];
                reader.read_exact(&mut body).unwrap();
                let mut form = Url::parse("http://example.invalid/").unwrap();
                form.set_query(Some(std::str::from_utf8(&body).unwrap()));
                let fields: Vec<_> = form.query_pairs().collect();
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "param");
                let param: Value = serde_json::from_str(&fields[0].1).unwrap();
                let mut reply = fixture();
                if matches!(step, 2 | 4) {
                    let command = if step == 2 {
                        "grep -aF -e 'failure: code=7' '/data/logs/sample/sample.log.info'"
                    } else {
                        "grep -aF -e 'failure: code=7' '/data/logs/sample/sample.log.wf.2024-02-29-23'"
                    };
                    assert_eq!(
                        param,
                        json!({"git_pid":"7", "custom_ips":[], "target_keys":["sample-1"], "command":command})
                    );
                    reply = json!({"status":{"status_code":0},"results":[{"ip":"192.0.2.7","module_name":"sample","user":"demo","return_code":0,"truncated":false,"output":format!("INFO: 2024-02-29 23:05:00.001 {keyword} order=demo-7 token=fixture\nINFO: 2024-02-29 23:00:00.000 {keyword} old")} ]});
                } else {
                    assert_eq!(param, json!({"git_pid":"7"}));
                    if step == 5 {
                        reply["targets"] = json!([]);
                    }
                }
                let body = reply.to_string();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let mut session = super::super::new_session(
            super::super::LoginPage::Prodlog,
            base.clone(),
            "demo".into(),
        )
        .unwrap();
        session
            .jar
            .add_cookie_str("PHPSESSID=fixture; Path=/", &base);
        session.client = Client::builder()
            .no_proxy()
            .cookie_provider(session.jar.clone())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        tauri::async_runtime::block_on(async {
            let mut runtime = LogRuntime::default();
            runtime.0.credentials =
                std::sync::Arc::new(super::super::tests::FakeCredentialStore::default());
            {
                let mut state = runtime.0.state.lock().await;
                state.loaded = true;
                state.session = Some(session);
            }
            assert!(runtime.status().await.unwrap().connected);
            assert_eq!(
                runtime.log_modules().await.unwrap(),
                vec![("7".into(), "sample".into())]
            );
            let catalog = runtime.resolve_prodlog("7".into()).await.unwrap();
            let target = &catalog.targets[0].id;
            let search = |log: &str, hour: Option<String>, raw| {
                runtime.search_prodlog(
                    "7".into(),
                    target.clone(),
                    log.into(),
                    keyword.into(),
                    1,
                    hour,
                    raw,
                )
            };
            assert!(search("info", Some("2023-02-29-00".into()), false)
                .await
                .unwrap_err()
                .starts_with("InvalidInput:"));
            let result = search("info", None, false).await.unwrap();
            assert_eq!(json!(result.scope)["fileScope"], "fullFile");
            assert!(json!(result.scope).get("tailLines").is_none());
            assert_eq!(json!(result.scope).get("archiveHour"), Some(&Value::Null));
            assert_eq!(result.lines, vec!["[敏感内容已隐藏]"]);
            assert!(result.truncated);
            let archive = search("wf", Some("2024-02-29-23".into()), true)
                .await
                .unwrap();
            assert_eq!(json!(archive.scope)["archiveHour"], "2024-02-29-23");
            assert_eq!(
                archive.lines,
                vec![format!(
                    "INFO: 2024-02-29 23:05:00.001 {keyword} order=demo-7 token=fixture"
                )]
            );
            assert!(archive.truncated);
            assert_eq!(
                search("wf", Some("2024-02-29-23".into()), true)
                    .await
                    .unwrap_err(),
                INVALID_TARGET
            );
        });
        server.join().unwrap();
    }

    #[test]
    fn builds_only_two_fixed_commands_for_one_resolved_target() {
        let catalog = catalog("7", &fixture()).unwrap();
        let target = &catalog.targets[0];
        let path = |path: &str| path.to_owned();
        let param = single_file_param(&catalog, &target.id, "wf", None, path).unwrap();
        assert_eq!(param["custom_ips"], serde_json::json!([]));
        assert_eq!(param["target_keys"], serde_json::json!(["sample-1"]));
        assert!(param["command"]
            .as_str()
            .unwrap()
            .ends_with("/data/logs/sample/sample.log.wf"));
        assert!(single_file_param(&catalog, "192.0.2.42", "info", None, path).is_err());
        assert!(single_file_param(&catalog, &target.id, "../../etc/passwd", None, path).is_err());
    }

    #[test]
    fn rejects_unsafe_modules_mismatched_membership_and_duplicate_targets() {
        for name in [
            "demo/..",
            "demo/.hidden",
            "demo/foo;id",
            "demo/foo bar",
            "demo/",
        ] {
            let mut value = fixture();
            value["module"]["git_name"] = name.into();
            assert!(catalog("7", &value).is_err());
        }
        let mut value = fixture();
        value["targets"][0]["git_pid"] = "8".into();
        assert!(catalog("7", &value).is_err());
        let mut value = fixture();
        let first = value["targets"][0].clone();
        value["targets"].as_array_mut().unwrap().push(first);
        assert!(catalog("7", &value).is_err());
    }

    #[test]
    fn enforces_body_budget_and_never_exports_upstream_errors() {
        let decode = |body: Vec<u8>| {
            let response = tauri::http::Response::new(body);
            tauri::async_runtime::block_on(decode_response(response.into()))
        };
        assert!(decode(fixture().to_string().into_bytes()).is_ok());
        assert_eq!(
            decode(vec![b'x'; MAX_BYTES + 1]).unwrap_err(),
            "Limit: SCM 日志响应超出字节上限"
        );
        assert_eq!(
            decode(br#"<input type="password">"#.to_vec()).unwrap_err(),
            SESSION_EXPIRED
        );
        for (code, expected) in [
            (json!(17), "ScmBusiness: 17"),
            (json!("-23"), "ScmBusiness: -23"),
            (json!(null), BAD_RESPONSE),
            (json!(2147483648_i64), BAD_RESPONSE),
            (json!("17 synthetic-sensitive-value"), BAD_RESPONSE),
        ] {
            let body = json!({"status":{"status_code":code},"message":"synthetic-sensitive-value"});
            assert_eq!(decode(body.to_string().into_bytes()).unwrap_err(), expected);
        }
    }

    #[test]
    fn accepts_only_real_calendar_hours_in_exact_ascii_format() {
        for hour in [
            "2000-02-29-23",
            "2024-02-29-00",
            "1900-02-28-12",
            "2025-12-31-23",
        ] {
            assert!(valid_archive_hour(hour), "{hour}");
        }
        for hour in [
            "",
            "0000-01-01-00",
            "1900-02-29-00",
            "2024-04-31-00",
            "2024-00-01-00",
            "2024-13-01-00",
            "2024-01-00-00",
            "2024-01-01-24",
            "2024-1-01-01",
            "２０２４-01-01-00",
            "2024-01-01-00\n",
            "2024-01-01-00;",
            "2024/01/01/00",
        ] {
            assert!(!valid_archive_hour(hour), "{hour:?}");
        }
    }

    #[test]
    fn only_accepts_bounded_literal_keywords() {
        for value in [
            "ERROR",
            "请求失败",
            "trace-123_45.6",
            "error: id=7",
            "$(id);*",
            "a/b",
        ] {
            assert!(validate_keyword(value).is_ok(), "{value}");
        }
        for value in ["", "  ", "a\nb", "a\0b", &"x".repeat(129)] {
            assert!(validate_keyword(value).is_err(), "{value}");
        }
    }

    #[test]
    fn does_not_export_private_machine_identity() {
        let catalog = catalog("7", &fixture()).unwrap();
        let exported = serde_json::to_string(&catalog).unwrap();
        for internal in ["sample-1", "192.0.2.7", "demo"] {
            assert!(!exported.contains(internal));
        }
    }
}
