use crate::config::Result;
use std::{
    path::Path,
    process::{Command, Output},
};

fn clean_environment(command: &mut Command) {
    // 仅清理子进程环境，避免继承的 Git 定位变量将查询指向其他仓库。
    let inherited = std::env::vars_os().map(|(key, _)| key);
    let overrides: Vec<_> = command.get_envs().map(|(key, _)| key.to_owned()).collect();
    for key in inherited.chain(overrides) {
        let name = key.to_string_lossy();
        let git_variable = if cfg!(windows) {
            name.to_ascii_uppercase().starts_with("GIT_")
        } else {
            name.starts_with("GIT_")
        };
        if git_variable {
            command.env_remove(key);
        }
    }
    command.env("LC_ALL", "C").env("GIT_OPTIONAL_LOCKS", "0");
}

fn git_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 只读查询不弹出额外控制台窗口。
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    clean_environment(&mut command);
    command
}

fn run_git(path: &Path, args: &[&str]) -> Result<Output> {
    git_command(path)
        .args(args)
        .output()
        .map_err(|e| format!("无法执行 Git: {e}"))
}

fn git_error(output: &Output) -> String {
    format!(
        "读取 Git 分支失败（{}）: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

pub fn read_branch(path: &Path) -> Result<String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("路径不存在或无法访问（{}）: {e}", path.display()))?;
    if !path.is_dir() {
        return Err("Git 路径必须为目录".into());
    }
    // symbolic-ref 不依赖提交存在，也支持 .git 为文件的 worktree。
    let branch = run_git(&path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if branch.status.success() {
        let name =
            String::from_utf8(branch.stdout).map_err(|e| format!("分支名称编码无效: {e}"))?;
        let name = name.trim_end_matches(['\r', '\n']);
        if name.is_empty() {
            return Err("Git 返回了空分支名称".into());
        }
        return Ok(name.into());
    }
    let repository = run_git(&path, &["rev-parse", "--git-dir"])?;
    if repository.status.success() {
        // --quiet 的退出码 1 表示 HEAD 不是符号引用，其他失败仍须报告。
        return if branch.status.code() == Some(1) {
            Ok("游离 HEAD".into())
        } else {
            Err(git_error(&branch))
        };
    }
    // 只识别 Git 明确的非仓库诊断，权限、损坏、所有权等错误不能吞掉。
    if String::from_utf8_lossy(&repository.stderr)
        .starts_with("fatal: not a git repository (or any")
    {
        Ok("非 Git 仓库".into())
    } else {
        Err(git_error(&repository))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    fn git(path: &Path, args: &[&str]) {
        let output = git_command(path)
            // 测试不读取用户或系统配置，不使用用户模板、钩子和签名设置。
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "user.name=Git Test",
                "-c",
                "user.email=git-test@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", git_error(&output));
    }

    fn repository() -> TempDir {
        let dir = tempdir().unwrap();
        git(
            dir.path(),
            &["init", "--initial-branch=main", "--template="],
        );
        dir
    }

    fn commit(path: &Path) {
        git(path, &["commit", "--allow-empty", "-m", "test"]);
    }

    #[test]
    fn unborn_branch_is_returned_without_creating_a_commit() {
        let dir = repository();
        assert_eq!(read_branch(dir.path()).unwrap(), "main");
        assert!(!dir.path().join(".git/refs/heads/main").exists());
    }

    #[test]
    fn normal_branch_and_subdirectory_are_supported_without_changes() {
        let dir = repository();
        commit(dir.path());
        git(dir.path(), &["checkout", "-b", "feature/read-branch"]);
        let nested = dir.path().join("子目录 with spaces");
        fs::create_dir(&nested).unwrap();
        let head = fs::read(dir.path().join(".git/HEAD")).unwrap();
        let index = fs::read(dir.path().join(".git/index")).ok();
        assert_eq!(read_branch(dir.path()).unwrap(), "feature/read-branch");
        assert_eq!(read_branch(&nested).unwrap(), "feature/read-branch");
        assert_eq!(fs::read(dir.path().join(".git/HEAD")).unwrap(), head);
        assert_eq!(fs::read(dir.path().join(".git/index")).ok(), index);
    }

    #[test]
    fn detached_head_is_reported() {
        let dir = repository();
        commit(dir.path());
        git(dir.path(), &["checkout", "--detach", "HEAD"]);
        assert_eq!(read_branch(dir.path()).unwrap(), "游离 HEAD");
    }

    #[test]
    fn non_repository_is_reported() {
        let dir = tempdir().unwrap();
        assert_eq!(read_branch(dir.path()).unwrap(), "非 Git 仓库");
        assert!(!dir.path().join(".git").exists());
    }

    #[test]
    fn worktree_and_its_subdirectory_use_the_worktree_branch() {
        let dir = repository();
        commit(dir.path());
        let worktrees = tempdir().unwrap();
        let worktree = worktrees.path().join("linked tree");
        git(
            dir.path(),
            &[
                "worktree",
                "add",
                "-b",
                "linked",
                worktree.to_str().unwrap(),
            ],
        );
        let nested = worktree.join("nested");
        fs::create_dir(&nested).unwrap();
        assert!(worktree.join(".git").is_file());
        assert_eq!(read_branch(&worktree).unwrap(), "linked");
        assert_eq!(read_branch(&nested).unwrap(), "linked");
        assert_eq!(read_branch(dir.path()).unwrap(), "main");
    }

    #[test]
    fn missing_path_and_file_are_errors() {
        let dir = tempdir().unwrap();
        assert!(read_branch(&dir.path().join("missing")).is_err());
        let file = dir.path().join("file");
        fs::write(&file, "test").unwrap();
        assert!(read_branch(&file).is_err());
        assert!(read_branch(Path::new("")).is_err());
    }

    #[test]
    fn broken_git_directory_is_an_error() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".git"), "gitdir: missing-repository\n").unwrap();
        assert!(read_branch(dir.path()).is_err());
    }

    #[test]
    fn git_environment_cannot_redirect_the_command_to_another_repository() {
        let target = repository();
        let other = repository();
        git(other.path(), &["symbolic-ref", "HEAD", "refs/heads/wrong"]);
        let mut command = git_command(target.path());
        command
            .env("GIT_DIR", other.path().join(".git"))
            .env("GIT_WORK_TREE", other.path())
            .env("GIT_COMMON_DIR", other.path().join(".git"))
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "core.bare")
            .env("GIT_CONFIG_VALUE_0", "true");
        clean_environment(&mut command);
        let output = command
            .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", git_error(&output));
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "main");
    }
}
