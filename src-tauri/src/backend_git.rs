use crate::{config::Result, tasks};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default = "default_remote")]
    pub remote: String,
    #[serde(default)]
    pub scm_git_id: String,
    #[serde(default)]
    pub scm_module_name: String,
    #[serde(default)]
    pub development_branch: String,
    #[serde(default)]
    pub test_branch: String,
}
fn default_remote() -> String {
    "origin".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_git: bool,
    pub current_branch: String,
    pub error: String,
}
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryStatus {
    pub id: String,
    pub current_branch: String,
    pub head: String,
    pub dirty: bool,
    pub merging: bool,
    pub rebasing: bool,
    pub remote_url: String,
    pub branches: Vec<String>,
    pub root: String,
    pub error: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRepository {
    pub repository: Repository,
    pub root: String,
    pub common_dir: String,
    pub current_branch: String,
    pub source_sha: String,
    #[serde(default)]
    pub development_remote_sha: String,
    #[serde(default)]
    pub sync_development_branch: bool,
    pub target_branch: String,
    pub target_sha: String,
    pub create_tracking: bool,
    pub remote_sha: String,
    #[serde(default)]
    pub sync_test_branch: bool,
    // 仅比较配置身份；不向前端或日志传递可能包含凭据的真实 URL。
    #[serde(skip)]
    remote_identity: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub plan_id: String,
    pub project_id: String,
    pub operation: String,
    pub push: bool,
    #[serde(default)]
    pub keep_test_pom: bool,
    pub repositories: Vec<PlanRepository>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomConflict {
    path: String,
    test_content: String,
    development_content: String,
    test_branch: String,
    development_branch: String,
    #[serde(default)]
    push: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomConflictBlock {
    line: usize,
    test_content: String,
    development_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomConflictFile {
    path: String,
    blocks: Vec<PomConflictBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch_diff: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    diff_contexts: BTreeMap<String, String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PomConflictSnapshot {
    path: String,
    worktree_blob: String,
    index_identity: u64,
    remote_identity: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PomBlocksSnapshot {
    index_identity: u64,
    worktree_files: BTreeMap<String, u64>,
    remote_identity: u64,
    manual_remaining: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PomBlocksResolution {
    push: bool,
    manual_remaining: bool,
}
#[derive(Clone, Deserialize)]
pub struct PomBlockChoice {
    path: String,
    line: usize,
    side: PomSide,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PomSide {
    Test,
    Development,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryResult {
    pub repo_id: String,
    pub status: String,
    pub message: String,
    pub worktree_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pom_conflict: Option<PomConflict>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pom_conflict_files: Vec<PomConflictFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pom_blocks_resolution: Option<PomBlocksResolution>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationState {
    pub plan_id: String,
    pub task_id: String,
    pub status: String,
    pub repositories: Vec<RepositoryResult>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Continuation {
    source_plan_id: String,
    started: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    plan: Plan,
    state: OperationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    continuation: Option<Continuation>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pom_conflicts: HashMap<String, PomConflictSnapshot>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pom_blocks_snapshots: HashMap<String, PomBlocksSnapshot>,
}
#[derive(Default)]
pub struct Runtime {
    entries: Mutex<HashMap<String, Entry>>,
}

// 同一 Git common dir 的关联工作树共享分支引用，复验直至写入完成必须互斥。
static BUSY_REPOSITORIES: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);
struct RepositoryGuard(String);
impl RepositoryGuard {
    fn acquire(common_dir: &str) -> Result<Self> {
        if !BUSY_REPOSITORIES.lock().unwrap().insert(common_dir.into()) {
            return Err("仓库有其他 Git 操作进行中，请稍后重试".into());
        }
        Ok(Self(common_dir.into()))
    }
}
impl Drop for RepositoryGuard {
    fn drop(&mut self) {
        BUSY_REPOSITORIES.lock().unwrap().remove(&self.0);
    }
}

fn repository_common_dir(path: &Path) -> Result<String> {
    canonical(&path.join(run(path, &["rev-parse", "--git-common-dir"], None)?))
}

fn git_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path);
    // 与现有 git.rs 一致，清理全部继承的 GIT_ 环境，防止仓库定位污染。
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    command
        .env("LC_ALL", "C")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_EDITOR", "true")
        // 保留仓库自己的 SSH、钩子及签名配置，仅禁止后台交互提示。
        .env("SSH_ASKPASS_REQUIRE", "never")
        .args(["-c", "credential.interactive=false"]);
    #[cfg(test)]
    command
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ALLOW_PROTOCOL", "file");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}
// 不传播 Git 原始 stderr；远端/凭据助手可能将 token 或密码写入任何错误行。
fn run(path: &Path, args: &[&str], task: Option<&tasks::Task>) -> Result<String> {
    run_checked(path, args, || task.map_or(Ok(()), tasks::Task::check))
}
fn run_checked(path: &Path, args: &[&str], check: impl Fn() -> Result<()>) -> Result<String> {
    run_output(path, args, check, false)
}
fn run_output(
    path: &Path,
    args: &[&str],
    check: impl Fn() -> Result<()>,
    preserve_newlines: bool,
) -> Result<String> {
    let bytes = run_bytes(path, args, check)?;
    let output = String::from_utf8(bytes).map_err(|_| "Git 输出不是有效 UTF-8".to_owned())?;
    Ok(if preserve_newlines {
        output
    } else {
        output.trim_end_matches(['\r', '\n']).to_owned()
    })
}
fn run_bytes(path: &Path, args: &[&str], check: impl Fn() -> Result<()>) -> Result<Vec<u8>> {
    check()?;
    let mut command = git_command(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let (job, mut child) = crate::windows_job::Job::spawn(&mut command)?;
    #[cfg(not(windows))]
    let mut child = command.spawn().map_err(|_| "无法启动 Git".to_string())?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(16 * 1024 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(1024 * 1024).read_to_end(&mut bytes);
    });
    let started = Instant::now();
    let outcome = loop {
        if let Err(error) = check() {
            break Err(error);
        }
        if started.elapsed() > Duration::from_secs(60) {
            break Err("Git 命令超时，现场已保留".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("Git 命令失败；请检查分支、认证或保留的工作区".into())
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => break Err("读取 Git 进程状态失败".into()),
        }
    };
    // 查询和写入都在有限时间内退出；任务取消后终止整个进程组，避免认证子进程阻塞。
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    job.terminate();
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(windows)]
    drop(job);
    let bytes = reader.join().unwrap_or_default();
    let _ = errors.join();
    outcome?;
    Ok(bytes)
}
fn redact(value: &str) -> String {
    let value = value.split(['?', '#']).next().unwrap_or_default();
    if let Some((scheme, rest)) = value.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        return format!(
            "{scheme}://{}{}{}",
            authority.rsplit('@').next().unwrap_or_default(),
            if path.is_empty() { "" } else { "/" },
            path
        );
    }
    value.into()
}
fn canonical(path: &Path) -> Result<String> {
    path.canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|_| "仓库路径不存在或无法访问".into())
}
fn root(path: &Path) -> Result<String> {
    canonical(Path::new(&run(
        path,
        &["rev-parse", "--show-toplevel"],
        None,
    )?))
}
fn branch(path: &Path) -> String {
    run(path, &["symbolic-ref", "--quiet", "--short", "HEAD"], None).unwrap_or_default()
}
fn sha(path: &Path, reference: &str) -> Result<String> {
    run(
        path,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
        None,
    )
}
fn valid_branch(path: &Path, name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('-') {
        return Err("必须明确配置有效分支".into());
    }
    run(
        path,
        &["check-ref-format", &format!("refs/heads/{name}")],
        None,
    )
    .map(|_| ())
}
fn remote_ref(repo: &Repository, target: &str) -> String {
    format!("refs/remotes/{}/{target}", repo.remote)
}
// 只更新 remote-tracking refs；显式 refspec 不受用户 fetch 配置影响，也不移动本地分支。
fn fetch_remote(path: &Path, repo: &Repository, task: Option<&tasks::Task>) -> Result<()> {
    fetch_remote_checked(path, repo, || task.map_or(Ok(()), tasks::Task::check))
}
fn fetch_remote_checked(
    path: &Path,
    repo: &Repository,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    check()?;
    validate_remote(repo)?;
    run_checked(
        path,
        &[
            "fetch",
            "--prune",
            "--no-tags",
            "--no-recurse-submodules",
            "--",
            &repo.remote,
            &format!("+refs/heads/*:refs/remotes/{}/*", repo.remote),
        ],
        check,
    )
    .map(|_| ())
}
fn validate_remote_name(remote: &str) -> Result<()> {
    // 远端名必须能作为 refs/remotes/<远端>/<分支> 中的单个路径分量。
    if remote.is_empty()
        || remote.starts_with(['-', '.'])
        || remote.ends_with(".lock")
        || remote.contains("..")
        || remote.contains("@{")
        || remote.contains(['/', '\\', ':', ' ', '~', '^', '?', '*', '['])
        || remote.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("远端名称无效".into());
    }
    Ok(())
}
fn validate_remote(repo: &Repository) -> Result<()> {
    let path = Path::new(&repo.path);
    validate_remote_name(&repo.remote)?;
    let remotes = run(path, &["remote"], None)?;
    if !remotes.lines().any(|r| r == repo.remote) {
        return Err("所选远端不存在".into());
    }
    Ok(())
}
pub fn scan(parent: String) -> Result<Vec<Candidate>> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for entry in fs::read_dir(&parent).map_err(|_| "无法读取父目录".to_string())? {
        let entry = entry.map_err(|_| "无法读取子目录".to_string())?;
        // 仅扫描一级可见目录，跳过 .git、IDE 配置等所有隐藏目录。
        if !entry.path().is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = match canonical(&entry.path()) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if !seen.insert(path.clone()) {
            continue;
        }
        let mut candidate = Candidate {
            id: uuid::Uuid::new_v4().to_string(),
            name: entry.file_name().to_string_lossy().into(),
            path: path.clone(),
            is_git: false,
            current_branch: String::new(),
            error: String::new(),
        };
        match root(Path::new(&path)) {
            Ok(actual) if actual == path => {
                candidate.is_git = true;
                candidate.current_branch = branch(Path::new(&path));
            }
            Ok(_) => candidate.error = "此目录属于父级 Git 仓库".into(),
            Err(_) if entry.path().join(".git").exists() => {
                candidate.error = "Git 仓库不可读取".into()
            }
            Err(_) => (),
        }
        results.push(candidate);
    }
    results.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(results)
}
fn status(repo: &Repository) -> Result<RepositoryStatus> {
    let path = Path::new(&repo.path);
    let root = root(path)?;
    let git_path = |name: &str| -> Result<bool> {
        let p = run(path, &["rev-parse", "--git-path", name], None)?;
        Ok(path.join(p).exists())
    };
    Ok(RepositoryStatus {
        id: repo.id.clone(),
        current_branch: branch(path),
        head: sha(path, "HEAD")?,
        dirty: !run(
            path,
            &["status", "--porcelain=v1", "--untracked-files=normal"],
            None,
        )?
        .is_empty(),
        merging: git_path("MERGE_HEAD")?,
        rebasing: git_path("rebase-merge")? || git_path("rebase-apply")?,
        remote_url: if validate_remote(repo).is_ok() {
            redact(&run(path, &["remote", "get-url", &repo.remote], None)?)
        } else {
            String::new()
        },
        branches: run(
            path,
            &[
                "for-each-ref",
                "--format=%(refname)",
                "refs/heads/",
                &format!("refs/remotes/{}/", repo.remote),
            ],
            None,
        )?
        .lines()
        .filter(|reference| *reference != format!("refs/remotes/{}/HEAD", repo.remote))
        .filter_map(|reference| {
            reference
                .strip_prefix("refs/heads/")
                .or_else(|| reference.strip_prefix(&format!("refs/remotes/{}/", repo.remote)))
        })
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect(),
        root,
        error: String::new(),
    })
}
pub fn statuses(repositories: Vec<Repository>) -> Vec<RepositoryStatus> {
    repositories
        .iter()
        .map(|r| {
            status(r).unwrap_or_else(|error| RepositoryStatus {
                id: r.id.clone(),
                error,
                ..Default::default()
            })
        })
        .collect()
}
pub fn repository_remote_urls(repositories: Vec<Repository>) -> HashMap<String, String> {
    repositories
        .into_iter()
        .filter(|repo| repo.scm_git_id.trim().is_empty())
        .filter_map(|repo| {
            let url = repository_remote_url(&repo)?;
            Some((repo.id, url))
        })
        .collect()
}

fn repository_remote_url(repo: &Repository) -> Option<String> {
    validate_remote_name(&repo.remote).ok()?;
    let path = Path::new(&repo.path);
    if !path.join(".git").exists() {
        return None;
    }
    // 显式定位 Git 元数据，支持 worktree，也防止普通子目录继承父仓库远端。
    let url = run(
        path,
        &["--git-dir=.git", "remote", "get-url", "--", &repo.remote],
        None,
    )
    .ok()?;
    if url.contains(['\r', '\n', '\0']) {
        return None;
    }
    // 缺少拉取 URL 时 Git 可能回显远端名。仅在此时检查是否配置了同名地址。
    if url == repo.remote
        && run(
            path,
            &[
                "--git-dir=.git",
                "config",
                "--get-all",
                &format!("remote.{}.url", repo.remote),
            ],
            None,
        )
        .ok()?
        .trim()
        .is_empty()
    {
        return None;
    }
    let url = redact(&url);
    (!url.trim().is_empty()).then_some(url)
}

fn occupied(path: &Path, target: &str, allow_current: bool) -> Result<()> {
    let list = run(path, &["worktree", "list", "--porcelain"], None)?;
    let expected = format!("branch refs/heads/{target}");
    for record in list.split("\n\n") {
        if record.lines().any(|line| line == expected) {
            let current = record
                .lines()
                .find_map(|l| l.strip_prefix("worktree "))
                .and_then(|p| canonical(Path::new(p)).ok());
            if !(allow_current && current.as_deref() == Some(root(path)?.as_str())) {
                return Err("目标分支已被其他 worktree 占用".into());
            }
        }
    }
    Ok(())
}
// 比较 Git 展开 insteadOf/pushInsteadOf 后的真实地址，不绕过用户的远端配置。
fn validate_push_remote(repo: &Repository) -> Result<()> {
    validate_remote(repo)?;
    let path = Path::new(&repo.path);
    if run(
        path,
        &[
            "config",
            "--bool",
            "--get",
            &format!("remote.{}.mirror", repo.remote),
        ],
        None,
    )
    .unwrap_or_default()
        == "true"
    {
        return Err("远端配置了 mirror 推送，无法保证只推送开发分支，请先整理远端配置".into());
    }
    let fetch = run(path, &["remote", "get-url", "--all", &repo.remote], None)?;
    let push = run(
        path,
        &["remote", "get-url", "--push", "--all", &repo.remote],
        None,
    )?;
    if fetch.lines().count() != 1 || push.lines().count() != 1 || fetch.is_empty() || fetch != push
    {
        return Err(
            "推送开发分支要求唯一且一致的 fetch/push URL；请先整理远端 pushurl 配置".into(),
        );
    }
    Ok(())
}
fn push_remote_sha(repo: &Repository, check: impl Fn() -> Result<()>) -> Result<String> {
    check()?;
    validate_push_remote(repo)?;
    let reference = format!("refs/heads/{}", repo.development_branch);
    // 与 Task::check 共用取消回调，取消时 run_checked 会终止整个 Git 进程组。
    let output = run_checked(
        Path::new(&repo.path),
        &["ls-remote", "--refs", "--", &repo.remote, &reference],
        check,
    )?;
    if output.is_empty() {
        return Ok(String::new());
    }
    let mut lines = output.lines();
    let (oid, name) = lines
        .next()
        .and_then(|line| line.split_once('\t'))
        .ok_or("远端开发分支响应无效")?;
    if lines.next().is_some()
        || name != reference
        || ![40, 64].contains(&oid.len())
        || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("远端开发分支响应无效".into());
    }
    Ok(oid.into())
}
fn validate_options(operation: &str, push: bool, keep_test_pom: bool) -> Result<()> {
    if ![
        "switch",
        "switch-test",
        "switch-master",
        "merge",
        "push-dev",
        "pull-current",
    ]
    .contains(&operation)
    {
        return Err("未知 Git 操作".into());
    }
    if keep_test_pom && operation != "merge" {
        return Err("仅合并计划支持保留测试分支 pom.xml".into());
    }
    if operation == "push-dev" && !push {
        return Err("推送开发分支必须显式确认 push=true".into());
    }
    if push && !["merge", "push-dev"].contains(&operation) {
        return Err("仅合并或推送开发分支计划支持显式推送".into());
    }
    Ok(())
}
#[cfg(test)]
fn snapshot(repo: &Repository, operation: &str) -> Result<PlanRepository> {
    snapshot_with_task(repo, operation, None)
}
fn snapshot_with_task(
    repo: &Repository,
    operation: &str,
    task: Option<&tasks::Task>,
) -> Result<PlanRepository> {
    if let Some(task) = task {
        task.check()?;
    }
    if ![
        "switch",
        "switch-test",
        "switch-master",
        "merge",
        "push-dev",
        "pull-current",
    ]
    .contains(&operation)
    {
        return Err("未知 Git 操作".into());
    }
    let path = Path::new(&repo.path);
    let status = status(repo)?;
    if status.dirty || status.merging || status.rebasing {
        return Err("仓库有未提交修改或正在 merge/rebase".into());
    }
    if operation == "pull-current" {
        return pull_snapshot(repo, status, task);
    }
    // 测试/master 切换只依赖各自目标，保留真实当前分支作为重验来源。
    if !["switch-test", "switch-master"].contains(&operation) {
        valid_branch(path, &repo.development_branch)?;
    }
    if operation == "push-dev" {
        if status.current_branch != repo.development_branch {
            return Err("推送开发分支要求当前就在配置的开发分支".into());
        }
        let source_sha = sha(path, &format!("refs/heads/{}", repo.development_branch))
            .map_err(|_| "本地开发分支不存在".to_string())?;
        if source_sha != status.head {
            return Err("开发分支与当前 HEAD 不一致，请重新准备".into());
        }
        // 每次重验查询实际远端，避免仅依赖可能过期的 remote-tracking ref。
        let remote_sha = push_remote_sha(repo, || task.map_or(Ok(()), tasks::Task::check))?;
        if !remote_sha.is_empty()
            && run(
                path,
                &["merge-base", "--is-ancestor", &remote_sha, &source_sha],
                None,
            )
            .is_err()
        {
            return Err("远端开发分支领先或已分叉，请先同步或处理分支后重新准备".into());
        }
        let common = run(path, &["rev-parse", "--git-common-dir"], None)?;
        return Ok(PlanRepository {
            repository: repo.clone(),
            root: status.root,
            common_dir: canonical(&path.join(common))?,
            current_branch: status.current_branch,
            source_sha,
            development_remote_sha: String::new(),
            sync_development_branch: false,
            target_branch: repo.development_branch.clone(),
            target_sha: remote_sha.clone(),
            create_tracking: false,
            remote_sha,
            sync_test_branch: false,
            remote_identity: remote_identity(repo),
        });
    }
    let target = match operation {
        "switch" => repo.development_branch.as_str(),
        "switch-master" => "master",
        _ => repo.test_branch.as_str(),
    };
    valid_branch(path, target)?;
    if operation == "merge"
        && (status.current_branch != repo.development_branch
            || repo.development_branch == repo.test_branch)
    {
        return Err("当前来源分支与开发分支不匹配，或来源与目标相同".into());
    }
    let local = sha(path, &format!("refs/heads/{target}"));
    let remote_sha = sha(path, &remote_ref(repo, target)).unwrap_or_default();
    let (target_sha, create_tracking) = match local {
        Ok(s) => (s, false),
        Err(_) if !remote_sha.is_empty() => (remote_sha.clone(), true),
        Err(_) => return Err("目标分支不存在，且没有明确的远端分支".into()),
    };
    occupied(path, target, operation != "merge")?;
    // 预检不移动本地分支：领先提交保留，落后允许执行时快进，仅分叉拒绝。
    let mut sync_test_branch = false;
    if operation == "merge"
        && !remote_sha.is_empty()
        && target_sha != remote_sha
        && run(
            path,
            &["merge-base", "--is-ancestor", &remote_sha, &target_sha],
            task,
        )
        .is_err()
    {
        run(
            path,
            &["merge-base", "--is-ancestor", &target_sha, &remote_sha],
            task,
        )
        .map_err(|_| "本地测试分支与远端已分叉，请先处理分支后重新准备".to_string())?;
        sync_test_branch = true;
    }
    let development_remote_sha = if operation == "merge" {
        sha(path, &remote_ref(repo, &repo.development_branch)).unwrap_or_default()
    } else {
        String::new()
    };
    let mut sync_development_branch = false;
    if !development_remote_sha.is_empty()
        && development_remote_sha != status.head
        && run(
            path,
            &[
                "merge-base",
                "--is-ancestor",
                &development_remote_sha,
                &status.head,
            ],
            task,
        )
        .is_err()
    {
        run(
            path,
            &[
                "merge-base",
                "--is-ancestor",
                &status.head,
                &development_remote_sha,
            ],
            task,
        )
        .map_err(|_| "本地开发分支与远端已分叉，请先处理分支后重新准备".to_string())?;
        sync_development_branch = true;
    }
    let common = run(path, &["rev-parse", "--git-common-dir"], None)?;
    Ok(PlanRepository {
        repository: repo.clone(),
        root: status.root,
        common_dir: canonical(&path.join(common))?,
        current_branch: status.current_branch,
        source_sha: status.head,
        development_remote_sha,
        sync_development_branch,
        target_branch: target.to_owned(),
        target_sha,
        create_tracking,
        remote_sha,
        sync_test_branch,
        remote_identity: remote_identity(repo),
    })
}
// upstream 可以异名，但必须位于用户选定的远端。只有未配置 upstream 时才回退到同名分支。
fn pull_target(repo: &Repository, current: &str) -> Result<String> {
    let path = Path::new(&repo.path);
    valid_branch(path, current)
        .map_err(|_| "拉取需要已检出的本地分支，请检查 detached HEAD".to_string())?;
    validate_remote(repo)?;
    let key = |suffix| format!("branch.{current}.{suffix}");
    let read = |name: &str| run(path, &["config", "--default", "", "--get", name], None);
    let remote = read(&key("remote"))?;
    let merge_key = key("merge");
    let merge = read(&merge_key)?;
    if remote.is_empty() && merge.is_empty() {
        return Ok(current.into());
    }
    if remote != repo.remote || merge.is_empty() {
        return Err("当前分支的 upstream 与所选远端不匹配或配置不完整".into());
    }
    if run(path, &["config", "--get-all", &merge_key], None)? != merge {
        return Err("当前分支配置了多个 upstream".into());
    }
    let target = merge
        .strip_prefix("refs/heads/")
        .ok_or("upstream 不是远端分支")?;
    valid_branch(path, target)?;
    Ok(target.into())
}

fn pull_snapshot(
    repo: &Repository,
    status: RepositoryStatus,
    task: Option<&tasks::Task>,
) -> Result<PlanRepository> {
    let path = Path::new(&repo.path);
    let target = pull_target(repo, &status.current_branch)?;
    let tracking = remote_ref(repo, &target);
    if target == "HEAD" || run(path, &["symbolic-ref", "-q", &tracking], task).is_ok() {
        return Err("远端跟踪引用是符号引用，无法安全拉取".into());
    }
    // 只更新目标跟踪引用，并禁用隐式 refmap，避免自定义 fetch 配置移动其他本地分支。
    let identity = remote_identity(repo);
    run(
        path,
        &[
            "fetch",
            "--no-tags",
            "--no-prune",
            "--no-recurse-submodules",
            "--refmap=",
            "--",
            &repo.remote,
            &format!("+refs/heads/{target}:{tracking}"),
        ],
        task,
    )
    .map_err(|_| "无法获取所选远端分支，请检查分支是否存在及远端连接".to_string())?;
    let remote_sha = sha(path, &tracking)?;
    if status.head != remote_sha
        && run(
            path,
            &["merge-base", "--is-ancestor", &remote_sha, &status.head],
            task,
        )
        .is_err()
    {
        run(
            path,
            &["merge-base", "--is-ancestor", &status.head, &remote_sha],
            task,
        )
        .map_err(|_| "当前分支与远端已分叉，无法快进拉取".to_string())?;
    }
    let common_dir = repository_common_dir(path)?;
    Ok(PlanRepository {
        repository: repo.clone(),
        root: status.root,
        common_dir,
        current_branch: status.current_branch,
        source_sha: status.head.clone(),
        target_sha: status.head,
        target_branch: target,
        create_tracking: false,
        remote_sha,
        remote_identity: identity,
        development_remote_sha: String::new(),
        sync_development_branch: false,
        sync_test_branch: false,
    })
}

fn remote_identity(repo: &Repository) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for args in [
        vec!["remote", "get-url", "--all", &repo.remote],
        vec!["remote", "get-url", "--push", "--all", &repo.remote],
    ] {
        run(Path::new(&repo.path), &args, None)
            .unwrap_or_default()
            .hash(&mut hash);
    }
    hash.finish()
}
#[cfg(test)]
fn preflight(repos: &[Repository], operation: &str) -> Result<Vec<PlanRepository>> {
    preflight_with_task(repos, operation, None)
}
fn preflight_with_task(
    repos: &[Repository],
    operation: &str,
    task: Option<&tasks::Task>,
) -> Result<Vec<PlanRepository>> {
    if repos.is_empty() {
        return Err("请选择仓库".into());
    }
    let mut roots = HashSet::new();
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    let mut errors = Vec::new();
    for repo in repos {
        if let Some(task) = task {
            task.check()?;
        }
        let snapshot = repository_common_dir(Path::new(&repo.path)).and_then(|common| {
            let _guard = RepositoryGuard::acquire(&common)?;
            snapshot_with_task(repo, operation, task)
        });
        match snapshot {
            Ok(saved) if roots.insert(saved.common_dir.clone()) && ids.insert(repo.id.clone()) => {
                result.push(saved)
            }
            Ok(_) => errors.push("存在重复 Git 根或仓库 id".into()),
            Err(error) => errors.push(format!("{}: {error}", repo.id)),
        }
    }
    if let Some(task) = task {
        task.check()?;
    }
    if errors.is_empty() {
        Ok(result)
    } else {
        Err(errors.join("；"))
    }
}
#[cfg(test)]
fn revalidate(saved: &PlanRepository, operation: &str) -> Result<()> {
    revalidate_with_task(saved, operation, None)
}
fn revalidate_with_task(
    saved: &PlanRepository,
    operation: &str,
    task: Option<&tasks::Task>,
) -> Result<()> {
    let now = snapshot_with_task(&saved.repository, operation, task)?;
    // 切换目标以执行时远端为准；仅远端跟踪引用变化不使计划失效。
    // 尚未创建的本地目标，其 target_sha 也来自远端，需与本地分支变化区分。
    let sync_switch = ["switch-test", "switch-master"].contains(&operation);
    if now.root != saved.root
        || now.common_dir != saved.common_dir
        || now.target_branch != saved.target_branch
        || now.current_branch != saved.current_branch
        || now.source_sha != saved.source_sha
        || (now.target_sha != saved.target_sha && !(sync_switch && saved.create_tracking))
        || now.create_tracking != saved.create_tracking
        || (!sync_switch && now.remote_sha != saved.remote_sha)
        || now.development_remote_sha != saved.development_remote_sha
        || now.sync_development_branch != saved.sync_development_branch
        || now.sync_test_branch != saved.sync_test_branch
        || now.remote_identity != saved.remote_identity
    {
        return Err("仓库已变化，请重新准备操作".into());
    }
    Ok(())
}
pub fn prepare(
    app: &AppHandle,
    project_id: String,
    repositories: Vec<Repository>,
    operation: String,
    push: bool,
    keep_test_pom: bool,
) -> Result<Plan> {
    prepare_plan(
        app,
        project_id,
        repositories,
        operation,
        push,
        keep_test_pom,
        None,
    )
}

fn pending_repositories(entry: &Entry) -> Result<impl Iterator<Item = &RepositoryResult>> {
    if entry.state.status != "paused" {
        return Err("仅暂停的 Git 批次可继续".into());
    }
    if entry
        .state
        .repositories
        .iter()
        .any(|r| !r.worktree_path.is_empty())
    {
        return Err("请先处理保留的 Git 工作区".into());
    }
    // 旧版会把重启前尚未确认的预检草稿恢复为 paused，不能把它当成已执行批次。
    if entry.state.task_id.is_empty()
        && entry
            .state
            .repositories
            .iter()
            .all(|r| r.status == "pending")
    {
        return Err("原 Git 批次尚未开始执行".into());
    }
    let mut pending = entry
        .state
        .repositories
        .iter()
        .filter(|r| r.status == "pending")
        .peekable();
    if pending.peek().is_none() {
        return Err("没有未执行的仓库".into());
    }
    Ok(pending)
}

fn remaining_repositories(entry: &Entry, configured: &[Repository]) -> Result<Vec<Repository>> {
    // 以原批次的待执行顺序匹配当前已保存的仓库配置。
    pending_repositories(entry)?
        .map(|result| {
            configured
                .iter()
                .find(|repo| repo.id == result.repo_id)
                .cloned()
                .ok_or_else(|| format!("未执行的仓库 {} 已不在项目配置中", result.repo_id))
        })
        .collect()
}

pub fn prepare_remaining(app: &AppHandle, plan_id: String) -> Result<Plan> {
    let source = {
        let runtime = app.state::<Runtime>();
        let mut entries = runtime.entries.lock().unwrap();
        if !entries.contains_key(&plan_id) {
            // 可直接通过历史 ID 继续，同时加载它的后续批次以检查是否已交接。
            uuid::Uuid::parse_str(&plan_id).map_err(|_| "计划不存在")?;
            let base = tasks::data_dir(app)?.join("backend-worktrees");
            let saved = restore_entry(&base.join(&plan_id).join("operation.json"))?;
            load_operations(&base, &mut entries, &saved.plan.project_id)?;
        }
        continuation_source(&entries, &plan_id)?.clone()
    };
    let config = tasks::load(app)?;
    let project = config
        .projects
        .iter()
        .find(|p| p.id == source.plan.project_id && p.project_type == "backend")
        .ok_or("后端项目已不在当前配置中")?;
    let repositories = remaining_repositories(&source, &project.sub_projects)?;
    prepare_plan(
        app,
        source.plan.project_id,
        repositories,
        source.plan.operation,
        source.plan.push,
        source.plan.keep_test_pom,
        Some(plan_id),
    )
}

fn prepare_plan(
    app: &AppHandle,
    project_id: String,
    repositories: Vec<Repository>,
    operation: String,
    push: bool,
    keep_test_pom: bool,
    source_plan_id: Option<String>,
) -> Result<Plan> {
    validate_options(&operation, push, keep_test_pom)?;
    if repositories.is_empty() {
        return Err("无效操作或未选择仓库".into());
    }
    let mut fetch_repositories = Vec::new();
    for repo in &repositories {
        // 当前分支拉取在逐仓快照中解析 upstream，只 fetch 指定分支。
        if operation == "pull-current" {
            continue;
        }
        let remotes = run(Path::new(&repo.path), &["remote"], None)?;
        // 无任何远端的本地仓库仍可切换/合并；推送必须有明确存在的远端。
        if remotes.is_empty() && !push {
            continue;
        }
        validate_remote(repo)?;
        if operation == "push-dev" {
            validate_push_remote(repo)?;
        }
        fetch_repositories.push(repo.clone());
    }
    // fetch 和后续预检共用受管任务，新增远端查询也必须响应取消及更新/退出等待。
    let (sender, receiver) = std::sync::mpsc::channel();
    let preflight_operation = operation.clone();
    tasks::run_backend(app.clone(), project_id.clone(), move |task| {
        let result = (|| {
            for repo in &fetch_repositories {
                let path = Path::new(&repo.path);
                let _guard = RepositoryGuard::acquire(&repository_common_dir(path)?)?;
                fetch_remote(path, repo, Some(task))?;
            }
            preflight_with_task(&repositories, &preflight_operation, Some(task))
        })();
        let _ = sender.send(result.clone());
        result.map(|_| "success")
    })?;
    let prepared_repositories = receiver
        .recv()
        .map_err(|_| "Git 准备任务异常".to_string())??;
    let plan = Plan {
        plan_id: uuid::Uuid::new_v4().to_string(),
        project_id,
        operation: operation.clone(),
        push,
        keep_test_pom,
        repositories: prepared_repositories,
    };
    let state = OperationState {
        plan_id: plan.plan_id.clone(),
        task_id: String::new(),
        status: "prepared".into(),
        repositories: plan
            .repositories
            .iter()
            .map(|r| RepositoryResult {
                repo_id: r.repository.id.clone(),
                status: "pending".into(),
                message: String::new(),
                worktree_path: String::new(),
                pom_conflict: None,
                pom_conflict_files: Vec::new(),
                pom_blocks_resolution: None,
            })
            .collect(),
    };
    let entry = Entry {
        plan: plan.clone(),
        state,
        continuation: source_plan_id.map(|source_plan_id| Continuation {
            source_plan_id,
            started: false,
        }),
        pom_conflicts: HashMap::new(),
        pom_blocks_snapshots: HashMap::new(),
    };
    let runtime = app.state::<Runtime>();
    let mut entries = runtime.entries.lock().unwrap();
    if let Some(continuation) = &entry.continuation {
        continuation_source(&entries, &continuation.source_plan_id)?;
    }
    persist_entry(app, &entry)?;
    entries.insert(plan.plan_id.clone(), entry);
    Ok(plan)
}

fn persist_entry(app: &AppHandle, entry: &Entry) -> Result<()> {
    let path = tasks::data_dir(app)?
        .join("backend-worktrees")
        .join(&entry.plan.plan_id)
        .join("operation.json");
    let text = journal_json(entry)?;
    crate::config::atomic_write(&path, &text).map_err(|_| "无法保存 Git 操作记录".into())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationJournal {
    #[serde(flatten)]
    entry: Entry,
    #[serde(default)]
    remote_identities: Vec<u64>,
}
fn journal_json(entry: &Entry) -> Result<String> {
    // 日记只保存快照身份，POM 正文从匹配的工作区和 Git 对象重新读取。
    let mut journal = OperationJournal {
        entry: entry.clone(),
        remote_identities: entry
            .plan
            .repositories
            .iter()
            .map(|saved| saved.remote_identity)
            .collect(),
    };
    for result in &mut journal.entry.state.repositories {
        result.pom_conflict = None;
        result.pom_conflict_files.clear();
        result.pom_blocks_resolution = None;
    }
    serde_json::to_string(&journal).map_err(|_| "无法序列化 Git 记录".into())
}
fn persist_current(app: &AppHandle, id: &str) -> Result<()> {
    let runtime = app.state::<Runtime>();
    let entries = runtime.entries.lock().unwrap();
    persist_entry(app, entries.get(id).ok_or("计划不存在")?)
}
fn read_operation_record(path: &Path) -> Result<Entry> {
    let bytes = fs::read(path).map_err(|_| "无法读取 Git 操作记录".to_string())?;
    let OperationJournal {
        mut entry,
        remote_identities,
    } = serde_json::from_slice(&bytes).map_err(|_| "Git 操作记录损坏，已保留".to_string())?;
    if !remote_identities.is_empty() {
        if remote_identities.len() != entry.plan.repositories.len() {
            return Err("Git 操作远端身份记录无效".into());
        }
        for (saved, identity) in entry.plan.repositories.iter_mut().zip(remote_identities) {
            saved.remote_identity = identity;
        }
    }
    let id = &entry.plan.plan_id;
    if uuid::Uuid::parse_str(id).is_err()
        || path
            .parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            != Some(id.as_str())
        || entry.state.plan_id != *id
        || entry.plan.repositories.len() != entry.state.repositories.len()
    {
        return Err("Git 操作记录身份无效".into());
    }
    for (index, result) in entry.state.repositories.iter().enumerate() {
        let expected = path.parent().unwrap().join(index.to_string());
        if result.repo_id != entry.plan.repositories[index].repository.id
            || (!result.worktree_path.is_empty() && Path::new(&result.worktree_path) != expected)
        {
            return Err("Git 操作工作区记录无效".into());
        }
    }
    if let Some(continuation) = &entry.continuation {
        if continuation.source_plan_id == *id
            || uuid::Uuid::parse_str(&continuation.source_plan_id).is_err()
            || (continuation.started && entry.state.task_id.is_empty())
        {
            return Err("Git 续跑来源记录无效".into());
        }
    }
    Ok(entry)
}

fn restore_entry(path: &Path) -> Result<Entry> {
    let mut entry = read_operation_record(path)?;
    for (index, result) in entry.state.repositories.iter_mut().enumerate() {
        if result.status == "running" {
            result.status = "error".into();
            result.message = "应用曾退出，请检查保留的工作区".into();
        }
        result.pom_conflict = None;
        result.pom_conflict_files.clear();
        result.pom_blocks_resolution = None;
        let worktree = Path::new(&result.worktree_path);
        let operation = path.parent().unwrap();
        if entry.plan.operation != "merge"
            || !operation_worktree_is_safe(operation.parent().unwrap(), operation, index, worktree)
        {
            continue;
        }
        let saved = &entry.plan.repositories[index];
        // 旧记录和手工处理的冲突可能没有可选边快照，仍从当前工作树恢复预览。
        result.pom_conflict_files =
            read_pom_conflict_files(saved, worktree, None).unwrap_or_default();
        if let Some(expected) = entry.pom_conflicts.get(&result.repo_id) {
            if let Ok((mut preview, found)) = inspect_pom_conflict(saved, worktree, None) {
                if found == *expected {
                    preview.push = entry.plan.push;
                    result.pom_conflict = Some(preview);
                }
            }
        }
        if let Some(expected) = entry.pom_blocks_snapshots.get(&result.repo_id) {
            if let Some(files) = recover_pom_blocks(saved, worktree, expected, None) {
                publish_pom_blocks(result, files, expected, entry.plan.push);
            }
        }
    }
    // 仅恢复查看/处理入口，不允许重跑过期的 plan 或自动 push。
    if entry.state.status != "success" {
        entry.state.status = "paused".into();
    }
    Ok(entry)
}

fn continuation_claimed(entries: &HashMap<String, Entry>, plan_id: &str) -> bool {
    entries.values().any(|entry| {
        entry.continuation.as_ref().is_some_and(|continuation| {
            continuation.source_plan_id == plan_id
                && (continuation.started || entry.state.status == "running")
        })
    })
}

fn continuation_source<'a>(
    entries: &'a HashMap<String, Entry>,
    plan_id: &str,
) -> Result<&'a Entry> {
    let source = entries.get(plan_id).ok_or("计划不存在")?;
    if continuation_claimed(entries, plan_id) {
        return Err("原批次的未执行仓库已交给新计划".into());
    }
    let _pending = pending_repositories(source)?;
    Ok(source)
}

fn load_operations(
    base: &Path,
    entries: &mut HashMap<String, Entry>,
    project_id: &str,
) -> Result<()> {
    let mut restored = Vec::new();
    if base.is_dir() {
        for item in fs::read_dir(base).map_err(|_| "无法读取 Git 操作目录".to_string())? {
            let item = item.map_err(|_| "无法读取 Git 操作目录".to_string())?;
            if !item
                .file_type()
                .map_err(|_| "无法读取 Git 操作目录".to_string())?
                .is_dir()
            {
                continue;
            }
            let path = item.path().join("operation.json");
            if !path.is_file() {
                continue;
            }
            if item
                .file_name()
                .to_str()
                .is_some_and(|id| entries.contains_key(id))
            {
                continue;
            }
            let entry = restore_entry(&path)?;
            // 未确认的续跑草稿不能在重启后成为新的待处理批次，已完成的续跑仍记录交接。
            if entry.plan.project_id == project_id
                && entry
                    .continuation
                    .as_ref()
                    .map_or(entry.state.status != "success", |c| c.started)
            {
                restored.push(entry);
            }
        }
    }
    // 整组记录通过校验后再加载，避免只读到来源而漏掉已交接的新计划。
    for entry in restored {
        entries.entry(entry.plan.plan_id.clone()).or_insert(entry);
    }
    Ok(())
}

pub fn list_operations(app: &AppHandle, project_id: String) -> Result<Vec<OperationState>> {
    let base = tasks::data_dir(app)?.join("backend-worktrees");
    let runtime = app.state::<Runtime>();
    let mut entries = runtime.entries.lock().unwrap();
    load_operations(&base, &mut entries, &project_id)?;
    let handed_off: HashSet<_> = entries
        .values()
        .filter_map(|entry| {
            entry
                .continuation
                .as_ref()
                .filter(|c| c.started)
                .map(|c| c.source_plan_id.as_str())
        })
        .collect();
    let mut result: Vec<_> = entries
        .values()
        .filter(|e| {
            e.plan.project_id == project_id
                && e.state.status != "success"
                && e.continuation.as_ref().is_none_or(|c| c.started)
                && !handed_off.contains(e.plan.plan_id.as_str())
        })
        .map(|e| e.state.clone())
        .collect();
    result.sort_by(|a, b| a.plan_id.cmp(&b.plan_id));
    Ok(result)
}
fn clear_operation_records(
    base: &Path,
    entries: &mut HashMap<String, Entry>,
    project_id: &str,
    selected_plan_id: Option<&str>,
) -> Result<Vec<String>> {
    let items = match fs::read_dir(base) {
        Ok(items) => items,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("无法读取 Git 操作目录".into()),
    };
    let mut records = HashMap::new();
    for item in items {
        let item = item.map_err(|_| "无法读取 Git 操作目录")?;
        if !item
            .file_type()
            .map_err(|_| "无法读取 Git 操作目录")?
            .is_dir()
        {
            continue;
        }
        let path = item.path().join("operation.json");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() => (),
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("无法读取 Git 操作记录".into()),
        }
        let entry = read_operation_record(&path)?;
        records.insert(entry.plan.plan_id.clone(), entry);
    }
    let handed_off: HashSet<_> = records
        .values()
        .filter(|entry| entry.plan.project_id == project_id)
        .filter_map(|entry| entry.continuation.as_ref().filter(|c| c.started))
        .map(|c| c.source_plan_id.as_str())
        .collect();
    let mut candidates: Vec<_> = records
        .values()
        .filter(|entry| {
            let state = &entry.state;
            entry.plan.project_id == project_id
                && !state.task_id.is_empty()
                && (matches!(state.status.as_str(), "success" | "cancelled")
                    || (state.status == "paused"
                        // 仅显式选中的暂停记录可放弃尚未执行的仓库。
                        && (selected_plan_id == Some(state.plan_id.as_str())
                            || handed_off.contains(state.plan_id.as_str())
                            || state
                                .repositories
                                .iter()
                                .all(|repo| repo.status != "pending"))))
                && state
                    .repositories
                    .iter()
                    .all(|repo| repo.worktree_path.is_empty())
                && entries.get(&state.plan_id).is_none_or(|live| {
                    !matches!(
                        live.state.status.as_str(),
                        "prepared" | "running" | "aborting"
                    ) && live
                        .state
                        .repositories
                        .iter()
                        .all(|repo| repo.worktree_path.is_empty())
                })
        })
        .collect();
    let mut removed = Vec::new();
    // 先清除续跑来源，再清除交接记录，避免重启后把原批次误当成可续跑。
    loop {
        let Some(index) = candidates.iter().position(|entry| {
            entry.continuation.as_ref().is_none_or(|c| {
                removed.contains(&c.source_plan_id)
                    || (!records.contains_key(&c.source_plan_id)
                        && !entries.contains_key(&c.source_plan_id))
            })
        }) else {
            break;
        };
        let entry = candidates.swap_remove(index);
        let id = &entry.plan.plan_id;
        let directory = base.join(id);
        // 工作区即使在日志中漏记，也不能失去恢复所需的记录。
        let mut has_other_files = false;
        for item in fs::read_dir(&directory).map_err(|_| "无法读取 Git 操作目录")? {
            if item.map_err(|_| "无法读取 Git 操作目录")?.file_name() != "operation.json" {
                has_other_files = true;
                break;
            }
        }
        if has_other_files {
            continue;
        }
        fs::remove_file(directory.join("operation.json")).map_err(|_| "无法清除 Git 操作记录")?;
        entries.remove(id);
        removed.push(id.clone());
        // 只移除空目录，不触碰任何工作区或仓库文件。
        let _ = fs::remove_dir(directory);
    }
    Ok(removed)
}

pub fn clear_operations(
    app: &AppHandle,
    project_id: String,
    selected_plan_id: Option<String>,
) -> Result<Vec<String>> {
    let base = tasks::data_dir(app)?.join("backend-worktrees");
    let runtime = app.state::<Runtime>();
    let mut entries = runtime.entries.lock().unwrap();
    clear_operation_records(
        &base,
        &mut entries,
        &project_id,
        selected_plan_id.as_deref(),
    )
}

pub fn state(app: &AppHandle, plan_id: String) -> Result<OperationState> {
    let runtime = app.state::<Runtime>();
    let entries = runtime.entries.lock().unwrap();
    let mut state = entries.get(&plan_id).ok_or("计划不存在")?.state.clone();
    if entries.values().any(|entry| {
        entry
            .continuation
            .as_ref()
            .is_some_and(|c| c.started && c.source_plan_id == plan_id)
    }) {
        state.repositories.retain(|r| r.status != "pending");
    }
    Ok(state)
}
fn update(app: &AppHandle, id: &str, change: impl FnOnce(&mut OperationState)) {
    update_entry(app, id, |entry| change(&mut entry.state));
}
fn update_entry(app: &AppHandle, id: &str, change: impl FnOnce(&mut Entry)) {
    if let Some(entry) = app.state::<Runtime>().entries.lock().unwrap().get_mut(id) {
        change(entry);
        if persist_entry(app, entry).is_err() {
            let _ = app.emit(
                "backend-git-notice",
                "Git 操作记录保存失败，请保留工作区路径并检查应用数据目录",
            );
        }
    }
    let _ = app.emit("backend-git-updated", serde_json::json!({ "planId": id }));
}
const MAX_POM_BYTES: usize = 1024 * 1024;
const CONFLICT_MARKERS: [&str; 4] = ["<<<<<<<", "|||||||", "=======", ">>>>>>>"];

fn is_one_conflict_block(content: &str) -> bool {
    // 仅接受完整的 Git 文本冲突标记；支持 merge、diff3 和 zdiff3。
    let mut stage = 0;
    for line in content.lines() {
        stage = if line.starts_with("<<<<<<< ") && stage == 0 {
            1
        } else if line.starts_with("||||||| ") && stage == 1 {
            2
        } else if line == "=======" && [1, 2].contains(&stage) {
            3
        } else if line.starts_with(">>>>>>> ") && stage == 3 {
            4
        } else if CONFLICT_MARKERS
            .iter()
            .any(|&marker| line.starts_with(marker))
        {
            return false;
        } else {
            stage
        };
    }
    stage == 4
}

fn pom_blob(worktree: &Path, oid: &str, task: Option<&tasks::Task>) -> Result<String> {
    let size = run(worktree, &["cat-file", "-s", oid], task)?
        .parse::<usize>()
        .map_err(|_| "无法读取 POM 版本")?;
    if size > MAX_POM_BYTES {
        return Err("POM 内容过大，请手工处理".into());
    }
    let content = run_output(
        worktree,
        &["cat-file", "blob", oid],
        || task.map_or(Ok(()), tasks::Task::check),
        true,
    )?;
    if content.len() != size || content.contains('\0') {
        return Err("无法读取完整文本 POM，请手工处理".into());
    }
    Ok(content)
}

fn pom_preview_blob(worktree: &Path, oid: &str, task: Option<&tasks::Task>) -> Option<String> {
    // 两侧都必须是索引中的完整 blob OID，并符合 POM 文本预览限制。
    if !matches!(oid.len(), 40 | 64) || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    pom_blob(worktree, oid, task).ok()
}

fn pom_branch_diff(
    worktree: &Path,
    test: &str,
    development: &str,
    task: Option<&tasks::Task>,
) -> Option<String> {
    let mut diff = run_output(
        worktree,
        &[
            "diff",
            "--text",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--word-diff=none",
            "--unified=0",
            "--inter-hunk-context=0",
            test,
            development,
            "--",
        ],
        || task.map_or(Ok(()), tasks::Task::check),
        true,
    )
    .ok()?;
    if diff.len() > MAX_POM_BYTES {
        return None;
    }
    // 只返回 hunks，省略文件头及 blob 名称。
    let start = diff.find("\n@@ ")? + 1;
    Some(diff.split_off(start))
}

type PomXmlContexts = Vec<(std::ops::Range<usize>, usize)>;

struct PomXmlElement<'a> {
    name: &'a str,
    inner_start: usize,
    owner: Option<usize>,
    label: &'a str,
    group: Option<&'a str>,
    artifact: Option<&'a str>,
}

