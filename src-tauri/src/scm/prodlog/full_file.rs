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
    pub timestamps: Vec<String>,
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
        Query::Count => "-c ",
        Query::Sample(n) if (1..=MAX_MATCHES).contains(&n) => "",
        _ => return Err("InvalidInput: 无效的匹配行数上限".into()),
    };
    single_file_param(
        resolution,
        &target.id,
        log_id,
        archive_hour.map(ArchiveHour::parse).transpose()?,
        |path| format!("grep -aF {flags}-e '{keyword}' '{path}'"),
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

// 只读取行首日志时间，不把正文中提到的其他时间误当作该行的时间。
fn timestamp(line: &str) -> Result<&str> {
    static PREFIX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^(?:[A-Z]+:[ \t]+)?([0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-5][0-9]:[0-5][0-9]\.[0-9]{3})(?:[ \t]|$)").expect("static log timestamp")
    });
    let stamp = PREFIX
        .captures(line)
        .and_then(|m| m.get(1))
        .map(|m| m.as_str())
        .filter(|s| super::valid_archive_hour(&s[..13].replace(' ', "-")))
        .ok_or("Protocol: 命中行缺少有效日志时间，无法确定最新记录")?;
    Ok(stamp)
}

fn read_sample(
    value: &Value,
    target: &Target,
    keyword: &str,
    limit: usize,
    raw: bool,
) -> Result<FullFileResult> {
    let (code, output) = checked_output(value, target)?;
    if (code == 1) != output.is_empty() {
        return Err(BAD_RESPONSE.into());
    }
    // SCM 响应已受 MAX_BYTES 限制。先验证所有命中，再以文件中靠后的行打破同毫秒平局。
    let mut matches = Vec::new();
    for (index, line) in output.split_terminator('\n').enumerate() {
        if !line.contains(keyword) {
            return Err(BAD_RESPONSE.into());
        }
        matches.push((timestamp(line)?, index, line));
    }
    matches.sort_unstable_by_key(|&(stamp, index, _)| std::cmp::Reverse((stamp, index)));
    let total_matches = matches.len() as u64;
    let mut truncated = matches.len() > limit;
    let mut lines = Vec::new();
    let mut timestamps = Vec::new();
    for (stamp, _, line) in matches.into_iter().take(limit) {
        let (text, shortened) = bounded_line(line, raw);
        lines.push(text);
        timestamps.push(stamp.to_owned());
        truncated |= shortened;
    }
    Ok(FullFileResult {
        total_matches,
        lines,
        timestamps,
        truncated,
    })
}

