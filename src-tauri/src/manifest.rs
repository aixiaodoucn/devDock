use crate::config::{atomic_write, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, ops::Range, path::Path};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestInfo {
    pub version_name: String,
    pub version_code: String,
    pub appid: String,
}
// 空格替换注释，保持 UTF-8 字节偏移；字符串内的 // 和转义不能当注释处理。
fn clean(text: &str) -> Result<String> {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let (mut i, mut string) = (0, false);
    while i < b.len() {
        if string {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == b'"' {
                string = false
            }
            i += 1;
            continue;
        }
        if b[i] == b'"' {
            string = true;
            i += 1;
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1
            }
        } else if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                out[i] = b' ';
                i += 1
            }
            if i + 1 >= b.len() {
                return Err("未闭合 JSONC 注释".into());
            }
            out[i] = b' ';
            out[i + 1] = b' ';
            i += 2
        } else {
            i += 1
        }
    }
    // JSONC 的尾逗号同样只替换一个字节。
    let mut i = 0;
    let mut string = false;
    while i < out.len() {
        if string {
            if out[i] == b'\\' {
                i += 2;
                continue;
            }
            if out[i] == b'"' {
                string = false
            }
        } else if out[i] == b'"' {
            string = true
        } else if out[i] == b',' {
            let mut j = i + 1;
            while j < out.len() && out[j].is_ascii_whitespace() {
                j += 1
            }
            if j < out.len() && (out[j] == b'}' || out[j] == b']') {
                out[i] = b' '
            }
        }
        i += 1
    }
    String::from_utf8(out).map_err(|e| e.to_string())
}
fn spans(text: &str) -> Result<HashMap<String, Range<usize>>> {
    let c = clean(text)?;
    let value: serde_json::Value = serde_json::from_str(&c).map_err(|e| e.to_string())?;
    if !value.is_object() {
        return Err("manifest 必须为对象".into());
    }
    let b = c.as_bytes();
    let mut i = 0;
    let mut depth = 0;
    let mut result = HashMap::new();
    while i < b.len() {
        match b[i] {
            b'{' | b'[' => {
                depth += 1;
                i += 1
            }
            b'}' | b']' => {
                depth -= 1;
                i += 1
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < b.len() {
                    if b[i] == b'\\' {
                        i += 2
                    } else if b[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1
                    }
                }
                if depth == 1 {
                    let key: String =
                        serde_json::from_str(&c[start..i]).map_err(|e| e.to_string())?;
                    let mut j = i;
                    while j < b.len() && b[j].is_ascii_whitespace() {
                        j += 1
                    }
                    if j < b.len() && b[j] == b':' {
                        j += 1;
                        while b[j].is_ascii_whitespace() {
                            j += 1
                        }
                        let start = j;
                        if b[j] == b'"' {
                            j += 1;
                            while j < b.len() {
                                if b[j] == b'\\' {
                                    j += 2
                                } else if b[j] == b'"' {
                                    j += 1;
                                    break;
                                } else {
                                    j += 1
                                }
                            }
                        } else {
                            while j < b.len() && !b",}\r\n \t".contains(&b[j]) {
                                j += 1
                            }
                        }
                        if result.insert(key, start..j).is_some() {
                            return Err("manifest 根字段重复".into());
                        }
                    }
                }
            }
            _ => i += 1,
        }
    }
    Ok(result)
}
pub fn field(text: &str, key: &str) -> Result<serde_json::Value> {
    let s = spans(text)?;
    let r = s.get(key).ok_or_else(|| format!("缺少根字段 {key}"))?;
    serde_json::from_str(&clean(text)?[r.clone()]).map_err(|e| e.to_string())
}
pub fn replace(text: &str, key: &str, value: &serde_json::Value) -> Result<String> {
    let s = spans(text)?;
    let r = s.get(key).ok_or_else(|| format!("缺少根字段 {key}"))?;
    let mut out = text.to_string();
    out.replace_range(r.clone(), &value.to_string());
    Ok(out)
}
pub fn info(text: &str) -> Result<ManifestInfo> {
    let string = |k| -> Result<String> {
        field(text, k)?
            .as_str()
            .map(str::to_owned)
            .ok_or(format!("{k} 必须是字符串"))
    };
    let code = field(text, "versionCode")?;
    Ok(ManifestInfo {
        version_name: string("versionName")?,
        version_code: code
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| code.to_string()),
        appid: string("appid")?,
    })
}
pub fn validate_version(name: &str, code: &str) -> Result<()> {
    if !regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?$")
        .unwrap()
        .is_match(name)
        || name.len() > 80
    {
        return Err("版本名需为 1.2.3 格式".into());
    }
    if code.is_empty()
        || !code.bytes().all(|c| c.is_ascii_digit())
        || code
            .parse::<u32>()
            .map_or(true, |v| v == 0 || v > 2_100_000_000)
    {
        return Err("versionCode 需为 1..2100000000 的整数".into());
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Recovery {
    pub path: String,
    pub original: String,
    pub temporary: String,
    // 旧恢复记录没有换行风格；空值表示不调整现有换行。
    #[serde(default)]
    pub line_ending: String,
}
// 记录构建前的换行风格，恢复时写回，避免构建工具重写后留下不同格式。
pub fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "crlf"
    } else {
        "lf"
    }
}
fn apply_line_ending(text: &str, style: &str) -> String {
    let lf = text.replace("\r\n", "\n");
    match style {
        "crlf" => lf.replace('\n', "\r\n"),
        "lf" => lf,
        _ => text.to_owned(),
    }
}
pub fn restore(record: &Recovery) -> Result<String> {
    let path = Path::new(&record.path);
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let current = field(&text, "appid")?;
    if current == record.original {
        return Ok(format!("{} AppID 已恢复", record.path));
    }
    if current != record.temporary {
        return Err(format!(
            "{} AppID 已被用户修改，保留恢复记录且不覆盖",
            record.path
        ));
    }
    let restored = replace(&text, "appid", &record.original.clone().into())?;
    atomic_write(path, &apply_line_ending(&restored, &record.line_ending))?;
    Ok(format!("{} AppID 已恢复，保留新版本号", record.path))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jsonc_patch() {
        let text="{ // 中文\n\"appid\":\"old\", /* note */ \"versionName\":\"1.0.0\",\"versionCode\":1,\"nested\":{\"appid\":\"nested\"},}";
        let updated = replace(text, "appid", &"new".into()).unwrap();
        assert!(updated.contains("// 中文"));
        assert!(updated.contains("/* note */"));
        assert!(updated.contains("\"appid\":\"nested\""));
        assert_eq!(info(&updated).unwrap().appid, "new");
        assert_eq!(replace(&updated, "appid", &"old".into()).unwrap(), text);
    }
    #[test]
    fn recovery_preserves_version_and_conflicts() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("manifest.json");
        let r = Recovery {
            path: p.to_string_lossy().into(),
            original: "old".into(),
            temporary: "temp".into(),
            line_ending: "lf".into(),
        };
        std::fs::write(
            &p,
            r#"{"appid":"temp","versionName":"2.0.0","versionCode":"20"}"#,
        )
        .unwrap();
        restore(&r).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert_eq!(info(&text).unwrap().version_name, "2.0.0");
        assert_eq!(info(&text).unwrap().appid, "old");
        std::fs::write(&p, replace(&text, "appid", &"user".into()).unwrap()).unwrap();
        assert!(restore(&r).is_err());
        assert_eq!(
            info(&std::fs::read_to_string(&p).unwrap()).unwrap().appid,
            "user"
        );
    }
    #[test]
    fn versions_preserve_jsonc_and_value_types() {
        let text = r#"{
          // "appid": "disabled"
          "appid": "active", "versionName": "1.0.0", "versionCode": "10",
          "url": "https://example.test/中文", "nested": {"versionCode": 99},
        }"#;
        let updated = replace(text, "versionName", &"2.3.4".into()).unwrap();
        let updated = replace(&updated, "versionCode", &"234".into()).unwrap();
        assert_eq!(field(&updated, "versionCode").unwrap(), "234");
        assert!(updated.contains("// \"appid\": \"disabled\""));
        assert!(updated.contains("https://example.test/中文"));
        assert!(updated.contains("\"nested\": {\"versionCode\": 99}"));
        let numeric = replace(&updated, "versionCode", &serde_json::json!(234)).unwrap();
        assert_eq!(field(&numeric, "versionCode").unwrap(), 234);
        assert!(info("{ /* unclosed").is_err());
    }
    #[test]
    fn restore_keeps_original_line_endings() {
        let d = tempfile::tempdir().unwrap();
        for (original_text, temporary_text, expected) in [
            (
                "{\n \"appid\": \"old\",\n \"versionName\": \"1.0.0\",\n \"versionCode\": \"10\"\n}\n",
                "{\r\n \"appid\": \"temp\",\r\n \"versionName\": \"2.0.0\",\r\n \"versionCode\": \"20\"\r\n}\r\n",
                "lf",
            ),
            (
                "{\r\n \"appid\": \"old\",\r\n \"versionName\": \"1.0.0\",\r\n \"versionCode\": \"10\"\r\n}\r\n",
                "{\n \"appid\": \"temp\",\n \"versionName\": \"2.0.0\",\n \"versionCode\": \"20\"\n}\n",
                "crlf",
            ),
        ] {
            let p = d.path().join("manifest.json");
            std::fs::write(&p, temporary_text).unwrap();
            let record = Recovery {
                path: p.to_string_lossy().into(),
                original: "old".into(),
                temporary: "temp".into(),
                line_ending: line_ending(original_text).into(),
            };
            restore(&record).unwrap();
            let text = std::fs::read_to_string(&p).unwrap();
            assert_eq!(line_ending(&text), expected);
            assert_eq!(info(&text).unwrap().appid, "old");
            // 版本号属于本次构建结果，恢复换行时不能回退。
            assert_eq!(info(&text).unwrap().version_name, "2.0.0");
            assert!(!text.contains("\r\n\r\n"));
        }
        // 旧记录没有换行风格时保持文件当前格式。
        let p = d.path().join("legacy.json");
        std::fs::write(&p, "{\r\n \"appid\": \"temp\"\r\n}\r\n").unwrap();
        let legacy: Recovery = serde_json::from_str(&format!(
            r#"{{"path":"{}","original":"old","temporary":"temp"}}"#,
            p.display()
        ))
        .unwrap();
        assert!(legacy.line_ending.is_empty());
        restore(&legacy).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("\r\n"));
    }
    #[test]
    fn validation() {
        assert!(validate_version("1.2.3", "123").is_ok());
        assert!(validate_version("x;evil", "1").is_err());
        assert!(validate_version("1.2.3", "0").is_err());
        assert!(field(r#"{"appid":"a","appid":"b"}"#, "appid").is_err());
    }
}
