//! 单机全文件字面搜索。只使用 SCM 的 grep，不使用管道、解释器或命令回退。
use super::{
    bounded_line, post, scm_code, single_file_param, validate_keyword, ArchiveHour,
    ProdlogResolution, Result, Session, Target, BAD_RESPONSE, MAX_MATCHES, QUERY,
};
use serde::Serialize;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FullFileResult {
    pub total_matches: u64,
    pub lines: Vec<String>,
    pub truncated: bool,
}

#[derive(Clone, Copy)]
enum Query {
    Count,
    Sample(usize),
}

pub(super) fn validate_scan_keyword(keyword: &str) -> Result<()> {
    validate_keyword(keyword)?;
    // -e 后的模式由单引号包裹；ASCII 双引号与逗号是普通数据，不允许单引号或控制字符。
    if !keyword
        .chars()
        .all(|c| c.is_alphanumeric() || " _-.:=,\"".contains(c))
    {
        return Err("InvalidInput: 全文件关键词含不支持的字符".into());
    }
    Ok(())
}

fn build_param(
    resolution: &ProdlogResolution,
    target: &Target,
    log_id: &str,
    archive_hour: Option<&str>,
    keyword: &str,
    query: Query,
) -> Result<Value> {
    validate_scan_keyword(keyword)?;
    let flags = match query {
        Query::Count => "-c".to_owned(),
        Query::Sample(n) if (1..=MAX_MATCHES).contains(&n) => format!("-m {}", n + 1),
        _ => return Err("InvalidInput: 无效的匹配行数上限".into()),
    };
    single_file_param(
        resolution,
        &target.id,
        log_id,
        archive_hour.map(ArchiveHour::parse).transpose()?,
        |path| format!("grep -aF {flags} -e '{keyword}' '{path}'"),
    )
}

fn checked_output<'a>(value: &'a Value, target: &Target) -> Result<(i32, &'a str)> {
    let [item] = value["results"].as_array().ok_or(BAD_RESPONSE)?.as_slice() else {
        return Err(BAD_RESPONSE.into());
    };
    if item["ip"].as_str().and_then(|s| s.parse().ok()) != Some(target.ip)
        || item["module_name"].as_str() != Some(&target.module_name)
        || item["user"].as_str() != Some(&target.user)
    {
        return Err(BAD_RESPONSE.into());
    }
    let code = scm_code(&item["return_code"]).ok_or(BAD_RESPONSE)?;
    if !matches!(code, 0 | 1) {
        return Err(format!("Protocol: SCM grep 命令失败（返回码 {code}）"));
    }
    if item["truncated"].as_bool().ok_or(BAD_RESPONSE)? {
        return Err("Limit: SCM 截断了 grep 响应，无法验证结果".into());
    }
    Ok((code, item["output"].as_str().ok_or(BAD_RESPONSE)?))
}

fn read_count(value: &Value, target: &Target) -> Result<u64> {
    let (code, raw) = checked_output(value, target)?;
    let text = raw.strip_suffix('\n').unwrap_or(raw);
    let count = text.parse::<u64>().map_err(|_| BAD_RESPONSE)?;
    if count.to_string() != text || (code == 1) != (count == 0) {
        return Err(BAD_RESPONSE.into());
    }
    Ok(count)
}

fn read_sample(
    value: &Value,
    target: &Target,
    keyword: &str,
    limit: usize,
    raw: bool,
) -> Result<(Vec<String>, usize, bool)> {
    let (code, output) = checked_output(value, target)?;
    if (code == 1) != output.is_empty() {
        return Err(BAD_RESPONSE.into());
    }
    let mut lines = Vec::new();
    let mut observed = 0;
    let mut shortened = false;
    for line in output.split_terminator('\n') {
        observed += 1;
        if observed > limit + 1 || !line.contains(keyword) {
            return Err(BAD_RESPONSE.into());
        }
        if observed <= limit {
            let (text, truncated) = bounded_line(line, raw);
            lines.push(text);
            shortened |= truncated;
        }
    }
    Ok((lines, observed, shortened))
}