fn pom_xml_contexts(xml: &str) -> Option<(PomXmlContexts, Vec<String>)> {
    // 保留原始字节位置。注释、CDATA、处理指令和属性中的尖括号不参与层级定位。
    static TOKEN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r#"(?s)\A(?:<!--.*?-->|<!\[CDATA\[.*?\]\]>|<\?.*?\?>|<(/?)([^\s/<>='"\x00]+)(?:\s+(?:[^\s/<>='"\x00]+\s*=\s*(?:"[^"]*"|'[^']*')\s*)*|\s*)(/?)>)"#,
        )
        .expect("POM XML token pattern")
    });
    fn local(name: &str) -> &str {
        name.rsplit(':').next().unwrap_or(name)
    }
    let mut stack = Vec::<PomXmlElement>::new();
    let mut owners = Vec::new();
    let mut contexts = Vec::new();
    let mut active = None;
    let mut active_start = 0;
    let mut offset = 0;
    let mut root_seen = false;
    let mut switch = |previous: Option<usize>, next, position| {
        if let Some(owner) = previous {
            if active_start < position {
                contexts.push((active_start..position, owner));
            }
        }
        active_start = position;
        next
    };
    while let Some(relative) = xml[offset..].find('<') {
        let start = offset + relative;
        if stack.is_empty()
            && !xml[offset..start]
                .trim_matches('\u{feff}')
                .trim()
                .is_empty()
        {
            return None;
        }
        let token = TOKEN.captures(&xml[start..])?;
        offset = start + token.get(0)?.end();
        let Some(name) = token.get(2).map(|capture| capture.as_str()) else {
            continue;
        };
        let tag = local(name);
        let closing = &token[1] == "/";
        let empty = &token[3] == "/";
        if !closing {
            if stack.is_empty() {
                if root_seen || tag != "project" {
                    return None;
                }
                root_seen = true;
            }
            let parent = stack.last().map(|element| local(element.name));
            let label = match (tag, parent) {
                ("dependency", Some("dependencies")) => "依赖",
                ("plugin", Some("plugins")) => "插件",
                ("parent", Some("project")) if stack.len() == 1 => "父 POM",
                ("version", Some("project")) if stack.len() == 1 => "项目版本",
                (_, Some("properties"))
                    if stack.iter().rev().nth(1).is_some_and(|element| {
                        matches!(local(element.name), "project" | "profile")
                    }) =>
                {
                    "属性"
                }
                _ => "",
            };
            let owner = if label.is_empty() {
                active
            } else {
                let id = owners.len();
                owners.push(String::new());
                active = switch(active, Some(id), start);
                Some(id)
            };
            stack.push(PomXmlElement {
                name,
                inner_start: offset,
                owner,
                label,
                group: None,
                artifact: None,
            });
        }
        if closing || empty {
            let element = stack.pop()?;
            if element.name != name {
                return None;
            }
            if !element.label.is_empty() {
                let group = element
                    .group
                    .or_else(|| (element.label == "插件").then_some("org.apache.maven.plugins"));
                owners[element.owner?] = match (element.label, group, element.artifact) {
                    ("属性", _, _) => format!("属性 {tag}"),
                    (label, Some(group), Some(artifact)) => format!("{label} {group}:{artifact}"),
                    (label, _, Some(artifact)) => format!("{label} {artifact}"),
                    (label, _, _) => label.into(),
                };
                active = switch(active, stack.last().and_then(|parent| parent.owner), offset);
            }
            if closing && matches!(tag, "groupId" | "artifactId") {
                let value = xml[element.inner_start..start].trim();
                if !value.is_empty() && value.len() <= 160 && !value.contains(['<', '\n', '\r']) {
                    if let Some(parent) = stack.last_mut() {
                        match tag {
                            "groupId" => parent.group = Some(value),
                            _ => parent.artifact = Some(value),
                        }
                    }
                }
            }
        }
    }
    (root_seen && stack.is_empty() && xml[offset..].trim().is_empty()).then_some((contexts, owners))
}

fn pom_side_contexts(test: Option<String>, development: Option<String>) -> Option<String> {
    match (test, development) {
        (Some(test), Some(development)) if test != development => {
            Some(format!("测试：{test}；开发：{development}"))
        }
        (test, development) => test.or(development),
    }
}

struct PomPreviewSide<'a> {
    lines: Vec<&'a str>,
    offsets: Vec<usize>,
    occurrences: HashMap<&'a str, Vec<usize>>,
    contexts: PomXmlContexts,
    labels: Vec<String>,
}

impl<'a> PomPreviewSide<'a> {
    fn new(blob: &'a str) -> Self {
        let lines = blob.lines().collect::<Vec<_>>();
        let mut occurrences = HashMap::<_, Vec<_>>::new();
        let mut offsets = Vec::with_capacity(lines.len() + 1);
        for (i, line) in lines.iter().enumerate() {
            occurrences.entry(*line).or_default().push(i);
            offsets.push(line.as_ptr() as usize - blob.as_ptr() as usize);
        }
        offsets.push(blob.len());
        let (contexts, labels) = pom_xml_contexts(blob).unwrap_or_default();
        Self {
            lines,
            offsets,
            occurrences,
            contexts,
            labels,
        }
    }

    fn context(&self, range: std::ops::Range<usize>) -> Option<String> {
        if range.is_empty() || range.end > self.lines.len() {
            return None;
        }
        let start = self.offsets[range.start];
        let end = self.offsets[range.end];
        let first = self.contexts.partition_point(|(span, _)| span.end <= start);
        let mut labels = Vec::new();
        for (span, owner) in &self.contexts[first..] {
            if span.start >= end {
                break;
            }
            let label = self.labels[*owner].as_str();
            if !labels.contains(&label) {
                if labels.len() == 4 {
                    labels.push("…");
                    break;
                }
                labels.push(label);
            }
        }
        (!labels.is_empty()).then(|| labels.join("、"))
    }

    fn marker_context(
        &self,
        fragment: &str,
        before: &[&str],
        after: &[&str],
        changed: &[std::ops::Range<usize>],
    ) -> Option<String> {
        let lines = fragment.lines().collect::<Vec<_>>();
        // 以冲突片段中出现次数最少的行缩小搜索范围。按两侧连续相邻行消除歧义，
        // 工作区标记行号不参与 blob 定位，因为自动合并及前面标记都可能改变行号。
        let (anchor, candidates) = lines
            .iter()
            .enumerate()
            .map(|(i, line)| (i, self.occurrences.get(line).map_or(&[][..], Vec::as_slice)))
            .min_by_key(|(_, candidates)| candidates.len())?;
        // 避免在恶意重复文本上反复扫描大量相同片段。无法可靠定位时省略语义提示。
        if candidates
            .len()
            .saturating_mul(lines.len() + before.len() + after.len())
            > MAX_POM_BYTES
        {
            return None;
        }
        let mut best = None;
        let mut context = None;
        for candidate in candidates {
            let Some(start) = candidate.checked_sub(anchor) else {
                continue;
            };
            let end = start + lines.len();
            if self.lines.get(start..end) != Some(&lines) {
                continue;
            }
            let first = changed.partition_point(|range| range.end <= start);
            let matches_diff = changed.get(first).is_some_and(|range| range.start < end);
            let preceding = self.lines[..start]
                .iter()
                .rev()
                .zip(before)
                .take_while(|(a, b)| a == b)
                .count();
            let following = self.lines[end..]
                .iter()
                .zip(after)
                .take_while(|(a, b)| a == b)
                .count();
            let score = (matches_diff, preceding + following);
            if best.is_none_or(|best| score > best) {
                best = Some(score);
                context = self.context(start..end);
            } else if best == Some(score) && self.context(start..end) != context {
                context = None;
            }
        }
        context
    }
}

fn pom_hunk_range(value: &str, prefix: char) -> Option<(std::ops::Range<usize>, String)> {
    let value = value.strip_prefix(prefix)?;
    let (line, count) = value.split_once(',').unwrap_or((value, "1"));
    let line = line.parse::<usize>().ok()?;
    let count = count.parse::<usize>().ok()?;
    if count == 0 {
        // Git 的零长度范围引用插入/删除点之前的行，0 表示文件开头。
        let position = if line == 0 {
            "文件开头".into()
        } else {
            format!("第 {line} 行后")
        };
        return Some((line..line, position));
    }
    let start = line.checked_sub(1)?;
    let end = start.checked_add(count)?;
    let location = if count == 1 {
        format!("第 {line} 行")
    } else {
        format!("第 {line}–{end} 行")
    };
    Some((start..end, location))
}

fn pom_diff_contexts(
    diff: &str,
    test: &PomPreviewSide<'_>,
    development: &PomPreviewSide<'_>,
    changed: &mut [Vec<std::ops::Range<usize>>; 2],
) -> BTreeMap<String, String> {
    diff.lines()
        .filter_map(|header| {
            let (coordinates, _) = header.strip_prefix("@@ ")?.split_once(" @@")?;
            let (left, right) = coordinates.split_once(' ')?;
            let (test_lines, test_location) = pom_hunk_range(left, '-')?;
            let (development_lines, development_location) = pom_hunk_range(right, '+')?;
            if !test_lines.is_empty() {
                changed[0].push(test_lines.clone());
            }
            if !development_lines.is_empty() {
                changed[1].push(development_lines.clone());
            }
            let context =
                pom_side_contexts(test.context(test_lines), development.context(development_lines))
                    .unwrap_or_else(|| format!("测试：{test_location}；开发：{development_location}"));
            Some((header.to_owned(), context))
        })
        .collect()
}

