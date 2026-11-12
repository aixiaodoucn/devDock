use crate::{config::Result, tasks};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
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
    pub target_branch: String,
    pub target_sha: String,
    pub create_tracking: bool,
    pub remote_sha: String,
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
pub struct RepositoryResult {
    pub repo_id: String,
    pub status: String,
    pub message: String,
    pub worktree_path: String,
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
struct Entry {
    plan: Plan,
    state: OperationState,
}
#[derive(Default)]
pub struct Runtime {
    entries: Mutex<HashMap<String, Entry>>,
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
    String::from_utf8(bytes)
        .map(|s| s.trim_end_matches(['\r', '\n']).to_string())
        .map_err(|_| "Git 输出不是有效 UTF-8".into())
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
fn validate_remote(repo: &Repository) -> Result<()> {
    let path = Path::new(&repo.path);
    if repo.remote.is_empty()
        || repo.remote.starts_with('-')
        || repo.remote.contains(['/', '\\', ':'])
    {
        return Err("远端名称无效".into());
    }
    valid_branch(path, &format!("{}/probe", repo.remote))?;
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
    if !["switch", "switch-test", "merge", "push-dev"].contains(&operation) {
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
    if !["switch", "switch-test", "merge", "push-dev"].contains(&operation) {
        return Err("未知 Git 操作".into());
    }
    let path = Path::new(&repo.path);
    let status = status(repo)?;
    if status.dirty || status.merging || status.rebasing {
        return Err("仓库有未提交修改或正在 merge/rebase".into());
    }
    // 测试切换只依赖测试目标，保留真实当前分支作为重验来源。
    if operation != "switch-test" {
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
            target_branch: repo.development_branch.clone(),
            target_sha: remote_sha.clone(),
            create_tracking: false,
            remote_sha,
            remote_identity: remote_identity(repo),
        });
    }
    let target = if operation == "switch" {
        &repo.development_branch
    } else {
        &repo.test_branch
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
    // 允许保留本地领先提交；落后或分叉必须由用户明确整理，绝不自动覆盖。
    if operation == "merge"
        && !remote_sha.is_empty()
        && target_sha != remote_sha
        && run(
            path,
            &["merge-base", "--is-ancestor", &remote_sha, &target_sha],
            None,
        )
        .is_err()
    {
        return Err("本地测试分支落后于远端或已分叉，请先同步或处理分支".into());
    }
    let common = run(path, &["rev-parse", "--git-common-dir"], None)?;
    Ok(PlanRepository {
        repository: repo.clone(),
        root: status.root,
        common_dir: canonical(&path.join(common))?,
        current_branch: status.current_branch,
        source_sha: status.head,
        target_branch: target.clone(),
        target_sha,
        create_tracking,
        remote_sha,
        remote_identity: remote_identity(repo),
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
        match snapshot_with_task(repo, operation, task) {
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
    if now.root != saved.root
        || now.common_dir != saved.common_dir
        || now.current_branch != saved.current_branch
        || now.source_sha != saved.source_sha
        || now.target_sha != saved.target_sha
        || now.create_tracking != saved.create_tracking
        || now.remote_sha != saved.remote_sha
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
    validate_options(&operation, push, keep_test_pom)?;
    if repositories.is_empty() {
        return Err("无效操作或未选择仓库".into());
    }
    let mut fetch_repositories = Vec::new();
    for repo in &repositories {
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
                run(
                    Path::new(&repo.path),
                    &[
                        "fetch",
                        "--prune",
                        "--no-tags",
                        "--no-recurse-submodules",
                        "--",
                        &repo.remote,
                        &format!("+refs/heads/*:refs/remotes/{}/*", repo.remote),
                    ],
                    Some(task),
                )?;
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
            })
            .collect(),
    };
    let entry = Entry {
        plan: plan.clone(),
        state,
    };
    persist_entry(app, &entry)?;
    app.state::<Runtime>()
        .entries
        .lock()
        .unwrap()
        .insert(plan.plan_id.clone(), entry);
    Ok(plan)
}

fn persist_entry(app: &AppHandle, entry: &Entry) -> Result<()> {
    let path = tasks::data_dir(app)?
        .join("backend-worktrees")
        .join(&entry.plan.plan_id)
        .join("operation.json");
    let text = serde_json::to_string(entry).map_err(|_| "无法序列化 Git 记录".to_string())?;
    crate::config::atomic_write(&path, &text).map_err(|_| "无法保存 Git 操作记录".into())
}
fn persist_current(app: &AppHandle, id: &str) -> Result<()> {
    let runtime = app.state::<Runtime>();
    let entries = runtime.entries.lock().unwrap();
    persist_entry(app, entries.get(id).ok_or("计划不存在")?)
}
fn restore_entry(path: &Path) -> Result<Entry> {
    let bytes = fs::read(path).map_err(|_| "无法读取 Git 操作记录".to_string())?;
    let mut entry: Entry =
        serde_json::from_slice(&bytes).map_err(|_| "Git 操作记录损坏，已保留".to_string())?;
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
    for (index, result) in entry.state.repositories.iter_mut().enumerate() {
        let expected = path.parent().unwrap().join(index.to_string());
        if result.repo_id != entry.plan.repositories[index].repository.id
            || (!result.worktree_path.is_empty() && Path::new(&result.worktree_path) != expected)
        {
            return Err("Git 操作工作区记录无效".into());
        }
        if result.status == "running" {
            result.status = "error".into();
            result.message = "应用曾退出，请检查保留的工作区".into();
        }
    }
    // 仅恢复查看/处理入口，不允许重跑过期的 plan 或自动 push。
    if entry.state.status != "success" {
        entry.state.status = "paused".into();
    }
    Ok(entry)
}
pub fn list_operations(app: &AppHandle, project_id: String) -> Result<Vec<OperationState>> {
    let base = tasks::data_dir(app)?.join("backend-worktrees");
    let runtime = app.state::<Runtime>();
    let mut entries = runtime.entries.lock().unwrap();
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
            let entry = restore_entry(&path)?;
            if entry.plan.project_id == project_id && entry.state.status != "success" {
                entries.entry(entry.plan.plan_id.clone()).or_insert(entry);
            }
        }
    }
    let mut result: Vec<_> = entries
        .values()
        .filter(|e| e.plan.project_id == project_id && e.state.status != "success")
        .map(|e| e.state.clone())
        .collect();
    result.sort_by(|a, b| a.plan_id.cmp(&b.plan_id));
    Ok(result)
}
pub fn state(app: &AppHandle, plan_id: String) -> Result<OperationState> {
    app.state::<Runtime>()
        .entries
        .lock()
        .unwrap()
        .get(&plan_id)
        .map(|e| e.state.clone())
        .ok_or("计划不存在".into())
}
fn update(app: &AppHandle, id: &str, change: impl FnOnce(&mut OperationState)) {
    if let Some(entry) = app.state::<Runtime>().entries.lock().unwrap().get_mut(id) {
        change(&mut entry.state);
        if persist_entry(app, entry).is_err() {
            let _ = app.emit(
                "backend-git-notice",
                "Git 操作记录保存失败，请保留工作区路径并检查应用数据目录",
            );
        }
    }
    let _ = app.emit("backend-git-updated", serde_json::json!({ "planId": id }));
}
// -z 保留空格、制表符及 Unicode；只解析索引元信息，不读取或记录文件内容。
fn unmerged_paths(worktree: &Path, task: Option<&tasks::Task>) -> Result<Vec<(String, bool)>> {
    let entries = run(worktree, &["ls-files", "--unmerged", "-z"], task)?;
    if !entries.is_empty() && !entries.ends_with('\0') {
        return Err("冲突路径输出不完整，现场已保留".into());
    }
    let mut paths = std::collections::BTreeMap::new();
    for entry in entries.split_terminator('\0') {
        let (metadata, path) = entry
            .split_once('\t')
            .ok_or("冲突索引格式无效，现场已保留")?;
        let stage = metadata
            .rsplit_once(' ')
            .map(|(_, stage)| stage)
            .ok_or("冲突索引格式无效，现场已保留")?;
        if !["1", "2", "3"].contains(&stage) || path.is_empty() {
            return Err("冲突索引格式无效，现场已保留".into());
        }
        let has_ours = paths.entry(path.to_owned()).or_insert(false);
        *has_ours |= stage == "2";
    }
    Ok(paths.into_iter().collect())
}

fn resolve_test_pom_conflicts(worktree: &Path, task: Option<&tasks::Task>) -> Result<()> {
    let conflicts = unmerged_paths(worktree, task)?;
    if conflicts.is_empty() {
        // 签名或钩子失败也可能留下 MERGE_HEAD；没有冲突时绝不擅自重试提交。
        return Err("合并失败且没有可处理的冲突，工作区已保留，请检查签名或钩子".into());
    }
    let mut count = 0;
    for (path, has_ours) in conflicts {
        if Path::new(&path).file_name() != Some(std::ffi::OsStr::new("pom.xml")) {
            continue;
        }
        // 临时 worktree 附着测试分支，stage 2 / ours 是合并前测试版本。
        // literal pathspec 防止文件名中的魔法前缀或通配符影响其他路径。
        if has_ours {
            run(
                worktree,
                &["--literal-pathspecs", "checkout", "--ours", "--", &path],
                task,
            )?;
            run(worktree, &["--literal-pathspecs", "add", "--", &path], task)?;
        } else {
            // 测试侧删除时保留删除；普通 rm 失败即停下，不能强制覆盖用户修改。
            run(worktree, &["--literal-pathspecs", "rm", "--", &path], task)?;
        }
        count += 1;
    }
    if let Some(task) = task {
        if count > 0 {
            task.emit(
                "log",
                &format!("已对{count}个pom.xml冲突保留测试版本"),
                None,
                None,
            );
        }
    }
    if !unmerged_paths(worktree, task)?.is_empty() {
        return Err(format!("已对{count}个pom.xml冲突保留测试版本；仍有其他文件冲突，请在保留的工作区手工处理并提交"));
    }
    // 使用正常提交，完整沿用仓库签名和钩子；失败保留工作区供手工处理。
    run(worktree, &["commit", "--no-edit"], task).map(|_| ())
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
    revalidate_with_task(saved, operation, task)?;
    let repo = &saved.repository;
    let path = Path::new(&saved.root);
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
    if operation == "switch" || operation == "switch-test" {
        run(path, &["switch", "--no-guess", &saved.target_branch], task)?;
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
    if let Err(error) = run(
        worktree,
        &["merge", "--no-edit", "--", &saved.source_sha],
        task,
    ) {
        let marker = run(worktree, &["rev-parse", "--git-path", "MERGE_HEAD"], task)?;
        if !worktree.join(marker).exists() {
            return Err(error);
        }
        if !keep_test_pom {
            return if unmerged_paths(worktree, task)?.is_empty() {
                Err(error)
            } else {
                Err("合并存在冲突，请在保留工作区手工合并代码".into())
            };
        }
        resolve_test_pom_conflicts(worktree, task)?;
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
pub fn execute(app: AppHandle, plan_id: String) -> Result<String> {
    let plan = {
        let runtime = app.state::<Runtime>();
        let mut entries = runtime.entries.lock().unwrap();
        let entry = entries.get_mut(&plan_id).ok_or("计划不存在")?;
        if entry.state.status != "prepared" {
            return Err("计划已执行，请重新准备".into());
        }
        entry.state.status = "running".into();
        entry.plan.clone()
    };
    let operation_id = plan.plan_id.clone();
    let app_for_error = app.clone();
    let result = tasks::run_backend(app, plan.project_id.clone(), move |task| {
        update(&task.app, &plan.plan_id, |s| s.task_id = task.id.clone());
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
                    s.repositories[0].status = "error".into();
                    s.repositories[0].message = error.clone();
                });
                return Err(error);
            }
        };
        for (index, saved) in plan.repositories.iter().enumerate() {
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
                    update(&task.app, &plan.plan_id, |s| {
                        s.status = if cancelled { "cancelled" } else { "paused" }.into();
                        s.repositories[index].status =
                            if cancelled { "cancelled" } else { "error" }.into();
                        s.repositories[index].message = error.clone();
                    });
                    return Err(error);
                }
            }
        }
        update(&task.app, &plan.plan_id, |s| s.status = "success".into());
        Ok("success")
    });
    match result {
        Ok(task_id) => {
            update(&app_for_error, &operation_id, |s| {
                s.task_id = task_id.clone()
            });
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
pub fn abort(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    finish_worktree(app, plan_id, repo_id, false)
}
pub fn complete(app: &AppHandle, plan_id: String, repo_id: String) -> Result<()> {
    finish_worktree(app, plan_id, repo_id, true)
}
fn finish_worktree(
    app: &AppHandle,
    plan_id: String,
    repo_id: String,
    complete: bool,
) -> Result<()> {
    let (saved, worktree, index, project_id) = {
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
        let value = (
            entry.plan.repositories[index].clone(),
            PathBuf::from(&result.worktree_path),
            index,
            entry.plan.project_id.clone(),
        );
        entry.state.status = "aborting".into();
        value
    };
    let operations = tasks::data_dir(app)?.join("backend-worktrees");
    let operation = operations.join(&plan_id);
    let expected = operation.join(index.to_string());
    let safe = (|| -> Result<bool> {
        let canonical_operations = operations.canonicalize().map_err(|_| "无法确认操作目录")?;
        let canonical_operation = operation.canonicalize().map_err(|_| "无法确认操作目录")?;
        let canonical_worktree = worktree.canonicalize().map_err(|_| "无法确认工作区")?;
        Ok(worktree == expected
            && canonical_operation == canonical_operations.join(&plan_id)
            && canonical_worktree == canonical_operation.join(index.to_string()))
    })();
    if !safe.unwrap_or(false) {
        update(app, &plan_id, |s| s.status = "paused".into());
        return Err("无法确认本次工作区，拒绝 abort".into());
    }
    let id = plan_id.clone();
    let result = tasks::run_backend(app.clone(), project_id, move |task| {
        update(&task.app, &id, |s| s.task_id = task.id.clone());
        let outcome = if complete {
            complete_worktree(&saved, &worktree, Some(task))
        } else {
            abort_worktree(&saved, &worktree, Some(task))
        };
        update(&task.app, &id, |s| {
            s.status = "paused".into();
            s.repositories[index].status = if outcome.is_ok() {
                if complete {
                    "success"
                } else {
                    "aborted"
                }
            } else {
                "error"
            }
            .into();
            if outcome.is_ok() {
                s.repositories[index].worktree_path.clear();
                s.repositories[index].message = if complete {
                    "手工提交已确认，工作区已清理；未自动推送"
                } else {
                    "本次 merge 已中止，工作区已清理"
                }
                .into();
                if s.repositories
                    .iter()
                    .all(|r| ["success", "aborted"].contains(&r.status.as_str()))
                {
                    s.status = "success".into();
                }
            } else if let Err(error) = &outcome {
                s.repositories[index].message = error.clone();
            }
        });
        outcome.map(|_| "success")
    });
    if result.is_err() {
        update(app, &plan_id, |s| s.status = "paused".into());
    }
    result.map(|_| ())
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
    run(
        worktree,
        &["merge-base", "--is-ancestor", &saved.source_sha, &head],
        task,
    )?;
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
    // 合并已提交或用户手工移动过目标时，不把“移除工作区”冒充撤销合并。
    if sha(worktree, "HEAD")? != saved.target_sha {
        return Err("测试分支已有新提交，不能自动中止；请确认后完成处理，提交不会被撤销".into());
    }
    let merge_head = run(worktree, &["rev-parse", "--git-path", "MERGE_HEAD"], task)?;
    if worktree.join(merge_head).exists() {
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
    fn pom_option_keeps_test_version_and_merges_development_code() {
        let (dir, repo) = pom_fixture(
            &[
                "pom.xml",
                "module space/子模块/pom.xml",
                ":(glob)**/pom.xml",
            ],
            false,
            false,
        );
        // A clean pom change must survive even when other poms conflict.
        fs::create_dir(dir.path().join("clean module")).unwrap();
        fs::write(dir.path().join("clean module/pom.xml"), "incoming\n").unwrap();
        git(dir.path(), &["add", "clean module/pom.xml"]);
        git(dir.path(), &["commit", "-m", "clean incoming"]);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).unwrap();
        for path in [
            "pom.xml",
            "module space/子模块/pom.xml",
            ":(glob)**/pom.xml",
        ] {
            assert_eq!(
                git(dir.path(), &["show", &format!("test:{path}")]),
                "test pom"
            );
        }
        assert_eq!(
            git(dir.path(), &["show", "test:clean module/pom.xml"]),
            "incoming"
        );
        assert_eq!(git(dir.path(), &["show", "test:file"]), "development code");
        assert_eq!(git(dir.path(), &["rev-parse", "test^1"]), saved.target_sha);
        assert_eq!(git(dir.path(), &["rev-parse", "test^2"]), saved.source_sha);
        assert_eq!(branch(dir.path()), "dev");
        assert!(!worktree.exists());
    }

    #[test]
    fn pom_option_leaves_other_conflicts_uncommitted() {
        let (_dir, repo) = pom_fixture(&["pom.xml", "not-pom.xml"], true, false);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        let error = perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(()))
            .unwrap_err();
        assert!(error.contains("其他文件冲突"));
        assert_eq!(
            fs::read_to_string(worktree.join("pom.xml")).unwrap(),
            "test pom\n"
        );
        assert_eq!(
            git(&worktree, &["diff", "--name-only", "--diff-filter=U"]),
            "file\nnot-pom.xml"
        );
        assert_eq!(sha(&worktree, "HEAD").unwrap(), saved.target_sha);
        assert!(worktree
            .join(run(&worktree, &["rev-parse", "--git-path", "MERGE_HEAD"], None).unwrap())
            .exists());
    }

    #[test]
    fn pom_option_preserves_test_side_deletion() {
        let (dir, repo) = pom_fixture(&["pom.xml", "module space/pom.xml"], false, true);
        let saved = snapshot(&repo, "merge").unwrap();
        let tmp = tempdir().unwrap();
        let worktree = tmp.path().join("merge");
        perform_with_options(&saved, "merge", false, true, &worktree, None, || Ok(())).unwrap();
        assert!(git(dir.path(), &["ls-tree", "-r", "--name-only", "test"])
            .lines()
            .all(|p| Path::new(p).file_name() != Some(std::ffi::OsStr::new("pom.xml"))));
        assert_eq!(git(dir.path(), &["show", "test:file"]), "development code");
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
        assert!(unmerged_paths(&worktree, None).unwrap().is_empty());
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
        assert!(unmerged_paths(&worktree, None).unwrap().is_empty());
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
        for changed in ["source", "target", "remote-sha", "remote-url"] {
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
                "remote-sha" => {
                    git(
                        dir.path(),
                        &["update-ref", "refs/remotes/origin/test", &next],
                    );
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
    fn behind_or_diverged_test_branch_is_rejected_without_overwriting_local_commits() {
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
        assert!(snapshot(&repo, "merge").is_err());
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
                }],
            },
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