/// resolution 应由调用方在当前会话重新解析。两次查询不构成文件快照；
/// total_matches 是计数查询时的准确行数（不需要第二次查询时取样即完整结果）。
pub(super) async fn scan_one(
    session: &Session,
    resolution: &ProdlogResolution,
    target: &Target,
    log_id: &str,
    archive_hour: Option<&str>,
    keyword: &str,
    max_lines: usize,
    raw: bool,
) -> Result<FullFileResult> {
    let param = |query| build_param(resolution, target, log_id, archive_hour, keyword, query);
    let count = || async { read_count(&post(session, QUERY, param(Query::Count)?).await?, target) };
    if max_lines == 0 {
        let total_matches = count().await?;
        return Ok(FullFileResult {
            total_matches,
            lines: vec![],
            truncated: total_matches > 0,
        });
    }
    let response = post(session, QUERY, param(Query::Sample(max_lines))?).await?;
    let (lines, observed, shortened) = read_sample(&response, target, keyword, max_lines, raw)?;
    let total_matches = if observed > max_lines {
        let n = count().await?;
        if n < observed as u64 {
            return Err("Protocol: 两次 SCM 查询间日志已变化".into());
        }
        n
    } else {
        observed as u64
    };
    Ok(FullFileResult {
        total_matches,
        lines,
        truncated: shortened || observed > max_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, process::Command};

    fn resolution() -> ProdlogResolution {
        super::super::catalog("7", &json!({"module":{"git_name":"demo/sample"},"targets":[{"git_pid":"7","key":"machine1","ip":"192.0.2.1","user":"tester","module_name":"sample"}]})).unwrap()
    }

    fn reply(code: i32, output: &str) -> Value {
        json!({"results":[{"ip":"192.0.2.1","user":"tester","module_name":"sample","return_code":code.to_string(),"truncated":false,"output":output}]})
    }

    #[test]
    fn real_grep_counts_beyond_tail_and_handles_long_matching_lines() {
        let keyword = "请求 traceId=001.2";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "unrelated 请求 traceId=001x2").unwrap();
        writeln!(file, "early {keyword} {keyword}").unwrap();
        writeln!(file, "{}{keyword} password=private", "长".repeat(90_000)).unwrap();
        writeln!(file, "{keyword} {}", "短 ".repeat(2000)).unwrap();
        for i in 0..250 {
            writeln!(file, "{keyword} {i}").unwrap();
        }
        writeln!(file, "未签署, exampleId=42, date=demo").unwrap();
        writeln!(file, "未签署, exampleId=420, date=demo").unwrap();
        writeln!(file, "{}", r#"{"exampleId":42,"ok":true}"#).unwrap();
        writeln!(file, "{}", r#"{"exampleId":420,"ok":true}"#).unwrap();
        let cat = resolution();
        let target = &cat.targets[0];
        let run = |k, kind| {
            let param = build_param(&cat, target, "info", None, k, kind).unwrap();
            let command = param["command"].as_str().unwrap().replace(
                "/data/logs/sample/sample.log.info",
                file.path().to_str().unwrap(),
            );
            let out = Command::new("sh").arg("-c").arg(command).output().unwrap();
            reply(
                out.status.code().unwrap(),
                std::str::from_utf8(&out.stdout).unwrap(),
            )
        };
        for needle in ["未签署, exampleId=42,", r#""exampleId":42,"#] {
            assert_eq!(read_count(&run(needle, Query::Count), target).unwrap(), 1);
            let (sample, count, _) =
                read_sample(&run(needle, Query::Sample(1)), target, needle, 1, true).unwrap();
            assert_eq!(count, 1);
            assert!(sample[0].contains(needle));
        }
        assert_eq!(
            read_count(&run(keyword, Query::Count), target).unwrap(),
            253
        );
        let (lines, observed, shortened) =
            read_sample(&run(keyword, Query::Sample(3)), target, keyword, 3, false).unwrap();
        assert_eq!(observed, 4);
        assert!(shortened);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], format!("early {keyword} {keyword}"));
        assert_eq!(lines[1], "[敏感内容已隐藏]");
        assert!(lines[2].len() <= 1024);
        assert_eq!(
            read_count(&run("missing", Query::Count), target).unwrap(),
            0
        );
        assert_eq!(
            read_sample(
                &run("missing", Query::Sample(1)),
                target,
                "missing",
                1,
                false
            )
            .unwrap()
            .1,
            0
        );
    }

    #[test]
    fn raw_sample_marks_only_actual_byte_truncation() {
        let cat = resolution();
        let target = &cat.targets[0];
        let prefix = "ERR order=demo token=fixture\t";
        let exact = format!("{prefix}{}", "a".repeat(1024 - prefix.len()));
        let over = format!("{}界", &exact[..1023]);
        for (line, expected, truncated) in [
            (exact.as_str(), exact.as_str(), false),
            (over.as_str(), &exact[..1023], true),
        ] {
            let response = reply(0, line);
            assert_eq!(
                read_sample(&response, target, "ERR", 1, true).unwrap(),
                (vec![expected.to_owned()], 1, truncated)
            );
            assert_eq!(
                read_sample(&response, target, "ERR", 1, false).unwrap(),
                (vec!["[敏感内容已隐藏]".into()], 1, false)
            );
        }
    }

    #[test]
    fn rejects_shell_syntax_and_preserves_authorized_scope() {
        let cat = resolution();
        let build = |log, hour, k, q| build_param(&cat, &cat.targets[0], log, hour, k, q);
        for k in [
            "'", "a'b", "\"a'b\"", "a\\b", "$(id)", "a;b", "a\nb", "  ", "`id`", "*",
        ] {
            assert!(build("wf", None, k, Query::Count).is_err(), "{k:?}");
        }
        for n in [0, MAX_MATCHES + 1] {
            assert!(build("wf", None, "err", Query::Sample(n)).is_err());
        }
        assert!(build("../x", None, "err", Query::Count).is_err());
        assert!(build("wf", Some("2024-02-30-01"), "err", Query::Count).is_err());
        let p = build("wf", Some("2024-02-29-01"), "请求 trace-id_7", Query::Count).unwrap();
        assert_eq!(p["target_keys"], json!(["machine1"]));
        assert_eq!(p["custom_ips"], json!([]));
        assert_eq!(
            p["command"],
            "grep -aF -c -e '请求 trace-id_7' '/data/logs/sample/sample.log.wf.2024-02-29-01'"
        );
    }

    #[test]
    fn rejects_partial_malformed_or_wrong_machine_responses() {
        let cat = resolution();
        let target = &cat.targets[0];
        let parse = |v: &Value| read_count(v, target);
        let ok = reply(0, "123\n");
        assert_eq!(parse(&ok).unwrap(), 123);
        for (field, bad) in [
            ("return_code", json!(1)),
            ("return_code", json!(2)),
            ("return_code", json!(null)),
            ("ip", json!("192.0.2.2")),
            ("module_name", json!("else")),
            ("user", json!("else")),
            ("truncated", json!(true)),
        ] {
            let mut v = ok.clone();
            v["results"][0][field] = bad;
            assert!(parse(&v).is_err());
        }
        assert!(parse(&json!({"results":[ok["results"][0],ok["results"][0]]})).is_err());
        for out in ["", "0", "2\nerror", "-1", " 1", "01"] {
            assert!(parse(&reply(0, out)).is_err(), "{out}");
        }
        assert!(parse(&reply(1, "")).is_err());
        for (code, out) in [
            (1, "oops"),
            (2, ""),
            (0, ""),
            (0, "irrelevant"),
            (0, "ERR\nERR\nERR"),
        ] {
            assert!(read_sample(&reply(code, out), target, "ERR", 1, false).is_err());
        }
    }
}