fn validate_pom_merge(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<()> {
    let manual = "无法确认 POM 冲突所属的合并，请在工作区手工处理";
    validate_worktree(saved, worktree, task)?;
    let merge_head = run(worktree, &["rev-parse", "--git-path", "MERGE_HEAD"], task)?;
    let mut merge_heads = String::new();
    fs::File::open(worktree.join(merge_head))
        .map_err(|_| manual)?
        .take(256)
        .read_to_string(&mut merge_heads)
        .map_err(|_| manual)?;
    if sha(worktree, "HEAD")? != merge_target(saved)
        || merge_heads.lines().ne(std::iter::once(merge_source(saved)))
    {
        return Err(manual.into());
    }
    Ok(())
}

#[cfg(test)]
fn parse_pom_conflict_blocks(content: &str, width: usize) -> Option<Vec<PomConflictBlock>> {
    let (blocks, _) = scan_pom_conflict_blocks(content, width)?;
    Some(blocks)
}

fn scan_pom_conflict_blocks(
    content: &str,
    width: usize,
) -> Option<(Vec<PomConflictBlock>, Vec<std::ops::Range<usize>>)> {
    if width == 0 || width > content.len() {
        return None;
    }
    #[derive(Clone, Copy)]
    enum Marker {
        Start,
        Base,
        Separator,
        End,
    }
    let markers = [
        ("<".repeat(width), Marker::Start),
        ("|".repeat(width), Marker::Base),
        ("=".repeat(width), Marker::Separator),
        (">".repeat(width), Marker::End),
    ];
    let mut blocks = Vec::new();
    let mut ranges = Vec::new();
    let mut current: Option<(PomConflictBlock, Marker)> = None;
    let mut start = 0;
    let mut offset = 0;
    for (index, line) in content.split_inclusive('\n').enumerate() {
        let line_start = offset;
        offset += line.len();
        let marker_line = line.trim_end_matches(['\n', '\r']);
        let marker = markers.iter().find_map(|(prefix, kind)| {
            let suffix = marker_line.strip_prefix(prefix)?;
            (suffix.is_empty() || (!matches!(kind, Marker::Separator) && suffix.starts_with(' ')))
                .then_some(*kind)
        });
        match (marker, current.as_mut()) {
            (Some(Marker::Start), None) => {
                start = line_start;
                current = Some((
                    PomConflictBlock {
                        line: index + 1,
                        test_content: String::new(),
                        development_content: String::new(),
                        context: None,
                    },
                    Marker::Start,
                ));
            }
            (Some(Marker::Base), Some((_, stage @ Marker::Start))) => *stage = Marker::Base,
            (Some(Marker::Separator), Some((_, stage @ (Marker::Start | Marker::Base)))) => {
                *stage = Marker::Separator;
            }
            (Some(Marker::End), Some((_, Marker::Separator))) => {
                blocks.push(current.take()?.0);
                ranges.push(start..offset);
            }
            (Some(_), _) => return None,
            (None, Some((block, Marker::Start))) => block.test_content.push_str(line),
            (None, Some((block, Marker::Separator))) => block.development_content.push_str(line),
            (None, _) => {}
        }
    }
    current.is_none().then_some((blocks, ranges))
}

fn read_pom_conflict_files(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<Vec<PomConflictFile>> {
    validate_pom_merge(saved, worktree, task)?;
    let index = run_bytes(worktree, &["ls-files", "--unmerged", "-z"], || {
        task.map_or(Ok(()), tasks::Task::check)
    })?;
    Ok(scan_pom_conflict_files(worktree, &index, task)?
        .into_iter()
        .map(|file| file.preview)
        .collect())
}

struct ScannedPomFile {
    preview: PomConflictFile,
    content: String,
    width: usize,
    ranges: Vec<std::ops::Range<usize>>,
}

fn scan_pom_conflict_files(
    worktree: &Path,
    index: &[u8],
    task: Option<&tasks::Task>,
) -> Result<Vec<ScannedPomFile>> {
    let root = worktree.canonicalize().map_err(|_| "无法确认 POM 工作区")?;
    let mut paths = BTreeMap::<_, [Option<&str>; 2]>::new();
    // 逐项解码，其他冲突文件的路径编码或文件类型不会影响 POM 的预览。
    for record in index.split_inclusive(|&byte| byte == 0) {
        let Some(entry) = record
            .strip_suffix(&[0])
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        else {
            continue;
        };
        let Some((metadata, path)) = entry.split_once('\t') else {
            continue;
        };
        let relative = Path::new(path);
        if relative.file_name() != Some(std::ffi::OsStr::new("pom.xml"))
            || !relative
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
        {
            continue;
        }
        let mut fields = metadata.split(' ');
        if !matches!(fields.next(), Some("100644" | "100755")) {
            continue;
        }
        let oid = fields.next();
        let slot = match fields.next() {
            Some("2") => 0,
            Some("3") => 1,
            _ => continue,
        };
        paths.entry(path).or_default()[slot] = oid;
    }
    let mut files = Vec::new();
    for (path, sides) in paths {
        if let Some(task) = task {
            task.check()?;
        }
        let [Some(test), Some(development)] = sides else {
            continue;
        };
        let file = root.join(path);
        let Ok(metadata) = fs::symlink_metadata(&file) else {
            continue;
        };
        if !metadata.is_file()
            || metadata.len() > MAX_POM_BYTES as u64
            || !file
                .canonicalize()
                .is_ok_and(|file| file.starts_with(&root))
        {
            continue;
        }
        let mut content = String::new();
        if fs::File::open(&file)
            .and_then(|file| {
                file.take(MAX_POM_BYTES as u64 + 1)
                    .read_to_string(&mut content)
            })
            .is_err()
            || content.len() > MAX_POM_BYTES
            || content.contains('\0')
        {
            continue;
        }
        let Ok(attrs) = run(
            worktree,
            &["check-attr", "-z", "conflict-marker-size", "--", path],
            task,
        ) else {
            continue;
        };
        let width = attrs
            .split_terminator('\0')
            .nth(2)
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|&width| width > 0)
            .unwrap_or(7);
        if let Some((mut blocks, ranges)) =
            scan_pom_conflict_blocks(&content, width).filter(|(blocks, _)| !blocks.is_empty())
        {
            let mut branch_diff = None;
            let mut diff_contexts = BTreeMap::new();
            if let (Some(test_blob), Some(development_blob)) = (
                pom_preview_blob(worktree, test, task),
                pom_preview_blob(worktree, development, task),
            ) {
                branch_diff = pom_branch_diff(worktree, test, development, task);
                let test = PomPreviewSide::new(&test_blob);
                let development = PomPreviewSide::new(&development_blob);
                let mut changed = [Vec::new(), Vec::new()];
                if let Some(diff) = &branch_diff {
                    diff_contexts = pom_diff_contexts(diff, &test, &development, &mut changed);
                }
                for (i, (block, range)) in blocks.iter_mut().zip(&ranges).enumerate() {
                    if let Some(task) = task {
                        task.check()?;
                    }
                    let previous_end = i.checked_sub(1).map_or(0, |i| ranges[i].end);
                    let next_start = ranges.get(i + 1).map_or(content.len(), |next| next.start);
                    let before = content[previous_end..range.start]
                        .lines()
                        .rev()
                        .take(12)
                        .collect::<Vec<_>>();
                    let after = content[range.end..next_start]
                        .lines()
                        .take(12)
                        .collect::<Vec<_>>();
                    block.context = pom_side_contexts(
                        test.marker_context(&block.test_content, &before, &after, &changed[0]),
                        development.marker_context(
                            &block.development_content,
                            &before,
                            &after,
                            &changed[1],
                        ),
                    );
                }
            }
            if let Some(task) = task {
                task.check()?;
            }
            files.push(ScannedPomFile {
                preview: PomConflictFile {
                    path: path.into(),
                    blocks,
                    branch_diff,
                    diff_contexts,
                },
                content,
                width,
                ranges,
            });
        }
    }
    Ok(files)
}

fn identity(value: impl Hash) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hash);
    hash.finish()
}

fn staged_index(worktree: &Path, task: Option<&tasks::Task>) -> Result<Vec<u8>> {
    run_bytes(worktree, &["ls-files", "--stage", "-z"], || {
        task.map_or(Ok(()), tasks::Task::check)
    })
}

fn unmerged_paths(index: &[u8]) -> Result<BTreeMap<&[u8], Vec<&[u8]>>> {
    let mut paths = BTreeMap::<_, Vec<_>>::new();
    for record in index
        .strip_suffix(&[0])
        .unwrap_or(index)
        .split(|&byte| byte == 0)
    {
        let tab = record
            .iter()
            .position(|&byte| byte == b'\t')
            .ok_or("无法读取 Git 索引")?;
        let (metadata, path) = (&record[..tab], &record[tab + 1..]);
        match metadata.last() {
            Some(b'0') => {}
            Some(b'1'..=b'3') => paths.entry(path).or_default().push(metadata),
            _ => return Err("无法读取 Git 索引".into()),
        }
    }
    Ok(paths)
}

fn inspect_pom_blocks(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<(PomBlocksSnapshot, Vec<ScannedPomFile>)> {
    let manual = "无法确认 POM 冲突块，请在工作区手工处理";
    validate_pom_merge(saved, worktree, task)?;
    let index = staged_index(worktree, task)?;
    let conflicts = unmerged_paths(&index)?;
    if run(worktree, &["config", "--get", "merge.default"], task).is_ok() {
        return Err(manual.into());
    }
    // AUTO_MERGE 是 Git 生成的初始冲突树，可区分实际标记和用户后续修改。
    let auto_merge = run(
        worktree,
        &["rev-parse", "--verify", "AUTO_MERGE^{tree}"],
        task,
    )?;
    let root = worktree.canonicalize().map_err(|_| manual)?;
    let mut files = Vec::new();
    let mut worktree_files = BTreeMap::new();
    for file in scan_pom_conflict_files(worktree, &index, task)? {
        let path = file.preview.path.as_str();
        if let Some(entries) = conflicts.get(path.as_bytes()) {
            if confirm_pom_block_file(saved, worktree, &root, &auto_merge, &file, entries, task)
                .is_ok()
            {
                worktree_files.insert(path.to_owned(), identity(file.content.as_bytes()));
                files.push(file);
            }
        }
    }
    if files.is_empty() || staged_index(worktree, task)? != index {
        return Err(manual.into());
    }
    let manual_remaining = files.len() != conflicts.len();
    if !manual_remaining {
        // 提交整笔合并前，隔离工作区不能含有其他手工修改。
        let changed = run_bytes(worktree, &["diff-files", "--name-only", "-z"], || {
            task.map_or(Ok(()), tasks::Task::check)
        })?;
        let staged_changes = run_bytes(
            worktree,
            &[
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "-z",
                &auto_merge,
                "--",
            ],
            || task.map_or(Ok(()), tasks::Task::check),
        )?;
        if [&changed, &staged_changes].into_iter().any(|changes| {
            changes
                .split(|&byte| byte == 0)
                .filter(|path| !path.is_empty())
                .any(|path| !conflicts.contains_key(path))
        }) || !run_bytes(
            worktree,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            || task.map_or(Ok(()), tasks::Task::check),
        )?
        .is_empty()
        {
            return Err(manual.into());
        }
    }
    Ok((
        PomBlocksSnapshot {
            index_identity: identity(&index),
            worktree_files,
            remote_identity: saved.remote_identity,
            manual_remaining,
        },
        files,
    ))
}

fn confirm_pom_block_file(
    saved: &PlanRepository,
    worktree: &Path,
    root: &Path,
    auto_merge: &str,
    file: &ScannedPomFile,
    entries: &[&[u8]],
    task: Option<&tasks::Task>,
) -> Result<()> {
    let manual = "无法确认 POM 文本冲突";
    let path = file.preview.path.as_str();
    let disk_path = root.join(path);
    if disk_path.canonicalize().ok().as_ref() != Some(&disk_path) {
        return Err(manual.into());
    }
    let attrs = run(
        worktree,
        &["check-attr", "-z", "merge", "filter", "--", path],
        task,
    )?;
    if !matches!(
        attrs.split_terminator('\0').collect::<Vec<_>>().as_slice(),
        [
            _,
            "merge",
            "unspecified" | "set" | "text",
            _,
            "filter",
            "unspecified" | "unset"
        ]
    ) {
        return Err(manual.into());
    }
    let mut stage_mode = None;
    for entry in entries {
        let metadata = std::str::from_utf8(entry).map_err(|_| manual)?;
        let mut fields = metadata.split(' ');
        let mode = fields.next().ok_or(manual)?;
        let oid = fields.next().ok_or(manual)?;
        if !matches!(mode, "100644" | "100755")
            || stage_mode.replace(mode).is_some_and(|other| other != mode)
        {
            return Err(manual.into());
        }
        let revision = match fields.next() {
            Some("1") => None,
            Some("2") => Some(merge_target(saved)),
            Some("3") => Some(merge_source(saved)),
            _ => return Err(manual.into()),
        };
        // 只接受原路径上的文本版本，避免误处理重命名及源文件中已有的标记。
        if let Some(revision) = revision {
            if run(
                worktree,
                &["rev-parse", "--verify", &format!("{revision}:{path}")],
                task,
            )? != oid
            {
                return Err(manual.into());
            }
        }
        let content = pom_blob(worktree, oid, task)?;
        if content.lines().any(|line| {
            [b'<', b'|', b'=', b'>'].into_iter().any(|marker| {
                line.as_bytes()
                    .iter()
                    .take_while(|&&byte| byte == marker)
                    .count()
                    >= file.width
            })
        }) {
            return Err(manual.into());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&disk_path)
            .map_err(|_| manual)?
            .permissions()
            .mode();
        if (mode & 0o100 != 0) != (stage_mode == Some("100755")) {
            return Err(manual.into());
        }
    }
    let baseline = run(
        worktree,
        &["rev-parse", "--verify", &format!("{auto_merge}:{path}")],
        task,
    )?;
    if run(worktree, &["hash-object", "--", path], task)? != baseline
        || fs::read(&disk_path).map_err(|_| manual)? != file.content.as_bytes()
    {
        return Err(manual.into());
    }
    Ok(())
}

fn inspect_pom_conflict(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<(PomConflict, PomConflictSnapshot)> {
    use std::hash::{Hash, Hasher};
    let manual = "无法确认单处 POM 内容冲突，请在工作区手工处理";
    validate_pom_merge(saved, worktree, task)?;
    let index = run(worktree, &["ls-files", "--stage", "-z"], task)?;
    if !index.ends_with('\0') {
        return Err(manual.into());
    }
    let mut conflict_path = None;
    let mut stages = [None; 3];
    let mut stage_mode = None;
    for entry in index.split_terminator('\0') {
        let (metadata, path) = entry.split_once('\t').ok_or(manual)?;
        let mut fields = metadata.split(' ');
        let mode = fields.next().ok_or(manual)?;
        let oid = fields.next().ok_or(manual)?;
        let stage = fields.next().ok_or(manual)?;
        if stage == "0" {
            continue;
        }
        let slot = match stage {
            "1" => 0,
            "2" => 1,
            "3" => 2,
            _ => return Err(manual.into()),
        };
        if stages[slot].replace(oid).is_some()
            || conflict_path.is_some_and(|other| other != path)
            || stage_mode.is_some_and(|other| other != mode)
            || !["100644", "100755"].contains(&mode)
        {
            return Err(manual.into());
        }
        conflict_path = Some(path);
        stage_mode = Some(mode);
    }
    let path = conflict_path.ok_or(manual)?;
    let [Some(_), Some(test), Some(development)] = stages else {
        return Err(manual.into());
    };
    let relative = Path::new(path);
    if relative.file_name() != Some(std::ffi::OsStr::new("pom.xml"))
        || !relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Err(manual.into());
    }
    // 两侧都必须保有同一路径的原始 blob，避免把重命名等结构冲突当成文本冲突。
    for (revision, expected) in [
        (merge_target(saved), test),
        (merge_source(saved), development),
    ] {
        let spec = format!("{revision}:{path}");
        if run(worktree, &["rev-parse", "--verify", &spec], task)? != expected {
            return Err(manual.into());
        }
    }
    // 只接受 Git 的默认文本合并。自定义驱动与非默认标记交给手工处理。
    let attrs = run(
        worktree,
        &[
            "check-attr",
            "-z",
            "merge",
            "conflict-marker-size",
            "--",
            path,
        ],
        task,
    )?;
    let attrs: Vec<_> = attrs.split_terminator('\0').collect();
    if !matches!(
        attrs.as_slice(),
        [
            _,
            "merge",
            "unspecified" | "set",
            _,
            "conflict-marker-size",
            "unspecified" | "7"
        ]
    ) || run(worktree, &["config", "--get", "merge.default"], task).is_ok()
    {
        return Err(manual.into());
    }
    if run(worktree, &["diff-files", "--name-only", "-z"], task)?
        .split_terminator('\0')
        .any(|changed| changed != path)
        || !run(
            worktree,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            task,
        )?
        .is_empty()
    {
        return Err(manual.into());
    }
    let file = worktree.join(relative);
    if !fs::symlink_metadata(&file).map_err(|_| manual)?.is_file()
        || !file
            .canonicalize()
            .map_err(|_| manual)?
            .starts_with(worktree.canonicalize().map_err(|_| manual)?)
    {
        return Err(manual.into());
    }
    let before = run(worktree, &["hash-object", "--no-filters", "--", path], task)?;
    let mut content = String::new();
    fs::File::open(&file)
        .map_err(|_| manual)?
        .take(MAX_POM_BYTES as u64 + 1)
        .read_to_string(&mut content)
        .map_err(|_| manual)?;
    if content.len() > MAX_POM_BYTES || content.contains('\0') || !is_one_conflict_block(&content) {
        return Err(manual.into());
    }
    let test_content = pom_blob(worktree, test, task)?;
    let development_content = pom_blob(worktree, development, task)?;
    // 只检查可选的两侧版本，祖先中的历史标记不会写入合并结果。
    if [&test_content, &development_content]
        .into_iter()
        .any(|text| {
            text.lines().any(|line| {
                CONFLICT_MARKERS
                    .iter()
                    .any(|&marker| line.starts_with(marker))
            })
        })
    {
        return Err(manual.into());
    }
    // 将工作树 POM 与完整索引纳入快照，确认时拒绝覆盖其间的手工修改。
    if run(worktree, &["ls-files", "--stage", "-z"], task)? != index {
        return Err(manual.into());
    }
    let mut index_hash = std::collections::hash_map::DefaultHasher::new();
    index.hash(&mut index_hash);
    let worktree_blob = run(worktree, &["hash-object", "--no-filters", "--", path], task)?;
    if before != worktree_blob {
        return Err(manual.into());
    }
    Ok((
        PomConflict {
            path: path.into(),
            test_content,
            development_content,
            test_branch: saved.target_branch.clone(),
            development_branch: saved.repository.development_branch.clone(),
            push: false,
        },
        PomConflictSnapshot {
            path: path.into(),
            worktree_blob,
            index_identity: index_hash.finish(),
            remote_identity: remote_identity(&saved.repository),
        },
    ))
}

// 切换后以已确认的本地目标 SHA 为基线；fetch 只能改变远端引用，不能改变本地现场。
fn validate_checkout(saved: &PlanRepository, branch: &str, expected_head: &str) -> Result<()> {
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
    let current = status(repo)?;
    let common = run(path, &["rev-parse", "--git-common-dir"], None)?;
    if current.root != saved.root
        || canonical(&path.join(common))? != saved.common_dir
        || current.current_branch != branch
        || current.head != expected_head
        || sha(path, &format!("refs/heads/{branch}"))? != expected_head
        || current.dirty
        || current.merging
        || current.rebasing
        || remote_identity(repo) != saved.remote_identity
    {
        return Err("仓库在切换或同步期间已变化，现场已保留".into());
    }
    Ok(())
}
fn validate_switched_target(saved: &PlanRepository) -> Result<()> {
    validate_checkout(saved, &saved.target_branch, &saved.target_sha)
}
fn sync_switched_target(saved: &PlanRepository, task: Option<&tasks::Task>) -> Result<()> {
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
    validate_switched_target(saved)?;
    if run(path, &["remote"], task)?.is_empty() {
        if let Some(task) = task {
            task.emit(
                "log",
                "已切换本地分支；仓库未配置远端，未执行远端同步",
                None,
                None,
            );
        }
        return Ok(());
    }
    fetch_remote(path, repo, task)?;
    validate_switched_target(saved)?;
    let remote_sha = sha(path, &remote_ref(repo, &saved.target_branch))
        .map_err(|_| "远端目标分支不存在，无法同步".to_string())?;
    // 允许远端在预检后更新；只快进执行时 fetch 到的提交，本地领先时保持原样。
    let local_ahead = run(
        path,
        &[
            "merge-base",
            "--is-ancestor",
            &remote_sha,
            &saved.target_sha,
        ],
        task,
    )
    .is_ok();
    if !local_ahead
        && run(
            path,
            &[
                "merge-base",
                "--is-ancestor",
                &saved.target_sha,
                &remote_sha,
            ],
            task,
        )
        .is_err()
    {
        return Err("本地目标分支与远端已分叉，请手工处理；未创建合并提交".into());
    }
    validate_switched_target(saved)?;
    if sha(path, &remote_ref(repo, &saved.target_branch))? != remote_sha {
        return Err("远端引用在同步期间已变化，请重新准备".into());
    }
    run(
        path,
        &[
            "merge",
            "--ff-only",
            "--no-edit",
            "--no-autostash",
            "--",
            &remote_sha,
        ],
        task,
    )?;
    let mut synced = saved.clone();
    if !local_ahead {
        synced.target_sha = remote_sha;
    }
    validate_switched_target(&synced)
}

#[cfg(test)]
fn perform(
    saved: &PlanRepository,
    operation: &str,
    push: bool,
    worktree: &Path,
    task: Option<&tasks::Task>,
    on_worktree: impl Fn() -> Result<()>,
) -> Result<()> {
    perform_with_options(saved, operation, push, false, worktree, task, on_worktree)
}
// source_sha 始终保留预检本地 HEAD；合并与恢复使用确认同步后的来源。
fn merge_source(saved: &PlanRepository) -> &str {
    if saved.sync_development_branch {
        &saved.development_remote_sha
    } else {
        &saved.source_sha
    }
}

fn merge_target(saved: &PlanRepository) -> &str {
    if saved.sync_test_branch {
        &saved.remote_sha
    } else {
        &saved.target_sha
    }
}

fn validate_merge_state(
    saved: &PlanRepository,
    worktree: &Path,
    source: &str,
    target: &str,
    task: Option<&tasks::Task>,
) -> Result<()> {
    validate_merge_refs(saved, worktree, source, target, task)?;
    if !run(worktree, &["status", "--porcelain=v1"], task)?.is_empty() {
        return Err("测试工作区存在改动，已保留现场".into());
    }
    for name in ["MERGE_HEAD", "rebase-merge", "rebase-apply"] {
        let marker = run(worktree, &["rev-parse", "--git-path", name], task)?;
        if worktree.join(marker).exists() {
            return Err("测试工作区正在 merge/rebase，已保留现场".into());
        }
    }
    Ok(())
}

fn validate_merge_refs(
    saved: &PlanRepository,
    worktree: &Path,
    source: &str,
    target: &str,
    task: Option<&tasks::Task>,
) -> Result<()> {
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
    validate_worktree(saved, worktree, task)?;
    let current = status(repo)?;
    let common = run(path, &["rev-parse", "--git-common-dir"], task)?;
    if current.root != saved.root
        || canonical(&path.join(common))? != saved.common_dir
        || current.current_branch != saved.current_branch
        || current.head != source
        || current.dirty
        || current.merging
        || current.rebasing
        || remote_identity(repo) != saved.remote_identity
        || sha(path, &remote_ref(repo, &saved.target_branch)).unwrap_or_default()
            != saved.remote_sha
        || sha(path, &remote_ref(repo, &repo.development_branch)).unwrap_or_default()
            != saved.development_remote_sha
        || sha(path, &format!("refs/heads/{}", repo.development_branch))? != source
        || sha(worktree, "HEAD")? != target
    {
        return Err("仓库或远端在同步时已变化，请重新准备操作；工作区已保留".into());
    }
    Ok(())
}

fn perform_with_options(
    saved: &PlanRepository,
    operation: &str,
    push: bool,
    keep_test_pom: bool,
    worktree: &Path,
    task: Option<&tasks::Task>,
    on_worktree: impl Fn() -> Result<()>,
) -> Result<()> {
    validate_options(&operation, push, keep_test_pom)?;
    if let Some(task) = task {
        task.check()?;
    }
    let _guard = RepositoryGuard::acquire(&saved.common_dir)?;
    revalidate_with_task(saved, operation, task)?;
    // 新建跟踪分支时使用刚重验过的远端 tip，后续仍按实际创建基线保护本地 SHA。
    let tracking_saved =
        if ["switch-test", "switch-master"].contains(&operation) && saved.create_tracking {
            let mut updated = saved.clone();
            updated.target_sha = sha(
                Path::new(&saved.root),
                &remote_ref(&saved.repository, &saved.target_branch),
            )?;
            Some(updated)
        } else {
            None
        };
    let saved = tracking_saved.as_ref().unwrap_or(saved);
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
    if operation == "pull-current" {
        validate_checkout(saved, &saved.current_branch, &saved.source_sha)?;
        if pull_target(repo, &saved.current_branch)? != saved.target_branch
            || sha(path, &remote_ref(repo, &saved.target_branch))? != saved.remote_sha
        {
            return Err("当前分支的远端已变化，请重新准备操作".into());
        }
        if run(
            path,
            &[
                "merge-base",
                "--is-ancestor",
                &saved.remote_sha,
                &saved.source_sha,
            ],
            task,
        )
        .is_ok()
        {
            return Ok(());
        }
        run(
            path,
            &[
                "merge",
                "--ff-only",
                "--no-squash",
                "--commit",
                "--no-overwrite-ignore",
                "--no-autostash",
                "--",
                &saved.remote_sha,
            ],
            task,
        )?;
        return validate_checkout(saved, &saved.current_branch, &saved.remote_sha);
    }
    if operation == "push-dev" {
        // 固定已确认的提交；即使之后本地 ref 更新也绝不推送未经确认的提交。
        // 禁止配置附带 tags、mirror 或递归推送其他仓库；不跳过 hooks/签名/SSH。
        return run(
            path,
            &[
                "push",
                "--porcelain",
                "--no-force",
                "--no-mirror",
                "--no-follow-tags",
                "--recurse-submodules=no",
                "--",
                &repo.remote,
                &format!(
                    "{}:refs/heads/{}",
                    saved.source_sha, repo.development_branch
                ),
            ],
            task,
        )
        .map(|_| ());
    }
    if saved.create_tracking {
        run(
            path,
            &[
                "branch",
                "--track",
                &saved.target_branch,
                &remote_ref(repo, &saved.target_branch),
            ],
            task,
        )?;
    }
    if ["switch", "switch-test", "switch-master"].contains(&operation) {
        if let Err(error) = run(path, &["switch", "--no-guess", &saved.target_branch], task) {
            // post-checkout 钩子失败时 Git 可能已切换成功，不能误报为尚未切换。
            return Err(
                if ["switch-test", "switch-master"].contains(&operation)
                    && branch(path) == saved.target_branch
                {
                    format!("已切换到 {}，未同步远端；{error}", saved.target_branch)
                } else {
                    error
                },
            );
        }
        if ["switch-test", "switch-master"].contains(&operation) {
            return sync_switched_target(saved, task).map_err(|error| {
                format!(
                    "已切换到 {}，无法确认远端同步成功；现场已保留；{error}",
                    saved.target_branch
                )
            });
        }
        return Ok(());
    }
    if worktree.exists() {
        return Err("操作 worktree 路径已存在，拒绝复用".into());
    }
    let worktree_text = worktree.to_str().ok_or("worktree 路径编码无效")?;
    on_worktree()?;
    run(
        path,
        &["worktree", "add", "--", worktree_text, &saved.target_branch],
        task,
    )?;
    if sha(worktree, "HEAD")? != saved.target_sha {
        return Err("目标分支在创建工作区时已变化，已保留现场".into());
    }
    // fetch 前后均检查两个分支，避免覆盖并发 tracking ref 变化而漏报。
    validate_merge_state(saved, worktree, &saved.source_sha, &saved.target_sha, task)?;
    // 在独立工作区重新拉取，固定用户确认过的远端 SHA；远端变化必须重新准备。
    if !run(path, &["remote"], task)?.is_empty() {
        fetch_remote(worktree, repo, task)?;
    }
    validate_merge_state(saved, worktree, &saved.source_sha, &saved.target_sha, task)?;
    if saved.sync_development_branch {
        // 不自动暂存或重写开发历史；失败保留已经完成的快进与 hooks 现场。
        run(
            path,
            &[
                "merge",
                "--ff-only",
                "--no-edit",
                "--no-autostash",
                "--",
                &saved.development_remote_sha,
            ],
            task,
        )?;
    }
    validate_merge_state(
        saved,
        worktree,
        merge_source(saved),
        &saved.target_sha,
        task,
    )?;
    if saved.sync_test_branch {
        // 不使用 reset/rebase；--ff-only 即使发生并发变化也不能覆盖本地提交。
        run(
            worktree,
            &[
                "merge",
                "--ff-only",
                "--no-edit",
                "--no-autostash",
                "--",
                &saved.remote_sha,
            ],
            task,
        )?;
        if sha(worktree, "HEAD")? != saved.remote_sha {
            return Err("测试分支在快进时已变化，已保留现场".into());
        }
    }
    validate_merge_state(
        saved,
        worktree,
        merge_source(saved),
        merge_target(saved),
        task,
    )?;
    if let Err(error) = run(
        worktree,
        &[
            "merge",
            "--no-edit",
            "--no-autostash",
            "--",
            merge_source(saved),
        ],
        task,
    ) {
        return if run(worktree, &["ls-files", "--unmerged", "-z"], task)
            .is_ok_and(|conflicts| !conflicts.is_empty())
        {
            Err("合并存在冲突，请在保留工作区手工合并代码".into())
        } else {
            Err(error)
        };
    }
    finish_merged_worktree(saved, worktree, push, task)
}

fn finish_merged_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    push: bool,
    task: Option<&tasks::Task>,
) -> Result<()> {
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
    let worktree_text = worktree.to_str().ok_or("worktree 路径编码无效")?;
    let merged_head = sha(worktree, "HEAD")?;
    validate_merge_state(saved, worktree, merge_source(saved), &merged_head, task)?;
    for baseline in [merge_source(saved), merge_target(saved)] {
        run(
            worktree,
            &["merge-base", "--is-ancestor", baseline, &merged_head],
            task,
        )?;
    }
    if push {
        validate_remote(repo)?;
        // 精确分支 refspec，普通 push 的非快进保护负责拒绝并发远端提交。
        run(
            worktree,
            &[
                "push",
                "--porcelain",
                "--",
                &repo.remote,
                &format!(
                    "refs/heads/{}:refs/heads/{}",
                    saved.target_branch, saved.target_branch
                ),
            ],
            task,
        )?;
    }
    if !run(worktree, &["status", "--porcelain=v1"], task)?.is_empty() {
        return Err("合并工作区仍有修改，已保留".into());
    }
    run(path, &["worktree", "remove", "--", worktree_text], task)?;
    Ok(())
}
fn start_continuation(
    entry: &mut Entry,
    task_id: &str,
    persist: impl FnOnce(&Entry) -> Result<()>,
) -> Result<()> {
    let mut started = entry.clone();
    started.continuation.as_mut().ok_or("缺少续跑来源")?.started = true;
    started.state.task_id = task_id.into();
    // 交接和任务 ID 一次落盘，重启后可用新批次恢复，包括尚未处理首个仓库的情况。
    persist(&started)?;
    *entry = started;
    Ok(())
}

