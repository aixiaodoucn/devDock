//! 索引只含列表摘要和保留设置；每条记录的 SQL、结果与重跑参数独立落盘。
use super::*;
use std::collections::HashSet;

const INDEX: &str = "index.json";
const PREFIX: &str = "execution-";
const MAX_LIMIT: usize = 10_000;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Summary {
    pub(super) id: String,
    started_at: u64,
    tool: String,
    connection: String,
    can_rerun: bool,
    has_result: bool,
}
impl From<&Execution> for Summary {
    fn from(r: &Execution) -> Self {
        Self {
            id: r.id.clone(),
            started_at: r.started_at,
            tool: r.tool.clone(),
            connection: r.connection.clone(),
            can_rerun: r.replay.is_some(),
            has_result: r.result.is_some(),
        }
    }
}
#[derive(Deserialize, Serialize)]
struct Index {
    limit: usize,
    records: Vec<Summary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pending_removals: Vec<String>,
}

fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
pub(super) fn detail_path(dir: &Path, id: &str) -> Result<std::path::PathBuf> {
    if !valid_id(id) {
        return Err("无效的执行记录 ID".into());
    }
    Ok(dir.join(format!("{PREFIX}{id}.json")))
}
fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    crate::config::atomic_write(
        path,
        &serde_json::to_string(value).map_err(|_| "无法编码 SQLPad 执行记录")?,
    )
}
fn unlink(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法清理 SQLPad 历史文件".into()),
    }
}
impl Index {
    fn load(dir: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .recursive(true)
            .create(dir)
            .map_err(|_| "无法创建执行记录目录")?;
        let index: Self = match fs::read(dir.join(INDEX)) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|_| "SQLPad 执行记录索引格式无效")?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let legacy: Vec<Execution> = match fs::read(dir.join(FILE)) {
                    Ok(bytes) => {
                        serde_json::from_slice(&bytes).map_err(|_| "SQLPad 执行记录格式无效")?
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(_) => return Err("无法读取旧版执行记录".into()),
                };
                let index = Self {
                    limit: LIMIT,
                    records: legacy.iter().map(Summary::from).collect(),
                    pending_removals: Vec::new(),
                };
                index.validate()?;
                // 先完成所有详情，再发布索引；中断后仍能从旧文件重新迁移。
                for r in legacy {
                    write(&detail_path(dir, &r.id)?, &r)?;
                }
                write(&dir.join(INDEX), &index)?;
                index
            }
            Err(_) => return Err("无法读取 SQLPad 执行索引".into()),
        };
        index.validate()?;
        unlink(&dir.join(FILE))?;
        Ok(index)
    }
    fn validate(&self) -> Result<()> {
        let mut ids = HashSet::new();
        if !(1..=MAX_LIMIT).contains(&self.limit)
            || self
                .records
                .iter()
                .any(|r| !valid_id(&r.id) || !ids.insert(&r.id))
            || self
                .pending_removals
                .iter()
                .any(|id| !valid_id(id) || ids.contains(id))
        {
            return Err("SQLPad 执行记录索引无效".into());
        }
        Ok(())
    }
    // 列表保留重跑插入位置；淘汰只按开始时间，同时间优先保留靠前记录。
    fn trim(&mut self, protected: &[&str]) -> Vec<String> {
        let excess = self.records.len().saturating_sub(self.limit);
        if excess == 0 {
            return Vec::new();
        }
        let mut ranked: Vec<_> = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, r)| !protected.contains(&r.id.as_str()))
            .collect();
        ranked.sort_unstable_by_key(|(i, r)| (r.started_at, std::cmp::Reverse(*i)));
        let remove: HashSet<_> = ranked
            .into_iter()
            .take(excess)
            .map(|(_, r)| r.id.clone())
            .collect();
        self.records.retain(|r| !remove.contains(&r.id));
        remove.into_iter().collect()
    }
}

