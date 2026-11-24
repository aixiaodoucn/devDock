use super::*;
use std::fs;
use tempfile::{tempdir, TempDir};

fn fixture(client: Client) -> (TempDir, Registration) {
    let dir = tempdir().unwrap();
    let home = dir.path().to_owned();
    let reg = Registration {
        client,
        config: home.join(if client == Client::Claude {
            ".claude.json"
        } else {
            "config.toml"
        }),
        exe: "/example/Dev Dock".into(),
        cli: home.join("fake-cli"),
        home,
    };
    (dir, reg)
}
fn record(reg: &Registration) -> String {
    match reg.client {
        Client::Claude => json!({"mcpServers":{(NAME):{"type":"stdio","command":reg.exe,"args":[STDIO],"env":{}}},"unrelated":"keep"}).to_string(),
        Client::Codex => format!("unrelated='keep'\n[mcp_servers.{NAME}]\ncommand='{}'\nargs=['{STDIO}']\n",reg.exe),
    }
}
#[test]
fn ownership_is_exact_and_bad_files_fail_closed() {
    for client in [Client::Claude, Client::Codex] {
        let (_dir, reg) = fixture(client);
        for data in [
            record(&reg).replace(&reg.exe, "/different"),
            record(&reg).replace(STDIO, "--other"),
            "malformed".into(),
        ] {
            fs::write(&reg.config, &data).unwrap();
            for action in [Action::Status, Action::Connect, Action::Disconnect] {
                assert_eq!(reg.perform(action).state, "conflict");
            }
            assert_eq!(fs::read_to_string(&reg.config).unwrap(), data);
        }
        let data = match client {
            Client::Claude => record(&reg).replace("\"env\":{}", "\"env\":{\"CUSTOM\":\"value\"}"),
            Client::Codex => format!("{}cwd='/custom'", record(&reg)),
        };
        fs::write(&reg.config, data).unwrap();
        assert_eq!(reg.perform(Action::Disconnect).state, "conflict");
    }
}
#[test]
fn project_or_local_registration_is_not_ours() {
    let (_dir, reg) = fixture(Client::Claude);
    fs::write(
        &reg.config,
        json!({"projects":{"/project":{"mcpServers":{(NAME):{}}}}}).to_string(),
    )
    .unwrap();
    assert_eq!(reg.perform(Action::Connect).state, "conflict");
    fs::write(&reg.config, record(&reg)).unwrap();
    fs::write(reg.home.join(".mcp.json"), record(&reg)).unwrap();
    assert_eq!(reg.perform(Action::Disconnect).state, "conflict");
}
#[test]
fn positional_arrays_do_not_count_as_named_fields() {
    let (_dir, reg) = fixture(Client::Claude);
    for data in [
        "[]".to_owned(),
        json!({"mcpServers":{(NAME):[reg.exe,[STDIO]]}}).to_string(),
    ] {
        fs::write(&reg.config, data).unwrap();
        assert_eq!(reg.perform(Action::Status).state, "conflict");
    }
}
#[test]
fn template_is_only_stdio() {
    let exe = "C:\\My App\\DevDock.exe";
    let value: serde_json::Value = serde_json::from_str(&template(exe)).unwrap();
    assert_eq!(
        value,
        json!({"mcpServers":{(NAME):{"type":"stdio","command":exe,"args":[STDIO]}}})
    );
}
#[cfg(unix)]
fn script(bin: &Path, contents: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(bin, format!("#!/bin/sh\nset -eu\n{contents}\n")).unwrap();
    fs::set_permissions(bin, fs::Permissions::from_mode(0o700)).unwrap();
}
#[cfg(unix)]
#[test]
fn fake_cli_lifecycle_checks_scope_arguments_and_postcondition() {
    for client in [Client::Claude, Client::Codex] {
        let (_dir, reg) = fixture(client);
        let file = reg.config.file_name().unwrap().to_str().unwrap();
        let scope = if client == Client::Claude {
            "test \"$1\" = --scope && test \"$2\" = user || exit 1; shift 2"
        } else {
            ":"
        };
        fs::write(reg.home.join("next"), record(&reg)).unwrap();
        script(
            &reg.cli,
            &format!(
                r#"test "$1" = mcp; action="$2"; shift 2
{scope}
test "$1" = '{NAME}'; shift
if test "$action" = add; then
  test "$#" = 3 && test "$1" = -- && test "$2" = '/example/Dev Dock' && test "$3" = '{STDIO}' || exit 1
  cp next '{file}'
elif test "$action" = remove && test "$#" = 0; then
  cp empty '{file}'
else exit 1; fi"#
            ),
        );
        fs::write(
            reg.home.join("empty"),
            if client == Client::Claude { "{}" } else { "" },
        )
        .unwrap();
        assert_eq!(reg.perform(Action::Status).state, "available");
        assert!(!reg.config.exists());
        assert_eq!(reg.perform(Action::Connect).state, "configured");
        assert_eq!(reg.perform(Action::Disconnect).state, "available");
        script(&reg.cli, "exit 0");
        assert_eq!(reg.perform(Action::Connect).state, "conflict");
        script(
            &reg.cli,
            "printf PRIVATE_STDOUT; printf PRIVATE_STDERR >&2; exit 1",
        );
        let status = reg.perform(Action::Connect);
        assert_eq!(status.state, "conflict");
        assert!(!status.message.contains("PRIVATE"));
    }
}
#[cfg(unix)]
#[test]
fn slow_cli_is_terminated() {
    use std::time::{Duration, Instant};
    let (_dir, reg) = fixture(Client::Claude);
    script(&reg.cli, "sleep 20");
    let start = Instant::now();
    assert!(cli::run(&reg.cli, &[], &reg.home, Duration::from_millis(50)).is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
}
