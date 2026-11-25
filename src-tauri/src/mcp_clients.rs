//! Manual user-scope MCP registration. Never start a configured MCP server to inspect it.
mod cli;
mod config;
#[cfg(test)]
mod tests;

use serde::Serialize;
use serde_json::json;
use std::{
    env,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{AppHandle, Manager, WebviewWindow};

const STDIO: &str = "--log-mcp-stdio";
const NAME: &str = if cfg!(debug_assertions) {
    "devDock-dev"
} else {
    "devDock"
};
static REGISTRATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, PartialEq, Eq)]
enum Client {
    Claude,
    Codex,
}
#[derive(Clone, Copy)]
enum Action {
    Status,
    Connect,
    Disconnect,
}
#[derive(Serialize)]
pub struct ClientStatus {
    state: &'static str,
    message: &'static str,
}
fn reply(state: &'static str, message: &'static str) -> ClientStatus {
    ClientStatus { state, message }
}
fn uncertain() -> ClientStatus {
    reply("conflict", "无法确认用户配置；请检查客户端配置后重试。")
}
fn executable() -> Option<String> {
    env::current_exe().ok()?.into_os_string().into_string().ok()
}
fn template(exe: &str) -> String {
    serde_json::to_string_pretty(
        &json!({"mcpServers": {(NAME): {"type":"stdio", "command":exe,"args":[STDIO]}}}),
    )
    .expect("JSON strings are serializable")
}

struct Registration {
    client: Client,
    home: PathBuf,
    config: PathBuf,
    exe: String,
    cli: PathBuf,
}
impl Registration {
    fn new(client: Client, home: PathBuf) -> Result<Self, ClientStatus> {
        let cli = cli::locate(client, &home)
            .ok_or_else(|| reply("missing", "未找到客户端 CLI；请安装客户端或手动复制配置。"))?;
        let config = match client {
            Client::Claude => env::var_os("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.clone())
                .join(".claude.json"),
            Client::Codex => env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
        };
        if !config.is_absolute() {
            return Err(uncertain());
        }
        Ok(Self {
            client,
            home,
            config,
            exe: executable().ok_or_else(uncertain)?,
            cli,
        })
    }
    fn perform(&self, action: Action) -> ClientStatus {
        // Serialize all app windows/clicks and inspect again immediately before any mutation.
        let Ok(_guard) = REGISTRATION.lock() else {
            return uncertain();
        };
        let before = config::inspect(self);
        let add = match (action, before.state) {
            (Action::Connect, "available") => true,
            (Action::Disconnect, "configured") => false,
            _ => return before,
        };
        let mut args = vec!["mcp", if add { "add" } else { "remove" }];
        if self.client == Client::Claude {
            args.extend(["--scope", "user"]);
        }
        args.push(NAME);
        if add {
            args.extend(["--", &self.exe, STDIO]);
        }
        if cli::run(&self.cli, &args, &self.home, cli::TIMEOUT).is_err() {
            return reply("conflict", "CLI 执行失败或超时；请刷新确认配置状态。");
        }
        let after = config::inspect(self);
        if after.state == if add { "configured" } else { "available" } {
            after
        } else {
            uncertain()
        }
    }
}

async fn dispatch(
    app: AppHandle,
    window: WebviewWindow,
    client: String,
    action: Action,
) -> ClientStatus {
    if window.label() != "main" {
        return reply("unsupported", "仅主窗口可以管理客户端。");
    }
    let client = match client.as_str() {
        "claude" => Client::Claude,
        "codex" => Client::Codex,
        _ => {
            return reply(
                "unsupported",
                "未验证此客户端的自动配置接口，请手动复制 stdio 配置。",
            )
        }
    };
    let Ok(home) = app.path().home_dir() else {
        return uncertain();
    };
    tauri::async_runtime::spawn_blocking(move || match Registration::new(client, home) {
        Ok(reg) => reg.perform(action),
        Err(status) => status,
    })
    .await
    .unwrap_or_else(|_| uncertain())
}

#[tauri::command]
pub async fn mcp_client_status(
    app: AppHandle,
    window: WebviewWindow,
    client: String,
) -> ClientStatus {
    dispatch(app, window, client, Action::Status).await
}
#[tauri::command]
pub async fn mcp_client_connect(
    app: AppHandle,
    window: WebviewWindow,
    client: String,
) -> ClientStatus {
    dispatch(app, window, client, Action::Connect).await
}
#[tauri::command]
pub async fn mcp_client_disconnect(
    app: AppHandle,
    window: WebviewWindow,
    client: String,
) -> ClientStatus {
    dispatch(app, window, client, Action::Disconnect).await
}
#[tauri::command]
pub fn mcp_client_template(app: AppHandle, window: WebviewWindow) -> Result<String, &'static str> {
    let _ = app;
    if window.label() != "main" {
        return Err("仅主窗口可以读取配置模板。");
    }
    executable()
        .map(|exe| template(&exe))
        .ok_or("无法获取当前应用的可执行路径。")
}
