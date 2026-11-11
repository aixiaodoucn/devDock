//! 仅用存活的 HBuilderX 编译进程确认归属，不使用编辑器选择或上次启动缓存。
use crate::config::Project;
use std::path::{Path, PathBuf};

pub const RUNNING_MARKER: &str = "__DEVDOCK_HX_ALREADY_RUNNING__:";

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default, PartialEq)]
struct Metadata {
    input: String,
    platform: String,
    script: String,
}

#[cfg(any(target_os = "macos", test))]
fn parse_process(bytes: &[u8], bundle: &Path) -> Option<Metadata> {
    let argc = i32::from_ne_bytes(bytes.get(..4)?.try_into().ok()?);
    if !(1..=1024).contains(&argc) || bytes.last() != Some(&0) {
        return None;
    }
    // KERN_PROCARGS2: argc、可执行文件名、NUL padding、argv、env。
    let mut fields = bytes.get(4..)?.split(|b| *b == 0);
    fields.next()?;
    let mut fields = fields.skip_while(|s| s.is_empty());
    let args: Vec<_> = fields.by_ref().take(argc as usize).collect();
    let plugins = bundle.join("Contents/HBuilderX/plugins");
    let entries = [
        plugins.join("uniapp-cli/bin/uniapp-cli.js"),
        plugins.join("uniapp-cli-vite/node_modules/@dcloudio/vite-plugin-uni/bin/uni.js"),
    ];
    if args.len() != argc as usize
        || !args.iter().any(|arg| {
            entries
                .iter()
                .any(|p| *arg == p.as_os_str().as_encoded_bytes())
        })
    {
        return None;
    }
    let mut values = std::collections::HashMap::new();
    for field in fields {
        let Some(equal) = field.iter().position(|b| *b == b'=') else {
            continue;
        };
        let key = &field[..equal];
        if [b"UNI_INPUT_DIR".as_slice(), b"UNI_PLATFORM", b"UNI_SCRIPT"].contains(&key) {
            let value = std::str::from_utf8(&field[equal + 1..]).ok()?;
            if values.insert(key, value).is_some() {
                return None;
            }
        }
    }
    let metadata = Metadata {
        input: values.get(b"UNI_INPUT_DIR".as_slice())?.to_string(),
        platform: values.get(b"UNI_PLATFORM".as_slice())?.to_string(),
        script: values
            .get(b"UNI_SCRIPT".as_slice())
            .unwrap_or(&"")
            .to_string(),
    };
    (Path::new(&metadata.input).is_absolute() && !metadata.platform.is_empty()).then_some(metadata)
}

fn normalized(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.components().collect())
}

#[derive(Debug, PartialEq)]
pub struct Owner {
    pub path: PathBuf,
    pub command: String,
}

#[cfg(any(target_os = "macos", test))]
fn resolve_owner(metadata: &[Metadata], label: &str) -> Option<Owner> {
    let mut owners = std::collections::HashMap::new();
    for item in metadata {
        let root = Path::new(&item.input);
        let basename = root.file_name().unwrap_or_default().to_string_lossy();
        if item.script.is_empty() {
            // 内置微信菜单必须同时匹配平台与真实目录后缀，不能以当前编辑项目补全归属。
            if item.platform == "mp-weixin"
                && [
                    format!("微信开发者工具 - [{basename}]"),
                    format!("微信开发者工具 - {basename}"),
                ]
                .contains(&label.to_owned())
            {
                owners.insert((normalized(root), label.to_owned()), root.to_path_buf());
            }
            continue;
        }
        // 仅解析 JSON，不执行项目脚本。命令以 UNI_SCRIPT 精确索引，禁止按平台猜测。
        let Ok(file) = std::fs::File::open(root.join("package.json")) else {
            continue;
        };
        use std::io::Read;
        let Ok(package) = serde_json::from_reader::<_, serde_json::Value>(file.take(1024 * 1024))
        else {
            continue;
        };
        let Some(scripts) = package.get("uni-app").and_then(|v| v.get("scripts")) else {
            continue;
        };
        let Some(script) = scripts.get(&item.script) else {
            continue;
        };
        let platform = |script: &serde_json::Value| {
            script
                .get("env")
                .and_then(|v| v.get("UNI_PLATFORM"))
                .and_then(|v| v.as_str())
                .is_some_and(|v| v == item.platform)
        };
        let Some(actual_title) = script.get("title").and_then(|v| v.as_str()) else {
            continue;
        };
        // HX 会把同平台多个自定义菜单同时变为停止；还需配置证明 incoming title 属于同平台。
        let matches = actual_title == label
            || scripts.as_object().is_some_and(|scripts| {
                scripts
                    .values()
                    .any(|s| s.get("title").and_then(|v| v.as_str()) == Some(label) && platform(s))
            });
        if platform(script) && matches {
            owners.insert(
                (normalized(root), actual_title.to_owned()),
                root.to_path_buf(),
            );
        }
    }
    (owners.len() == 1)
        .then(|| {
            owners
                .into_iter()
                .next()
                .map(|((_, command), path)| Owner { path, command })
        })
        .flatten()
}