#[derive(Default)]
pub(crate) struct History(SyncMutex<Option<Index>>);
impl History {
    fn with<T>(&self, dir: &Path, f: impl FnOnce(&mut Index) -> Result<T>) -> Result<T> {
        let mut slot = self.0.lock().map_err(|_| "SQLPad 执行记录不可用")?;
        if slot.is_none() {
            *slot = Some(Index::load(dir)?);
        }
        let result = f(slot.as_mut().unwrap());
        // 写盘失败时不继续使用部分修改的缓存，下一次从已提交索引恢复。
        if result.is_err() {
            *slot = None;
        }
        result
    }
    pub(super) fn list(&self, dir: &Path) -> Result<Vec<Summary>> {
        self.with(dir, |i| Ok(i.records.clone()))
    }
    pub(super) fn detail(&self, dir: &Path, id: &str) -> Result<Execution> {
        self.with(dir, |i| {
            if !i.records.iter().any(|r| r.id == id) {
                return Err("找不到原执行记录，请刷新".into());
            }
            read_detail(dir, id)
        })
    }
    pub(super) fn limit(&self, dir: &Path) -> Result<usize> {
        self.with(dir, |i| Ok(i.limit))
    }
    pub(super) fn set_limit(&self, dir: &Path, limit: usize) -> Result<usize> {
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err("保留条数必须是 1–10000 的整数".into());
        }
        self.with(dir, |i| {
            i.limit = limit;
            let removed = i.trim(&[]);
            commit(dir, i, removed)?;
            Ok(limit)
        })
    }
    pub(super) fn remove(&self, dir: &Path, id: Option<&str>) -> Result<()> {
        self.with(dir, |i| {
            let mut removed = Vec::new();
            i.records.retain(|r| {
                if id.is_none_or(|id| r.id == id) {
                    removed.push(r.id.clone());
                    false
                } else {
                    true
                }
            });
            commit(dir, i, removed)
        })
    }
    pub(super) fn start(
        &self,
        dir: &Path,
        tool: &str,
        connection: &str,
        sql: &str,
        replay: Option<&Replay>,
        parent: Option<&str>,
    ) -> Result<String> {
        self.with(dir, |i| {
            let pos = match parent {
                Some(id) => {
                    i.records
                        .iter()
                        .position(|r| r.id == id)
                        .ok_or("原记录已移除")?
                        + 1
                }
                None => 0,
            };
            let id = uuid::Uuid::new_v4().to_string();
            let record = Execution {
                id: id.clone(),
                started_at: crate::build_history::now_ms(),
                tool: tool.into(),
                connection: connection.into(),
                sql: sql.into(),
                result: None,
                error: Some(UNKNOWN.into()),
                replay: replay.cloned(),
            };
            write(&detail_path(dir, &id)?, &record)?;
            i.records.insert(pos, Summary::from(&record));
            let mut protected = vec![id.as_str()];
            if let Some(parent) = parent.filter(|_| i.limit > 1) {
                protected.push(parent);
            }
            let removed = i.trim(&protected);
            commit(dir, i, removed)?;
            Ok(id)
        })
    }
    pub(super) fn finish(&self, dir: &Path, id: &str, outcome: &Result<Value>) -> Result<()> {
        self.with(dir, |i| {
            // 已删除、清空或淘汰的在途记录完成后不能重新出现。
            let Some(row) = i.records.iter_mut().find(|r| r.id == id) else {
                return Ok(());
            };
            let mut record = read_detail(dir, id)?;
            record.result = outcome.as_ref().ok().cloned();
            record.error = outcome
                .as_ref()
                .err()
                .map(|e| crate::log_mcp::sqlpad_error(e).to_owned());
            write(&detail_path(dir, id)?, &record)?;
            row.has_result = record.result.is_some();
            write(&dir.join(INDEX), i)
        })
    }
}
fn read_detail(dir: &Path, id: &str) -> Result<Execution> {
    let bytes = fs::read(detail_path(dir, id)?).map_err(|_| "无法读取 SQLPad 记录详情")?;
    let record: Execution =
        serde_json::from_slice(&bytes).map_err(|_| "SQLPad 记录详情格式无效")?;
    if record.id != id {
        return Err("SQLPad 记录详情不匹配".into());
    }
    Ok(record)
}
fn commit(dir: &Path, index: &mut Index, removed: Vec<String>) -> Result<()> {
    // 保留明确删除或按额度淘汰的 ID，清理中断时下次写入可重试，不按文件名扫描删除。
    index.pending_removals.extend(removed);
    write(&dir.join(INDEX), index)?;
    for id in &index.pending_removals {
        unlink(&detail_path(dir, id)?)?;
    }
    index.pending_removals.clear();
    Ok(())
}
