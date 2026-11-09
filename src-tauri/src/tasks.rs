use crate::{
    accessibility::{self, PermissionPrompt},
    config::{self, Action, AppConfig, Project, Result, Settings},
    manifest, nvm,
};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime},
};
use tauri::{AppHandle, Emitter, Manager};
#[derive(Default)]
pub struct Runtime {
    pub tasks: Mutex<HashMap<String, Arc<Control>>>,
    pub busy: AtomicBool,
    pub shutting_down: AtomicBool,
    pub recovery_lock: Mutex<()>,
}
#[derive(Default)]
pub struct Control {
    cancel: AtomicBool,
    pid: Mutex<Option<u32>>,
    permission_prompt: Mutex<PermissionPrompt>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Event {
    task_id: String,
    project_id: String,
    kind: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
}
#[derive(Clone)]
pub struct Task {
    pub app: AppHandle,
    pub id: String,
    pub project: String,
    pub control: Arc<Control>,
}
impl Task {
    pub fn emit(&self, kind: &str, message: &str, status: Option<&str>, path: Option<&Path>) {
        let _ = self.app.emit(
            "task-event",
            Event {
                task_id: self.id.clone(),
                project_id: self.project.clone(),
                kind: kind.into(),
                message: message.into(),
                status: status.map(str::to_owned),
                path: path.map(|p| p.to_string_lossy().into_owned()),
            },
        );
    }
    pub fn check(&self) -> Result<()> {
        if self.control.cancel.load(Ordering::SeqCst) {
            Err("任务已取消".into())
        } else {
            Ok(())
        }
    }
    pub fn command(&self, mut command: Command, timeout: u64) -> Result<()> {
        self.check()?;
        let automation = accessibility::is_automation_command(&command);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        *self.control.pid.lock().unwrap() = Some(child.id());
        let out = child.stdout.take().unwrap();
        let err = child.stderr.take().unwrap();
        let t = self.clone();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(out)
                .lines()
                .map_while(std::result::Result::ok)
            {
                t.emit("log", &line, None, None)
            }
        });
        let t = self.clone();
        let errors = std::thread::spawn(move || {
            let mut denial = None;
            for line in BufReader::new(err)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if automation && accessibility::is_denied(&line) {
                    let message = t.control.permission_prompt.lock().unwrap().denied();
                    if denial.is_none() {
                        t.emit("log", &message, None, None);
                        denial = Some(message);
                    }
                } else {
                    t.emit("log", &line, None, None)
                }
            }
            denial
        });
        let start = Instant::now();
        let result = loop {
            if self.check().is_err() || start.elapsed() > Duration::from_secs(timeout) {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break Err(if self.check().is_err() {
                    "任务已取消"
                } else {
                    "命令执行超时"
                }
                .into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(format!("进程退出失败: {status}"))
                    }
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
                Err(e) => {
                    kill_group(child.id());
                    let _ = child.wait();
                    break Err(e.to_string());
                }
            }
        };
        // 子进程可能继承输出管道，先终止残留进程再等待读取线程，避免阻塞。
        kill_group(child.id());
        *self.control.pid.lock().unwrap() = None;
        let _ = reader.join();
        if let Ok(Some(denial)) = errors.join() {
            if self.check().is_ok() {
                return Err(denial);
            }
        }
        result
    }
}
fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = pid;
}
pub fn mac_only() -> Result<()> {
    if cfg!(target_os = "macos") {
        Ok(())
    } else {
        Err("此自动化功能首版仅支持 macOS；Windows 仅预留打包架构".into())
    }
}
pub fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
pub fn config_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_dir(app)?.join("config.json"))
}
pub fn load(app: &AppHandle) -> Result<AppConfig> {
    let p = config_path(app)?;
    if !p.exists() {
        return Ok(AppConfig::default());
    }
    config::parse(&fs::read_to_string(p).map_err(|e| e.to_string())?)
}
pub fn scripts(app: &AppHandle) -> Result<PathBuf> {
    let p = data_dir(app)?.join("scripts");
    for (name, text) in [
        ("build-app.sh", include_str!("../../scripts/build-app.sh")),
        ("build.sh", include_str!("../../scripts/build.sh")),
        (
            "hbuilderx.applescript",
            include_str!("../../scripts/hbuilderx.applescript"),
        ),
        (
            "terminal.applescript",
            include_str!("../../scripts/terminal.applescript"),
        ),
    ] {
        config::atomic_write(&p.join(name), text)?;
    }
    Ok(p)
}
pub fn recover(app: &AppHandle) -> Result<Vec<String>> {
    let runtime = app.state::<Runtime>();
    let _guard = runtime.recovery_lock.lock().unwrap();
    if runtime.busy.load(Ordering::SeqCst) {
        return Err("构建正在运行，不能执行恢复".into());
    }
    recover_inner(app)
}
pub fn recover_inner(app: &AppHandle) -> Result<Vec<String>> {
    let dir = data_dir(app)?.join("pending");
    let mut logs = vec![];
    if !dir.exists() {
        return Ok(logs);
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let result = (|| {
            let r: manifest::Recovery =
                serde_json::from_str(&fs::read_to_string(&p).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let msg = manifest::restore(&r)?;
            fs::remove_file(&p).map_err(|e| e.to_string())?;
            Ok::<_, String>(msg)
        })();
        logs.push(match result {
            Ok(msg) => msg,
            Err(e) => format!("恢复待处理 {}: {e}", p.display()),
        })
    }
    Ok(logs)
}
fn spawn(
    app: AppHandle,
    project: String,
    exclusive: bool,
    work: impl FnOnce(&Task) -> Result<&'static str> + Send + 'static,
) -> Result<String> {
    mac_only()?;
    if exclusive {
        PermissionPrompt::default().ensure()?;
    }
    let runtime = app.state::<Runtime>();
    let _guard = runtime.recovery_lock.lock().unwrap();
    if runtime.shutting_down.load(Ordering::SeqCst) {
        return Err("应用正在退出并恢复 AppID，不能启动新任务".into());
    }
    if exclusive && runtime.busy.swap(true, Ordering::SeqCst) {
        return Err("HBuilderX 正忙，请等待当前任务完成".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let task = Task {
        app: app.clone(),
        id: id.clone(),
        project,
        control: Arc::new(Control::default()),
    };
    runtime
        .tasks
        .lock()
        .unwrap()
        .insert(id.clone(), task.control.clone());
    std::thread::spawn(move || {
        task.emit("status", "任务开始", Some("running"), None);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&task)))
            .unwrap_or_else(|_| Err("后台任务异常；AppID 恢复记录已保留，请执行恢复".into()));
        let (status, message) = match result {
            Ok(s) => (s, "任务结束".to_string()),
            Err(e) => (
                if task.control.cancel.load(Ordering::SeqCst) {
                    "cancelled"
                } else {
                    "error"
                },
                e,
            ),
        };
        if task.control.cancel.load(Ordering::SeqCst) {
            task.emit("log", "已停止本应用管理的进程；HBuilderX 内部发行或已交给终端的命令无法保证撤回，请在对应应用确认", None, None);
        }
        let runtime = task.app.state::<Runtime>();
        if exclusive {
            runtime.busy.store(false, Ordering::SeqCst)
        }
        runtime.tasks.lock().unwrap().remove(&task.id);
        task.emit("status", &message, Some(status), None);
    });
    Ok(id)
}
pub fn cancel(app: &AppHandle, id: &str) -> Result<()> {
    let runtime = app.state::<Runtime>();
    let tasks = runtime.tasks.lock().unwrap();
    let control = tasks
        .get(id)
        .ok_or("任务不存在或已经结束；终端中已触发的命令需在终端自行停止")?;
    control.cancel.store(true, Ordering::SeqCst);
    if let Some(pid) = *control.pid.lock().unwrap() {
        kill_group(pid)
    }
    Ok(())
}
pub fn cancel_all(app: &AppHandle) {
    for c in app.state::<Runtime>().tasks.lock().unwrap().values() {
        c.cancel.store(true, Ordering::SeqCst);
        if let Some(pid) = *c.pid.lock().unwrap() {
            kill_group(pid)
        }
    }
}
fn fallback<'a>(value: &'a str, other: &'a str) -> &'a str {
    if value.is_empty() {
        other
    } else {
        value
    }
}
fn hbuilder(settings: &Settings) -> Result<String> {
    let p = if settings.hbuilderx_path.is_empty() {
        "/Applications/HBuilderX.app"
    } else {
        &settings.hbuilderx_path
    };
    if !Path::new(p).is_absolute() || !Path::new(p).is_dir() {
        return Err("请配置 HBuilderX.app 绝对路径".into());
    }
    Ok(p.into())
}
fn app_vue(project: &Project) -> Result<PathBuf> {
    let root = Path::new(&project.path);
    [root.join("App.vue"), root.join("src/App.vue")]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("项目中找不到 App.vue".into())
}
fn menu_command(
    scripts: &Path,
    project: &Project,
    settings: &Settings,
    mode: &str,
) -> Result<Command> {
    let mut c = Command::new("/bin/bash");
    c.arg(scripts.join("build-app.sh"))
        .arg(mode)
        .arg(hbuilder(settings)?)
        .arg(&project.path)
        .arg(app_vue(project)?);
    Ok(c)
}
pub fn action(
    app: AppHandle,
    project: Project,
    action: Action,
    settings: Settings,
) -> Result<String> {
    if !Path::new(&project.path).is_dir() {
        return Err("项目目录不存在".into());
    }
    if !["menu", "shell"].contains(&action.kind.as_str()) {
        return Err("未知动作类型".into());
    }
    let exclusive = action.kind == "menu";
    if exclusive
        && (action.menu_path.len() < 2 || !["运行", "发行"].contains(&action.menu_path[0].as_str()))
    {
        return Err("菜单路径必须从运行/发行开始".into());
    }
    spawn(app, project.id.clone(), exclusive, move |task| {
        let scripts = scripts(&task.app)?;
        if action.kind == "menu" {
            let mut c = menu_command(&scripts, &project, &settings, "menu")?;
            c.args(&action.menu_path);
            task.command(c, 45)?;
        } else {
            let terminal = fallback(&project.terminal, &settings.terminal);
            let terminal = if terminal.is_empty() {
                "Terminal"
            } else {
                terminal
            };
            if terminal != "Terminal" && !Path::new(terminal).is_absolute() {
                return Err("终端必须使用绝对路径".into());
            }
            if accessibility::terminal_needs_accessibility(terminal) {
                task.control.permission_prompt.lock().unwrap().ensure()?;
            }
            // 仅 Node shell 动作有明确选择时激活；交给终端前再次验证安装文件。
            let command = nvm::command_for(&project, &action)?;
            let mut c = Command::new("/usr/bin/osascript");
            c.arg(scripts.join("terminal.applescript"))
                .arg(terminal)
                .arg(&project.path)
                .arg(command);
            task.command(c, 30)?;
            task.emit(
                "log",
                "命令已交给终端，应用不能确认其退出状态或取消终端进程",
                None,
                None,
            );
        }
        Ok("triggered")
    })
}
fn files(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(true) {
                continue;
            }
            if p.is_dir() {
                out.extend(files(&p, extension))
            } else if extension.is_empty()
                || p.extension().and_then(|s| s.to_str()) == Some(extension)
            {
                out.push(p)
            }
        }
    }
    out
}
fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}
fn snapshot(root: &Path, ext: &str) -> HashMap<PathBuf, (SystemTime, u64)> {
    files(root, ext)
        .into_iter()
        .filter_map(|p| stamp(&p).map(|s| (p, s)))
        .collect()
}
fn wait_fresh(
    task: &Task,
    root: &Path,
    ext: &str,
    before: &HashMap<PathBuf, (SystemTime, u64)>,
    started: SystemTime,
    timeout: u64,
) -> Result<Vec<PathBuf>> {
    let deadline = Instant::now();
    let mut prior = HashMap::new();
    let mut stable = 0;
    loop {
        task.check()?;
        let now = snapshot(root, ext);
        let fresh: Vec<_> = now
            .iter()
            .filter(|(p, s)| s.1 > 0 && s.0 >= started && before.get(*p) != Some(*s))
            .map(|(p, _)| p.clone())
            .collect();
        if !fresh.is_empty() && now == prior {
            stable += 1
        } else {
            stable = 0
        }
        if stable >= 3 {
            return Ok(fresh);
        }
        prior = now;
        if deadline.elapsed() > Duration::from_secs(timeout) {
            return Err("等待新产物超时；未将旧文件当作构建成功".into());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
pub fn build(
    app: AppHandle,
    project: Project,
    mode: String,
    name: String,
    code: String,
) -> Result<String> {
    mac_only()?;
    manifest::validate_version(&name, &code)?;
    if project.project_type != "uniapp"
        || !["apk", "wgt-android", "wgt-ios"].contains(&mode.as_str())
    {
        return Err("项目类型或构建模式无效".into());
    }
    let appid = if mode == "wgt-ios" {
        &project.ios_app_id
    } else {
        &project.android_app_id
    };
    if !regex::Regex::new(r"^[A-Za-z0-9_-]+$")
        .unwrap()
        .is_match(appid)
    {
        return Err("请配置有效的平台 AppID".into());
    }
    let path = config::manifest_path(&project)?;
    let settings = load(&app)?.settings;
    hbuilder(&settings)?;
    app_vue(&project)?;
    spawn(app, project.id.clone(), true, move |task| {
        build_inner(task, &project, &settings, &mode, &name, &code, &path)
    })
}
fn build_inner(
    task: &Task,
    p: &Project,
    settings: &Settings,
    mode: &str,
    name: &str,
    code: &str,
    path: &Path,
) -> Result<&'static str> {
    let scripts = scripts(&task.app)?;
    let pending = data_dir(&task.app)?.join("pending");
    // 未解决的恢复记录会阻止再次修改源码，包括上次崩溃遗留的用户改动冲突。
    let logs = recover_inner(&task.app)?;
    for log in logs {
        task.emit("log", &log, None, None)
    }
    if pending.exists()
        && fs::read_dir(&pending)
            .map_err(|e| e.to_string())?
            .next()
            .is_some()
    {
        return Err("尚有未解决的 AppID 恢复记录，请处理后重试".into());
    }
    let appid = if mode == "wgt-ios" {
        &p.ios_app_id
    } else {
        &p.android_app_id
    };
    let original = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let original_id = manifest::info(&original)?.appid;
    let mut updated = manifest::replace(&original, "versionName", &name.into())?;
    let old_code = manifest::field(&original, "versionCode")?;
    updated = manifest::replace(
        &updated,
        "versionCode",
        &if old_code.is_string() {
            code.into()
        } else {
            serde_json::json!(code.parse::<u32>().map_err(|e| e.to_string())?)
        },
    )?;
    updated = manifest::replace(&updated, "appid", &appid.clone().into())?;
    let record = manifest::Recovery {
        path: path.to_string_lossy().into(),
        original: original_id,
        temporary: appid.clone(),
    };
    let journal = pending.join(format!("{}.json", task.id));
    config::atomic_write(
        &journal,
        &serde_json::to_string(&record).map_err(|e| e.to_string())?,
    )?;
    let result = (|| {
        task.check()?;
        config::atomic_write(path, &updated)?;
        if p.sync_main_version {
            sync_main(path.parent().unwrap().join("main.js"), name)?
        }
        task.emit(
            "log",
            "版本已写入源码；仅 AppID 会在任务结束后恢复",
            None,
            None,
        );
        let root = Path::new(&p.path);
        if mode == "apk" {
            let output = build_output_dir(root, &p.output_dir, mode);
            let android = Path::new(&p.android_project_path);
            let jdk = resolve_jdk(fallback(&p.jdk_path, &settings.jdk_path))?;
            if !android.is_absolute() || !android.join("gradlew").is_file() {
                return Err("请配置 Android 项目、Gradle wrapper 和 JDK".into());
            }
            if !regex::Regex::new(r"^[A-Za-z0-9_-]+$")
                .unwrap()
                .is_match(&p.android_module)
            {
                return Err("Android module 应为单一模块目录名".into());
            }
            sync_gradle(android.join(&p.android_module), name, code)?;
            let resources = root.join("unpackage/resources").join(appid);
            let before = snapshot(&resources, "");
            let started = SystemTime::now();
            task.command(menu_command(&scripts, p, settings, mode)?, 45)?;
            let fresh_resources = wait_fresh(task, &resources, "", &before, started, 300)?;
            let generated_manifest = resources.join("www/manifest.json");
            if !fresh_resources.contains(&generated_manifest) {
                return Err("本地资源 manifest.json 未更新，拒绝复制旧资源".into());
            }
            let generated: serde_json::Value = serde_json::from_str(
                &fs::read_to_string(&generated_manifest).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("生成资源 manifest 无效: {e}"))?;
            if generated["id"].as_str() != Some(appid)
                || generated["version"]["name"].as_str() != Some(name)
            {
                return Err("生成资源的 AppID/版本与目标不一致".into());
            }
            let apk_dir = android.join(&p.android_module).join("build/outputs/apk");
            let before = snapshot(&apk_dir, "apk");
            let started = SystemTime::now();
            let gradle_task = if p.gradle_task.is_empty() {
                "assembleRelease"
            } else {
                &p.gradle_task
            };
            let mut c = Command::new("/bin/bash");
            c.arg(scripts.join("build.sh"))
                .arg(&resources)
                .arg(android)
                .arg(&p.android_module)
                .arg(jdk)
                .arg(gradle_task)
                .arg(appid);
            task.command(c, 900)?;
            let fresh = wait_fresh(task, &apk_dir, "apk", &before, started, 15)?;
            for artifact in fresh {
                publish_apk(task, &artifact, &output)?
            }
        } else {
            let release = build_output_dir(root, &p.output_dir, mode);
            let source = release.join(format!("{appid}.wgt"));
            let before = snapshot(&release, "wgt");
            let started = SystemTime::now();
            task.command(menu_command(&scripts, p, settings, mode)?, 60)?;
            let fresh = wait_fresh(task, &release, "wgt", &before, started, 300)?;
            if !fresh.contains(&source) {
                return Err("新 WGT 与当前 AppID 不一致".into());
            }
            verify_archive(task, &source)?;
            task.check()?;
            let target = finalize_wgt(&source, name, mode == "wgt-ios")?;
            task.emit("artifact", "已生成 WGT 产物", None, Some(&target));
        }
        task.check()?;
        Ok("success")
    })();
    // 失败或取消也必须恢复 AppID；出现冲突或恢复失败时保留记录。
    match manifest::restore(&record) {
        Ok(log) => {
            task.emit("log", &log, None, None);
            fs::remove_file(journal).map_err(|e| e.to_string())?;
        }
        Err(e) => {
            task.emit("log", &e, None, None);
            return Err(format!("AppID 恢复未完成: {e}; 构建结果: {result:?}"));
        }
    }
    // 恢复完成后再次检查取消；打开目录失败只警告，不改变成功的构建结果。
    let result = result.and_then(|status| task.check().map(|_| status));
    finish_build(
        result,
        settings.open_output_after_build,
        || {
            let output = build_output_dir(Path::new(&p.path), &p.output_dir, mode);
            let status = Command::new("/usr/bin/open")
                .arg(&output)
                .status()
                .map_err(|e| e.to_string())?;
            if !status.success() {
                return Err(format!("{}: {status}", output.display()));
            }
            Ok(())
        },
        |warning| task.emit("log", warning, None, None),
    )
}
fn build_output_dir(root: &Path, configured: &str, mode: &str) -> PathBuf {
    if mode != "apk" || configured.is_empty() {
        root.join("unpackage/release")
    } else {
        // join 对绝对路径直接使用该路径，相对路径则以项目目录为基准。
        root.join(configured)
    }
}
fn finish_build(
    result: Result<&'static str>,
    open_enabled: bool,
    open: impl FnOnce() -> Result<()>,
    warn: impl FnOnce(&str),
) -> Result<&'static str> {
    let status = result?;
    if open_enabled {
        if let Err(error) = open() {
            warn(&format!("警告：构建已完成，但打开输出目录失败: {error}"));
        }
    }
    Ok(status)
}
fn verify_archive(task: &Task, source: &Path) -> Result<()> {
    task.check()?;
    let mut verify = Command::new("/usr/bin/unzip");
    verify.arg("-t").arg(source);
    task.command(verify, 60)
}
fn finalize_wgt(source: &Path, version: &str, ios: bool) -> Result<PathBuf> {
    let release = source.parent().ok_or("WGT 文件缺少输出目录")?;
    let suffix = if ios { "-ios" } else { "" };
    let target = release.join(format!("{version}{suffix}.wgt"));
    // 产物校验完成后在同目录重命名；macOS 原子替换同版本包，不影响其他版本或平台。
    fs::rename(source, &target).map_err(|e| format!("WGT 重命名失败: {e}"))?;
    Ok(target)
}
fn copy_apk(source: &Path, output: &Path) -> Result<PathBuf> {
    let filename = source.file_name().ok_or("APK 缺少文件名")?;
    let original = fs::canonicalize(source).map_err(|e| e.to_string())?;
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let target = output.join(filename);
    // 用户可直接选择原生输出目录，此时不复制，避免源文件被自身覆盖。
    if fs::canonicalize(&target).ok().as_ref() == Some(&original) {
        return Ok(target);
    }
    // 保留 Gradle 原始名称，同名包复制完成后再原子替换，不暴露未复制完的文件。
    let temporary = output.join(format!(".dev-dock-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        fs::copy(source, &temporary).map_err(|e| e.to_string())?;
        fs::rename(&temporary, &target).map_err(|e| e.to_string())?;
        Ok(target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn publish_apk(task: &Task, source: &Path, output: &Path) -> Result<()> {
    verify_archive(task, source)?;
    let target = copy_apk(source, output)?;
    task.emit("artifact", "已生成 APK 产物", None, Some(&target));
    Ok(())
}
fn resolve_jdk(configured: &str) -> Result<PathBuf> {
    // 显式配置优先，其次继承系统 JAVA_HOME；图形应用未继承环境时查询系统 JDK。
    let candidate = if !configured.is_empty() {
        PathBuf::from(configured)
    } else if let Some(home) = std::env::var_os("JAVA_HOME").filter(|v| !v.is_empty()) {
        PathBuf::from(home)
    } else {
        let output = Command::new("/usr/libexec/java_home")
            .output()
            .map_err(|e| format!("无法查询系统 JDK: {e}"))?;
        if !output.status.success() {
            return Err("系统未找到 JDK，请设置 JAVA_HOME 或在设置中选择 JDK".into());
        }
        PathBuf::from(
            String::from_utf8(output.stdout)
                .map_err(|e| e.to_string())?
                .trim(),
        )
    };
    if !candidate.is_absolute() || !candidate.join("bin/java").is_file() {
        return Err(format!(
            "JDK 路径无效: {}，请检查配置或 JAVA_HOME",
            candidate.display()
        ));
    }
    Ok(candidate)
}
fn sync_gradle(module: PathBuf, name: &str, code: &str) -> Result<()> {
    let path = [module.join("build.gradle"), module.join("build.gradle.kts")]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("模块缺少 build.gradle / build.gradle.kts")?;
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    config::atomic_write(&path, &patch_gradle(&text, name, code)?)
}
fn patch_gradle(text: &str, name: &str, code: &str) -> Result<String> {
    // 仅修改唯一明确的版本字面量，表达式配置需要用户手动整理。
    let version_name = regex::Regex::new(
        r#"(?m)^(\s*versionName\s*(?:=\s*)?)["'][^"'\r\n]*["']([ \t]*(?://[^\r\n]*)?)$"#,
    )
    .unwrap();
    let version_code =
        regex::Regex::new(r"(?m)^(\s*versionCode\s*(?:=\s*)?)[0-9]+([ \t]*(?://[^\r\n]*)?)$")
            .unwrap();
    if version_name.find_iter(text).count() != 1 || version_code.find_iter(text).count() != 1 {
        return Err("Gradle 版本字段需为唯一的字面量 versionName/versionCode；表达式或多 flavor 配置需手动整理".into());
    }
    let updated = version_name.replace(text, |c: &regex::Captures| {
        format!("{}{}{}", &c[1], serde_json::to_string(name).unwrap(), &c[2])
    });
    Ok(version_code
        .replace(&updated, |c: &regex::Captures| {
            format!("{}{}{}", &c[1], code, &c[2])
        })
        .into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apk_archive_keeps_gradle_filename_and_replaces_same_name_safely() {
        let dir = tempfile::tempdir().unwrap();
        let source_dir = dir.path().join("native apk");
        let output = dir.path().join("uniapp/release");
        fs::create_dir_all(&source_dir).unwrap();
        for filename in ["hyx-3.8.8-202611091037.apk", "other-app-arm64-release.apk"] {
            let source = source_dir.join(filename);
            fs::write(&source, "first package").unwrap();
            let target = copy_apk(&source, &output).unwrap();
            assert_eq!(target, output.join(filename));
            fs::write(&source, "new package contents").unwrap();
            assert_eq!(copy_apk(&source, &output).unwrap(), target);
            assert_eq!(fs::read_to_string(&target).unwrap(), "new package contents");
            assert_eq!(fs::read_to_string(&source).unwrap(), "new package contents");
        }
        assert_eq!(fs::read_dir(output).unwrap().count(), 2);
    }
    #[test]
    fn apk_archive_can_be_the_native_output_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("hyx-3.8.8-202611091037.apk");
        fs::write(&source, "keep original package").unwrap();
        assert_eq!(copy_apk(&source, dir.path()).unwrap(), source);
        assert_eq!(fs::read_to_string(source).unwrap(), "keep original package");
    }
    #[test]
    fn output_directory_matches_published_artifacts() {
        let root = Path::new("/tmp/project");
        for mode in ["wgt-android", "wgt-ios", "apk"] {
            assert_eq!(
                build_output_dir(root, "", mode),
                root.join("unpackage/release")
            );
        }
        assert_eq!(
            build_output_dir(root, "dist/apk", "apk"),
            root.join("dist/apk")
        );
        assert_eq!(
            build_output_dir(root, "/tmp/output", "apk"),
            PathBuf::from("/tmp/output")
        );
        assert_eq!(
            build_output_dir(root, "/tmp/output", "wgt-ios"),
            root.join("unpackage/release")
        );
    }
    #[test]
    fn completed_build_only_opens_enabled_success_and_open_failure_is_warning() {
        for message in ["构建失败", "已取消", "AppID 恢复未完成"] {
            let result = finish_build(
                Err(message.into()),
                true,
                || panic!("不应打开目录"),
                |_| panic!("不应警告"),
            );
            assert_eq!(result.unwrap_err(), message);
        }
        assert_eq!(
            finish_build(
                Ok("success"),
                false,
                || panic!("不应打开目录"),
                |_| panic!("不应警告")
            )
            .unwrap(),
            "success"
        );
        let mut opened = false;
        assert_eq!(
            finish_build(
                Ok("success"),
                true,
                || {
                    opened = true;
                    Ok(())
                },
                |_| panic!("不应警告")
            )
            .unwrap(),
            "success"
        );
        assert!(opened);
        let mut warning = String::new();
        assert_eq!(
            finish_build(
                Ok("success"),
                true,
                || Err("访达不可用".into()),
                |message| warning = message.into()
            )
            .unwrap(),
            "success"
        );
        assert!(warning.contains("访达不可用"));
    }
    #[test]
    fn wgt_versions_are_placed_directly_in_release() {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("unpackage/release");
        fs::create_dir_all(&release).unwrap();
        let source = release.join("__UNI__TEST.wgt");
        fs::write(&source, "android package").unwrap();
        let android = finalize_wgt(&source, "3.8.8", false).unwrap();
        assert_eq!(android, release.join("3.8.8.wgt"));
        assert!(!source.exists());
        fs::write(&source, "ios package").unwrap();
        let ios = finalize_wgt(&source, "3.8.8", true).unwrap();
        assert_eq!(ios, release.join("3.8.8-ios.wgt"));
        assert_eq!(fs::read_to_string(android).unwrap(), "android package");
        assert_eq!(fs::read_to_string(ios).unwrap(), "ios package");
        assert!(!release.join("dev-dock").exists());
    }
    #[test]
    fn wgt_same_version_replaces_only_matching_platform() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("__UNI__TEST.wgt");
        fs::write(dir.path().join("3.8.8.wgt"), "previous android").unwrap();
        fs::write(dir.path().join("3.8.8-ios.wgt"), "keep ios").unwrap();
        fs::write(dir.path().join("3.8.7.wgt"), "keep previous version").unwrap();
        fs::write(&source, "new android").unwrap();
        let output = finalize_wgt(&source, "3.8.8", false).unwrap();
        assert_eq!(fs::read_to_string(output).unwrap(), "new android");
        assert_eq!(
            fs::read_to_string(dir.path().join("3.8.8-ios.wgt")).unwrap(),
            "keep ios"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("3.8.7.wgt")).unwrap(),
            "keep previous version"
        );
    }
    #[test]
    fn native_versions_preserve_comments() {
        let original = "android {\n defaultConfig {\n  versionName = \"1.0.0\" // name\n  versionCode = 10 // code\n }\n}";
        let updated = patch_gradle(original, "2.0.0", "20").unwrap();
        assert!(updated.contains("versionName = \"2.0.0\" // name"));
        assert!(updated.contains("versionCode = 20 // code"));
        assert!(patch_gradle("versionName project.version\nversionCode 1", "2.0.0", "20").is_err());
    }
    #[test]
    fn artifact_snapshot_ignores_old_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.wgt");
        fs::write(&file, "old").unwrap();
        let old = snapshot(dir.path(), "wgt");
        assert_eq!(old, snapshot(dir.path(), "wgt"));
        fs::write(&file, "new archive bytes").unwrap();
        assert_ne!(old, snapshot(dir.path(), "wgt"));
    }
}
fn sync_main(path: PathBuf, version: &str) -> Result<()> {
    let text = fs::read_to_string(&path).map_err(|e| format!("main.js: {e}"))?;
    let re = regex::Regex::new(
        r#"(?s)(function\s+appVersion\s*\(\s*\)\s*\{\s*return\s*)["'][^"']*["']"#,
    )
    .unwrap();
    if re.find_iter(&text).count() != 1 {
        return Err(
            "main.js 需包含唯一 function appVersion() { return '版本' }，未修改不明确的函数".into(),
        );
    }
    let changed = re.replace(&text, |caps: &regex::Captures| {
        format!("{}{}", &caps[1], serde_json::to_string(version).unwrap())
    });
    config::atomic_write(&path, &changed)
}