pub fn execute(app: AppHandle, plan_id: String) -> Result<String> {
    let (plan, source_plan_id) = {
        let runtime = app.state::<Runtime>();
        let mut entries = runtime.entries.lock().unwrap();
        let entry = entries.get(&plan_id).ok_or("计划不存在")?;
        if entry.state.status != "prepared" {
            return Err("计划已执行，请重新准备".into());
        }
        let source_plan_id = entry
            .continuation
            .as_ref()
            .map(|c| c.source_plan_id.clone());
        if let Some(source) = &source_plan_id {
            continuation_source(&entries, source)?;
        }
        let plan = entry.plan.clone();
        entries.get_mut(&plan_id).unwrap().state.status = "running".into();
        (plan, source_plan_id)
    };
    let operation_id = plan.plan_id.clone();
    let app_for_error = app.clone();
    let is_continuation = source_plan_id.is_some();
    let (sender, receiver) = std::sync::mpsc::channel();
    let result = tasks::run_backend(app, plan.project_id.clone(), move |task| {
        if let Some(source) = source_plan_id {
            let started = {
                let runtime = task.app.state::<Runtime>();
                let mut entries = runtime.entries.lock().unwrap();
                let entry = entries.get_mut(&plan.plan_id).ok_or("计划不存在")?;
                start_continuation(entry, &task.id, |e| persist_entry(&task.app, e))
            };
            if let Err(error) = started {
                update(&task.app, &plan.plan_id, |s| s.status = "prepared".into());
                let _ = sender.send(Err(error.clone()));
                return Err(error);
            }
            let _ = sender.send(Ok(()));
            for id in [&source, &plan.plan_id] {
                let _ = task
                    .app
                    .emit("backend-git-updated", serde_json::json!({ "planId": id }));
            }
        } else {
            update(&task.app, &plan.plan_id, |s| s.task_id = task.id.clone());
        }
        let base = tasks::data_dir(&task.app)
            .map(|dir| dir.join("backend-worktrees").join(&plan.plan_id))
            .and_then(|base| {
                if plan.operation == "merge" {
                    fs::create_dir_all(&base).map_err(|_| "无法创建 Git 工作区目录".to_string())?;
                }
                Ok(base)
            });
        let base = match base {
            Ok(base) => base,
            Err(error) => {
                update(&task.app, &plan.plan_id, |s| {
                    s.status = "paused".into();
                    if !is_continuation {
                        s.repositories[0].status = "error".into();
                        s.repositories[0].message = error.clone();
                    }
                });
                return Err(error);
            }
        };
        run_plan_repositories(
            task,
            &plan,
            &base,
            0..plan.repositories.len(),
            is_continuation,
        )
    });
    match result {
        Ok(task_id) => {
            if is_continuation {
                receiver
                    .recv()
                    .map_err(|_| "Git 续跑启动异常".to_string())??;
            } else {
                update(&app_for_error, &operation_id, |s| {
                    // 任务可能已暂停并由冲突处理任务接手，不覆盖较新的任务 ID。
                    if s.task_id.is_empty() {
                        s.task_id = task_id.clone();
                    }
                });
            }
            Ok(task_id)
        }
        Err(error) => {
            update(&app_for_error, &operation_id, |s| {
                s.status = "prepared".into()
            });
            Err(error)
        }
    }
}
fn run_plan_repositories(
    task: &tasks::Task,
    plan: &Plan,
    base: &Path,
    indices: impl IntoIterator<Item = usize>,
    pause_on_cancel: bool,
) -> Result<&'static str> {
    for index in indices {
        if pause_on_cancel {
            if let Err(error) = task.check() {
                update(&task.app, &plan.plan_id, |s| s.status = "paused".into());
                return Err(error);
            }
        }
        let saved = &plan.repositories[index];
        let worktree = base.join(index.to_string());
        update(&task.app, &plan.plan_id, |s| {
            s.repositories[index].status = "running".into()
        });
        task.emit("log", "正在执行后端 Git 仓库操作", None, None);
        let result = perform_with_options(
            saved,
            &plan.operation,
            plan.push,
            plan.keep_test_pom,
            &worktree,
            Some(task),
            || {
                update(&task.app, &plan.plan_id, |s| {
                    s.repositories[index].worktree_path = worktree.to_string_lossy().into()
                });
                persist_current(&task.app, &plan.plan_id)
            },
        );
        match result {
            Ok(()) => update(&task.app, &plan.plan_id, |s| {
                s.repositories[index].status = "success".into();
                s.repositories[index].message = "操作完成".into();
                s.repositories[index].worktree_path.clear();
            }),
            Err(error) => {
                let cancelled = task.check().is_err();
                let mut files = Vec::new();
                let mut resolution = None;
                if !cancelled
                    && plan.operation == "merge"
                    && base.parent().is_some_and(|operations| {
                        operation_worktree_is_safe(operations, base, index, &worktree)
                    })
                {
                    resolution = validate_merge_refs(
                        saved,
                        &worktree,
                        merge_source(saved),
                        merge_target(saved),
                        Some(task),
                    )
                    .and_then(|_| inspect_pom_blocks(saved, &worktree, Some(task)))
                    .ok();
                    if resolution.is_none() {
                        files = read_pom_conflict_files(saved, &worktree, Some(task))
                            .unwrap_or_default();
                    }
                }
                update_entry(&task.app, &plan.plan_id, |entry| {
                    let s = &mut entry.state;
                    s.repositories[index].pom_conflict_files = files;
                    if let Some((snapshot, files)) = resolution {
                        let result = &mut s.repositories[index];
                        publish_pom_blocks(result, files, &snapshot, plan.push);
                        entry
                            .pom_blocks_snapshots
                            .insert(result.repo_id.clone(), snapshot);
                    }
                    s.status = if cancelled && !pause_on_cancel {
                        "cancelled"
                    } else {
                        "paused"
                    }
                    .into();
                    s.repositories[index].status =
                        if cancelled { "cancelled" } else { "error" }.into();
                    s.repositories[index].message = error.clone();
                });
                return Err(error);
            }
        }
    }
    update(&task.app, &plan.plan_id, |s| {
        s.status = if s
            .repositories
            .iter()
            .all(|r| ["success", "aborted"].contains(&r.status.as_str()))
        {
            "success"
        } else {
            "paused"
        }
        .into();
    });
    Ok("success")
}
pub fn abort(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    finish_worktree(app, plan_id, repo_id, WorktreeAction::Abort)
}
pub fn complete(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    finish_worktree(app, plan_id, repo_id, WorktreeAction::Complete)
}
pub fn resolve_pom(app: &AppHandle, plan_id: String, repo_id: String, side: PomSide) -> Result<()> {
    finish_worktree(app, plan_id, repo_id, WorktreeAction::ResolvePom(side))
}
pub fn resolve_pom_blocks(
    app: &AppHandle,
    plan_id: String,
    repo_id: String,
    choices: Vec<PomBlockChoice>,
) -> Result<()> {
    finish_worktree(
        app,
        plan_id,
        repo_id,
        WorktreeAction::ResolvePomBlocks(choices),
    )
}
enum WorktreeAction {
    Abort,
    Complete,
    ResolvePom(PomSide),
    ResolvePomBlocks(Vec<PomBlockChoice>),
}
fn operation_worktree_is_safe(
    base: &Path,
    operation: &Path,
    index: usize,
    worktree: &Path,
) -> bool {
    let index = index.to_string();
    (|| -> Option<bool> {
        let canonical_base = base.canonicalize().ok()?;
        let canonical_operation = operation.canonicalize().ok()?;
        let canonical_worktree = worktree.canonicalize().ok()?;
        Some(
            worktree == operation.join(&index)
                && canonical_operation == canonical_base.join(operation.file_name()?)
                && canonical_worktree == canonical_operation.join(&index),
        )
    })()
    .unwrap_or(false)
}
enum WorktreeIde {
    Idea,
    WebStorm,
}

impl WorktreeIde {
    fn name(&self) -> &'static str {
        match self {
            Self::Idea => "IntelliJ IDEA",
            Self::WebStorm => "WebStorm",
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn bundle_ids(&self) -> &'static [&'static str] {
        match self {
            Self::Idea => &["com.jetbrains.intellij", "com.jetbrains.intellij.ce"],
            Self::WebStorm => &["com.jetbrains.WebStorm"],
        }
    }
}

pub fn open_worktree_in_idea(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    open_worktree_in_ide(app, plan_id, repo_id, WorktreeIde::Idea)
}

pub fn open_worktree_in_webstorm(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    open_worktree_in_ide(app, plan_id, repo_id, WorktreeIde::WebStorm)
}

fn open_worktree_in_ide(
    app: &AppHandle,
    plan_id: String,
    repo_id: String,
    ide: WorktreeIde,
) -> Result<()> {
    let operations = tasks::data_dir(app)?.join("backend-worktrees");
    let runtime = app.state::<Runtime>();
    let entries = runtime.entries.lock().unwrap();
    let entry = entries.get(&plan_id).ok_or("计划不存在")?;
    let worktree = retained_worktree_for_ide(&operations, &entry.state, &repo_id)?;
    // 持有记录锁直到系统接收打开请求，避免完成或中止操作同时清理工作树。
    launch_worktree_ide(worktree, ide)
}

fn retained_worktree_for_ide<'a>(
    operations: &Path,
    state: &'a OperationState,
    repo_id: &str,
) -> Result<&'a Path> {
    if matches!(state.status.as_str(), "running" | "aborting") {
        return Err("请先等待任务结束".into());
    }
    let (index, result) = state
        .repositories
        .iter()
        .enumerate()
        .find(|(_, result)| result.repo_id == repo_id)
        .ok_or("仓库不在计划中")?;
    if result.worktree_path.is_empty() {
        return Err("该仓库没有本次操作工作区".into());
    }
    let worktree = Path::new(&result.worktree_path);
    let operation = operations.join(&state.plan_id);
    if !worktree.is_dir() || !operation_worktree_is_safe(operations, &operation, index, worktree) {
        return Err("无法确认本次工作区，拒绝打开".into());
    }
    Ok(worktree)
}

#[cfg(target_os = "macos")]
fn launch_worktree_ide(worktree: &Path, ide: WorktreeIde) -> Result<()> {
    for &bundle_id in ide.bundle_ids() {
        let output = Command::new("/usr/bin/open")
            .args(["-b", bundle_id, "--"])
            .arg(worktree)
            .output()
            .map_err(|error| format!("无法启动 macOS 打开命令：{error}"))?;
        if output.status.success() {
            return Ok(());
        }
    }
    let name = ide.name();
    Err(format!("无法在 {name} 中打开工作区，请确认已安装 {name}"))
}

#[cfg(not(target_os = "macos"))]
fn launch_worktree_ide(_worktree: &Path, ide: WorktreeIde) -> Result<()> {
    Err(format!("在 {} 中打开工作区目前仅支持 macOS", ide.name()))
}

