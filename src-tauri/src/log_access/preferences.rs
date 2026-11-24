use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;

const FILE_NAME: &str = "log-access-preferences.json";
const MAX_BYTES: u64 = 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preferences {
    pub(super) scan_all: bool,
    pub(super) raw_subject: Option<String>,
}

pub(super) fn subject(base_url: &str, username: &str) -> String {
    let base = base_url.trim();
    let site = Url::parse(base).map_or_else(|_| base.to_owned(), |url| url.to_string());
    let mut hash = Sha256::new();
    // 长度前缀区分站点和账号边界；账号大小写沿用 SCM 登录语义。
    hash.update((site.len() as u64).to_be_bytes());
    hash.update(site.as_bytes());
    hash.update(username.trim().as_bytes());
    format!("{:x}", hash.finalize())
}

impl Preferences {
    fn validate(&self) -> Result<(), String> {
        if let Some(value) = &self.raw_subject {
            if value.len() != 64
                || !value
                    .bytes()
                    .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            {
                return Err("日志偏好中的身份指纹无效".into());
            }
        }
        Ok(())
    }

    pub(super) fn load(dir: &Path) -> Result<Self, String> {
        let file = match File::open(dir.join(FILE_NAME)) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(format!("读取日志偏好失败：{err}")),
        };
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|err| format!("读取日志偏好失败：{err}"))?;
        if data.len() as u64 > MAX_BYTES {
            return Err("日志偏好文件过大".into());
        }
        let prefs: Self = serde_json::from_slice(&data).map_err(|_| "日志偏好格式无效")?;
        prefs.validate()?;
        Ok(prefs)
    }

    pub(super) fn save(&self, dir: &Path) -> Result<(), String> {
        self.validate()?;
        let data = serde_json::to_vec(self).map_err(|_| "日志偏好序列化失败")?;
        fs::create_dir_all(dir).map_err(|err| format!("创建日志偏好目录失败：{err}"))?;
        // 同目录临时文件写完并同步后再原子替换，读者只能看见完整版本。
        let mut temp =
            NamedTempFile::new_in(dir).map_err(|err| format!("创建日志偏好文件失败：{err}"))?;
        temp.write_all(&data)
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|err| format!("写入日志偏好失败：{err}"))?;
        temp.persist(dir.join(FILE_NAME))
            .map_err(|err| format!("替换日志偏好失败：{}", err.error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_tracks_normalized_site_and_case_sensitive_account() {
        let value = subject(" HTTPS://EXAMPLE.INVALID:443/ ", " Alice ");
        assert_eq!(value, subject("https://example.invalid", "Alice"));
        assert_ne!(value, subject("https://example.invalid", "alice"));
        assert_ne!(value, subject("https://example.invalid:8443", "Alice"));
        assert_ne!(value, subject("https://other.invalid", "Alice"));
    }

    #[test]
    fn persists_only_settings_and_replaces_them_on_revocation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("app-data");
        let defaults = Preferences::load(&dir).unwrap();
        assert!(!defaults.scan_all && defaults.raw_subject.is_none());
        let digest = subject("https://example.invalid", "test-user");
        let prefs = Preferences {
            scan_all: true,
            raw_subject: Some(digest.clone()),
        };
        prefs.save(&dir).unwrap();
        let disk: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(
            disk,
            serde_json::json!({"scan_all": true, "raw_subject": digest})
        );
        assert_eq!(Preferences::load(&dir).unwrap(), prefs);
        let invalid = Preferences {
            scan_all: false,
            raw_subject: Some("plaintext".into()),
        };
        assert!(invalid.save(&dir).is_err());
        assert_eq!(Preferences::load(&dir).unwrap(), prefs);
        defaults.save(&dir).unwrap();
        assert_eq!(Preferences::load(&dir).unwrap(), defaults);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn rejects_malformed_and_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        for document in [
            "{",
            "{}",
            r#"{"scan_all":1}"#,
            r#"{"scan_all":true,"unknown":null}"#,
            r#"{"scan_all":true,"raw_subject":"plaintext"}"#,
        ] {
            fs::write(&path, document).unwrap();
            assert!(Preferences::load(dir.path()).is_err());
        }
        let mut data = serde_json::to_vec(&Preferences::default()).unwrap();
        data.resize(MAX_BYTES as usize, b' ');
        fs::write(&path, &data).unwrap();
        assert_eq!(
            Preferences::load(dir.path()).unwrap(),
            Preferences::default()
        );
        data.push(b' ');
        fs::write(path, data).unwrap();
        assert!(Preferences::load(dir.path()).is_err());
    }
}