pub fn running_message(owner: Option<&Path>, current: &Project, projects: &[Project]) -> String {
    let Some(owner) = owner else {
        return format!(
            "已有项目占用该命令，无法确认项目归属；当前项目「{}」未启动",
            current.name
        );
    };
    let name = projects
        .iter()
        .find(|p| normalized(Path::new(&p.path)) == normalized(owner))
        .map(|p| p.name.clone())
        .unwrap_or_else(|| {
            owner
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    if normalized(owner) == normalized(Path::new(&current.path)) {
        format!("项目「{name}」已在运行，无需重复启动")
    } else {
        format!(
            "有项目「{name}」已经在运行了；当前项目「{}」未启动",
            current.name
        )
    }
}

pub fn allow_stop(
    owner: Option<&Path>,
    current: &Project,
    projects: &[Project],
) -> crate::config::Result<()> {
    if owner.is_some_and(|path| normalized(path) == normalized(Path::new(&current.path))) {
        return Ok(());
    }
    Err(format!(
        "{}；不能停止，请在 HBuilderX 中确认对应项目的任务",
        running_message(owner, current, projects)
    ))
}

#[cfg(target_os = "macos")]
pub fn discover(bundle: &Path, label: &str) -> Option<Owner> {
    use std::process::Command;
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,comm="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&output.stdout).ok()?;
    let processes: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (pid, rest) = line.split_once(char::is_whitespace)?;
            let (parent, executable) = rest.trim_start().split_once(char::is_whitespace)?;
            Some((
                pid.parse::<i32>().ok()?,
                parent.parse::<i32>().ok()?,
                executable.trim(),
            ))
        })
        .collect();
    let main = bundle.join("Contents/MacOS/HBuilderX");
    let node = bundle.join("Contents/HBuilderX/plugins/node/node");
    let roots: std::collections::HashSet<_> = processes
        .iter()
        .filter(|p| Path::new(p.2) == main)
        .map(|p| p.0)
        .collect();
    let mut metadata = Vec::new();
    for (pid, parent, executable) in &processes {
        if Path::new(executable) != node {
            continue;
        }
        let mut ancestor = *parent;
        let mut descendant = false;
        for _ in 0..64 {
            if roots.contains(&ancestor) {
                descendant = true;
                break;
            }
            let Some(next) = processes.iter().find(|p| p.0 == ancestor) else {
                break;
            };
            if next.1 == ancestor {
                break;
            }
            ancestor = next.1;
        }
        if !descendant {
            continue;
        }
        // 仅 HX 自带 Node 后代可读；完整环境仅短暂留在内存，永不写日志。
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, *pid];
        let mut bytes = vec![0u8; 1024 * 1024];
        let mut size = bytes.len();
        let result = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                bytes.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || size > bytes.len() {
            continue;
        }
        if let Some(item) = parse_process(&bytes[..size], bundle) {
            metadata.push(item);
        }
    }
    resolve_owner(&metadata, label)
}

