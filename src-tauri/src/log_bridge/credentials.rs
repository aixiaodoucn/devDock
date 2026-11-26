use super::{Record, Result, OFFLINE};
use crate::local_credentials;

pub(crate) trait Store {
    fn load(&self) -> Result<Record>;
    fn save(&self, record: &Record) -> Result<()>;
    fn clear(&self) -> Result<()>;
}
pub(crate) struct LocalStore;
const KEY: &str = "mcp-bridge";

impl Store for LocalStore {
    fn load(&self) -> Result<Record> {
        let value = local_credentials::load(KEY)
            .map_err(|_| OFFLINE)?
            .ok_or(OFFLINE)?;
        serde_json::from_str(&value).map_err(|_| OFFLINE.into())
    }
    fn save(&self, record: &Record) -> Result<()> {
        let value = serde_json::to_string(record).map_err(|_| "无法编码 MCP 桥接凭据")?;
        local_credentials::save(KEY, &value)
    }
    fn clear(&self) -> Result<()> {
        local_credentials::clear(KEY).map_err(|_| "无法删除本机 MCP 桥接凭据，请重试停用".into())
    }
}
