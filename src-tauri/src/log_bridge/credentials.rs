use super::{Record, Result, OFFLINE};

pub(crate) trait Store {
    fn load(&self) -> Result<Record>;
    fn save(&self, record: &Record) -> Result<()>;
    fn clear(&self) -> Result<()>;
}
pub(crate) struct SystemStore;
const UNAVAILABLE: &str = "日志 MCP 专用系统凭据存储不可用";

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SystemStore {
    fn entry(&self) -> Result<keyring::Entry> {
        let service = if cfg!(debug_assertions) {
            "cn.aixiaodou.devdock.dev.log-mcp"
        } else {
            "cn.aixiaodou.devdock.log-mcp"
        };
        keyring::Entry::new(service, "stdio").map_err(|_| UNAVAILABLE.into())
    }
}
#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Store for SystemStore {
    fn load(&self) -> Result<Record> {
        let value = self.entry()?.get_password().map_err(|_| OFFLINE)?;
        serde_json::from_str(&value).map_err(|_| OFFLINE.into())
    }
    fn save(&self, record: &Record) -> Result<()> {
        self.entry()?
            .set_password(&serde_json::to_string(record).map_err(|_| UNAVAILABLE)?)
            .map_err(|_| UNAVAILABLE.into())
    }
    fn clear(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法删除系统中的日志 MCP 专用凭据，请重试停用".into()),
        }
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Store for SystemStore {
    fn load(&self) -> Result<Record> {
        Err(OFFLINE.into())
    }
    fn save(&self, _: &Record) -> Result<()> {
        Err(UNAVAILABLE.into())
    }
    fn clear(&self) -> Result<()> {
        Ok(())
    }
}