#[cfg(not(target_os = "macos"))]
pub fn discover(_bundle: &Path, _label: &str) -> Option<Owner> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(path: &Path, name: &str) -> Project {
        serde_json::from_value(serde_json::json!({
            "id":name,"name":name,"type":"uniapp","path":path,"platforms":["dingtalk"],
            "manifestPath":"","androidAppId":"","iosAppId":"","androidProjectPath":"",
            "androidModule":"","jdkPath":"","gradleTask":"","outputDir":"",
            "syncMainVersion":false,"terminal":"","commands":[]
        }))
        .unwrap()
    }

    fn process(args: &[&str], env: &[&str]) -> Vec<u8> {
        let mut bytes = (args.len() as i32).to_ne_bytes().to_vec();
        bytes.extend_from_slice(
            b"/Applications/HBuilderX.app/Contents/HBuilderX/plugins/node/node\0\0",
        );
        for item in args.iter().chain(env) {
            bytes.extend_from_slice(item.as_bytes());
            bytes.push(0);
        }
        bytes
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "显式传入期望目录，只读验证当前 HX 运行进程，不启动项目"]
    fn live_read_only_owner_probe() {
        let expected = std::env::var("DEVDOCK_TEST_HX_OWNER").expect("需要期望目录");
        let label = std::env::var("DEVDOCK_TEST_HX_LABEL").expect("需要命令名称");
        let owner = discover(Path::new("/Applications/HBuilderX.app"), &label)
            .expect("应从运行中的编译进程识别归属");
        assert_eq!(normalized(&owner.path), normalized(Path::new(&expected)));
        println!(
            "只读归属验证通过：{}，实际命令：{}",
            owner.path.display(),
            owner.command
        );
    }

    #[test]
    fn running_owner_same_cross_unknown_and_unconfigured() {
        let dir = tempfile::tempdir().unwrap();
        let a = project(&dir.path().join("a"), "门店甲");
        let b = project(&dir.path().join("b"), "门店乙");
        let projects = vec![a.clone(), b.clone()];
        assert_eq!(
            running_message(Some(Path::new(&a.path)), &a, &projects),
            "项目「门店甲」已在运行，无需重复启动"
        );
        assert_eq!(
            running_message(Some(Path::new(&a.path)), &b, &projects),
            "有项目「门店甲」已经在运行了；当前项目「门店乙」未启动"
        );
        assert_eq!(
            running_message(None, &b, &projects),
            "已有项目占用该命令，无法确认项目归属；当前项目「门店乙」未启动"
        );
        assert!(
            running_message(Some(&dir.path().join("真实目录")), &b, &projects)
                .contains("「真实目录」")
        );
    }

    #[test]
    fn process_metadata_is_whitelisted_and_requires_hx_compiler() {
        // 测试数据使用宿主路径，与解析器的 Path::join 和绝对路径校验保持一致。
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("HBuilderX.app");
        let plugins = bundle.join("Contents/HBuilderX/plugins");
        let hook = plugins.join("uniapp-extension/static/kill.js");
        let compiler = plugins.join("uniapp-cli/bin/uniapp-cli.js");
        let input = dir.path().join("项目 space").join("$(touch injected)");
        let input_env = format!("UNI_INPUT_DIR={}", input.display());
        let args = ["node", hook.to_str().unwrap(), compiler.to_str().unwrap()];
        let raw = process(
            &args,
            &[
                "SECRET=do-not-expose",
                &input_env,
                "UNI_SCRIPT=dev;echo nope",
                "UNI_PLATFORM=mp-alipay",
            ],
        );
        let metadata = parse_process(&raw, &bundle).expect("应识别宿主路径构造的编译进程元数据");
        assert_eq!(metadata.input, input.to_string_lossy());
        assert_eq!(metadata.script, "dev;echo nope");
        assert!(!format!("{metadata:?}").contains("SECRET"));
        let unrelated = dir.path().join("uniapp-cli.js");
        assert!(parse_process(
            &process(
                &["node", unrelated.to_str().unwrap()],
                &[&input_env, "UNI_PLATFORM=mp-alipay"],
            ),
            &bundle,
        )
        .is_none());
        assert!(parse_process(&raw, &dir.path().join("Other.app")).is_none());
        assert!(parse_process(&[1, 2, 3], &bundle).is_none());
        let other_input_env = format!("UNI_INPUT_DIR={}", dir.path().join("other").display());
        assert!(parse_process(
            &process(
                &args,
                &[&input_env, &other_input_env, "UNI_PLATFORM=mp-alipay"]
            ),
            &bundle,
        )
        .is_none());
    }

    #[test]
    fn stop_requires_same_real_owner_and_never_uses_current_project_as_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let a = project(&dir.path().join("a"), "项目甲");
        let b = project(&dir.path().join("b"), "项目乙");
        let projects = vec![a.clone(), b.clone()];
        assert!(allow_stop(Some(Path::new(&a.path)), &a, &projects).is_ok());
        let message = allow_stop(Some(Path::new(&a.path)), &b, &projects).unwrap_err();
        assert!(
            message.contains("项目甲")
                && message.contains("项目乙")
                && message.contains("不能停止")
        );
        assert!(allow_stop(None, &b, &projects)
            .unwrap_err()
            .contains("无法确认项目归属"));
    }

    #[test]
    fn related_stopped_platform_menu_reports_actual_custom_command() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"uni-app":{"scripts":{"dev":{"title":"开发","env":{"UNI_PLATFORM":"mp-alipay"}},"prod":{"title":"生产","env":{"UNI_PLATFORM":"mp-alipay"}},"wx":{"title":"微信","env":{"UNI_PLATFORM":"mp-weixin"}}}}}"#).unwrap();
        let metadata = vec![Metadata {
            input: dir.path().to_string_lossy().into(),
            script: "dev".into(),
            platform: "mp-alipay".into(),
        }];
        assert_eq!(resolve_owner(&metadata, "生产").unwrap().command, "开发");
        assert!(resolve_owner(&metadata, "微信").is_none());
        assert!(resolve_owner(&metadata, "未知钉钉").is_none());
    }

    #[test]
    fn builtin_weixin_requires_live_platform_and_real_directory_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let metadata = vec![Metadata {
            input: dir.path().join("real-project").to_string_lossy().into(),
            script: String::new(),
            platform: "mp-weixin".into(),
        }];
        assert!(resolve_owner(&metadata, "微信开发者工具 - [real-project]").is_some());
        assert!(resolve_owner(&metadata, "微信开发者工具 - real-project").is_some());
        assert!(resolve_owner(&metadata, "微信开发者工具 - [current-editor]").is_none());
        assert!(resolve_owner(&metadata, "微信开发者工具 - [real-project] 指定页面").is_none());
    }

    #[test]
    fn real_paths_match_configured_alias_without_prefix_collision() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("real");
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        let alias = dir.path().join("alias");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&root, &alias).unwrap();
        #[cfg(not(unix))]
        let alias = root.clone();
        let a = project(&alias, "配置名称");
        assert!(running_message(Some(&root), &a, &[a.clone()]).contains("项目「配置名称」已在运行"));
        assert!(
            !running_message(Some(&dir.path().join("real-other")), &a, &[a.clone()])
                .contains("「配置名称」已经在运行")
        );
    }

    #[test]
    fn owner_requires_live_matching_command_and_unambiguous_real_path() {
        let dir = tempfile::tempdir().unwrap();
        let title = "钉钉小程序-开发;$(touch never)";
        let mut metadata = vec![];
        for name in ["one", "two"] {
            let root = dir.path().join(name);
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("package.json"), serde_json::to_vec(&serde_json::json!({"uni-app":{"scripts":{"dev":{"title":title,"env":{"UNI_PLATFORM":"mp-alipay"}}}}})).unwrap()).unwrap();
            metadata.push(Metadata {
                input: root.to_string_lossy().into(),
                platform: "mp-alipay".into(),
                script: "dev".into(),
            });
        }
        assert_eq!(
            resolve_owner(&metadata[..1], title),
            Some(Owner {
                path: dir.path().join("one"),
                command: title.into()
            })
        );
        assert!(resolve_owner(&metadata, title).is_none());
        assert!(resolve_owner(&metadata[..1], "生产").is_none());
        assert!(
            resolve_owner(&[], title).is_none(),
            "手动停止后不能沿用旧归属"
        );
        metadata[0].platform = "mp-weixin".into();
        assert!(resolve_owner(&metadata[..1], title).is_none());
        metadata[0].platform = "mp-alipay".into();
        std::fs::remove_file(dir.path().join("one/package.json")).unwrap();
        assert!(resolve_owner(&metadata[..1], title).is_none());
    }
}
