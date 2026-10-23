use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub servers: Vec<Server>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Server {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default = "game_port")]
    pub port: u16,
    #[serde(default = "local_mode")]
    pub mode: String,
    #[serde(default)]
    pub directory: String,
    #[serde(default = "java")]
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "rcon_port")]
    pub rcon_port: u16,
    #[serde(default)]
    pub rcon_password: String,
    #[serde(default)]
    pub agent_url: String,
    #[serde(default)]
    pub agent_token: String,
    #[serde(default)]
    pub agent_server_id: String,
    #[serde(default)]
    pub telemetry_url: String,
    #[serde(default)]
    pub telemetry_token: String,
    #[serde(default)]
    pub viewers: Vec<Viewer>,
}

fn game_port() -> u16 {
    25565
}
fn rcon_port() -> u16 {
    25575
}
fn local_mode() -> String {
    "local".into()
}
fn java() -> String {
    "java".into()
}

impl Server {
    pub fn public(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap();
        value["rcon_password"] = "".into();
        value["agent_token"] = "".into();
        value["telemetry_token"] = "".into();
        value["has_rcon_password"] = (!self.rcon_password.is_empty()).into();
        value["has_agent_token"] = (!self.agent_token.is_empty()).into();
        value["has_telemetry_token"] = (!self.telemetry_token.is_empty()).into();
        value
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.host.trim().is_empty() || self.port == 0 {
            return Err("A name, hostname, and valid game port are required.".into());
        }
        if !["local", "rcon", "agent"].contains(&self.mode.as_str()) {
            return Err("Choose local, RCON, or agent management.".into());
        }
        if self.mode == "local" && (self.directory.is_empty() || self.executable.is_empty()) {
            return Err("Local servers need a working directory and executable.".into());
        }
        if self.mode == "rcon" && (self.rcon_password.is_empty() || self.rcon_port == 0) {
            return Err("RCON requires a password and port.".into());
        }
        if self.mode == "agent" {
            let url = reqwest::Url::parse(&self.agent_url).map_err(|_| "Invalid agent URL.")?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "Use an HTTP(S) agent URL without credentials, query, or fragment.".into(),
                );
            }
            if self.agent_token.is_empty() || self.agent_server_id.is_empty() {
                return Err("Agent token and remote server ID are required.".into());
            }
        }
        for viewer in &self.viewers {
            viewer.validate()?;
        }
        if !self.telemetry_url.is_empty() {
            Viewer {
                name: "Telemetry".into(),
                url: self.telemetry_url.clone(),
            }
            .validate()?;
            if self.telemetry_token.is_empty() {
                return Err("The telemetry bridge requires its access token.".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Viewer {
    pub name: String,
    pub url: String,
}
impl Viewer {
    pub fn validate(&self) -> Result<(), String> {
        let url = reqwest::Url::parse(&self.url).map_err(|_| "Invalid viewer URL.")?;
        if self.name.trim().is_empty()
            || !["http", "https"].contains(&url.scheme())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "Viewers need a name and an HTTP(S) URL without embedded credentials.".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub id: u64,
    pub time: String,
    pub level: String,
    pub text: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    #[serde(default)]
    pub uuid: String,
    #[serde(default)]
    pub ping_ms: Option<u32>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Resources {
    pub cpu: f32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub process_memory: Option<u64>,
    pub process_cpu: Option<f32>,
    pub uptime: u64,
    pub disk_free: u64,
    pub disk_total: u64,
    pub cpu_count: usize,
    #[serde(default)]
    pub heap_used: Option<u64>,
    #[serde(default)]
    pub heap_max: Option<u64>,
    #[serde(default)]
    pub memory_limit: Option<u64>,
    #[serde(default)]
    pub host_uptime: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub online: bool,
    pub managed: bool,
    pub motd: String,
    pub icon: Option<String>,
    pub version: String,
    pub players_online: u32,
    pub players_max: u32,
    pub players: Vec<Player>,
    pub player_list_complete: bool,
    pub latency_ms: Option<u64>,
    pub resources: Option<Resources>,
    pub commands: Vec<String>,
    pub command_source: String,
    pub error: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub uptime_seconds: Option<u64>,
    #[serde(default)]
    pub telemetry_error: Option<String>,
    #[serde(default)]
    pub log_source: String,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            online: false,
            managed: false,
            motd: "Waiting for server…".into(),
            icon: None,
            version: "—".into(),
            players_online: 0,
            players_max: 0,
            players: vec![],
            player_list_complete: false,
            latency_ms: None,
            resources: None,
            commands: vec![],
            command_source: "unavailable".into(),
            error: None,
            updated_at: None,
            uptime_seconds: None,
            telemetry_error: None,
            log_source: "console".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_script_viewers_and_redacts_credentials() {
        assert!(Viewer {
            name: "bad".into(),
            url: "javascript:alert(1)".into()
        }
        .validate()
        .is_err());
        let s: Server = serde_json::from_value(serde_json::json!({
            "name":"Test", "host":"localhost", "rcon_password":"private", "agent_token":"secret", "telemetry_token":"private-telemetry"
        }))
        .unwrap();
        let public = s.public();
        assert_eq!(public["rcon_password"], "");
        assert_eq!(public["agent_token"], "");
        assert_eq!(public["telemetry_token"], "");
        assert_eq!(public["has_rcon_password"], true);
    }
}
