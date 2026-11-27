//! MCP 只接受保存过的项目/仓库标识。实际 Git 预检、仓库锁和执行时重验交给 backend_git。
use crate::{
    backend_git::{self, Repository},
    config::{Project, Result},
    scm::{self, project_create::SessionBinding},
    scm_approval, tasks,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

const TTL: Duration = Duration::from_secs(300);
const HISTORY_TTL: Duration = Duration::from_secs(86400);
const INVALID: &str = "MCP Git 计划无效、已过期或不属于该项目，请重新准备";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrepareArgs {
    project_name: String,
    operation: String,
    repository_ids: Option<Vec<String>>,
    scm_project_id: Option<String>,
    #[serde(default)]
    push: bool,
    #[serde(default)]
    keep_test_pom: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Lookup {
    project_name: String,
    plan_id: String,
}

#[derive(Default)]
struct Cache(Mutex<HashMap<String, Arc<Entry>>>);
struct Entry {
    project_id: String,
    fingerprint: [u8; 32],
    details: Value,
    session: SessionBinding,
    issued: Instant,
    consumed: AtomicBool,
}
fn cache(app: &AppHandle) -> tauri::State<'_, Cache> {
    if app.try_state::<Cache>().is_none() {
        app.manage(Cache::default());
    }
    app.state::<Cache>()
}
fn valid_key(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= 256
}
fn resolve(projects: Vec<Project>, name: &str) -> Result<Project> {
    if !valid_key(name) {
        return Err("需要完整 DevDock 项目名称".into());
    }
    let mut matches = projects.into_iter().filter(|p| p.name == name);
    let p = matches.next().ok_or("未找到完整名称匹配的 DevDock 项目")?;
    if matches.next().is_some() {
        return Err("DevDock 项目重名，无法唯一定位".into());
    }
    if p.project_type != "backend" {
        return Err("仅支持 DevDock 后端项目".into());
    }
    Ok(p)
}
fn project(app: &AppHandle, name: &str) -> Result<Project> {
    resolve(tasks::load(app)?.projects, name)
}
fn fingerprint(p: &Project) -> Result<[u8; 32]> {
    Ok(Sha256::digest(serde_json::to_vec(p).map_err(|_| "无法读取项目配置快照")?).into())
}
fn verify_config(app: &AppHandle, name: &str, id: &str, saved: &[u8; 32]) -> Result<()> {
    let p = project(app, name)?;
    if p.id != id || fingerprint(&p)? != *saved {
        return Err("项目配置已更改，请重新准备 Git 计划".into());
    }
    Ok(())
}
fn selected(p: &Project, ids: Option<&[String]>) -> Result<Vec<Repository>> {
    let Some(ids) = ids else {
        return Ok(p.sub_projects.clone());
    };
    let mut seen = HashSet::new();
    if ids.is_empty() || ids.len() > 128 {
        return Err("仓库选择数量无效".into());
    }
    ids.iter()
        .map(|id| {
            if !valid_key(id) || !seen.insert(id) {
                return Err("仓库 ID 为空或重复".into());
            }
            p.sub_projects
                .iter()
                .find(|r| &r.id == id)
                .cloned()
                .ok_or_else(|| "仓库不属于指定的 DevDock 项目".into())
        })
        .collect()
}
// SCM 模块只认 gitId；未显式选择时取交集，显式选择时不允许静默漏掉仓库。
fn map_modules(
    repos: Vec<Repository>,
    modules: &[scm::ScmModule],
    explicit: bool,
) -> Result<Vec<Repository>> {
    let mut mapped = Vec::new();
    let mut seen = HashSet::new();
    for mut r in repos {
        let id = r.scm_git_id.trim();
        let mut matches = modules
            .iter()
            .filter(|m| !id.is_empty() && m.git_id.as_deref().map(str::trim) == Some(id));
        let Some(m) = matches.next() else {
            if explicit {
                return Err(format!("仓库「{}」没有匹配的 SCM Git ID", r.name));
            }
            continue;
        };
        if matches.next().is_some() || !seen.insert(id.to_owned()) || m.branch.trim().is_empty() {
            return Err(format!(
                "仓库「{}」的 SCM Git ID 或模块分支不唯一或为空",
                r.name
            ));
        }
        r.development_branch = m.branch.trim().to_owned();
        r.scm_module_name = m.module_name.clone();
        mapped.push(r);
    }
    if mapped.is_empty() {
        return Err("未找到 SCM 模块与已配置仓库的 Git ID 匹配项".into());
    }
    Ok(mapped)
}
fn operation(args: &PrepareArgs) -> Result<&str> {
    let op = args.operation.as_str();
    if (op != "switch-dev" && args.scm_project_id.is_some())
        || (args.push && !matches!(op, "push-dev" | "merge"))
        || (op == "push-dev" && !args.push)
        || (args.keep_test_pom && op != "merge")
    {
        return Err("Git 操作选项不适用于此计划".into());
    }
    match op {
        "switch-dev" => Ok("switch"),
        "switch-test" | "switch-master" | "merge" | "push-dev" | "pull-current" => Ok(op),
        _ => Err("未知 Git 操作".into()),
    }
}
async fn verify_session(app: &AppHandle, session: &SessionBinding) -> Result<()> {
    app.state::<scm::Runtime>().ensure_binding(session).await
}
async fn approve(
    app: &AppHandle,
    tool: &str,
    session: &SessionBinding,
    details: &Value,
) -> Result<()> {
    scm_approval::approve(app, tool, &session.site, &session.account, details).await?;
    verify_session(app, session).await
}
pub(super) async fn prepare(app: &AppHandle, args: &Value) -> Result<Value> {
    let a: PrepareArgs = serde_json::from_value(args.clone()).map_err(|_| "Git 预检参数无效")?;
    let op = operation(&a)?.to_owned();
    let p = project(app, &a.project_name)?;
    let saved = fingerprint(&p)?;
    let mut repos = selected(&p, a.repository_ids.as_deref())?;
    let session = app.state::<scm::Runtime>().session_binding().await?;
    let mut scm_id = None;
    if op == "switch" {
        let id = a
            .scm_project_id
            .as_deref()
            .unwrap_or(&p.scm_development_project_id);
        if id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("switch-dev 需要纯数字 SCM 项目 ID".into());
        }
        let modules = scm::project_modules(app, id.to_owned()).await?;
        verify_session(app, &session).await?;
        repos = map_modules(repos, &modules, a.repository_ids.is_some())?;
        scm_id = Some(id.to_owned());
    }
    if repos.is_empty() {
        return Err("没有选择已配置的仓库".into());
    }
    let preview: Vec<_> = repos
        .iter()
        .map(|r| {
            let target = match op.as_str() {
                "switch" | "push-dev" => Some(r.development_branch.as_str()),
                "switch-test" | "merge" => Some(r.test_branch.as_str()),
                "switch-master" => Some("master"),
                _ => None, // pull-current 的真实上游必须由 Git 预检解析。
            };
            json!({"repository":r,"targetBranch":target})
        })
        .collect();
    let intent = json!({"projectId":p.id,"projectName":p.name,"operation":a.operation,"push":a.push,"keepTestPom":a.keep_test_pom,"scmProjectId":scm_id,"repositories":preview});
    approve(app, "backend_git_prepare", &session, &intent).await?;
    let handle = app.clone();
    let name = p.name.clone();
    let id = p.id.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        verify_config(&handle, &name, &id, &saved)?;
        backend_git::prepare(&handle, id, repos, op, a.push, a.keep_test_pom)
    })
    .await
    .map_err(|_| "Git 预检任务异常")??;
    verify_session(app, &session).await?;
    verify_config(app, &p.name, &p.id, &saved)?;
    let issued = Instant::now();
    let expires = (SystemTime::now() + TTL)
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间异常")?
        .as_millis() as u64;
    let mut details = serde_json::to_value(&plan).map_err(|_| "无法读取 Git 计划")?;
    details["projectName"] = json!(p.name);
    details["operation"] = json!(a.operation);
    details["scmProjectId"] = json!(scm_id);
    details["expiresAtMs"] = json!(expires);
    if matches!(a.operation.as_str(), "switch-test" | "switch-master") {
        details["syncPolicy"] = json!("预检 SHA 为当前快照；切换后若已配置远端，按执行时最新远端分支仅快进同步，分叉则拒绝合并。");
    }
    let cache = cache(app);
    let mut entries = cache.0.lock().map_err(|_| INVALID)?;
    entries.retain(|_, e| {
        e.issued.elapsed()
            < if e.consumed.load(Ordering::Acquire) {
                HISTORY_TTL
            } else {
                TTL
            }
    });
    if entries.len() >= 256 {
        return Err("MCP Git 计划缓存已满".into());
    }
    entries.insert(
        plan.plan_id,
        Arc::new(Entry {
            project_id: p.id,
            fingerprint: saved,
            details: details.clone(),
            session,
            issued,
            consumed: AtomicBool::new(false),
        }),
    );
    Ok(details)
}
fn lookup(app: &AppHandle, args: &Value, prepared: bool) -> Result<(Lookup, Arc<Entry>)> {
    let q: Lookup = serde_json::from_value(args.clone()).map_err(|_| "Git 计划查询参数无效")?;
    if !valid_key(&q.plan_id) {
        return Err(INVALID.into());
    }
    let entry = cache(app)
        .0
        .lock()
        .map_err(|_| INVALID)?
        .get(&q.plan_id)
        .cloned()
        .ok_or(INVALID)?;
    let used = entry.consumed.load(Ordering::Acquire);
    if (prepared && used) || entry.issued.elapsed() >= if used { HISTORY_TTL } else { TTL } {
        return Err(INVALID.into());
    }
    verify_config(app, &q.project_name, &entry.project_id, &entry.fingerprint)?;
    Ok((q, entry))
}
fn claim(
    project_id: &str,
    issued: Instant,
    consumed: &AtomicBool,
    plan_id: &str,
    approved_plan_id: &str,
    approved_project_id: &str,
) -> Result<()> {
    // 审批返回后再检查有效期；原子消费，避免两次审批并发启动一个任务。
    if plan_id != approved_plan_id
        || project_id != approved_project_id
        || issued.elapsed() >= TTL
        || consumed.swap(true, Ordering::AcqRel)
    {
        return Err(INVALID.into());
    }
    Ok(())
}
pub(super) async fn execute(
    app: &AppHandle,
    approved_plan_id: &str,
    approved_project_id: &str,
    args: &Value,
) -> Result<Value> {
    let (q, entry) = lookup(app, args, true)?;
    verify_session(app, &entry.session).await?;
    if let Some(id) = entry.details["scmProjectId"].as_str() {
        // 只重验已审批仓库，不能把后来加入的模块带入执行范围。
        let modules = scm::project_modules(app, id.to_owned()).await?;
        verify_session(app, &entry.session).await?;
        let repos: Vec<Repository> = entry.details["repositories"]
            .as_array()
            .ok_or(INVALID)?
            .iter()
            .map(|r| {
                serde_json::from_value(r["repository"].clone()).map_err(|_| INVALID.to_string())
            })
            .collect::<Result<_>>()?;
        let current = map_modules(repos.clone(), &modules, true)?;
        if repos.iter().zip(&current).any(|(a, b)| {
            a.development_branch != b.development_branch || a.scm_module_name != b.scm_module_name
        }) {
            return Err("SCM 模块或开发分支已改变，请重新准备 Git 计划".into());
        }
    }
    verify_config(app, &q.project_name, &entry.project_id, &entry.fingerprint)?;
    claim(
        &entry.project_id,
        entry.issued,
        &entry.consumed,
        &q.plan_id,
        approved_plan_id,
        approved_project_id,
    )?;
    let task_id = backend_git::execute(app.clone(), q.plan_id.clone())?;
    Ok(json!({"projectId":entry.project_id,"planId":q.plan_id,"taskId":task_id}))
}
pub(super) async fn state(app: &AppHandle, args: &Value) -> Result<Value> {
    let (q, entry) = lookup(app, args, false)?;
    verify_session(app, &entry.session).await?;
    let s = backend_git::state(app, q.plan_id)?;
    // 冲突源码和 POM 内容由用户在工作台查看，不通过 MCP 传出。
    let repositories:Vec<_>=s.repositories.iter().map(|r|json!({"repoId":r.repo_id,"status":r.status,"message":r.message,"worktreePath":r.worktree_path})).collect();
    Ok(
        json!({"projectId":entry.project_id,"planId":s.plan_id,"taskId":s.task_id,"status":s.status,"repositories":repositories}),
    )
}
pub(super) async fn dispatch(app: &AppHandle, tool: &str, args: &Value) -> Result<Value> {
    match tool {
        "backend_git_prepare" => prepare(app, args).await,
        "backend_git_state" => state(app, args).await,
        "backend_git_execute" => {
            let (q, entry) = lookup(app, args, true)?;
            verify_session(app, &entry.session).await?;
            approve(app, tool, &entry.session, &entry.details).await?;
            execute(app, &q.plan_id, &entry.project_id, args).await
        }
        _ => Err("未知 Git 工具".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn repo(id: &str, git: &str) -> Repository {
        serde_json::from_value(json!({"id":id,"name":id,"path":format!("/repo/{id}"),"scmGitId":git,"developmentBranch":"stale"})).unwrap()
    }
    fn module(id: &str, branch: &str) -> scm::ScmModule {
        scm::ScmModule {
            git_id: Some(id.into()),
            module_name: "module".into(),
            branch: branch.into(),
            git_remote_url: None,
        }
    }
    fn group() -> Project {
        serde_json::from_value(json!({"id":"g","name":"Group","type":"backend","path":"/g","platforms":[],"manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"","androidModule":"","jdkPath":"","gradleTask":"","outputDir":"","syncMainVersion":false,"terminal":"","commands":[],"subProjects":[repo("a","12"),repo("b","")]})).unwrap()
    }
    #[test]
    fn exact_group_and_saved_repositories_only() {
        let p = group();
        assert!(resolve(vec![p.clone()], "Gro").is_err());
        assert!(resolve(vec![p.clone(), p.clone()], "Group").is_err());
        let mut node = p.clone();
        node.project_type = "nodejs".into();
        assert!(resolve(vec![node], "Group").is_err());
        assert!(selected(&p, Some(&["foreign".into()])).is_err());
        assert!(selected(&p, Some(&["a".into(), "a".into()])).is_err());
        let mut changed = p.clone();
        changed.sub_projects[0].path = "/other".into();
        assert_ne!(fingerprint(&p).unwrap(), fingerprint(&changed).unwrap());
    }
    #[test]
    fn scm_switch_uses_exact_unique_intersection_no_guess() {
        let p = group();
        let modules = vec![module("12", "actual"), module("13", "other")];
        let mapped = map_modules(p.sub_projects.clone(), &modules, false).unwrap();
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].development_branch, "actual");
        assert!(map_modules(p.sub_projects.clone(), &modules, true).is_err());
        for bad in [
            vec![module("12", "")],
            vec![module("12", "a"), module("12", "b")],
            vec![module("13", "a")],
        ] {
            assert!(map_modules(p.sub_projects.clone(), &bad, false).is_err());
        }
    }
    #[test]
    fn denies_client_git_targets_and_respects_backend_options() {
        for field in ["path", "branch", "repositories", "projectId"] {
            let mut v = json!({"projectName":"Group","operation":"switch-dev"});
            v[field] = json!("bad");
            assert!(serde_json::from_value::<PrepareArgs>(v).is_err());
        }
        for v in [
            json!({"operation":"switch"}),
            json!({"operation":"push-dev"}),
            json!({"operation":"switch-master","push":true}),
            json!({"operation":"pull-current","scmProjectId":"123"}),
        ] {
            let mut v = v;
            v["projectName"] = json!("Group");
            assert!(operation(&serde_json::from_value(v).unwrap()).is_err());
        }
    }
    #[test]
    fn approval_is_bound_single_use_and_expires() {
        let now = Instant::now();
        let used = AtomicBool::new(false);
        assert!(claim("g", now, &used, "p", "else", "g").is_err());
        assert!(claim("g", now, &used, "p", "p", "else").is_err());
        assert!(claim("g", now - TTL, &used, "p", "p", "g").is_err());
        assert!(claim("g", now, &used, "p", "p", "g").is_ok());
        assert!(claim("g", now, &used, "p", "p", "g").is_err());
    }
}