/// 一次 grep 扫完整个文件；计数模式仅传输计数，不取日志正文。
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
    let query = if max_lines == 0 {
        Query::Count
    } else {
        Query::Sample(max_lines)
    };
    let param = build_param(resolution, target, log_id, archive_hour, keyword, query)?;
    let response = post(session, QUERY, param).await?;
    if max_lines == 0 {
        let total_matches = read_count(&response, target)?;
        return Ok(FullFileResult {
            total_matches,
            lines: vec![],
            timestamps: vec![],
            truncated: total_matches > 0,
        });
    }
    read_sample(&response, target, keyword, max_lines, raw)
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
    fn latest_from_archive_is_not_first_or_last_n() {
        let cat = resolution();
        let target = &cat.targets[0];
        let keyword = "id=1.2,";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "INFO: 2024-02-29 10:00:00.998 {keyword} password=private"
        )
        .unwrap();
        for i in 0..250 {
            writeln!(file, "INFO: 2024-02-29 10:00:00.{i:03} {keyword}").unwrap();
        }
        writeln!(file, "INFO: 2024-02-29 10:00:00.999 {keyword} newest").unwrap();
        writeln!(file, "INFO: 2024-02-29 10:00:00.001 {keyword} old").unwrap();
        writeln!(file, "unrelated id=1x2,").unwrap();
        let param = build_param(
            &cat,
            target,
            "info",
            Some("2024-02-29-10"),
            keyword,
            Query::Sample(2),
        )
        .unwrap();
        let command = param["command"].as_str().unwrap().replace(
            "/data/logs/sample/sample.log.info.2024-02-29-10",
            file.path().to_str().unwrap(),
        );
        let output = Command::new("sh").args(["-c", &command]).output().unwrap();
        let response = reply(
            output.status.code().unwrap(),
            std::str::from_utf8(&output.stdout).unwrap(),
        );
        let result = read_sample(&response, target, keyword, 2, false).unwrap();
        assert_eq!(result.total_matches, 253);
        assert_eq!(
            result.timestamps,
            ["2024-02-29 10:00:00.999", "2024-02-29 10:00:00.998"]
        );
        assert!(result.lines[0].ends_with("newest"));
        assert_eq!(result.lines[1], "[敏感内容已隐藏]");
        assert!(result.truncated);
    }

    #[test]
    fn real_grep_counts_beyond_tail_and_matches_literal_keywords() {
        let keyword = "请求 traceId=001.2";
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "unrelated 请求 traceId=001x2").unwrap();
        for i in 0..253 {
            writeln!(file, "INFO: 2024-02-29 10:00:00.{i:03} {keyword}").unwrap();
        }
        writeln!(
            file,
            "2024-02-29 10:00:00.000 未签署, exampleId=42, date=demo"
        )
        .unwrap();
        writeln!(
            file,
            "2024-02-29 10:00:00.000 未签署, exampleId=420, date=demo"
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"2024-02-29 10:00:00.000 {"exampleId":42,"ok":true}"#
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            r#"2024-02-29 10:00:00.000 {"exampleId":420,"ok":true}"#
        )
        .unwrap();
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
            let sample =
                read_sample(&run(needle, Query::Sample(1)), target, needle, 1, true).unwrap();
            assert_eq!(sample.total_matches, 1);
            assert!(sample.lines[0].contains(needle));
        }
        assert_eq!(
            read_count(&run(keyword, Query::Count), target).unwrap(),
            253
        );
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
            .total_matches,
            0
        );
    }

    #[test]
    fn does_not_report_latest_from_partial_or_unordered_evidence() {
        let cat = resolution();
        let target = &cat.targets[0];
        let first = "INFO: 2024-02-29 10:00:00.999 ERR";
        for bad in [
            "ERR",
            "ERR body 2024-01-01 00:00:00.000",
            "INFO: 2024-02-30 00:00:00.000 ERR",
            "2024-01-01 00:00:00.0001 ERR",
        ] {
            let response = reply(0, &format!("{first}\n{bad}\n"));
            assert!(read_sample(&response, target, "ERR", 1, false).is_err());
        }
        let mut partial = reply(0, first);
        partial["results"][0]["truncated"] = json!(true);
        assert!(read_sample(&partial, target, "ERR", 1, true).is_err());

        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        for query in [Query::Count, Query::Sample(1)] {
            let param = build_param(&cat, target, "info", None, "ERR", query).unwrap();
            let command = param["command"].as_str().unwrap().replace(
                "/data/logs/sample/sample.log.info",
                missing.to_str().unwrap(),
            );
            let output = Command::new("sh").args(["-c", &command]).output().unwrap();
            let response = reply(
                output.status.code().unwrap(),
                std::str::from_utf8(&output.stdout).unwrap(),
            );
            assert!(checked_output(&response, target).is_err());
        }
    }

    #[test]
    fn raw_sample_marks_only_actual_byte_truncation() {
        let cat = resolution();
        let target = &cat.targets[0];
        let prefix = "2024-02-29 10:00:00.000 ERR order=demo token=fixture\t";
        let exact = format!("{prefix}{}", "a".repeat(1024 - prefix.len()));
        let over = format!("{}界", &exact[..1023]);
        for (line, expected, truncated) in [
            (exact.as_str(), exact.as_str(), false),
            (over.as_str(), &exact[..1023], true),
        ] {
            let response = reply(0, line);
            let sample = read_sample(&response, target, "ERR", 1, true).unwrap();
            assert_eq!(sample.lines, [expected]);
            assert_eq!(sample.truncated, truncated);
            assert_eq!(
                read_sample(&response, target, "ERR", 1, false)
                    .unwrap()
                    .lines,
                ["[敏感内容已隐藏]"]
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
