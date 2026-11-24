use super::*;
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::{collections::BTreeMap, fmt};
use std::{
    fs::File,
    io::{ErrorKind, Read},
};

const MAX_CONFIG: u64 = 32 * 1024 * 1024;

// Missing files are normal; unreadable, malformed or oversized files are not empty configs.
fn read(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() || meta.len() > MAX_CONFIG => return Err(()),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            // A broken symlink is not an invitation to create a new config.
            return match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
                _ => Err(()),
            };
        }
        Err(_) => return Err(()),
        _ => (),
    }
    let mut data = String::new();
    File::open(path)
        .map_err(|_| ())?
        .take(MAX_CONFIG + 1)
        .read_to_string(&mut data)
        .map_err(|_| ())?;
    if data.len() as u64 > MAX_CONFIG {
        Err(())
    } else {
        Ok(Some(data))
    }
}

// Deserialize only the selected server; skip credentials and unrelated server definitions.
#[derive(Default)]
struct Servers(Option<Server>);
impl<'de> Deserialize<'de> for Servers {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Selected;
        impl<'de> Visitor<'de> for Selected {
            type Value = Servers;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("server map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Servers, A::Error> {
                let mut selected = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key == NAME {
                        if selected.is_some() {
                            return Err(serde::de::Error::custom("ambiguous registration"));
                        }
                        selected = Some(map.next_value::<Object<Server>>()?.0);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(Servers(selected))
            }
        }
        deserializer.deserialize_map(Selected)
    }
}
// Serde ordinarily accepts positional arrays for structs too; only named configuration fields count.
struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct OnlyMap<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for OnlyMap<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        de.deserialize_map(OnlyMap(std::marker::PhantomData))
    }
}
#[derive(Default, Deserialize)]
enum Transport {
    #[default]
    #[serde(skip)]
    Implicit,
    #[serde(rename = "stdio")]
    Stdio,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Server {
    command: String,
    args: Vec<String>,
    #[serde(default, rename = "type")]
    kind: Transport,
    #[serde(default)]
    env: BTreeMap<String, IgnoredAny>,
}
impl Server {
    fn matches(&self, reg: &Registration) -> bool {
        self.command == reg.exe
            && self.args == [STDIO]
            && self.env.is_empty()
            && (matches!(self.kind, Transport::Implicit) || reg.client == Client::Claude)
    }
}
#[derive(Deserialize)]
struct ClaudeConfig {
    #[serde(default, rename = "mcpServers")]
    servers: Servers,
    #[serde(default)]
    projects: BTreeMap<String, Object<ClaudeProject>>,
}
#[derive(Deserialize)]
struct ClaudeProject {
    #[serde(default, rename = "mcpServers")]
    servers: Servers,
}
#[derive(Deserialize)]
struct CodexConfig {
    #[serde(default)]
    mcp_servers: Servers,
}
fn owned(reg: &Registration) -> Result<Option<bool>, ()> {
    let contents = read(&reg.config)?;
    let servers = match reg.client {
        Client::Claude => {
            let Object(config): Object<ClaudeConfig> =
                serde_json::from_str(contents.as_deref().unwrap_or("{}")).map_err(|_| ())?;
            if config.projects.values().any(|p| p.0.servers.0.is_some()) {
                return Ok(Some(false));
            }
            if let Some(project) = read(&reg.home.join(".mcp.json"))? {
                if serde_json::from_str::<Object<ClaudeProject>>(&project)
                    .map_err(|_| ())?
                    .0
                    .servers
                    .0
                    .is_some()
                {
                    return Ok(Some(false));
                }
            }
            config.servers
        }
        Client::Codex => {
            toml::from_str::<CodexConfig>(contents.as_deref().unwrap_or(""))
                .map_err(|_| ())?
                .mcp_servers
        }
    };
    Ok(servers.0.map(|v| v.matches(reg)))
}
pub(super) fn inspect(reg: &Registration) -> ClientStatus {
    match owned(reg) {
        Ok(Some(true)) => reply("configured", "已配置当前应用的用户级 stdio 服务。"),
        Ok(Some(false)) => reply("conflict", "同名项属于其他配置或作用域，未修改。"),
        Ok(None) => reply("available", "客户端可用，尚未配置当前服务。"),
        Err(()) => uncertain(),
    }
}