fn auto_resume_indices(entry: &Entry) -> Result<Vec<usize>> {
    if entry
        .state
        .repositories
        .iter()
        .any(|r| !r.worktree_path.is_empty())
    {
        return Ok(Vec::new());
    }
    let indices: Vec<_> = entry
        .state
        .repositories
        .iter()
        .enumerate()
        .filter_map(|(index, result)| (result.status == "pending").then_some(index))
        .collect();
    // 没有远端身份的旧记录无法验证，保留 pending 供用户重新预检。
    if indices
        .iter()
        .any(|&index| entry.plan.repositories[index].remote_identity == 0)
    {
        return Err("原计划缺少远端身份，请选择「继续」重新预检".into());
    }
    Ok(indices)
}
fn record_worktree_outcome(
    entry: &mut Entry,
    index: usize,
    action: &WorktreeAction,
    outcome: &Result<bool>,
) -> Vec<usize> {
    entry.state.status = "paused".into();
    let result = &mut entry.state.repositories[index];
    match outcome {
        Ok(true) => {
            result.status = "error".into();
            result.message = "所选 POM 冲突块已暂存，请在工作区手工解决其余冲突".into();
            return Vec::new();
        }
        Err(error) => {
            result.status = "error".into();
            result.message = error.clone();
            return Vec::new();
        }
        Ok(false) => {}
    }
    let (status, message) = match action {
        WorktreeAction::Abort => ("aborted", "本次 merge 已中止，工作区已清理"),
        WorktreeAction::Complete => ("success", "手工提交已确认，工作区已清理；未自动推送"),
        WorktreeAction::ResolvePom(_) => ("success", "已按所选 POM 版本完成合并"),
        WorktreeAction::ResolvePomBlocks(_) => ("success", "已按所选 POM 冲突块完成合并"),
    };
    result.status = status.into();
    result.message = message.into();
    result.worktree_path.clear();
    if entry
        .state
        .repositories
        .iter()
        .all(|r| ["success", "aborted"].contains(&r.status.as_str()))
    {
        entry.state.status = "success".into();
        return Vec::new();
    }
    if matches!(action, WorktreeAction::Abort) {
        return Vec::new();
    }
    match auto_resume_indices(entry) {
        Ok(indices) => {
            if !indices.is_empty() {
                // 在同一次状态更新中接续，避免其他命令抢走暂停批次。
                entry.state.status = "running".into();
            }
            indices
        }
        Err(error) => {
            let message = &mut entry.state.repositories[index].message;
            message.push_str("。已暂停后续仓库：");
            message.push_str(&error);
            Vec::new()
        }
    }
}
fn finish_worktree(
    app: &AppHandle,
    plan_id: String,
    repo_id: String,
    action: WorktreeAction,
) -> Result<()> {
    let operations = tasks::data_dir(app)?.join("backend-worktrees");
    let (saved, worktree, index, project_id, pom_snapshot, blocks_snapshot, push, guard) = {
        let runtime = app.state::<Runtime>();
        let mut entries = runtime.entries.lock().unwrap();
        let entry = entries.get_mut(&plan_id).ok_or("计划不存在")?;
        if entry.state.status == "running" || entry.state.status == "aborting" {
            return Err("请先等待任务结束".into());
        }
        let index = entry
            .state
            .repositories
            .iter()
            .position(|r| r.repo_id == repo_id)
            .ok_or("仓库不在计划中")?;
        let result = &entry.state.repositories[index];
        if result.worktree_path.is_empty() {
            return Err("该仓库没有本次操作工作区".into());
        }
        let pom_snapshot = if matches!(action, WorktreeAction::ResolvePom(_)) {
            if entry.state.status != "paused"
                || entry.plan.operation != "merge"
                || result.pom_conflict.is_none()
            {
                return Err("当前没有可选择版本的 POM 冲突".into());
            }
            Some(
                entry
                    .pom_conflicts
                    .get(&repo_id)
                    .cloned()
                    .ok_or("缺少 POM 冲突快照，请手工处理")?,
            )
        } else {
            None
        };
        let blocks_snapshot = if matches!(action, WorktreeAction::ResolvePomBlocks(_)) {
            if entry.state.status != "paused"
                || entry.plan.operation != "merge"
                || result.pom_blocks_resolution.is_none()
            {
                return Err("当前没有可选择版本的 POM 冲突块".into());
            }
            Some(
                entry
                    .pom_blocks_snapshots
                    .get(&repo_id)
                    .cloned()
                    .ok_or("缺少 POM 冲突块快照，请手工处理")?,
            )
        } else {
            None
        };
        let value = (
            entry.plan.repositories[index].clone(),
            PathBuf::from(&result.worktree_path),
            index,
            entry.plan.project_id.clone(),
            pom_snapshot,
            blocks_snapshot,
            entry.plan.push,
            RepositoryGuard::acquire(&entry.plan.repositories[index].common_dir)?,
        );
        entry.state.status = "aborting".into();
        value
    };
    let operation = operations.join(&plan_id);
    if !operation_worktree_is_safe(&operations, &operation, index, &worktree) {
        update(app, &plan_id, |s| s.status = "paused".into());
        return Err("无法确认本次工作区，拒绝处理".into());
    }
    let id = plan_id.clone();
    let result = tasks::run_backend(app.clone(), project_id, move |task| {
        update(&task.app, &id, |s| s.task_id = task.id.clone());
        let outcome = if !operation_worktree_is_safe(&operations, &operation, index, &worktree) {
            Err("无法确认本次工作区，拒绝处理".into())
        } else {
            match &action {
                WorktreeAction::Abort => {
                    abort_worktree(&saved, &worktree, Some(task)).map(|()| false)
                }
                WorktreeAction::Complete => {
                    complete_worktree(&saved, &worktree, Some(task)).map(|()| false)
                }
                WorktreeAction::ResolvePom(side) => resolve_pom_worktree(
                    &saved,
                    &worktree,
                    pom_snapshot.as_ref().unwrap(),
                    *side,
                    push,
                    Some(task),
                )
                .map(|()| false),
                WorktreeAction::ResolvePomBlocks(choices) => resolve_pom_blocks_worktree(
                    &saved,
                    &worktree,
                    blocks_snapshot.as_ref().unwrap(),
                    choices,
                    push,
                    Some(task),
                ),
            }
        };
        drop(guard);
        let manual_remaining = matches!(outcome, Ok(true));
        let mut recovered = None;
        let files = if (outcome.is_err() || manual_remaining)
            && operation_worktree_is_safe(&operations, &operation, index, &worktree)
        {
            if outcome.is_err() {
                if let Some(expected) = &blocks_snapshot {
                    recovered = recover_pom_blocks(&saved, &worktree, expected, Some(task));
                }
            }
            if recovered.is_none() {
                read_pom_conflict_files(&saved, &worktree, Some(task)).unwrap_or_default()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let mut resume = None;
        update_entry(&task.app, &id, |entry| {
            let s = &mut entry.state;
            s.repositories[index].pom_conflict_files = files;
            if outcome.is_ok() || matches!(action, WorktreeAction::ResolvePom(_)) {
                entry.pom_conflicts.remove(&repo_id);
                s.repositories[index].pom_conflict = None;
            }
            if matches!(action, WorktreeAction::ResolvePomBlocks(_)) || outcome.is_ok() {
                if let (Some(files), Some(expected)) = (recovered, &blocks_snapshot) {
                    publish_pom_blocks(&mut s.repositories[index], files, expected, push);
                } else {
                    entry.pom_blocks_snapshots.remove(&repo_id);
                    s.repositories[index].pom_blocks_resolution = None;
                }
            }
            let indices = record_worktree_outcome(entry, index, &action, &outcome);
            if !indices.is_empty() {
                resume = Some((entry.plan.clone(), indices));
            }
        });
        if let Some((plan, indices)) = resume {
            // 先确认已清理的工作区和后续计划状态已落盘，再操作下一仓库。
            if let Err(error) = persist_current(&task.app, &id) {
                update(&task.app, &id, |s| s.status = "paused".into());
                return Err(error);
            }
            return run_plan_repositories(task, &plan, &operation, indices, true);
        }
        outcome.map(|_| "success")
    });
    if result.is_err() {
        update(app, &plan_id, |s| s.status = "paused".into());
    }
    result.map(|_| ())
}

fn publish_pom_blocks(
    result: &mut RepositoryResult,
    files: Vec<ScannedPomFile>,
    snapshot: &PomBlocksSnapshot,
    push: bool,
) {
    result.pom_conflict_files = files.into_iter().map(|file| file.preview).collect();
    result.pom_blocks_resolution = Some(PomBlocksResolution {
        push: push && !snapshot.manual_remaining,
        manual_remaining: snapshot.manual_remaining,
    });
}

fn recover_pom_blocks(
    saved: &PlanRepository,
    worktree: &Path,
    expected: &PomBlocksSnapshot,
    task: Option<&tasks::Task>,
) -> Option<Vec<ScannedPomFile>> {
    let mut saved = saved.clone();
    saved.remote_identity = expected.remote_identity;
    validate_merge_refs(
        &saved,
        worktree,
        merge_source(&saved),
        merge_target(&saved),
        task,
    )
    .ok()?;
    let (found, files) = inspect_pom_blocks(&saved, worktree, task).ok()?;
    (&found == expected).then_some(files)
}

// 返回 true 表示 POM 已暂存，其余冲突仍须手工处理。
fn resolve_pom_blocks_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    expected: &PomBlocksSnapshot,
    choices: &[PomBlockChoice],
    push: bool,
    task: Option<&tasks::Task>,
) -> Result<bool> {
    let mut saved = saved.clone();
    saved.remote_identity = expected.remote_identity;
    let files = recover_pom_blocks(&saved, worktree, expected, task)
        .ok_or("POM 冲突或工作区已变化，请手工处理")?;
    let mut sides = HashMap::new();
    for choice in choices {
        if sides
            .insert((choice.path.as_str(), choice.line), choice.side)
            .is_some()
        {
            return Err("每个 POM 冲突块只能选择一次".into());
        }
    }
    let missing = "必须为每个实际 POM 冲突块选择一个版本";
    let mut resolved = Vec::new();
    for file in &files {
        let path = file.preview.path.as_str();
        let mut output = String::with_capacity(file.content.len());
        let mut offset = 0;
        for (block, range) in file.preview.blocks.iter().zip(&file.ranges) {
            let side = sides.remove(&(path, block.line)).ok_or(missing)?;
            output.push_str(&file.content[offset..range.start]);
            output.push_str(match side {
                PomSide::Test => &block.test_content,
                PomSide::Development => &block.development_content,
            });
            offset = range.end;
        }
        output.push_str(&file.content[offset..]);
        resolved.push((path, output));
    }
    if !sides.is_empty() {
        return Err(missing.into());
    }
    let index = staged_index(worktree, task)?;
    let mut remaining = unmerged_paths(&index)?;
    let root = worktree.canonicalize().map_err(|_| "无法确认 POM 工作区")?;
    let changed = "POM 冲突或工作区已变化，请手工处理";
    if identity(&index) != expected.index_identity {
        return Err(changed.into());
    }
    // 先验完所有选择和文件再写入，避免错误请求留下部分应用的结果。
    for file in &files {
        let path = file.preview.path.as_str();
        let disk_path = root.join(path);
        if disk_path.canonicalize().ok().as_ref() != Some(&disk_path)
            || fs::read(&disk_path).map_err(|_| changed)? != file.content.as_bytes()
        {
            return Err(changed.into());
        }
        remaining.remove(path.as_bytes());
    }
    if let Some(task) = task {
        task.check()?;
    }
    let mut args = vec!["--literal-pathspecs", "add", "--"];
    for (path, content) in &resolved {
        crate::config::atomic_write(&root.join(path), content)
            .map_err(|_| "无法写入 POM，工作区已保留")?;
        args.push(path);
    }
    run(worktree, &args, task)?;
    if unmerged_paths(&staged_index(worktree, task)?)? != remaining {
        return Err("暂存后的冲突列表已变化，工作区已保留".into());
    }
    if expected.manual_remaining {
        return Ok(true);
    }
    let tree = run(worktree, &["write-tree"], task)?;
    // 使用正常合并提交，核对钩子执行后的提交树及合并父提交再继续推送。
    run(worktree, &["commit", "--no-edit"], task)?;
    if run(worktree, &["rev-parse", "--verify", "HEAD^{tree}"], task)? != tree
        || sha(worktree, "HEAD^1")? != merge_target(&saved)
        || sha(worktree, "HEAD^2")? != merge_source(&saved)
    {
        return Err("提交后的合并内容已变化，工作区已保留".into());
    }
    finish_merged_worktree(&saved, worktree, push, task)?;
    Ok(false)
}

fn resolve_pom_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    expected: &PomConflictSnapshot,
    side: PomSide,
    push: bool,
    task: Option<&tasks::Task>,
) -> Result<()> {
    let mut saved = saved.clone();
    saved.remote_identity = expected.remote_identity;
    validate_merge_refs(
        &saved,
        worktree,
        merge_source(&saved),
        merge_target(&saved),
        task,
    )?;
    let (preview, found) = inspect_pom_conflict(&saved, worktree, task)?;
    if &found != expected {
        return Err("POM 冲突或工作区已变化，请手工处理".into());
    }
    let (checkout_side, index_stage) = match side {
        PomSide::Test => ("--ours", 2),
        PomSide::Development => ("--theirs", 3),
    };
    let confirmed_blob = run(
        worktree,
        &[
            "rev-parse",
            "--verify",
            &format!(":{index_stage}:{}", preview.path),
        ],
        task,
    )?;
    run(
        worktree,
        &[
            "--literal-pathspecs",
            "checkout",
            checkout_side,
            "--",
            &preview.path,
        ],
        task,
    )?;
    run(
        worktree,
        &["--literal-pathspecs", "add", "--", &preview.path],
        task,
    )?;
    if run(
        worktree,
        &["rev-parse", "--verify", &format!(":{}", preview.path)],
        task,
    )? != confirmed_blob
        || !run(worktree, &["ls-files", "--unmerged", "-z"], task)?.is_empty()
    {
        return Err("暂存内容与所选 POM 版本不符，请手工处理".into());
    }
    // 使用正常合并提交，保留仓库的提交钩子和签名规则。
    run(worktree, &["commit", "--no-edit"], task)?;
    if run(
        worktree,
        &["rev-parse", "--verify", &format!("HEAD:{}", preview.path)],
        task,
    )? != confirmed_blob
        || sha(worktree, "HEAD^1")? != merge_target(&saved)
        || sha(worktree, "HEAD^2")? != merge_source(&saved)
    {
        return Err("提交后的 POM 与所选版本不符，工作区已保留".into());
    }
    finish_merged_worktree(&saved, worktree, push, task)
}
fn validate_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<()> {
    // 验证分支、Git common-dir 和根身份，永不触及用户原仓库。
    if root(worktree)? != canonical(worktree)?
        || canonical(worktree)? == saved.root
        || branch(worktree) != saved.target_branch
    {
        return Err("工作区身份不匹配，拒绝 abort".into());
    }
    let common = run(worktree, &["rev-parse", "--git-common-dir"], task)?;
    if canonical(&worktree.join(common))? != saved.common_dir {
        return Err("工作区不属于本次仓库".into());
    }
    Ok(())
}
fn complete_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<()> {
    validate_worktree(saved, worktree, task)?;
    for name in ["MERGE_HEAD", "rebase-merge", "rebase-apply"] {
        let marker = run(worktree, &["rev-parse", "--git-path", name], task)?;
        if worktree.join(marker).exists() {
            return Err("请先手工解决冲突并提交，当前 merge/rebase 尚未结束".into());
        }
    }
    if !run(worktree, &["status", "--porcelain=v1"], task)?.is_empty() {
        return Err("工作区仍有未提交修改，已保留".into());
    }
    let head = sha(worktree, "HEAD")?;
    for baseline in [&saved.source_sha, &saved.target_sha]
        .into_iter()
        .chain(saved.sync_test_branch.then_some(&saved.remote_sha))
        .chain(
            saved
                .sync_development_branch
                .then_some(&saved.development_remote_sha),
        )
    {
        run(
            worktree,
            &["merge-base", "--is-ancestor", baseline, &head],
            task,
        )?;
    }
    run(
        Path::new(&saved.root),
        &[
            "worktree",
            "remove",
            "--",
            worktree.to_str().ok_or("路径编码无效")?,
        ],
        task,
    )
    .map(|_| ())
}
fn abort_worktree(
    saved: &PlanRepository,
    worktree: &Path,
    task: Option<&tasks::Task>,
) -> Result<()> {
    validate_worktree(saved, worktree, task)?;
    // 仅允许准备时基线或本次确认的快进基线；中止开发合并不撤销远端同步。
    // 合并已提交或用户手工移动过目标时，不把“移除工作区”冒充撤销合并。
    let head = sha(worktree, "HEAD")?;
    if head != saved.target_sha && !(saved.sync_test_branch && head == saved.remote_sha) {
        return Err("测试分支已有新提交，不能自动中止；请确认后完成处理，提交不会被撤销".into());
    }
    let merge_head = run(worktree, &["rev-parse", "--git-path", "MERGE_HEAD"], task)?;
    if worktree.join(merge_head).exists() {
        // 开发快进已留在原仓库，只允许中止本次确认来源的测试合并。
        if run(worktree, &["rev-parse", "MERGE_HEAD"], task)? != merge_source(saved) {
            return Err("合并来源已变化，拒绝中止；工作区已保留".into());
        }
        run(worktree, &["merge", "--abort"], task)?;
    }
    if !run(worktree, &["status", "--porcelain=v1"], task)?.is_empty() {
        return Err("工作区含额外修改，已保留".into());
    }
    run(
        Path::new(&saved.root),
        &[
            "worktree",
            "remove",
            "--",
            worktree.to_str().ok_or("路径编码无效")?,
        ],
        task,
    )
    .map(|_| ())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{tempdir, TempDir};
    fn git(path: &Path, args: &[&str]) -> String {
        let output = git_command(path)
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().into()
    }
    fn fixture() -> (TempDir, Repository) {
        let dir = tempdir().unwrap();
        git(dir.path(), &["init", "--initial-branch=dev", "--template="]);
        git(dir.path(), &["config", "user.name", "Fixture"]);
        git(
            dir.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(dir.path(), &["config", "commit.gpgSign", "false"]);
        fs::write(dir.path().join("file"), "base\n").unwrap();
        git(dir.path(), &["add", "file"]);
        git(dir.path(), &["commit", "-m", "base"]);
        git(dir.path(), &["branch", "test"]);
        let repo = Repository {
            id: "r".into(),
            name: "r".into(),
            path: dir.path().to_string_lossy().into(),
            remote: "origin".into(),
            scm_git_id: String::new(),
            scm_module_name: String::new(),
            development_branch: "dev".into(),
            test_branch: "test".into(),
        };
        (dir, repo)
    }
    // 所有分支及远端均为本地临时 fixture，不触及用户仓库。
    fn push_fixture(existing: bool) -> (TempDir, Repository, TempDir) {
        let (dir, repo) = fixture();
        let remote = tempdir().unwrap();
        git(remote.path(), &["init", "--bare", "--template="]);
        git(
            dir.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        if existing {
            git(dir.path(), &["push", "origin", "dev", "test"]);
        }
        fetch_fixture(&repo);
        (dir, repo, remote)
    }
    fn fetch_fixture(repo: &Repository) {
        git(
            Path::new(&repo.path),
            &[
                "fetch",
                "--prune",
                "--no-tags",
                "origin",
                "+refs/heads/*:refs/remotes/origin/*",
            ],
        );
    }
    fn switch_sync_fixture(operation: &str) -> (TempDir, Repository, TempDir, String) {
        let (dir, repo, remote) = push_fixture(true);
        let target = if operation == "switch-master" {
            "master"
        } else {
            "test"
        };
        if target == "master" {
            git(dir.path(), &["branch", target]);
            git(dir.path(), &["push", "origin", target]);
        }
        (dir, repo, remote, target.into())
    }
    fn child_commit(path: &Path, parent: &str, message: &str) -> String {
        git(
            path,
            &[
                "commit-tree",
                &format!("{parent}^{{tree}}"),
                "-p",
                parent,
                "-m",
                message,
            ],
        )
    }
    #[test]
    fn pull_current_uses_configured_remote_and_preserves_local_commits() {
        let (dir, mut repo, remote) = push_fixture(true);
        repo.development_branch.clear();
        repo.test_branch.clear();
        let path = dir.path();
        git(path, &["switch", "-c", "feature", "--track", "origin/test"]);
        let base = sha(path, "HEAD").unwrap();
        let tip = child_commit(remote.path(), &base, "remote advance");
        git(remote.path(), &["update-ref", "refs/heads/test", &tip]);
        let pull = || {
            let plan = snapshot(&repo, "pull-current")?;
            perform(&plan, "pull-current", false, path, None, || panic!())
        };
        pull().unwrap();
        assert_eq!(branch(path), "feature");
        assert_eq!(sha(path, "HEAD").unwrap(), tip);
        change(path, "local commit");
        let ahead = sha(path, "HEAD").unwrap();
        pull().unwrap();
        assert_eq!(sha(path, "HEAD").unwrap(), ahead);
        assert_eq!(sha(remote.path(), "refs/heads/test").unwrap(), tip);
        let other = child_commit(remote.path(), &tip, "diverged");
        git(remote.path(), &["update-ref", "refs/heads/test", &other]);
        assert!(snapshot(&repo, "pull-current").is_err());
        assert_eq!(sha(path, "HEAD").unwrap(), ahead);
    }

    #[test]
    fn pull_current_batch_uses_each_checkout_and_preserves_other_branches() {
        let (one, first, first_remote) = push_fixture(true);
        let (two, mut second, second_remote) = push_fixture(true);
        second.id = "second".into();
        git(two.path(), &["switch", "test"]);
        for (remote, name) in [(&first_remote, "dev"), (&second_remote, "test")] {
            let reference = format!("refs/heads/{name}");
            let base = sha(remote.path(), &reference).unwrap();
            let tip = child_commit(remote.path(), &base, "advance");
            git(remote.path(), &["update-ref", &reference, &tip]);
        }
        let plans = preflight(&[first, second], "pull-current").unwrap();
        for (plan, path, name, other) in [
            (&plans[0], one.path(), "dev", "test"),
            (&plans[1], two.path(), "test", "dev"),
        ] {
            assert_eq!(plan.current_branch, name);
            let untouched = sha(path, other).unwrap();
            perform(plan, "pull-current", false, path, None, || panic!()).unwrap();
            assert_eq!(branch(path), name);
            assert_eq!(sha(path, "HEAD").unwrap(), plan.remote_sha);
            assert_eq!(sha(path, other).unwrap(), untouched);
        }
    }

    #[test]
    fn pull_current_rechecks_confirmation_and_rejects_unsafe_states() {
        let (dir, repo, remote) = push_fixture(true);
        let path = dir.path();
        let saved = snapshot(&repo, "pull-current").unwrap();
        let guard = RepositoryGuard::acquire(&saved.common_dir).unwrap();
        assert!(
            perform(&saved, "pull-current", false, path, None, || panic!())
                .unwrap_err()
                .contains("其他 Git 操作")
        );
        drop(guard);
        assert!(validate_options("pull-current", true, false).is_err());
        assert!(validate_options("pull-current", false, true).is_err());
        let tip = child_commit(remote.path(), &saved.source_sha, "after confirmation");
        git(remote.path(), &["update-ref", "refs/heads/dev", &tip]);
        assert!(perform(&saved, "pull-current", false, path, None, || panic!()).is_err());
        assert_eq!(sha(path, "HEAD").unwrap(), saved.source_sha);
        let saved = snapshot(&repo, "pull-current").unwrap();
        fs::write(path.join("untracked"), "edit").unwrap();
        assert!(revalidate(&saved, "pull-current").is_err());
        fs::remove_file(path.join("untracked")).unwrap();
        fs::write(path.join(".git/MERGE_HEAD"), &tip).unwrap();
        assert!(snapshot(&repo, "pull-current").is_err());
        fs::remove_file(path.join(".git/MERGE_HEAD")).unwrap();
        git(path, &["switch", "--detach"]);
        assert!(snapshot(&repo, "pull-current").is_err());
        git(path, &["switch", "dev"]);
        git(path, &["config", "branch.dev.remote", "other"]);
        git(path, &["config", "branch.dev.merge", "refs/heads/dev"]);
        assert!(snapshot(&repo, "pull-current").is_err());
        git(path, &["config", "branch.dev.remote", "origin"]);
        git(remote.path(), &["update-ref", "-d", "refs/heads/dev"]);
        assert!(snapshot(&repo, "pull-current").is_err());
    }

    #[test]
    fn switch_sync_fetches_latest_and_preserves_ahead_or_diverged_history() {
        for operation in ["switch-test", "switch-master"] {
            for relation in ["behind", "ahead", "diverged"] {
                let (dir, repo, remote, target) = switch_sync_fixture(operation);
                let base = sha(dir.path(), "HEAD").unwrap();
                let local = child_commit(dir.path(), &base, "local");
                let upstream = child_commit(dir.path(), &base, "upstream");
                if relation != "behind" {
                    git(
                        dir.path(),
                        &["update-ref", &format!("refs/heads/{target}"), &local],
                    );
                }
                let saved = snapshot(&repo, operation).unwrap();
                // 预检后远端才更新，执行必须重新 fetch，不能只使用确认时的 tracking ref。
                if relation != "ahead" {
                    git(
                        dir.path(),
                        &[
                            "push",
                            remote.path().to_str().unwrap(),
                            &format!("{upstream}:refs/heads/{target}"),
                        ],
                    );
                }
                let result = perform(
                    &saved,
                    operation,
                    false,
                    &remote.path().join("unused"),
                    None,
                    || Ok(()),
                );
                assert_eq!(branch(dir.path()), target);
                if relation == "diverged" {
                    let error = result.unwrap_err();
                    assert!(
                        error.contains("分叉")
                            && error.contains("已切换")
                            && error.contains("无法确认远端同步成功"),
                        "{error}"
                    );
                    assert_eq!(sha(dir.path(), "HEAD").unwrap(), local);
                    assert!(!dir.path().join(".git/MERGE_HEAD").exists());
                } else {
                    result.unwrap();
                    assert_eq!(
                        sha(dir.path(), "HEAD").unwrap(),
                        if relation == "behind" {
                            upstream
                        } else {
                            local
                        }
                    );
                }
            }
        }
    }
    #[test]
    fn switch_sync_accepts_remote_tracking_updates_but_not_new_local_targets() {
        for operation in ["switch-test", "switch-master"] {
            for local_exists in [false, true] {
                let (dir, repo, remote, target) = switch_sync_fixture(operation);
                if !local_exists {
                    git(dir.path(), &["branch", "-D", &target]);
                }
                let saved = snapshot(&repo, operation).unwrap();
                let next = child_commit(dir.path(), &saved.target_sha, "remote advanced");
                git(
                    dir.path(),
                    &["push", "origin", &format!("{next}:refs/heads/{target}")],
                );
                fetch_fixture(&repo);
                if !local_exists {
                    git(dir.path(), &["branch", &target, &next]);
                    assert!(revalidate(&saved, operation).is_err());
                    git(dir.path(), &["branch", "-D", &target]);
                }
                perform(
                    &saved,
                    operation,
                    false,
                    &remote.path().join("unused"),
                    None,
                    || Ok(()),
                )
                .unwrap();
                assert_eq!(branch(dir.path()), target);
                assert_eq!(sha(dir.path(), "HEAD").unwrap(), next);
            }
        }
    }
    #[test]
    fn switch_sync_reports_fetch_failure_and_deleted_remote_branch() {
        for operation in ["switch-test", "switch-master"] {
            for deleted in [false, true] {
                let (dir, repo, remote, target) = switch_sync_fixture(operation);
                let saved = snapshot(&repo, operation).unwrap();
                if deleted {
                    git(
                        remote.path(),
                        &["update-ref", "-d", &format!("refs/heads/{target}")],
                    );
                } else {
                    // URL 身份不变，但服务端已不可读取。
                    fs::rename(
                        remote.path().join("HEAD"),
                        remote.path().join("hidden-head"),
                    )
                    .unwrap();
                }
                let error = perform(
                    &saved,
                    operation,
                    false,
                    &remote.path().join("unused"),
                    None,
                    || Ok(()),
                )
                .unwrap_err();
                assert!(
                    error.contains("已切换") && error.contains("无法确认远端同步成功"),
                    "{error}"
                );
                if deleted {
                    assert!(error.contains("远端目标分支不存在"), "{error}");
                }
                assert_eq!(branch(dir.path()), target);
                assert_eq!(sha(dir.path(), "HEAD").unwrap(), saved.target_sha);
            }
        }
    }
    #[test]
    fn switch_sync_rejects_all_dirty_states_before_and_after_preflight() {
        for operation in ["switch-test", "switch-master"] {
            for kind in ["unstaged", "staged", "untracked"] {
                let (dir, repo, remote, _) = switch_sync_fixture(operation);
                let saved = snapshot(&repo, operation).unwrap();
                let file = if kind == "untracked" {
                    "new-file"
                } else {
                    "file"
                };
                fs::write(dir.path().join(file), "user edit\n").unwrap();
                if kind == "staged" {
                    git(dir.path(), &["add", file]);
                }
                assert!(snapshot(&repo, operation).is_err());
                assert!(perform(
                    &saved,
                    operation,
                    false,
                    &remote.path().join("unused"),
                    None,
                    || Ok(())
                )
                .is_err());
                assert_eq!(branch(dir.path()), "dev");
                assert_eq!(
                    fs::read_to_string(dir.path().join(file)).unwrap(),
                    "user edit\n"
                );
            }
        }
    }
    #[cfg(unix)]
    #[test]
    fn switch_sync_stops_when_local_state_changes_during_fetch() {
        use std::os::unix::fs::PermissionsExt;
        for operation in ["switch-test", "switch-master"] {
            for changed in [
                "unstaged",
                "staged",
                "untracked",
                "head",
                "branch",
                "remote",
            ] {
                let (dir, repo, remote, target) = switch_sync_fixture(operation);
                let saved = snapshot(&repo, operation).unwrap();
                let next = child_commit(dir.path(), &saved.target_sha, "upstream");
                git(
                    dir.path(),
                    &[
                        "push",
                        remote.path().to_str().unwrap(),
                        &format!("{next}:refs/heads/{target}"),
                    ],
                );
                let mutation = match changed {
                    "unstaged" => "printf 'user edit\\n' > file".to_string(),
                    "staged" => "printf 'user edit\\n' > file; git add file".to_string(),
                    "untracked" => "printf 'user edit\\n' > new-file".to_string(),
                    "head" => format!("git update-ref refs/heads/{target} {next}"),
                    "branch" => "git symbolic-ref HEAD refs/heads/dev".to_string(),
                    _ => "git remote set-url origin /missing-remote".to_string(),
                };
                // 本地 upload-pack 在 fetch 进行中修改调用方现场，稳定覆盖网络等待窗口。
                let script = remote.path().join("upload-pack-fixture");
                let quoted_path = repo.path.replace('\'', "'\\''");
                fs::write(&script, format!("#!/bin/sh\n(cd '{quoted_path}' && {mutation}) || exit 1\nexec git-upload-pack \"$@\"\n")).unwrap();
                fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
                git(
                    dir.path(),
                    &[
                        "config",
                        "remote.origin.uploadpack",
                        script.to_str().unwrap(),
                    ],
                );
                let error = perform(
                    &saved,
                    operation,
                    false,
                    &remote.path().join("unused"),
                    None,
                    || Ok(()),
                )
                .unwrap_err();
                assert!(
                    error.contains("已切换") && error.contains("无法确认远端同步成功"),
                    "{changed}: {error}"
                );
                if ["unstaged", "staged", "untracked"].contains(&changed) {
                    let file = if changed == "untracked" {
                        "new-file"
                    } else {
                        "file"
                    };
                    assert_eq!(
                        fs::read_to_string(dir.path().join(file)).unwrap(),
                        "user edit\n"
                    );
                }
                assert_eq!(
                    sha(dir.path(), &format!("refs/heads/{target}")).unwrap(),
                    if changed == "head" {
                        next
                    } else {
                        saved.target_sha
                    }
                );
                assert!(!dir.path().join(".git/MERGE_HEAD").exists());
            }
        }
    }
    fn push_saved(saved: &PlanRepository, worktree: &Path) -> Result<()> {
        perform(saved, "push-dev", true, worktree, None, || {
            panic!("push-dev must not create a worktree")
        })
    }
    #[test]
    fn push_dev_updates_only_confirmed_development_and_creates_missing_remote_branch() {
        for existing in [false, true] {
            let (dir, repo, remote) = push_fixture(existing);
            let old_test = sha(dir.path(), "refs/heads/test").unwrap();
            change(dir.path(), "development\n");
            git(
                dir.path(),
                &["tag", "-a", "extra-tag", "-m", "must not push"],
            );
            git(dir.path(), &["config", "push.followTags", "true"]);
            git(
                dir.path(),
                &[
                    "config",
                    "remote.origin.push",
                    "refs/heads/test:refs/heads/unwanted",
                ],
            );
            git(dir.path(), &["config", "push.autoSetupRemote", "true"]);
            let before_worktrees = git(dir.path(), &["worktree", "list", "--porcelain"]);
            let saved = preflight(&[repo.clone()], "push-dev").unwrap().remove(0);
            assert_eq!(saved.target_branch, "dev");
            assert_eq!(saved.remote_sha.is_empty(), !existing);
            assert_eq!(saved.target_sha, saved.remote_sha);
            assert_eq!(saved.source_sha, sha(dir.path(), "HEAD").unwrap());
            assert!(!saved.create_tracking);
            let worktree = dir.path().join("must-not-exist");
            push_saved(&saved, &worktree).unwrap();
            assert!(!worktree.exists());
            assert_eq!(branch(dir.path()), "dev");
            assert_eq!(
                git(dir.path(), &["worktree", "list", "--porcelain"]),
                before_worktrees
            );
            assert_eq!(
                sha(remote.path(), "refs/heads/dev").unwrap(),
                saved.source_sha
            );
            assert_eq!(
                git(remote.path(), &["for-each-ref", "--format=%(refname)"]),
                if existing {
                    "refs/heads/dev\nrefs/heads/test"
                } else {
                    "refs/heads/dev"
                }
            );
            if existing {
                assert_eq!(sha(remote.path(), "refs/heads/test").unwrap(), old_test);
            }
            assert!(run(dir.path(), &["config", "--get", "branch.dev.remote"], None).is_err());
        }
    }
    #[test]
    fn push_dev_rejects_invalid_repository_states_and_duplicate_identity() {
        let (dir, mut repo, _remote) = push_fixture(true);
        let mut duplicate = repo.clone();
        duplicate.id = "duplicate".into();
        assert!(preflight(&[repo.clone(), duplicate], "push-dev").is_err());
        git(dir.path(), &["switch", "test"]);
        assert!(snapshot(&repo, "push-dev").is_err());
        git(dir.path(), &["branch", "-D", "dev"]);
        assert!(snapshot(&repo, "push-dev").is_err());
        git(dir.path(), &["switch", "-c", "dev"]);
        fs::write(dir.path().join("untracked"), "dirty").unwrap();
        assert!(snapshot(&repo, "push-dev").is_err());
        fs::remove_file(dir.path().join("untracked")).unwrap();
        for marker in ["MERGE_HEAD", "rebase-merge", "rebase-apply"] {
            let path = dir.path().join(".git").join(marker);
            fs::write(&path, "busy").unwrap();
            assert!(snapshot(&repo, "push-dev").is_err());
            fs::remove_file(path).unwrap();
        }
        repo.remote = "missing".into();
        assert!(snapshot(&repo, "push-dev").is_err());
        repo.remote = "../invalid".into();
        assert!(snapshot(&repo, "push-dev").is_err());
        repo.remote = "origin".into();
        git(dir.path(), &["remote", "remove", "origin"]);
        assert!(snapshot(&repo, "push-dev").is_err());
    }
    #[test]
    fn push_dev_rejects_remote_ahead_and_divergence() {
        for diverged in [false, true] {
            let (dir, repo, remote) = push_fixture(true);
            let base = sha(dir.path(), "HEAD").unwrap();
            change(dir.path(), "remote commit\n");
            git(dir.path(), &["push", "origin", "dev"]);
            git(dir.path(), &["reset", "--hard", &base]);
            if diverged {
                change(dir.path(), "local commit\n");
            }
            fetch_fixture(&repo);
            assert!(snapshot(&repo, "push-dev")
                .unwrap_err()
                .contains("领先或已分叉"));
            assert_ne!(
                sha(remote.path(), "refs/heads/dev").unwrap(),
                sha(dir.path(), "HEAD").unwrap()
            );
        }
    }
    #[test]
    fn push_dev_revalidates_edits_source_remote_and_root_identity() {
        for mutation in [
            "dirty",
            "source",
            "remote-ref",
            "remote-url",
            "push-url",
            "root",
        ] {
            let (dir, repo, remote) = push_fixture(true);
            change(dir.path(), "approved\n");
            let saved = snapshot(&repo, "push-dev").unwrap();
            let remote_before = sha(remote.path(), "refs/heads/dev").unwrap();
            let other = tempdir().unwrap();
            match mutation {
                "dirty" => fs::write(dir.path().join("untracked"), "new edits").unwrap(),
                "source" => change(dir.path(), "unapproved\n"),
                "remote-ref" => {
                    git(dir.path(), &["push", "origin", "dev"]);
                }
                "remote-url" => {
                    git(
                        dir.path(),
                        &[
                            "remote",
                            "set-url",
                            "origin",
                            other.path().to_str().unwrap(),
                        ],
                    );
                }
                "push-url" => {
                    git(
                        dir.path(),
                        &[
                            "remote",
                            "set-url",
                            "--push",
                            "origin",
                            other.path().to_str().unwrap(),
                        ],
                    );
                }
                "root" => {
                    fs::rename(dir.path().join(".git"), other.path().join("original-git")).unwrap();
                    git(dir.path(), &["init", "--initial-branch=dev", "--template="]);
                    git(dir.path(), &["add", "file"]);
                    git(dir.path(), &["commit", "-m", "replacement"]);
                    git(
                        dir.path(),
                        &["remote", "add", "origin", remote.path().to_str().unwrap()],
                    );
                }
                _ => unreachable!(),
            }
            assert!(
                push_saved(&saved, &dir.path().join("no-worktree")).is_err(),
                "{mutation}"
            );
            let expected = if mutation == "remote-ref" {
                &saved.source_sha
            } else {
                &remote_before
            };
            assert_eq!(&sha(remote.path(), "refs/heads/dev").unwrap(), expected);
            assert!(!dir.path().join("no-worktree").exists());
        }
    }
    #[test]
    fn push_dev_rejects_options_before_any_repository_change() {
        let (dir, repo, remote) = push_fixture(false);
        let saved = snapshot(&repo, "push-dev").unwrap();
        for (push, pom) in [(false, false), (false, true), (true, true)] {
            assert!(validate_options("push-dev", push, pom).is_err());
            assert!(perform_with_options(
                &saved,
                "push-dev",
                push,
                pom,
                &dir.path().join("no-worktree"),
                None,
                || panic!("no worktree")
            )
            .is_err());
        }
        assert!(validate_options("switch", true, false).is_err());
        assert!(validate_options("switch-test", true, false).is_err());
        assert!(sha(remote.path(), "refs/heads/dev").is_err());
        assert_eq!(branch(dir.path()), "dev");
    }
    #[test]
    fn push_dev_rejects_multiple_and_inconsistent_push_urls() {
        let (dir, repo, remote) = push_fixture(true);
        let other = tempdir().unwrap();
        git(
            dir.path(),
            &[
                "config",
                "remote.origin.pushurl",
                other.path().to_str().unwrap(),
            ],
        );
        assert!(snapshot(&repo, "push-dev")
            .unwrap_err()
            .contains("fetch/push URL"));
        git(
            dir.path(),
            &[
                "config",
                "remote.origin.pushurl",
                remote.path().to_str().unwrap(),
            ],
        );
        assert!(snapshot(&repo, "push-dev").is_ok());
        git(
            dir.path(),
            &[
                "config",
                "--add",
                "remote.origin.pushurl",
                remote.path().to_str().unwrap(),
            ],
        );
        assert!(snapshot(&repo, "push-dev")
            .unwrap_err()
            .contains("fetch/push URL"));
    }
    #[test]
    fn push_dev_remote_query_cancels_before_and_after_git_starts() {
        use std::cell::Cell;
        let (dir, repo, remote) = push_fixture(true);
        let expected = sha(remote.path(), "refs/heads/dev").unwrap();
        // 第一次检查在读取配置前，第三次检查在 ls-remote 进程启动后的轮询中。
        for cancel_at in [1, 3] {
            let checks = Cell::new(0);
            let started = Instant::now();
            let result = push_remote_sha(&repo, || {
                checks.set(checks.get() + 1);
                if checks.get() >= cancel_at {
                    Err("任务已取消".into())
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.unwrap_err(), "任务已取消");
            assert_eq!(checks.get(), cancel_at);
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(sha(remote.path(), "refs/heads/dev").unwrap(), expected);
            assert!(git(dir.path(), &["status", "--porcelain=v1"]).is_empty());
        }
    }
    #[test]
    fn push_dev_rejects_mirror_remote_configuration() {
        let (dir, repo, remote) = push_fixture(true);
        git(dir.path(), &["config", "remote.origin.mirror", "true"]);
        assert!(snapshot(&repo, "push-dev").unwrap_err().contains("mirror"));
        assert_eq!(
            sha(remote.path(), "refs/heads/dev").unwrap(),
            sha(dir.path(), "HEAD").unwrap()
        );
    }
    #[cfg(unix)]
    #[test]
    fn push_dev_preserves_pre_push_hook_and_pins_confirmed_sha() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, repo, remote) = push_fixture(true);
        change(dir.path(), "approved\n");
        let saved = snapshot(&repo, "push-dev").unwrap();
        let hooks = dir.path().join("custom-hooks");
        fs::create_dir(&hooks).unwrap();
        git(
            dir.path(),
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        // hooks 目录位于仓库内但必须不影响干净状态。
        fs::create_dir_all(dir.path().join(".git/info")).unwrap();
        fs::write(dir.path().join(".git/info/exclude"), "custom-hooks/\n").unwrap();
        let hook = hooks.join("pre-push");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(push_saved(&saved, &dir.path().join("no-worktree")).is_err());
        assert_eq!(
            sha(remote.path(), "refs/heads/dev").unwrap(),
            saved.remote_sha
        );
        change(dir.path(), "later commit\n");
        let later = sha(dir.path(), "HEAD").unwrap();
        git(dir.path(), &["reset", "--hard", &saved.source_sha]);
        // 在最后一次重验后更新本地 ref，远端仍只能收到计划确认的 SHA。
        fs::write(
            &hook,
            format!("#!/bin/sh\ngit update-ref refs/heads/dev {later}\n"),
        )
        .unwrap();
        push_saved(&saved, &dir.path().join("no-worktree")).unwrap();
        assert_eq!(sha(dir.path(), "HEAD").unwrap(), later);
        assert_eq!(
            sha(remote.path(), "refs/heads/dev").unwrap(),
            saved.source_sha
        );
    }
    fn pom_fixture(
        paths: &[&str],
        code_conflict: bool,
        delete_test: bool,
    ) -> (TempDir, Repository) {
        let (dir, repo) = fixture();
        for path in paths {
            let file = dir.path().join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "base pom\n").unwrap();
        }
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "-m", "base poms"]);
        git(dir.path(), &["branch", "-f", "test"]);
        git(dir.path(), &["switch", "test"]);
        for path in paths {
            if delete_test {
                fs::remove_file(dir.path().join(path)).unwrap();
            } else {
                fs::write(dir.path().join(path), "test pom\n").unwrap();
            }
        }
        if code_conflict {
            fs::write(dir.path().join("file"), "test code\n").unwrap();
        }
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "-m", "test poms"]);
        git(dir.path(), &["switch", "dev"]);
        for path in paths {
            fs::write(dir.path().join(path), "development pom\n").unwrap();
        }
        fs::write(dir.path().join("file"), "development code\n").unwrap();
        git(dir.path(), &["add", "--all"]);
        git(dir.path(), &["commit", "-m", "development poms"]);
        (dir, repo)
    }

    #[test]
    fn pom_conflicts_are_untouched_by_default() {
        let (_dir, repo) = pom_fixture(&["pom.xml"], false, false);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(()))
            .unwrap_err()
            .contains("手工合并代码"));
        assert_eq!(
            git(&worktree, &["diff", "--name-only", "--diff-filter=U"]),
            "pom.xml"
        );
        assert!(fs::read_to_string(worktree.join("pom.xml"))
            .unwrap()
            .contains("<<<<<<<"));
    }

    #[test]
    fn pom_conflict_allows_clean_sides_when_base_has_historical_markers() {
        let (dir, repo) = fixture();
        let pom = dir.path().join("pom.xml");
        let commit_pom = |content: &str| {
            fs::write(&pom, content).unwrap();
            git(dir.path(), &["add", "pom.xml"]);
            git(dir.path(), &["commit", "-m", "update pom"]);
        };
        commit_pom("<<<<<<< HEAD\nold test\n=======\nold dev\n>>>>>>> dev\n");
        git(dir.path(), &["branch", "-f", "test"]);
        git(dir.path(), &["switch", "test"]);
        commit_pom("<version>1-test</version>\n");
        git(dir.path(), &["switch", "dev"]);
        commit_pom("<version>2-dev</version>\n");

        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        let files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].blocks.len(), 1);
        let (preview, snapshot) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
        assert_eq!(preview.test_content, "<version>1-test</version>\n");
        assert_eq!(preview.development_content, "<version>2-dev</version>\n");
        resolve_pom_worktree(&saved, &worktree, &snapshot, PomSide::Test, false, None).unwrap();
        assert_eq!(
            git(dir.path(), &["show", "test:pom.xml"]),
            "<version>1-test</version>"
        );
    }

    #[test]
    fn pom_conflict_requires_confirmation_and_exposes_full_sides() {
        let (_dir, repo) = pom_fixture(&["module space/子模块/pom.xml"], false, false);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(
            perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).is_err()
        );
        let (preview, snapshot) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
        assert_eq!(preview.path, "module space/子模块/pom.xml");
        assert_eq!(preview.test_content, "test pom\n");
        assert_eq!(preview.development_content, "development pom\n");
        assert_eq!(preview.test_branch, "test");
        assert_eq!(preview.development_branch, "dev");
        assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);

        let file = worktree.join(&preview.path);
        let before = fs::read(&file).unwrap();
        let mut edited = before.clone();
        edited.extend_from_slice(b"user edit\n");
        fs::write(&file, &edited).unwrap();
        assert!(resolve_pom_worktree(
            &saved,
            &worktree,
            &snapshot,
            PomSide::Development,
            false,
            None
        )
        .is_err());
        assert_eq!(fs::read(&file).unwrap(), edited);
        fs::write(&file, before).unwrap();
        resolve_pom_worktree(
            &saved,
            &worktree,
            &snapshot,
            PomSide::Development,
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            git(
                Path::new(&saved.root),
                &["show", "test:module space/子模块/pom.xml"]
            ),
            "development pom"
        );
        assert_eq!(
            git(Path::new(&saved.root), &["show", "test:file"]),
            "development code"
        );
        assert!(!worktree.exists());
    }

    #[test]
    fn pom_conflict_leaves_multiple_files_and_modification_deletion_unresolved() {
        for (paths, code_conflict, delete_test) in [
            (&["pom.xml", "module/pom.xml"][..], false, false),
            (&["pom.xml"][..], true, false),
            (&["pom.xml"][..], false, true),
        ] {
            let (_dir, repo) = pom_fixture(paths, code_conflict, delete_test);
            let saved = snapshot(&repo, "merge").unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            assert!(
                perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(()))
                    .is_err()
            );
            assert!(inspect_pom_conflict(&saved, &worktree, None).is_err());
            let expected_paths: &[&str] = if delete_test {
                &[]
            } else if code_conflict {
                &["pom.xml"]
            } else {
                &["module/pom.xml", "pom.xml"]
            };
            let index_before = git(&worktree, &["ls-files", "--stage"]);
            let files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
            assert_eq!(
                files
                    .iter()
                    .map(|file| file.path.as_str())
                    .collect::<Vec<_>>(),
                expected_paths
            );
            for file in files {
                assert_eq!(
                    file.blocks,
                    vec![PomConflictBlock {
                        line: 1,
                        test_content: "test pom\n".into(),
                        development_content: "development pom\n".into(),
                        context: None,
                    }]
                );
            }
            assert_eq!(git(&worktree, &["ls-files", "--stage"]), index_before);
            let still_unmerged = git(&worktree, &["diff", "--name-only", "--diff-filter=U"]);
            assert!(still_unmerged.lines().any(|path| path == "pom.xml"));
            assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
        }
    }

    #[test]
    fn pom_conflict_rejects_multiple_blocks() {
        let (dir, repo) = fixture();
        let middle = (1..=20)
            .map(|i| format!("unchanged {i}\n"))
            .collect::<String>();
        let pom = dir.path().join("pom.xml");
        let write_pom = |version| {
            fs::write(&pom, format!("first {version}\n{middle}last {version}\n")).unwrap();
            git(dir.path(), &["add", "pom.xml"]);
            git(dir.path(), &["commit", "-m", version]);
        };
        write_pom("base");
        git(dir.path(), &["branch", "-f", "test"]);
        write_pom("dev");
        git(dir.path(), &["switch", "test"]);
        write_pom("test");
        git(dir.path(), &["switch", "dev"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        let conflict = fs::read_to_string(worktree.join("pom.xml")).unwrap();
        assert_eq!(conflict.matches("<<<<<<< ").count(), 2);
        assert!(inspect_pom_conflict(&saved, &worktree, None).is_err());
        let files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
        assert_eq!(
            files[0].blocks,
            vec![
                PomConflictBlock {
                    line: 1,
                    test_content: "first test\n".into(),
                    development_content: "first dev\n".into(),
                    context: None,
                },
                PomConflictBlock {
                    line: 26,
                    test_content: "last test\n".into(),
                    development_content: "last dev\n".into(),
                    context: None,
                },
            ]
        );
        assert_eq!(
            fs::read_to_string(worktree.join("pom.xml")).unwrap(),
            conflict
        );
        assert!(is_one_conflict_block(
            "<<<<<<< HEAD\nours\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> dev\n"
        ));
    }

    #[test]
    fn pom_blocks_resolution_keeps_automatic_merges_and_checks_all_choices_before_writing() {
        let (dir, repo) = fixture();
        let render = |changes: &[(usize, &str)]| {
            (1..=64)
                .map(|i| {
                    let text = changes
                        .iter()
                        .find(|(line, _)| *line == i)
                        .map_or("base", |(_, text)| *text);
                    format!("line {i}: {text}\n")
                })
                .collect::<String>()
        };
        let path = dir.path().join("pom.xml");
        let commit_pom = |message, changes: &[(usize, &str)]| {
            fs::write(&path, render(changes)).unwrap();
            git(dir.path(), &["add", "pom.xml"]);
            git(dir.path(), &["commit", "-m", message]);
        };
        commit_pom("base", &[]);
        git(dir.path(), &["branch", "-f", "test"]);
        commit_pom("dev", &[(1, "dev"), (28, "dev"), (64, "dev")]);
        git(dir.path(), &["switch", "test"]);
        commit_pom("test", &[(1, "test"), (12, "test"), (64, "test")]);
        git(dir.path(), &["switch", "dev"]);
        let remote = tempdir().unwrap();
        git(remote.path(), &["init", "--bare"]);
        git(
            dir.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "origin", "dev", "test"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let mut entry = paused_batch();
        entry.plan.repositories = vec![saved.clone()];
        entry.state.repositories.truncate(1);
        let operation = tmp.path().join(&entry.plan.plan_id);
        fs::create_dir(&operation).unwrap();
        let worktree = operation.join("0");
        assert!(perform(&saved, "merge", true, &worktree, None, || Ok(())).is_err());
        let (expected, files) = inspect_pom_blocks(&saved, &worktree, None).unwrap();
        assert!(!expected.manual_remaining);
        assert_eq!(files[0].preview.blocks.len(), 2);
        let mut choices = block_choices(&files);
        choices[1].side = PomSide::Development;
        let result = &mut entry.state.repositories[0];
        result.repo_id = repo.id.clone();
        result.worktree_path = worktree.to_string_lossy().into();
        entry.pom_blocks_snapshots.insert(repo.id.clone(), expected);
        let journal = operation.join("operation.json");
        let contents = journal_json(&entry).unwrap();
        assert!(!contents.contains("line 1: test"));
        fs::write(&journal, contents).unwrap();
        let restored = restore_entry(&journal).unwrap();
        let result = &restored.state.repositories[0];
        assert_eq!(
            serde_json::to_value(result).unwrap()["pomBlocksResolution"],
            serde_json::json!({"push": true, "manualRemaining": false})
        );
        assert_eq!(result.pom_conflict_files[0].blocks.len(), 2);
        let expected = &restored.pom_blocks_snapshots[&repo.id];
        let saved = &restored.plan.repositories[0];
        let path = worktree.join("pom.xml");
        let original = fs::read(&path).unwrap();
        let index = git(&worktree, &["ls-files", "--stage"]);
        for invalid in [&choices[..1], &[choices[0].clone(), choices[0].clone()]] {
            assert!(
                resolve_pom_blocks_worktree(saved, &worktree, expected, invalid, false, None)
                    .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), original);
            assert_eq!(git(&worktree, &["ls-files", "--stage"]), index);
        }
        fs::write(&path, [original.as_slice(), b"manual edit\n"].concat()).unwrap();
        assert!(
            resolve_pom_blocks_worktree(saved, &worktree, expected, &choices, false, None).is_err()
        );
        assert!(fs::read(&path).unwrap().ends_with(b"manual edit\n"));
        fs::write(&path, &original).unwrap();
        fs::write(worktree.join("other"), "added to index\n").unwrap();
        git(&worktree, &["add", "other"]);
        assert!(
            resolve_pom_blocks_worktree(saved, &worktree, expected, &choices, false, None).is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        git(&worktree, &["rm", "-f", "other"]);
        assert!(
            !resolve_pom_blocks_worktree(saved, &worktree, expected, &choices, true, None).unwrap()
        );
        assert_eq!(
            git(remote.path(), &["show", "test:pom.xml"]),
            render(&[(1, "test"), (12, "test"), (28, "dev"), (64, "dev")]).trim_end()
        );
    }

    fn block_choices(files: &[ScannedPomFile]) -> Vec<PomBlockChoice> {
        files
            .iter()
            .flat_map(|file| {
                file.preview.blocks.iter().map(|block| PomBlockChoice {
                    path: file.preview.path.clone(),
                    line: block.line,
                    side: PomSide::Test,
                })
            })
            .collect()
    }

    #[test]
    fn pom_blocks_resolution_stages_only_supported_poms_when_other_conflicts_remain() {
        let (dir, repo) = pom_fixture(&["pom.xml", "module/pom.xml"], true, false);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        // POM 手工改动后仍有可预览的标记，但不能加入本次应用。
        let module = worktree.join("module/pom.xml");
        let edited = format!(
            "{}manually edited POM\n",
            fs::read_to_string(&module).unwrap()
        );
        fs::write(&module, &edited).unwrap();
        assert_eq!(
            read_pom_conflict_files(&saved, &worktree, None)
                .unwrap()
                .len(),
            2
        );
        let (expected, files) = inspect_pom_blocks(&saved, &worktree, None).unwrap();
        assert!(expected.manual_remaining);
        assert_eq!(files.len(), 1);
        let choices = block_choices(&files);
        assert!(
            resolve_pom_blocks_worktree(&saved, &worktree, &expected, &choices, true, None)
                .unwrap()
        );
        assert_eq!(git(&worktree, &["show", ":pom.xml"]), "test pom");
        assert_eq!(
            git(&worktree, &["diff", "--name-only", "--diff-filter=U"]),
            "file\nmodule/pom.xml"
        );
        assert_eq!(
            fs::read_to_string(worktree.join("module/pom.xml")).unwrap(),
            edited
        );
        assert_eq!(sha(dir.path(), "test").unwrap(), saved.target_sha);
    }

    #[test]
    fn pom_conflict_preview_includes_all_branch_differences_with_one_conflict_block() {
        let (dir, repo) = fixture();
        let pom = dir.path().join("pom.xml");
        let commit_pom = |side, changes: &[usize]| {
            let mut content = String::from("<project>\n");
            for i in 1..=56 {
                let version = if changes.contains(&i) { side } else { "base" };
                content.push_str(&format!("  <entry id=\"{i}\">{version}</entry>\n"));
            }
            content.push_str("</project>\n");
            fs::write(&pom, content).unwrap();
            git(dir.path(), &["add", "pom.xml"]);
            git(dir.path(), &["commit", "-m", side]);
        };
        commit_pom("base", &[]);
        git(dir.path(), &["branch", "-f", "test"]);
        git(dir.path(), &["switch", "test"]);
        commit_pom("test", &[4, 12, 28, 44]);
        git(dir.path(), &["switch", "dev"]);
        commit_pom("dev", &[4, 20, 36, 52]);
        git(dir.path(), &["config", "diff.interHunkContext", "100"]);
        git(dir.path(), &["config", "color.diff", "always"]);

        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        let merged_pom = worktree.join("pom.xml");
        let before = fs::read_to_string(&merged_pom).unwrap();
        let index_before = git(&worktree, &["ls-files", "--stage"]);
        assert_eq!(before.matches("<<<<<<< ").count(), 1);
        assert!(before.contains("<entry id=\"12\">test</entry>"));
        assert!(before.contains("<entry id=\"20\">dev</entry>"));

        let files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(file.blocks.len(), 1);
        let response = serde_json::to_value(file).unwrap();
        let diff = response["branchDiff"].as_str().unwrap();
        assert!(diff.starts_with("@@ "));
        assert_eq!(
            diff.lines().filter(|line| line.starts_with("@@ ")).count(),
            7
        );
        let changed_lines = [
            (4, "test", "dev"),
            (12, "test", "base"),
            (20, "base", "dev"),
            (28, "test", "base"),
            (36, "base", "dev"),
            (44, "test", "base"),
            (52, "base", "dev"),
        ]
        .into_iter()
        .flat_map(|(i, from, to)| {
            [
                format!("-  <entry id=\"{i}\">{from}</entry>"),
                format!("+  <entry id=\"{i}\">{to}</entry>"),
            ]
        })
        .collect::<Vec<_>>();
        assert_eq!(
            diff.lines()
                .filter(|line| !line.starts_with("@@ "))
                .collect::<Vec<_>>(),
            changed_lines
        );
        assert_eq!(fs::read_to_string(merged_pom).unwrap(), before);
        assert_eq!(git(&worktree, &["ls-files", "--stage"]), index_before);
        assert!(inspect_pom_conflict(&saved, &worktree, None).is_ok());
    }

    #[test]
    fn pom_conflict_context_distinguishes_repeated_versions_and_branch_line_offsets() {
        let (dir, repo) = fixture();
        let pom = dir.path().join("pom.xml");
        let commit_pom = |side, version, artifact, property, added: bool| {
            let padding = "  <!-- keep hunks separate -->\n".repeat(12);
            let extra = if added {
                "  <!-- development only -->\n"
            } else {
                ""
            };
            let xml = format!(
                "<project xmlns=\"http://maven.apache.org/POM/4.0.0\">\n\
                 {extra}{padding}  <parent>\n    <groupId>org.example</groupId>\n    <artifactId>{artifact}</artifactId>\n    <version>{version}</version>\n  </parent>\n\
                 {padding}  <properties>\n    <java.version>{property}</java.version>\n  </properties>\n\
                 {padding}  <dependencies>\n    <dependency>\n      <groupId>org.example</groupId>\n      <artifactId>library</artifactId>\n      <version>{version}</version>\n    </dependency>\n  </dependencies>\n\
                 {padding}  <build><plugins><plugin>\n    <artifactId>maven-compiler-plugin</artifactId>\n    <version>{version}</version>\n  </plugin></plugins></build>\n</project>\n"
            );
            fs::write(&pom, xml).unwrap();
            git(dir.path(), &["add", "pom.xml"]);
            git(dir.path(), &["commit", "-m", side]);
        };
        commit_pom("base", "1", "base-parent", "17", false);
        git(dir.path(), &["branch", "-f", "test"]);
        commit_pom("dev", "3", "dev-parent", "17", true);
        git(dir.path(), &["switch", "test"]);
        commit_pom("test", "2", "test-parent", "21", false);
        git(dir.path(), &["switch", "dev"]);
        git(dir.path(), &["config", "merge.conflictStyle", "diff3"]);

        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        let files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
        let response = serde_json::to_value(&files[0]).unwrap();
        let blocks = response["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        let parent = blocks[0]["context"].as_str().unwrap();
        assert!(parent.contains("父 POM"));
        assert!(parent.contains("org.example:test-parent"));
        assert!(parent.contains("org.example:dev-parent"));
        assert_eq!(blocks[1]["context"], "依赖 org.example:library");
        assert_eq!(
            blocks[2]["context"],
            "插件 org.apache.maven.plugins:maven-compiler-plugin"
        );

        let diff = response["branchDiff"].as_str().unwrap();
        let contexts = response["diffContexts"].as_object().unwrap();
        let headers = diff
            .lines()
            .filter(|line| line.starts_with("@@ "))
            .collect::<Vec<_>>();
        assert_eq!(contexts.len(), headers.len());
        assert!(headers.iter().all(|header| contexts.contains_key(*header)));
        assert_eq!(contexts[headers[0]], "测试：第 1 行后；开发：第 2 行");
        assert!(contexts.values().any(|value| value == "属性 java.version"));
        assert!(contexts
            .values()
            .any(|value| value == "插件 org.apache.maven.plugins:maven-compiler-plugin"));
    }

    #[test]
    fn pom_conflict_preview_preserves_crlf_empty_side_and_omits_diff3_base() {
        let content = "context\r\n<<<<<<<<< HEAD\r\n||||||||| base\r\nremoved\r\n=========\r\n  incoming  \r\n\r\n>>>>>>>>> dev";
        assert_eq!(
            parse_pom_conflict_blocks(content, 9),
            Some(vec![PomConflictBlock {
                line: 2,
                test_content: String::new(),
                development_content: "  incoming  \r\n\r\n".into(),
                context: None,
            }])
        );
        assert!(parse_pom_conflict_blocks("<<<<<<< HEAD\nours\n=======\ntheirs\n", 7).is_none());
    }

    #[test]
    fn pom_conflict_push_is_explicit_and_uses_selected_test_content() {
        let (dir, repo) = pom_fixture(&["pom.xml"], false, false);
        let remote = tempdir().unwrap();
        git(remote.path(), &["init", "--bare"]);
        git(
            dir.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "origin", "dev", "test"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(perform(&saved, "merge", true, &worktree, None, || Ok(())).is_err());
        assert_eq!(
            sha(remote.path(), "refs/heads/test").unwrap(),
            saved.target_sha
        );
        let (_, conflict) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
        resolve_pom_worktree(&saved, &worktree, &conflict, PomSide::Test, true, None).unwrap();
        assert_ne!(
            sha(remote.path(), "refs/heads/test").unwrap(),
            saved.target_sha
        );
        assert_eq!(git(remote.path(), &["show", "test:pom.xml"]), "test pom");
        assert_eq!(
            git(remote.path(), &["show", "test:file"]),
            "development code"
        );
        assert!(!worktree.exists());
    }

    #[test]
    fn pom_option_does_not_replace_clean_pom_changes() {
        let (dir, repo) = fixture();
        fs::write(dir.path().join("pom.xml"), "incoming pom\n").unwrap();
        git(dir.path(), &["add", "pom.xml"]);
        git(dir.path(), &["commit", "-m", "incoming pom"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        perform_with_options(
            &saved,
            "merge",
            false,
            true,
            &tmp.path().join("merge"),
            None,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(git(dir.path(), &["show", "test:pom.xml"]), "incoming pom");
    }

    #[test]
    fn pom_option_rejects_switch_before_changing_repository() {
        let (dir, repo) = fixture();
        let saved = snapshot(&repo, "switch").unwrap();
        let tmp = tempdir().unwrap();
        assert!(perform_with_options(
            &saved,
            "switch",
            false,
            true,
            &tmp.path().join("merge"),
            None,
            || Ok(())
        )
        .is_err());
        assert_eq!(branch(dir.path()), "dev");
    }

    #[cfg(unix)]
    #[test]
    fn pom_option_preserves_commit_hook_rejection() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, repo) = pom_fixture(&["pom.xml"], false, false);
        let hooks = dir.path().join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(
            perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).is_err()
        );
        let (_, snapshot) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
        assert!(
            resolve_pom_worktree(&saved, &worktree, &snapshot, PomSide::Test, false, None).is_err()
        );
        assert!(run(&worktree, &["ls-files", "--unmerged", "-z"], None)
            .unwrap()
            .is_empty());
        assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
        assert!(worktree.is_dir());
    }

    #[test]
    fn pom_option_preserves_signing_rejection_after_resolution() {
        let (dir, repo) = pom_fixture(&["pom.xml"], false, false);
        git(dir.path(), &["config", "commit.gpgSign", "true"]);
        git(
            dir.path(),
            &[
                "config",
                "gpg.program",
                dir.path().join("missing-signing-program").to_str().unwrap(),
            ],
        );
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(
            perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).is_err()
        );
        let (_, snapshot) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
        assert!(
            resolve_pom_worktree(&saved, &worktree, &snapshot, PomSide::Test, false, None).is_err()
        );
        assert!(run(&worktree, &["ls-files", "--unmerged", "-z"], None)
            .unwrap()
            .is_empty());
        assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
        assert!(worktree.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn pom_option_does_not_commit_after_nonconflict_merge_hook_failure() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, repo) = fixture();
        change(dir.path(), "development\n");
        git(dir.path(), &["config", "merge.ff", "false"]);
        let hooks = dir.path().join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-merge-commit");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        assert!(
            perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).is_err()
        );
        assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
        assert!(worktree.is_dir());
    }

    #[test]
    fn old_plan_defaults_pom_option_to_false() {
        let plan: Plan = serde_json::from_value(serde_json::json!({
            "planId": "old", "projectId": "p", "operation": "merge", "push": false, "repositories": []
        })).unwrap();
        assert!(!serde_json::to_value(plan).unwrap()["keepTestPom"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn preserves_repository_signing_configuration() {
        let (dir, repo) = fixture();
        git(dir.path(), &["config", "commit.gpgSign", "true"]);
        assert_eq!(
            run(dir.path(), &["config", "--get", "commit.gpgSign"], None).unwrap(),
            "true"
        );
        assert!(!git_command(dir.path())
            .get_envs()
            .any(|(key, value)| key == "GIT_SSH_COMMAND" && value.is_some()));
        change(dir.path(), "signed merge source\n");
        git(dir.path(), &["config", "merge.ff", "false"]);
        git(
            dir.path(),
            &[
                "config",
                "gpg.program",
                dir.path().join("missing-signing-program").to_str().unwrap(),
            ],
        );
        let saved = snapshot(&repo, "merge").unwrap();
        let temporary = tempdir().unwrap();
        assert!(perform(
            &saved,
            "merge",
            false,
            &temporary.path().join("merge"),
            None,
            || Ok(())
        )
        .is_err());
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), saved.target_sha);
    }

    #[cfg(unix)]
    #[test]
    fn merge_respects_repository_hooks_and_preserves_rejected_worktree() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, repo) = fixture();
        change(dir.path(), "development change\n");
        git(dir.path(), &["config", "merge.ff", "false"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let hooks = dir.path().join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-merge-commit");
        fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let temporary = tempdir().unwrap();
        let worktree = temporary.path().join("merge");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), saved.target_sha);
        assert_eq!(git(dir.path(), &["branch", "--show-current"]), "dev");
        assert!(worktree.is_dir());
    }

    #[test]
    fn branch_candidates_are_real_names_without_remote_prefix_or_head_alias() {
        let (dir, repo) = fixture();
        git(
            dir.path(),
            &["remote", "add", "origin", dir.path().to_str().unwrap()],
        );
        let head = git(dir.path(), &["rev-parse", "HEAD"]);
        git(
            dir.path(),
            &["update-ref", "refs/remotes/origin/feature/remote", &head],
        );
        git(
            dir.path(),
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/feature/remote",
            ],
        );
        let branches = status(&repo).unwrap().branches;
        assert!(branches.contains(&"feature/remote".to_owned()));
        assert!(!branches
            .iter()
            .any(|branch| branch.starts_with("origin/") || branch == "HEAD"));
    }

    #[test]
    fn abort_does_not_report_committed_merge_as_aborted() {
        let (dir, repo) = fixture();
        change(dir.path(), "already merged\n");
        let saved = snapshot(&repo, "merge").unwrap();
        let temporary = tempdir().unwrap();
        let worktree = temporary.path().join("merge");
        git(
            dir.path(),
            &["worktree", "add", worktree.to_str().unwrap(), "test"],
        );
        git(&worktree, &["merge", "--no-edit", &saved.source_sha]);
        let merged = git(dir.path(), &["rev-parse", "test"]);
        assert!(abort_worktree(&saved, &worktree, None).is_err());
        assert!(worktree.is_dir());
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), merged);
    }

    #[test]
    fn scan_does_not_promote_parent_repository_children() {
        let (dir, _) = fixture();
        // .git 由 fixture 创建，其余隐藏目录也应被排除，名称中间的点不受影响。
        for name in ["ordinary", "service.api", ".idea", ".claude", ".custom"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let list = scan(dir.path().to_string_lossy().into()).unwrap();
        assert_eq!(
            list.iter()
                .map(|candidate| candidate.name.as_str())
                .collect::<Vec<_>>(),
            vec!["ordinary", "service.api"]
        );
        assert!(!list.iter().find(|r| r.name == "ordinary").unwrap().is_git);
        let external = tempdir().unwrap();
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                external.path().join("linked").to_str().unwrap(),
                "test",
            ],
        );
        assert!(scan(external.path().to_string_lossy().into()).unwrap()[0].is_git);
    }
    #[test]
    fn preflight_rejects_dirty_missing_and_duplicate_roots() {
        let (dir, repo) = fixture();
        fs::write(dir.path().join("file"), "dirty").unwrap();
        assert!(snapshot(&repo, "switch").is_err());
        git(dir.path(), &["restore", "file"]);
        let mut missing = repo.clone();
        missing.development_branch = "missing".into();
        assert!(snapshot(&missing, "switch").is_err());
        assert!(preflight(&[repo.clone(), repo], "switch").is_err());
    }
    #[test]
    fn switch_master_changes_only_checkout_without_branch_mappings() {
        for current in ["dev", "feature", "master"] {
            let (dir, mut repo) = fixture();
            git(dir.path(), &["branch", "master"]);
            change(dir.path(), "development only\n");
            if current == "feature" {
                git(dir.path(), &["switch", "-c", current]);
            } else {
                git(dir.path(), &["switch", current]);
            }
            repo.development_branch.clear();
            repo.test_branch = "invalid test branch".into();
            let saved = preflight(&[repo.clone()], "switch-master")
                .unwrap()
                .remove(0);
            assert_eq!(saved.current_branch, current);
            assert_eq!(saved.target_branch, "master");
            assert_eq!(saved.repository.development_branch, repo.development_branch);
            assert_eq!(saved.repository.test_branch, repo.test_branch);
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("unused");
            perform(&saved, "switch-master", false, &worktree, None, || {
                panic!("switch-master must not create a merge worktree")
            })
            .unwrap();
            assert_eq!(branch(dir.path()), "master");
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), saved.target_sha);
            assert_eq!(git(dir.path(), &["show", "master:file"]), "base");
            assert_eq!(git(dir.path(), &["show", "dev:file"]), "development only");
            assert!(!worktree.exists());
        }
    }

    #[test]
    fn switch_master_requires_master_and_creates_remote_tracking() {
        let (dir, repo, remote) = push_fixture(false);
        git(dir.path(), &["branch", "main"]);
        git(dir.path(), &["push", "origin", "main"]);
        assert!(snapshot(&repo, "switch-master").is_err());
        git(dir.path(), &["push", "origin", "dev:master"]);
        fetch_fixture(&repo);
        let saved = snapshot(&repo, "switch-master").unwrap();
        assert!(saved.create_tracking);
        let tmp = tempdir().unwrap();
        perform(
            &saved,
            "switch-master",
            false,
            &tmp.path().join("unused"),
            None,
            || panic!("tracking switch must not create a merge worktree"),
        )
        .unwrap();
        assert_eq!(branch(dir.path()), "master");
        assert_eq!(
            git(dir.path(), &["config", "branch.master.remote"]),
            "origin"
        );
        assert_eq!(
            git(dir.path(), &["config", "branch.master.merge"]),
            "refs/heads/master"
        );
        assert_eq!(
            sha(remote.path(), "refs/heads/master").unwrap(),
            saved.target_sha
        );
    }

    #[test]
    fn switch_master_rejects_dirty_changes_and_occupied_worktrees() {
        for changed in [
            "dirty",
            "source",
            "target",
            "remote-url",
            "occupied",
            "MERGE_HEAD",
            "rebase-merge",
            "rebase-apply",
        ] {
            let (dir, repo, _remote) = push_fixture(false);
            git(dir.path(), &["branch", "master"]);
            git(dir.path(), &["push", "origin", "master"]);
            let saved = snapshot(&repo, "switch-master").unwrap();
            assert!(preflight(&[repo.clone(), repo.clone()], "switch-master").is_err());
            let tmp = tempdir().unwrap();
            let next = git(
                dir.path(),
                &[
                    "commit-tree",
                    &format!("{}^{{tree}}", saved.source_sha),
                    "-p",
                    &saved.source_sha,
                    "-m",
                    "next",
                ],
            );
            match changed {
                "dirty" => fs::write(dir.path().join("file"), "preserve edits").unwrap(),
                "source" | "target" => {
                    let reference = if changed == "source" {
                        "refs/heads/dev"
                    } else {
                        "refs/heads/master"
                    };
                    git(dir.path(), &["update-ref", reference, &next]);
                }
                "remote-url" => {
                    git(
                        dir.path(),
                        &["remote", "set-url", "origin", tmp.path().to_str().unwrap()],
                    );
                }
                "occupied" => {
                    git(
                        dir.path(),
                        &[
                            "worktree",
                            "add",
                            tmp.path().join("occupied").to_str().unwrap(),
                            "master",
                        ],
                    );
                }
                "MERGE_HEAD" => {
                    fs::write(dir.path().join(".git/MERGE_HEAD"), &saved.source_sha).unwrap()
                }
                marker => fs::create_dir(dir.path().join(".git").join(marker)).unwrap(),
            }
            if matches!(
                changed,
                "dirty" | "occupied" | "MERGE_HEAD" | "rebase-merge" | "rebase-apply"
            ) {
                assert!(snapshot(&repo, "switch-master").is_err(), "{changed}");
            }
            let worktree = tmp.path().join("unused");
            assert!(
                perform(&saved, "switch-master", false, &worktree, None, || panic!(
                    "must reject before creating worktree"
                ))
                .is_err(),
                "{changed}"
            );
            assert_eq!(branch(dir.path()), "dev");
            assert!(!worktree.exists());
            if changed == "dirty" {
                assert_eq!(
                    fs::read_to_string(dir.path().join("file")).unwrap(),
                    "preserve edits"
                );
            }
        }
    }

    #[test]
    fn switch_master_rejects_push_and_pom_before_creating_tracking() {
        let (dir, repo, _remote) = push_fixture(false);
        git(dir.path(), &["push", "origin", "dev:master"]);
        fetch_fixture(&repo);
        let saved = snapshot(&repo, "switch-master").unwrap();
        assert!(saved.create_tracking);
        let tmp = tempdir().unwrap();
        for (push, pom) in [(true, false), (false, true), (true, true)] {
            assert!(validate_options("switch-master", push, pom).is_err());
            assert!(perform_with_options(
                &saved,
                "switch-master",
                push,
                pom,
                &tmp.path().join("unused"),
                None,
                || panic!("must reject invalid options")
            )
            .is_err());
            assert_eq!(branch(dir.path()), "dev");
            assert!(sha(dir.path(), "refs/heads/master").is_err());
        }
    }

    #[test]
    fn switch_test_changes_only_checkout_from_development_other_or_target_branch() {
        for current in ["dev", "feature", "test"] {
            let (dir, mut repo) = fixture();
            change(dir.path(), "development only\n");
            if current == "feature" {
                git(dir.path(), &["switch", "-c", current]);
            } else {
                git(dir.path(), &["switch", current]);
            }
            // 测试切换不依赖开发分支配置，也不要求当前检出开发分支。
            repo.development_branch = "invalid development branch".into();
            let saved = preflight(&[repo], "switch-test").unwrap().remove(0);
            assert_eq!(saved.current_branch, current);
            assert_eq!(saved.target_branch, "test");
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("must-not-create");
            perform(&saved, "switch-test", false, &worktree, None, || {
                panic!("switch-test must not create a merge worktree")
            })
            .unwrap();
            assert_eq!(branch(dir.path()), "test");
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), saved.target_sha);
            assert_eq!(git(dir.path(), &["show", "test:file"]), "base");
            assert_eq!(git(dir.path(), &["show", "dev:file"]), "development only");
            assert!(!worktree.exists());
        }
    }

    #[test]
    fn switch_test_rejects_missing_invalid_dirty_and_duplicate_roots() {
        let (dir, repo) = fixture();
        for target in ["missing", "invalid branch", ""] {
            let mut invalid = repo.clone();
            invalid.test_branch = target.into();
            assert!(snapshot(&invalid, "switch-test").is_err());
        }
        assert!(preflight(&[repo.clone(), repo.clone()], "switch-test").is_err());
        for kind in ["unstaged", "untracked", "staged"] {
            let file = if kind == "untracked" {
                "new-file"
            } else {
                "file"
            };
            fs::write(dir.path().join(file), "preserve edits").unwrap();
            if kind == "staged" {
                git(dir.path(), &["add", file]);
            }
            assert!(snapshot(&repo, "switch-test").is_err(), "{kind}");
            if kind == "untracked" {
                fs::remove_file(dir.path().join(file)).unwrap();
            } else {
                git(dir.path(), &["restore", "--staged", "file"]);
                git(dir.path(), &["restore", "file"]);
            }
        }
        for marker in ["MERGE_HEAD", "rebase-merge", "rebase-apply"] {
            let path = dir.path().join(".git").join(marker);
            if marker == "MERGE_HEAD" {
                fs::write(&path, sha(dir.path(), "HEAD").unwrap()).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            assert!(snapshot(&repo, "switch-test").is_err(), "{marker}");
            if path.is_dir() {
                fs::remove_dir(&path).unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
        }
    }

    #[test]
    fn switch_test_revalidates_before_preserving_new_edits() {
        for kind in ["unstaged", "untracked", "staged"] {
            let (dir, repo) = fixture();
            let saved = snapshot(&repo, "switch-test").unwrap();
            let file = if kind == "untracked" {
                "new-file"
            } else {
                "file"
            };
            fs::write(dir.path().join(file), "new edits").unwrap();
            if kind == "staged" {
                git(dir.path(), &["add", file]);
            }
            let tmp = tempdir().unwrap();
            assert!(
                perform(
                    &saved,
                    "switch-test",
                    false,
                    &tmp.path().join("unused"),
                    None,
                    || panic!("must reject before creating worktree")
                )
                .is_err(),
                "{kind}"
            );
            assert_eq!(branch(dir.path()), "dev");
            assert_eq!(
                fs::read_to_string(dir.path().join(file)).unwrap(),
                "new edits"
            );
            if kind == "staged" {
                assert_eq!(
                    git(dir.path(), &["diff", "--cached", "--name-only"]),
                    "file"
                );
            }
        }
    }

    #[test]
    fn switch_test_rejects_target_occupied_after_preflight() {
        let (dir, repo) = fixture();
        let saved = snapshot(&repo, "switch-test").unwrap();
        let tmp = tempdir().unwrap();
        let occupied = tmp.path().join("occupied");
        git(
            dir.path(),
            &["worktree", "add", occupied.to_str().unwrap(), "test"],
        );
        assert!(snapshot(&repo, "switch-test")
            .unwrap_err()
            .contains("worktree"));
        assert!(perform(
            &saved,
            "switch-test",
            false,
            &tmp.path().join("unused"),
            None,
            || Ok(())
        )
        .is_err());
        assert_eq!(branch(dir.path()), "dev");
        assert_eq!(branch(&occupied), "test");
    }

    #[test]
    fn switch_test_rejects_pom_and_push_before_creating_tracking_branch() {
        let (dir, repo) = fixture();
        let bare = tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "--template="]);
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "origin", "test"]);
        git(dir.path(), &["branch", "-D", "test"]);
        let saved = snapshot(&repo, "switch-test").unwrap();
        assert!(saved.create_tracking);
        let tmp = tempdir().unwrap();
        for (push, pom) in [(true, false), (false, true)] {
            assert!(perform_with_options(
                &saved,
                "switch-test",
                push,
                pom,
                &tmp.path().join("unused"),
                None,
                || panic!("must reject invalid switch options")
            )
            .is_err());
            assert_eq!(branch(dir.path()), "dev");
            assert!(sha(dir.path(), "refs/heads/test").is_err());
        }
        perform(
            &saved,
            "switch-test",
            false,
            &tmp.path().join("unused"),
            None,
            || panic!("tracking switch must not create worktree"),
        )
        .unwrap();
        assert_eq!(branch(dir.path()), "test");
        assert_eq!(git(dir.path(), &["config", "branch.test.remote"]), "origin");
        assert_eq!(
            git(dir.path(), &["config", "branch.test.merge"]),
            "refs/heads/test"
        );
        assert_eq!(
            sha(bare.path(), "refs/heads/test").unwrap(),
            saved.target_sha
        );
    }

    #[test]
    fn switch_test_revalidation_rejects_source_target_remote_and_identity_changes() {
        for changed in ["source", "target", "remote-url"] {
            let (dir, repo) = fixture();
            let bare = tempdir().unwrap();
            git(bare.path(), &["init", "--bare", "--template="]);
            git(
                dir.path(),
                &["remote", "add", "origin", bare.path().to_str().unwrap()],
            );
            git(dir.path(), &["push", "origin", "test"]);
            let saved = snapshot(&repo, "switch-test").unwrap();
            let next = git(
                dir.path(),
                &[
                    "commit-tree",
                    &format!("{}^{{tree}}", saved.source_sha),
                    "-p",
                    &saved.source_sha,
                    "-m",
                    "new commit",
                ],
            );
            match changed {
                "source" => {
                    git(dir.path(), &["update-ref", "refs/heads/dev", &next]);
                }
                "target" => {
                    git(dir.path(), &["update-ref", "refs/heads/test", &next]);
                }
                _ => {
                    git(
                        dir.path(),
                        &["remote", "set-url", "origin", dir.path().to_str().unwrap()],
                    );
                }
            }
            let tmp = tempdir().unwrap();
            assert!(
                perform(
                    &saved,
                    "switch-test",
                    false,
                    &tmp.path().join("unused"),
                    None,
                    || Ok(())
                )
                .is_err(),
                "{changed}"
            );
            assert_eq!(branch(dir.path()), "dev");
        }
    }

    #[test]
    fn switch_revalidation_preserves_new_edits() {
        let (dir, mut repo) = fixture();
        git(dir.path(), &["switch", "test"]);
        repo.development_branch = "dev".into();
        let saved = snapshot(&repo, "switch").unwrap();
        fs::write(dir.path().join("file"), "keep my edits").unwrap();
        assert!(revalidate(&saved, "switch").is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("file")).unwrap(),
            "keep my edits"
        );
        assert_eq!(git(dir.path(), &["branch", "--show-current"]), "test");
    }

    fn change(path: &Path, value: &str) {
        fs::write(path.join("file"), value).unwrap();
        git(path, &["add", "file"]);
        git(path, &["commit", "-m", "change"]);
    }
    #[test]
    fn merge_uses_committed_source_and_removes_clean_worktree() {
        let (dir, repo) = fixture();
        change(dir.path(), "development\n");
        let saved = snapshot(&repo, "merge").unwrap();
        let temporary = tempdir().unwrap();
        let worktree = temporary.path().join("merge");
        perform(&saved, "merge", false, &worktree, None, || Ok(())).unwrap();
        assert!(!worktree.exists());
        assert_eq!(git(dir.path(), &["show", "test:file"]), "development");
        assert_eq!(git(dir.path(), &["branch", "--show-current"]), "dev");
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), saved.source_sha);
    }
    #[test]
    fn conflict_is_retained_and_abort_only_touches_operation_worktree() {
        let (dir, repo) = fixture();
        git(dir.path(), &["switch", "test"]);
        change(dir.path(), "test edit\n");
        git(dir.path(), &["switch", "dev"]);
        change(dir.path(), "development edit\n");
        let saved = snapshot(&repo, "merge").unwrap();
        let temporary = tempdir().unwrap();
        let worktree = temporary.path().join("conflict");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        assert!(worktree.exists());
        assert!(fs::read_to_string(worktree.join("file"))
            .unwrap()
            .contains("<<<<<<<"));
        assert!(!dir.path().join(".git/MERGE_HEAD").exists());
        assert!(abort_worktree(&saved, dir.path(), None).is_err());
        abort_worktree(&saved, &worktree, None).unwrap();
        assert!(!worktree.exists());
        assert_eq!(git(dir.path(), &["show", "test:file"]), "test edit");
        assert_eq!(git(dir.path(), &["show", "dev:file"]), "development edit");
    }
    #[test]
    fn remote_tracking_is_created_only_from_explicit_ref_and_push_is_opt_in() {
        let (dir, repo) = fixture();
        let bare = tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "--template="]);
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "origin", "dev", "test"]);
        git(dir.path(), &["fetch", "origin"]);
        let before = git(bare.path(), &["rev-parse", "refs/heads/test"]);
        change(dir.path(), "new dev\n");
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        perform(
            &saved,
            "merge",
            false,
            &tmp.path().join("local"),
            None,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(git(bare.path(), &["rev-parse", "refs/heads/test"]), before);
        // 前次本地合并使旧计划失效，但新计划必须保留领先远端的提交并允许普通推送。
        assert!(revalidate(&saved, "merge").is_err());
        let ahead = snapshot(&repo, "merge").unwrap();
        assert_eq!(ahead.target_sha, saved.source_sha);
        assert_eq!(ahead.remote_sha, before);
        change(dir.path(), "next dev\n");
        let next = snapshot(&repo, "merge").unwrap();
        perform(&next, "merge", true, &tmp.path().join("push"), None, || {
            Ok(())
        })
        .unwrap();
        assert_eq!(
            git(bare.path(), &["rev-parse", "refs/heads/test"]),
            next.source_sha
        );
        git(dir.path(), &["branch", "-d", "test"]);
        let tracking = snapshot(&repo, "merge").unwrap();
        assert!(tracking.create_tracking);
        perform(
            &tracking,
            "merge",
            false,
            &tmp.path().join("tracking"),
            None,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(git(dir.path(), &["config", "branch.test.remote"]), "origin");
    }
    #[test]
    fn behind_test_branch_is_planned_but_divergence_preserves_local_commits() {
        let (dir, repo) = fixture();
        let bare = tempdir().unwrap();
        git(bare.path(), &["init", "--bare", "--template="]);
        git(
            dir.path(),
            &["remote", "add", "origin", bare.path().to_str().unwrap()],
        );
        git(dir.path(), &["push", "origin", "test"]);
        let other = tempdir().unwrap();
        git(
            other.path(),
            &[
                "clone",
                "--branch",
                "test",
                "--template=",
                bare.path().to_str().unwrap(),
                "checkout",
            ],
        );
        let checkout = other.path().join("checkout");
        change(&checkout, "remote change\n");
        git(&checkout, &["push", "origin", "test"]);
        git(dir.path(), &["fetch", "origin"]);
        let original = git(dir.path(), &["rev-parse", "test"]);
        let saved = snapshot(&repo, "merge").unwrap();
        assert!(saved.sync_test_branch);
        assert_eq!(saved.target_sha, original);
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), original);
        git(dir.path(), &["switch", "test"]);
        change(dir.path(), "local commit to preserve\n");
        git(dir.path(), &["switch", "dev"]);
        let local = git(dir.path(), &["rev-parse", "test"]);
        assert!(snapshot(&repo, "merge").is_err());
        assert_eq!(git(dir.path(), &["rev-parse", "test"]), local);
        assert_eq!(
            git(dir.path(), &["show", "test:file"]),
            "local commit to preserve"
        );
    }
    fn behind_merge_fixture(conflict: bool) -> (TempDir, Repository, TempDir) {
        let (dir, repo, remote) = push_fixture(true);
        let local = sha(dir.path(), "test").unwrap();
        git(dir.path(), &["switch", "test"]);
        if conflict {
            change(dir.path(), "remote test edit\n");
        } else {
            fs::write(dir.path().join("remote-only"), "remote test commit\n").unwrap();
            git(dir.path(), &["add", "remote-only"]);
            git(dir.path(), &["commit", "-m", "remote test commit"]);
        }
        git(dir.path(), &["push", "origin", "test"]);
        git(dir.path(), &["switch", "dev"]);
        git(dir.path(), &["branch", "-f", "test", &local]);
        change(dir.path(), "approved development\n");
        (dir, repo, remote)
    }

    // 将已提交的开发变更放到真实 bare 远端，再恢复本地落后状态。
    fn behind_development_fixture(conflict: bool) -> (TempDir, Repository, TempDir) {
        let (dir, repo, remote) = behind_merge_fixture(conflict);
        let base = sha(dir.path(), "dev^").unwrap();
        git(dir.path(), &["push", "origin", "dev"]);
        git(dir.path(), &["reset", "--hard", &base]);
        (dir, repo, remote)
    }

    #[test]
    fn development_sync_plans_relations_without_changing_local_refs() {
        for relation in ["behind", "ahead", "diverged", "missing"] {
            let (dir, repo, remote) = behind_development_fixture(false);
            let upstream = sha(remote.path(), "dev").unwrap();
            match relation {
                "ahead" => {
                    git(dir.path(), &["reset", "--hard", &upstream]);
                    change(dir.path(), "ahead\n");
                }
                "diverged" => change(dir.path(), "diverged\n"),
                "missing" => {
                    git(remote.path(), &["update-ref", "-d", "refs/heads/dev"]);
                    fetch_fixture(&repo);
                    change(dir.path(), "local committed source\n");
                }
                _ => {}
            }
            let before = git(dir.path(), &["show-ref"]);
            let result = snapshot(&repo, "merge");
            assert_eq!(git(dir.path(), &["show-ref"]), before);
            if relation == "diverged" {
                assert!(result.is_err());
                continue;
            }
            let saved = result.unwrap();
            let json = serde_json::to_value(&saved).unwrap();
            assert_eq!(json["syncDevelopmentBranch"], relation == "behind");
            let source = sha(dir.path(), "HEAD").unwrap();
            assert_eq!(saved.source_sha, source);
            let tmp = tempdir().unwrap();
            perform(
                &saved,
                "merge",
                false,
                &tmp.path().join("merge"),
                None,
                || Ok(()),
            )
            .unwrap();
            let expected = if relation == "behind" {
                &upstream
            } else {
                &source
            };
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), *expected);
            git(
                dir.path(),
                &["merge-base", "--is-ancestor", expected, "test"],
            );
            git(
                dir.path(),
                &["merge-base", "--is-ancestor", &saved.remote_sha, "test"],
            );
            assert_eq!(sha(remote.path(), "test").unwrap(), saved.remote_sha);
            if relation == "missing" {
                assert!(sha(remote.path(), "refs/heads/dev").is_err());
            } else {
                assert_eq!(sha(remote.path(), "refs/heads/dev").unwrap(), upstream);
            }
        }
    }

    #[test]
    fn development_sync_rejects_new_remote_commits_and_dirty_states() {
        for mutation in [
            "remote",
            "unstaged",
            "staged",
            "untracked",
            "merge",
            "rebase",
        ] {
            let (dir, repo, remote) = behind_development_fixture(false);
            let saved = snapshot(&repo, "merge").unwrap();
            match mutation {
                "remote" => {
                    let next = child_commit(dir.path(), "origin/dev", "new remote");
                    git(
                        dir.path(),
                        &[
                            "push",
                            remote.path().to_str().unwrap(),
                            &format!("{next}:refs/heads/dev"),
                        ],
                    );
                }
                "unstaged" | "staged" => {
                    fs::write(dir.path().join("file"), "dirty").unwrap();
                    if mutation == "staged" {
                        git(dir.path(), &["add", "file"]);
                    }
                }
                "untracked" => fs::write(dir.path().join("untracked"), "dirty").unwrap(),
                "merge" => {
                    fs::write(dir.path().join(".git/MERGE_HEAD"), &saved.source_sha).unwrap()
                }
                "rebase" => fs::create_dir(dir.path().join(".git/rebase-merge")).unwrap(),
                _ => unreachable!(),
            }
            let tmp = tempdir().unwrap();
            assert!(
                perform(
                    &saved,
                    "merge",
                    false,
                    &tmp.path().join("merge"),
                    None,
                    || Ok(())
                )
                .is_err(),
                "{mutation}"
            );
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), saved.source_sha);
            assert_eq!(sha(dir.path(), "test").unwrap(), saved.target_sha);
        }
    }

    #[cfg(unix)]
    #[test]
    fn development_sync_preserves_changes_made_by_fast_forward_hook() {
        use std::os::unix::fs::PermissionsExt;
        for action in [
            "printf dirty > file",
            "git switch --detach",
            "git update-ref refs/remotes/origin/dev HEAD^",
            "git remote set-url origin changed",
            "git update-ref refs/heads/test HEAD",
            "exit 1",
        ] {
            let (dir, repo, _remote) = behind_development_fixture(false);
            let saved = snapshot(&repo, "merge").unwrap();
            let hook = dir.path().join(".git/hooks/post-merge");
            fs::create_dir_all(hook.parent().unwrap()).unwrap();
            fs::write(&hook, format!("#!/bin/sh\n{action}\n")).unwrap();
            fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            // Git 本身忽略 post-merge 退出码，故退出码场景只保证提交不丢失。
            let result = perform(&saved, "merge", false, &worktree, None, || Ok(()));
            if action != "exit 1" {
                assert!(result.is_err(), "{action}");
                assert!(worktree.exists());
            }
            assert_eq!(
                sha(dir.path(), "HEAD").unwrap(),
                saved.development_remote_sha
            );
            if action == "printf dirty > file" {
                assert_eq!(
                    fs::read_to_string(dir.path().join("file")).unwrap(),
                    "dirty"
                );
            }
        }
    }

    #[test]
    fn development_sync_old_journal_fields_default() {
        let (_dir, repo) = fixture();
        let mut json = serde_json::to_value(snapshot(&repo, "merge").unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("developmentRemoteSha");
        json.as_object_mut()
            .unwrap()
            .remove("syncDevelopmentBranch");
        let saved: PlanRepository = serde_json::from_value(json).unwrap();
        assert!(saved.development_remote_sha.is_empty());
        assert!(!saved.sync_development_branch);
        assert_eq!(merge_source(&saved), saved.source_sha);
    }

    #[test]
    fn development_sync_conflict_recovery_keeps_confirmed_development() {
        for finish in ["abort", "complete", "incomplete"] {
            let (dir, repo, _remote) = behind_development_fixture(true);
            let saved = snapshot(&repo, "merge").unwrap();
            let upstream = sha(dir.path(), "origin/dev").unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), upstream);
            if finish == "abort" {
                let marker = git(&worktree, &["rev-parse", "--git-path", "MERGE_HEAD"]);
                let marker = worktree.join(marker);
                fs::write(&marker, &saved.source_sha).unwrap();
                assert!(abort_worktree(&saved, &worktree, None).is_err());
                fs::write(&marker, &upstream).unwrap();
                abort_worktree(&saved, &worktree, None).unwrap();
            } else if finish == "complete" {
                fs::write(worktree.join("file"), "resolved\n").unwrap();
                git(&worktree, &["add", "file"]);
                git(&worktree, &["commit", "-m", "resolve"]);
                complete_worktree(&saved, &worktree, None).unwrap();
            } else {
                git(&worktree, &["merge", "--abort"]);
                assert!(complete_worktree(&saved, &worktree, None).is_err());
                abort_worktree(&saved, &worktree, None).unwrap();
            }
            assert_eq!(sha(dir.path(), "HEAD").unwrap(), upstream);
        }
    }

    #[test]
    fn merge_fast_forwards_behind_test_only_after_confirmation_and_preserves_remote_history() {
        let (dir, repo, remote) = behind_merge_fixture(false);
        let before = git(dir.path(), &["show-ref"]);
        let saved = preflight(&[repo.clone()], "merge").unwrap().remove(0);
        assert!(saved.sync_test_branch);
        assert_eq!(git(dir.path(), &["show-ref"]), before);
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        perform(&saved, "merge", false, &worktree, None, || {
            assert_eq!(sha(dir.path(), "test")?, saved.target_sha);
            Ok(())
        })
        .unwrap();
        assert_eq!(git(dir.path(), &["rev-parse", "test^1"]), saved.remote_sha);
        assert_eq!(git(dir.path(), &["rev-parse", "test^2"]), saved.source_sha);
        assert_eq!(
            git(dir.path(), &["show", "test:remote-only"]),
            "remote test commit"
        );
        assert_eq!(
            git(dir.path(), &["show", "test:file"]),
            "approved development"
        );
        assert_eq!(
            sha(remote.path(), "refs/heads/test").unwrap(),
            saved.remote_sha
        );
        assert_eq!(branch(dir.path()), "dev");
        assert_eq!(sha(dir.path(), "HEAD").unwrap(), saved.source_sha);
        assert!(git(dir.path(), &["status", "--porcelain=v1"]).is_empty());
        assert!(!worktree.exists());
    }

    #[test]
    fn merge_sync_rejects_remote_changes_since_confirmation_even_without_external_fetch() {
        for deleted in [false, true] {
            let (dir, repo, remote) = behind_merge_fixture(false);
            let saved = snapshot(&repo, "merge").unwrap();
            if deleted {
                git(remote.path(), &["update-ref", "-d", "refs/heads/test"]);
            } else {
                git(
                    remote.path(),
                    &["update-ref", "refs/heads/test", &saved.target_sha],
                );
            }
            // 本地 tracking ref 仍旧匹配；必须由执行中的 fetch 检测真实远端变化。
            revalidate(&saved, "merge").unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            assert!(perform(&saved, "merge", false, &worktree, None, || Ok(()))
                .unwrap_err()
                .contains("重新准备"));
            assert_eq!(sha(dir.path(), "test").unwrap(), saved.target_sha);
            assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
            abort_worktree(&saved, &worktree, None).unwrap();
            assert!(!worktree.exists());
        }
    }

    #[test]
    fn merge_fetch_cancels_before_and_after_git_starts_without_moving_local_branches() {
        use std::cell::Cell;
        let (dir, repo, _remote) = behind_merge_fixture(false);
        let local = sha(dir.path(), "test").unwrap();
        let source = sha(dir.path(), "dev").unwrap();
        for cancel_at in [1, 3] {
            let checks = Cell::new(0);
            let started = Instant::now();
            let result = fetch_remote_checked(dir.path(), &repo, || {
                checks.set(checks.get() + 1);
                if checks.get() >= cancel_at {
                    Err("任务已取消".into())
                } else {
                    Ok(())
                }
            });
            assert_eq!(result.unwrap_err(), "任务已取消");
            assert_eq!(checks.get(), cancel_at);
            assert!(started.elapsed() < Duration::from_secs(5));
            assert_eq!(sha(dir.path(), "test").unwrap(), local);
            assert_eq!(sha(dir.path(), "dev").unwrap(), source);
            assert_eq!(branch(dir.path()), "dev");
        }
    }

    #[test]
    fn merge_sync_rejects_source_target_tracking_and_identity_changes() {
        for change_kind in ["source", "target", "tracking", "identity"] {
            let (dir, repo, _remote) = behind_merge_fixture(false);
            let saved = snapshot(&repo, "merge").unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            let result = perform(&saved, "merge", false, &worktree, None, || {
                match change_kind {
                    "source" => change(dir.path(), "unapproved development\n"),
                    "target" => {
                        git(
                            dir.path(),
                            &["update-ref", "refs/heads/test", &saved.remote_sha],
                        );
                    }
                    "tracking" => {
                        git(
                            dir.path(),
                            &["update-ref", "refs/remotes/origin/test", &saved.target_sha],
                        );
                    }
                    "identity" => {
                        git(
                            dir.path(),
                            &["remote", "set-url", "origin", tmp.path().to_str().unwrap()],
                        );
                    }
                    _ => unreachable!(),
                }
                Ok(())
            });
            assert!(result.is_err(), "{change_kind}");
            if worktree.exists() {
                assert_ne!(sha(&worktree, "HEAD").unwrap(), saved.source_sha);
            }
            assert_eq!(git(dir.path(), &["show", "test:file"]), "base");
        }
    }

    #[test]
    fn merge_sync_conflict_abort_keeps_fast_forward_and_rejects_committed_resolution() {
        for committed in [false, true] {
            let (dir, repo, remote) = behind_merge_fixture(true);
            let saved = snapshot(&repo, "merge").unwrap();
            let tmp = tempdir().unwrap();
            let worktree = tmp.path().join("merge");
            assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
            assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.remote_sha);
            assert_eq!(branch(dir.path()), "dev");
            if committed {
                fs::write(worktree.join("file"), "resolved\n").unwrap();
                git(&worktree, &["add", "file"]);
                git(&worktree, &["commit", "-m", "resolve"]);
                assert!(abort_worktree(&saved, &worktree, None).is_err());
                complete_worktree(&saved, &worktree, None).unwrap();
            } else {
                abort_worktree(&saved, &worktree, None).unwrap();
                assert_eq!(sha(dir.path(), "test").unwrap(), saved.remote_sha);
                assert_eq!(git(dir.path(), &["show", "test:file"]), "remote test edit");
            }
            assert_eq!(
                sha(remote.path(), "refs/heads/test").unwrap(),
                saved.remote_sha
            );
            assert!(!worktree.exists());
        }
    }

    #[test]
    fn completed_repository_is_not_rolled_back_when_next_repository_fails() {
        let (first, repo) = fixture();
        let (second, mut other) = fixture();
        other.id = "second".into();
        change(first.path(), "first change\n");
        let saved = preflight(&[repo, other], "merge").unwrap();
        fs::write(second.path().join("file"), "new dirty state").unwrap();
        let tmp = tempdir().unwrap();
        perform(
            &saved[0],
            "merge",
            false,
            &tmp.path().join("first"),
            None,
            || Ok(()),
        )
        .unwrap();
        assert!(perform(
            &saved[1],
            "merge",
            false,
            &tmp.path().join("second"),
            None,
            || Ok(())
        )
        .is_err());
        assert_eq!(git(first.path(), &["show", "test:file"]), "first change");
        assert_eq!(
            fs::read_to_string(second.path().join("file")).unwrap(),
            "new dirty state"
        );
    }
    #[test]
    fn attached_target_and_stale_source_are_rejected() {
        let (dir, repo) = fixture();
        let saved = snapshot(&repo, "merge").unwrap();
        change(dir.path(), "changed source\n");
        assert!(revalidate(&saved, "merge").is_err());
        let external = tempdir().unwrap();
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                external.path().join("test").to_str().unwrap(),
                "test",
            ],
        );
        assert!(snapshot(&repo, "merge").is_err());
        let mut duplicate = repo.clone();
        duplicate.id = "other-worktree".into();
        duplicate.path = external.path().join("test").to_string_lossy().into();
        duplicate.development_branch = "test".into();
        assert!(preflight(&[repo, duplicate], "switch").is_err());
    }
    #[test]
    fn cancellation_stops_a_managed_query_without_modifying_files() {
        let (dir, _) = fixture();
        let calls = std::cell::Cell::new(0);
        let result = run_checked(dir.path(), &["status", "--porcelain"], || {
            calls.set(calls.get() + 1);
            if calls.get() > 1 {
                Err("任务已取消".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err(), "任务已取消");
        assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
    }

    #[test]
    fn manual_completion_requires_committed_clean_worktree() {
        let (dir, repo) = fixture();
        git(dir.path(), &["switch", "test"]);
        change(dir.path(), "test edit\n");
        git(dir.path(), &["switch", "dev"]);
        change(dir.path(), "dev edit\n");
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("conflict");
        assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
        assert!(complete_worktree(&saved, &worktree, None).is_err());
        fs::write(worktree.join("file"), "resolved\n").unwrap();
        git(&worktree, &["add", "file"]);
        git(&worktree, &["commit", "-m", "resolved"]);
        fs::write(worktree.join("extra"), "keep").unwrap();
        assert!(complete_worktree(&saved, &worktree, None).is_err());
        fs::remove_file(worktree.join("extra")).unwrap();
        complete_worktree(&saved, &worktree, None).unwrap();
        assert!(!worktree.exists());
        assert_eq!(git(dir.path(), &["show", "test:file"]), "resolved");
    }
    #[test]
    fn normal_merge_keeps_both_parent_histories() {
        let (dir, repo) = fixture();
        git(dir.path(), &["switch", "test"]);
        change(dir.path(), "test edit\n");
        let test_sha = git(dir.path(), &["rev-parse", "HEAD"]);
        git(dir.path(), &["switch", "dev"]);
        fs::write(dir.path().join("development"), "source").unwrap();
        git(dir.path(), &["add", "development"]);
        git(dir.path(), &["commit", "-m", "dev"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        perform(
            &saved,
            "merge",
            false,
            &tmp.path().join("merge"),
            None,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(git(dir.path(), &["rev-parse", "test^1"]), test_sha);
        assert_eq!(git(dir.path(), &["rev-parse", "test^2"]), saved.source_sha);
    }

    #[test]
    fn pom_conflict_journal_rebuilds_previews_without_persisting_bodies() {
        for (paths, also_conflicts_in_code) in [
            (&["pom.xml"][..], false),
            (&["module/pom.xml", "pom.xml"][..], true),
        ] {
            let (_dir, repo) = pom_fixture(paths, also_conflicts_in_code, false);
            let saved = snapshot(&repo, "merge").unwrap();
            let tmp = tempdir().unwrap();
            let mut entry = paused_batch();
            entry.plan.repositories = vec![saved.clone()];
            entry.state.repositories.truncate(1);
            let result = &mut entry.state.repositories[0];
            result.repo_id = repo.id.clone();
            result.status = "error".into();
            let operation = tmp.path().join(&entry.plan.plan_id);
            fs::create_dir(&operation).unwrap();
            let worktree = operation.join("0");
            assert!(perform(&saved, "merge", false, &worktree, None, || Ok(())).is_err());
            result.worktree_path = worktree.to_string_lossy().into();
            result.pom_conflict_files = read_pom_conflict_files(&saved, &worktree, None).unwrap();
            let expected = result.pom_conflict_files.clone();
            if !also_conflicts_in_code {
                let (choice, snapshot) = inspect_pom_conflict(&saved, &worktree, None).unwrap();
                result.pom_conflict = Some(choice);
                entry.pom_conflicts.insert(repo.id, snapshot);
            }
            let response = serde_json::to_value(result).unwrap();
            assert_eq!(
                response["pomConflictFiles"][0]["blocks"][0],
                serde_json::json!({
                    "line": 1, "testContent": "test pom\n", "developmentContent": "development pom\n"
                })
            );
            assert_eq!(
                response["pomConflictFiles"][0]["branchDiff"],
                "@@ -1 +1 @@\n-test pom\n+development pom\n"
            );
            let contents = journal_json(&entry).unwrap();
            assert!(!contents.contains("test pom"));
            assert!(!contents.contains("development pom"));
            assert!(!contents.contains("pomConflictFiles"));
            assert!(!contents.contains("branchDiff"));
            let journal = operation.join("operation.json");
            fs::write(&journal, contents).unwrap();
            let restored = restore_entry(&journal).unwrap();
            let result = &restored.state.repositories[0];
            assert_eq!(result.pom_conflict_files, expected);
            assert_eq!(
                result.pom_conflict.as_ref().map(|choice| choice.push),
                (!also_conflicts_in_code).then_some(true)
            );

            // 已手工暂存解决的 POM 不应在下次启动后继续显示。
            fs::write(worktree.join("pom.xml"), "resolved\n").unwrap();
            git(&worktree, &["add", "--", "pom.xml"]);
            let restored = restore_entry(&journal).unwrap();
            let result = &restored.state.repositories[0];
            assert_eq!(
                result.pom_conflict_files,
                expected
                    .into_iter()
                    .filter(|file| file.path != "pom.xml")
                    .collect::<Vec<_>>()
            );
            assert!(result.pom_conflict.is_none());
        }
    }

    #[test]
    fn journal_restores_as_paused_and_rejects_path_escape() {
        let (_dir, repo) = fixture();
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let operation = tmp.path().join(&id);
        fs::create_dir(&operation).unwrap();
        let mut entry = Entry {
            plan: Plan {
                plan_id: id.clone(),
                project_id: "p".into(),
                operation: "merge".into(),
                push: true,
                keep_test_pom: false,
                repositories: vec![saved],
            },
            state: OperationState {
                plan_id: id.clone(),
                task_id: "old-task".into(),
                status: "running".into(),
                repositories: vec![RepositoryResult {
                    repo_id: repo.id,
                    status: "running".into(),
                    message: String::new(),
                    worktree_path: operation.join("0").to_string_lossy().into(),
                    pom_conflict: None,
                    pom_conflict_files: Vec::new(),
                    pom_blocks_resolution: None,
                }],
            },
            continuation: None,
            pom_conflicts: HashMap::new(),
            pom_blocks_snapshots: HashMap::new(),
        };
        let journal = operation.join("operation.json");
        let mut legacy = serde_json::to_value(&entry).unwrap();
        legacy["plan"]
            .as_object_mut()
            .unwrap()
            .remove("keepTestPom");
        fs::write(&journal, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let restored = restore_entry(&journal).unwrap();
        assert!(!restored.plan.keep_test_pom);
        assert_eq!(restored.state.status, "paused");
        assert_eq!(restored.state.repositories[0].status, "error");
        entry.state.repositories[0].worktree_path =
            tmp.path().join("outside").to_string_lossy().into();
        fs::write(&journal, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(restore_entry(&journal).is_err());
    }
    fn paused_batch() -> Entry {
        let (_dir, repo) = fixture();
        let saved = snapshot(&repo, "merge").unwrap();
        let ids = ["first", "failed", "last", "middle"];
        let id = uuid::Uuid::new_v4().to_string();
        let entry = serde_json::json!({
            "plan": {
                "planId": id, "projectId": "p", "operation": "merge", "push": true,
                "keepTestPom": true,
                "repositories": ids.map(|id| {
                    let mut saved = saved.clone();
                    saved.repository.id = id.into();
                    saved
                })
            },
            "state": {
                "planId": id, "taskId": "old-task", "status": "paused",
                "repositories": ids.into_iter().zip(["success", "error", "pending", "pending"])
                    .map(|(id, status)| serde_json::json!({
                        "repoId": id, "status": status, "message": "", "worktreePath": ""
                    })).collect::<Vec<_>>()
            }
        });
        serde_json::from_value(entry).unwrap()
    }

    #[test]
    fn worktree_ide_uses_the_installed_application_bundle_ids() {
        assert_eq!(
            WorktreeIde::WebStorm.bundle_ids(),
            ["com.jetbrains.WebStorm"]
        );
        assert_eq!(
            WorktreeIde::Idea.bundle_ids(),
            ["com.jetbrains.intellij", "com.jetbrains.intellij.ce"]
        );
    }

    #[test]
    fn ide_worktree_uses_the_retained_directory_at_the_recorded_repository_index() {
        let tmp = tempdir().unwrap();
        let mut batch = paused_batch();
        let operation = tmp.path().join(&batch.state.plan_id);
        let worktree = operation.join("1");
        fs::create_dir_all(&worktree).unwrap();
        batch.state.repositories[1].worktree_path = worktree.to_string_lossy().into();

        assert_eq!(
            retained_worktree_for_ide(tmp.path(), &batch.state, "failed").unwrap(),
            worktree
        );
        assert_eq!(
            retained_worktree_for_ide(tmp.path(), &batch.state, "last").unwrap_err(),
            "该仓库没有本次操作工作区"
        );
        assert_eq!(
            retained_worktree_for_ide(tmp.path(), &batch.state, "unknown").unwrap_err(),
            "仓库不在计划中"
        );
        for status in ["running", "aborting"] {
            batch.state.status = status.into();
            assert_eq!(
                retained_worktree_for_ide(tmp.path(), &batch.state, "failed").unwrap_err(),
                "请先等待任务结束"
            );
        }
    }

    #[test]
    fn ide_worktree_rejects_unavailable_or_mismatched_directories() {
        let tmp = tempdir().unwrap();
        let mut batch = paused_batch();
        let operation = tmp.path().join(&batch.state.plan_id);
        let worktree = operation.join("1");
        let other = operation.join("0");
        fs::create_dir_all(&other).unwrap();
        batch.state.repositories[1].worktree_path = other.to_string_lossy().into();
        assert!(retained_worktree_for_ide(tmp.path(), &batch.state, "failed").is_err());

        batch.state.repositories[1].worktree_path = worktree.to_string_lossy().into();
        assert!(retained_worktree_for_ide(tmp.path(), &batch.state, "failed").is_err());
        fs::write(&worktree, "not a directory").unwrap();
        assert!(retained_worktree_for_ide(tmp.path(), &batch.state, "failed").is_err());

        #[cfg(unix)]
        {
            fs::remove_file(&worktree).unwrap();
            std::os::unix::fs::symlink(&other, &worktree).unwrap();
            assert!(retained_worktree_for_ide(tmp.path(), &batch.state, "failed").is_err());
        }
    }

    #[test]
    fn auto_resume_selects_only_pending_in_original_order_after_successful_cleanup() {
        let mut batch = paused_batch();
        batch.state.task_id = "resolve-task".into();
        batch.state.status = "aborting".into();
        batch.state.repositories[1].worktree_path = "/old/worktree".into();
        for saved in &mut batch.plan.repositories {
            saved.remote_identity = 1;
        }
        for action in [
            WorktreeAction::Complete,
            WorktreeAction::ResolvePom(PomSide::Test),
            WorktreeAction::ResolvePomBlocks(Vec::new()),
        ] {
            let mut entry = batch.clone();
            let original_plan = serde_json::to_value(&entry.plan).unwrap();
            assert_eq!(
                record_worktree_outcome(&mut entry, 1, &action, &Ok(false)),
                vec![2, 3]
            );
            assert_eq!(entry.state.status, "running");
            assert_eq!(entry.state.task_id, "resolve-task");
            assert_eq!(entry.state.repositories[1].status, "success");
            assert!(entry.state.repositories[1].worktree_path.is_empty());
            assert_eq!(serde_json::to_value(&entry.plan).unwrap(), original_plan);
            assert_eq!(entry.state.repositories[0].status, "success");
        }
    }

    #[test]
    fn auto_resume_keeps_pending_after_abort_failure_or_other_conflicts() {
        let mut batch = paused_batch();
        batch.state.repositories[1].worktree_path = "/old/worktree".into();
        for saved in &mut batch.plan.repositories {
            saved.remote_identity = 1;
        }
        for (action, outcome, expect_worktree) in [
            (WorktreeAction::Abort, Ok(false), false),
            (WorktreeAction::Complete, Err("失败".into()), true),
            (WorktreeAction::ResolvePomBlocks(Vec::new()), Ok(true), true),
        ] {
            let mut entry = batch.clone();
            assert!(record_worktree_outcome(&mut entry, 1, &action, &outcome).is_empty());
            assert_eq!(entry.state.status, "paused");
            assert_eq!(
                !entry.state.repositories[1].worktree_path.is_empty(),
                expect_worktree
            );
            assert_eq!(entry.state.repositories[2].status, "pending");
            assert_eq!(entry.state.repositories[3].status, "pending");
        }
        batch.state.repositories[0].worktree_path = "/other/worktree".into();
        assert!(
            record_worktree_outcome(&mut batch, 1, &WorktreeAction::Complete, &Ok(false))
                .is_empty()
        );
        assert_eq!(batch.state.status, "paused");
        assert_eq!(batch.state.repositories[0].worktree_path, "/other/worktree");
    }

    #[test]
    fn auto_resume_requires_verified_original_remote_identity_for_all_pending() {
        let mut entry = paused_batch();
        entry.state.repositories[1].status = "success".into();
        let (local, repo) = fixture();
        let remote = tempdir().unwrap();
        git(
            local.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let mut saved = snapshot(&repo, "merge").unwrap();
        saved.repository.id = entry.plan.repositories[2].repository.id.clone();
        entry.plan.repositories[2] = saved;
        entry.plan.repositories[3].remote_identity = 1;
        let dir = tempdir().unwrap();
        let path = dir.path().join(&entry.plan.plan_id).join("operation.json");
        let json = journal_json(&entry).unwrap();
        assert!(!json.contains(remote.path().to_str().unwrap()));
        assert!(!serde_json::to_string(&entry.plan)
            .unwrap()
            .contains("remoteIdentity"));
        crate::config::atomic_write(&path, &json).unwrap();
        let restored = restore_entry(&path).unwrap();
        assert_eq!(restored.state.status, "paused");
        assert_eq!(auto_resume_indices(&restored).unwrap(), vec![2, 3]);
        let restored_pending = &restored.plan.repositories[2];
        revalidate(restored_pending, "merge").unwrap();
        let changed_remote = tempdir().unwrap();
        git(
            local.path(),
            &[
                "remote",
                "set-url",
                "origin",
                changed_remote.path().to_str().unwrap(),
            ],
        );
        assert!(revalidate(restored_pending, "merge").is_err());

        fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        let mut legacy = restore_entry(&path).unwrap();
        assert!(
            record_worktree_outcome(&mut legacy, 1, &WorktreeAction::Complete, &Ok(false))
                .is_empty()
        );
        assert!(legacy.state.repositories[1].message.contains("「继续」"));
        assert_eq!(pending_repositories(&legacy).unwrap().count(), 2);

        entry.plan.repositories[3].remote_identity = 0;
        assert!(auto_resume_indices(&entry).is_err());
    }

    #[test]
    fn remaining_preflight_uses_legacy_options_and_pending_order_with_current_configuration() {
        let entry = paused_batch();
        let mut middle = entry.plan.repositories[3].repository.clone();
        middle.test_branch = "new-test".into();
        let last = entry.plan.repositories[2].repository.clone();
        let configured = [middle.clone(), last.clone()];

        let repositories = remaining_repositories(&entry, &configured).unwrap();
        assert!(entry.plan.push && entry.plan.keep_test_pom);
        assert_eq!(entry.plan.operation, "merge");
        assert_eq!(repositories[0].id, last.id);
        assert_eq!(repositories[1].id, middle.id);
        assert_eq!(repositories[1].test_branch, "new-test");
        assert!(remaining_repositories(&entry, &[last]).is_err());
    }

    #[test]
    fn remaining_preflight_requires_paused_pending_and_no_open_worktree() {
        let mut entry = paused_batch();
        let configured: Vec<_> = entry
            .plan
            .repositories
            .iter()
            .map(|r| r.repository.clone())
            .collect();
        for status in ["prepared", "running", "aborting", "cancelled", "success"] {
            entry.state.status = status.into();
            assert!(
                remaining_repositories(&entry, &configured).is_err(),
                "{status}"
            );
        }
        entry.state.status = "paused".into();
        entry.state.repositories[1].worktree_path = "/old/worktree".into();
        assert!(remaining_repositories(&entry, &configured).is_err());
        entry.state.repositories[1].worktree_path.clear();
        for result in &mut entry.state.repositories {
            if result.status == "pending" {
                result.status = "aborted".into();
            }
        }
        assert!(remaining_repositories(&entry, &configured).is_err());
    }

    #[test]
    fn remaining_preflight_rejects_unconfirmed_legacy_plan_after_restore() {
        let mut source = paused_batch();
        source.state.task_id.clear();
        source.state.status = "prepared".into();
        for result in &mut source.state.repositories {
            result.status = "pending".into();
        }
        let temp = tempdir().unwrap();
        let journal = temp
            .path()
            .join(&source.plan.plan_id)
            .join("operation.json");
        crate::config::atomic_write(&journal, &serde_json::to_string(&source).unwrap()).unwrap();
        let restored = restore_entry(&journal).unwrap();
        assert_eq!(restored.state.status, "paused");
        assert!(pending_repositories(&restored).is_err());
    }

    #[test]
    fn remaining_preflight_recovers_handoff_with_all_repositories_pending() {
        let source = paused_batch();
        let source_id = source.plan.plan_id.clone();
        let mut child = source.clone();
        let child_id = uuid::Uuid::new_v4().to_string();
        child.plan.plan_id = child_id.clone();
        child.plan.repositories.drain(..2);
        child.state.plan_id = child_id.clone();
        child.state.repositories.drain(..2);
        child.state.task_id.clear();
        child.state.status = "prepared".into();
        child.continuation = Some(Continuation {
            source_plan_id: source_id.clone(),
            started: false,
        });
        let temp = tempdir().unwrap();
        let persist = |entry: &Entry| {
            crate::config::atomic_write(
                &temp.path().join(&entry.plan.plan_id).join("operation.json"),
                &serde_json::to_string(entry).unwrap(),
            )
        };
        persist(&source).unwrap();
        persist(&child).unwrap();
        let mut restored = HashMap::new();
        load_operations(temp.path(), &mut restored, "p").unwrap();
        assert!(continuation_source(&restored, &source_id).is_ok());
        assert!(!restored.contains_key(&child_id));

        child.state.status = "running".into();
        assert!(start_continuation(&mut child, "new-task", |_| Err("落盘失败".into())).is_err());
        assert!(!child.continuation.as_ref().unwrap().started);
        assert!(child.state.task_id.is_empty());
        restored.insert(child_id.clone(), child.clone());
        assert!(continuation_source(&restored, &source_id).is_err());

        start_continuation(&mut child, "new-task", persist).unwrap();
        restored.clear();
        load_operations(temp.path(), &mut restored, "p").unwrap();
        assert!(continuation_source(&restored, &source_id).is_err());
        let recovered = continuation_source(&restored, &child_id).unwrap();
        assert_eq!(recovered.state.task_id, "new-task");
        assert_eq!(pending_repositories(recovered).unwrap().count(), 2);

        child.state.status = "success".into();
        persist(&child).unwrap();
        restored.clear();
        load_operations(temp.path(), &mut restored, "p").unwrap();
        assert!(continuation_source(&restored, &source_id).is_err());
    }

    #[test]
    fn clear_history_preserves_recovery_active_operations_and_other_projects() {
        let temp = tempdir().unwrap();
        let template = paused_batch();
        let record = |status: &str, project: &str, pending: bool, worktree: bool| {
            let mut entry = template.clone();
            let id = uuid::Uuid::new_v4().to_string();
            entry.plan.plan_id = id.clone();
            entry.plan.project_id = project.into();
            entry.state.plan_id = id.clone();
            entry.state.status = status.into();
            let dir = temp.path().join(&id);
            fs::create_dir(&dir).unwrap();
            if !pending {
                for repo in &mut entry.state.repositories {
                    repo.status = "success".into();
                }
            }
            if worktree {
                let path = dir.join("0");
                fs::create_dir(&path).unwrap();
                entry.state.repositories[0].worktree_path = path.to_string_lossy().into();
            }
            fs::write(dir.join("operation.json"), journal_json(&entry).unwrap()).unwrap();
            entry
        };
        let success = record("success", "p", false, false);
        let cancelled = record("cancelled", "p", true, false);
        let ended = record("paused", "p", false, false);
        let running = record("running", "p", true, false);
        let resumable = record("paused", "p", true, false);
        let retained = record("cancelled", "p", true, true);
        let other = record("success", "other", false, false);
        let mut entries = HashMap::from([(success.plan.plan_id.clone(), success.clone())]);

        let removed = clear_operation_records(temp.path(), &mut entries, "p", None).unwrap();
        assert_eq!(removed.len(), 3);
        for entry in [success, cancelled, ended] {
            assert!(removed.contains(&entry.plan.plan_id));
            assert!(!entries.contains_key(&entry.plan.plan_id));
            assert!(!temp
                .path()
                .join(entry.plan.plan_id)
                .join("operation.json")
                .exists());
        }
        for entry in [running, resumable, retained, other] {
            assert!(temp
                .path()
                .join(entry.plan.plan_id)
                .join("operation.json")
                .is_file());
        }
        assert!(
            clear_operation_records(temp.path(), &mut entries, "p", None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn clear_history_discards_only_the_selected_resumable_batch_without_a_worktree() {
        let temp = tempdir().unwrap();
        let selected = paused_batch();
        let unselected = paused_batch();
        let mut retained = paused_batch();
        let mut other_project = paused_batch();
        other_project.plan.project_id = "other".into();
        let worktree = temp.path().join(&retained.plan.plan_id).join("1");
        fs::create_dir_all(&worktree).unwrap();
        retained.state.repositories[1].worktree_path = worktree.to_string_lossy().into();
        for entry in [&selected, &unselected, &retained, &other_project] {
            let directory = temp.path().join(&entry.plan.plan_id);
            fs::create_dir_all(&directory).unwrap();
            fs::write(
                directory.join("operation.json"),
                journal_json(entry).unwrap(),
            )
            .unwrap();
        }
        let mut entries = HashMap::new();
        load_operations(temp.path(), &mut entries, "p").unwrap();
        let clear = |entries: &mut HashMap<String, Entry>, id: &str| {
            clear_operation_records(temp.path(), entries, "p", Some(id)).unwrap()
        };
        assert!(clear(&mut entries, &retained.plan.plan_id).is_empty());
        assert!(clear(&mut entries, &other_project.plan.plan_id).is_empty());
        let selected_id = &selected.plan.plan_id;
        assert_eq!(clear(&mut entries, selected_id), [selected_id.clone()]);
        assert!(!entries.contains_key(selected_id));
        assert!(entries.contains_key(&unselected.plan.plan_id));
        assert!(worktree.is_dir());
        entries.clear();
        load_operations(temp.path(), &mut entries, "p").unwrap();
        assert!(!entries.contains_key(selected_id));
        assert!(entries.contains_key(&unselected.plan.plan_id));
    }

    #[test]
    fn clear_history_removes_handoffs_only_after_their_source() {
        let temp = tempdir().unwrap();
        let source = paused_batch();
        let source_id = source.plan.plan_id.clone();
        let mut child = source.clone();
        let child_id = uuid::Uuid::new_v4().to_string();
        child.plan.plan_id = child_id.clone();
        child.state.plan_id = child_id.clone();
        child.state.status = "success".into();
        for repo in &mut child.state.repositories {
            repo.status = "success".into();
        }
        child.continuation = Some(Continuation {
            source_plan_id: source_id.clone(),
            started: true,
        });
        for entry in [&source, &child] {
            let directory = temp.path().join(&entry.plan.plan_id);
            fs::create_dir(&directory).unwrap();
            fs::write(
                directory.join("operation.json"),
                journal_json(entry).unwrap(),
            )
            .unwrap();
        }
        let unrecorded_worktree = temp.path().join(&source_id).join("0");
        fs::create_dir(&unrecorded_worktree).unwrap();
        let mut entries = HashMap::new();
        assert!(
            clear_operation_records(temp.path(), &mut entries, "p", None)
                .unwrap()
                .is_empty()
        );
        load_operations(temp.path(), &mut entries, "p").unwrap();
        assert!(continuation_source(&entries, &source_id).is_err());
        fs::remove_dir(unrecorded_worktree).unwrap();
        assert_eq!(
            clear_operation_records(temp.path(), &mut entries, "p", None).unwrap(),
            [source_id, child_id]
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn changing_remote_identity_invalidates_the_plan() {
        let (dir, repo) = fixture();
        let one = tempdir().unwrap();
        let two = tempdir().unwrap();
        git(
            dir.path(),
            &["remote", "add", "origin", one.path().to_str().unwrap()],
        );
        let saved = snapshot(&repo, "merge").unwrap();
        git(
            dir.path(),
            &["remote", "set-url", "origin", two.path().to_str().unwrap()],
        );
        assert!(revalidate(&saved, "merge").is_err());
        let json = serde_json::to_string(&saved).unwrap();
        assert!(!json.contains("remoteIdentity"));
        assert!(!json.contains(one.path().to_str().unwrap()));
    }
    #[test]
    fn repository_defaults_and_readonly_status_are_preserved() {
        let dto: Repository =
            serde_json::from_str(r#"{"id":"r","name":"r","path":"fixture"}"#).unwrap();
        assert_eq!(dto.remote, "origin");
        assert!(dto.test_branch.is_empty());
        let (dir, repo) = fixture();
        let index = fs::read(dir.path().join(".git/index")).unwrap();
        let rows = statuses(vec![repo]);
        assert!(!rows[0].dirty);
        assert!(rows[0].error.is_empty());
        assert_eq!(fs::read(dir.path().join(".git/index")).unwrap(), index);
    }
    #[test]
    fn repository_remote_urls_read_local_worktrees_and_redact_credentials() {
        let (dir, repo) = fixture();
        git(
            dir.path(),
            &["remote", "add", "upstream", "scm:service.git"],
        );
        git(
            dir.path(),
            &[
                "config",
                "url.https://user:secret@example.invalid/.insteadOf",
                "scm:",
            ],
        );
        let worktrees = tempdir().unwrap();
        let worktree = worktrees.path().join("linked tree");
        git(
            dir.path(),
            &["worktree", "add", worktree.to_str().unwrap(), "test"],
        );
        let linked = Repository {
            id: "linked".into(),
            path: worktree.to_string_lossy().into(),
            remote: "upstream".into(),
            scm_git_id: "  ".into(),
            ..repo.clone()
        };
        let nested = dir.path().join("nested");
        fs::create_dir(&nested).unwrap();
        git(&nested, &["init", "--template="]);
        git(
            &nested,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/nested.git?token=secret#fragment",
            ],
        );
        let nested = Repository {
            id: "nested".into(),
            path: nested.to_string_lossy().into(),
            ..repo
        };
        assert_eq!(
            repository_remote_urls(vec![linked, nested]),
            HashMap::from([
                (
                    "linked".into(),
                    "https://example.invalid/service.git".into()
                ),
                ("nested".into(), "https://example.invalid/nested.git".into()),
            ]),
        );
    }

    #[test]
    fn repository_remote_urls_skip_mapped_repositories_and_invalid_results() {
        let (dir, repo) = fixture();
        for (key, value) in [
            ("remote.origin.url", "git@example.invalid:service.git"),
            ("remote.same-name.url", "same-name"),
            ("remote.empty.url", ""),
            ("remote.blank.url", "   "),
            ("remote.redacted.url", "?token=secret"),
            (
                "remote.bad..name.url",
                "https://example.invalid/invalid.git",
            ),
        ] {
            git(dir.path(), &["config", key, value]);
        }
        let nested = dir.path().join("ordinary-directory");
        fs::create_dir(&nested).unwrap();
        let mut repositories = vec![
            repo.clone(),
            Repository {
                id: "mapped".into(),
                scm_git_id: " 123 ".into(),
                ..repo.clone()
            },
            Repository {
                id: "ordinary-directory".into(),
                path: nested.to_string_lossy().into(),
                ..repo.clone()
            },
            Repository {
                id: "missing-directory".into(),
                path: nested.join("missing").to_string_lossy().into(),
                ..repo.clone()
            },
        ];
        for remote in [
            "same-name",
            "empty",
            "blank",
            "redacted",
            "missing",
            "--all",
            "bad..name",
        ] {
            repositories.push(Repository {
                id: remote.into(),
                remote: remote.into(),
                ..repo.clone()
            });
        }
        assert_eq!(
            repository_remote_urls(repositories),
            HashMap::from([
                ("r".into(), "git@example.invalid:service.git".into()),
                ("same-name".into(), "same-name".into()),
            ]),
        );
    }

    #[test]
    fn command_is_literal_and_credentials_are_redacted() {
        let dir = tempdir().unwrap();
        let mut command = git_command(dir.path());
        command.args(["switch", "--", "a;$(touch bad)"]);
        assert_eq!(command.get_args().last().unwrap(), "a;$(touch bad)");
        assert_eq!(redact("https://user:secret@host/repo"), "https://host/repo");
        assert_eq!(
            redact("https://host/repo?token=secret"),
            "https://host/repo"
        );
    }
}
