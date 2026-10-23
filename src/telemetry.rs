//! Optional JVM tools and the read-only Bukkit telemetry bridge. No RCON is needed.
use crate::model::{Player, Server};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

#[derive(Deserialize)]
pub struct BridgeSample {
    pub heap_used: u64,
    pub heap_max: u64,
    pub uptime_seconds: u64,
    pub players: Vec<Player>,
    pub sampled_at_ms: u64,
}

/// Auto-discover only the bridge's simple, generated loopback configuration.
/// Custom/remote bindings use the explicit URL and token in the dashboard.
pub async fn local_bridge(server: &Server) -> Option<(String, String)> {
    if server.mode != "local" {
        return None;
    }
    let path = Path::new(&server.directory).join("plugins/void-dash-bridge/config.yml");
    if tokio::fs::metadata(&path).await.ok()?.len() > 65_536 {
        return None;
    }
    let text = tokio::fs::read_to_string(path).await.ok()?;
    let mut bind = None;
    let mut port = None;
    let mut token = None;
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(['\'', '"']);
        match key.trim() {
            "bind" => bind = Some(value.to_owned()),
            "port" => port = value.parse::<u16>().ok(),
            "token" => token = Some(value.to_owned()),
            _ => (),
        }
    }
    if !["127.0.0.1", "localhost", "0.0.0.0"].contains(&bind?.as_str()) {
        return None;
    }
    let token = token?;
    if token.len() < 32
        || !token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return None;
    }
    let port = port.filter(|p| *p > 0)?;
    Some((format!("http://127.0.0.1:{port}/v1/telemetry"), token))
}
impl BridgeSample {
    pub fn validate(&self) -> Result<(), String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if self.heap_max == 0 || self.heap_used > self.heap_max || self.players.len() > 100_000 {
            return Err("The telemetry bridge returned invalid heap or player data.".into());
        }
        if now.abs_diff(self.sampled_at_ms) > 30_000 {
            return Err(
                "The telemetry bridge sample is stale. Check the server and system clocks.".into(),
            );
        }
        Ok(())
    }
}

pub fn memory_bytes(value: &str) -> Option<u64> {
    let (digits, multiplier) = match value.chars().last()?.to_ascii_lowercase() {
        'k' => (&value[..value.len() - 1], 1024u64),
        'm' => (&value[..value.len() - 1], 1024u64.pow(2)),
        'g' => (&value[..value.len() - 1], 1024u64.pow(3)),
        _ => (value, 1),
    };
    digits
        .parse::<u64>()
        .ok()?
        .checked_mul(multiplier)
        .filter(|n| *n > 0)
}

// Java argument files support quoted arguments and # comments. Backslashes are
// retained (not shell-expanded); only memory switches are inspected, never executed.
fn argfile_tokens(text: &str) -> Vec<String> {
    let mut tokens = vec![];
    let mut token = String::new();
    let mut quote = None;
    let mut comment = false;
    for c in text.chars() {
        if comment {
            if c == '\n' {
                comment = false;
            } else {
                continue;
            }
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                token.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '#' if token.is_empty() => comment = true,
            c if c.is_whitespace() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            _ => token.push(c),
        }
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    tokens
}
pub async fn configured_heap(server: &Server) -> Option<u64> {
    let mut args = vec![];
    for arg in &server.args {
        if let Some(file) = arg.strip_prefix('@') {
            let path = Path::new(&server.directory).join(file);
            if let Ok(metadata) = tokio::fs::metadata(&path).await {
                if metadata.len() <= 1_048_576 {
                    if let Ok(text) = tokio::fs::read_to_string(path).await {
                        args.extend(argfile_tokens(&text));
                    }
                }
            }
        } else {
            args.push(arg.clone());
        }
    }
    let mut limit = None;
    for arg in args {
        if arg == "-jar" {
            break;
        }
        if let Some(size) = arg
            .strip_prefix("-Xmx")
            .or_else(|| arg.strip_prefix("-XX:MaxHeapSize="))
        {
            limit = memory_bytes(size);
        }
    }
    limit
}
fn tool_path(executable: &str, name: &str) -> Option<PathBuf> {
    let executable_path = Path::new(executable);
    let java_name = executable_path
        .file_stem()?
        .to_string_lossy()
        .to_ascii_lowercase();
    if java_name != "java" && java_name != "javaw" {
        return None;
    }
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let tool = format!("{name}{suffix}");
    if executable_path.components().count() > 1 {
        let path = std::fs::canonicalize(executable_path).ok()?;
        let sibling = path.parent()?.join(&tool);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let java = directory.join(format!("{java_name}{suffix}"));
            if let Ok(resolved) = std::fs::canonicalize(java) {
                if let Some(parent) = resolved.parent() {
                    let sibling = parent.join(&tool);
                    if sibling.is_file() {
                        return Some(sibling);
                    }
                }
            }
        }
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let candidate = PathBuf::from(home).join("bin").join(&tool);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|p| p.join(&tool))
        .find(|p| p.is_file())
}
async fn tool_output(executable: &str, name: &str, args: &[String]) -> Option<String> {
    let tool = tool_path(executable, name)?;
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(tool).args(args).kill_on_drop(true).output(),
    )
    .await
    .ok()?
    .ok()?;
    if !result.status.success() {
        return None;
    }
    String::from_utf8(result.stdout).ok()
}
pub fn parse_heap_used(output: &str) -> Option<u64> {
    let mut lines = output.lines();
    let columns: Vec<_> = lines.next()?.split_whitespace().collect();
    let values: Vec<_> = lines.next()?.split_whitespace().collect();
    let mut used = 0f64;
    for name in ["S0U", "S1U", "EU", "OU"] {
        let index = columns.iter().position(|c| *c == name)?;
        let value = values.get(index)?;
        used += if *value == "-" {
            0.0
        } else {
            value.parse::<f64>().ok()?
        };
    }
    if !used.is_finite() || used < 0.0 {
        return None;
    }
    Some((used * 1024.0) as u64)
}
pub async fn heap_used(executable: &str, pid: u32) -> Option<u64> {
    parse_heap_used(&tool_output(executable, "jstat", &["-gc".into(), pid.to_string()]).await?)
}
pub async fn heap_max(executable: &str, pid: u32) -> Option<u64> {
    let output = tool_output(executable, "jcmd", &[pid.to_string(), "VM.flags".into()]).await?;
    output
        .split_whitespace()
        .find_map(|word| word.strip_prefix("-XX:MaxHeapSize=").and_then(memory_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_heap_usage_without_counting_metaspace_or_committed_memory() {
        let sample =
            "S0C S1C S0U S1U EC EU OC OU MC MU\n0 1024 0 512 8192 2048 32768 4096 99999 99999\n";
        assert_eq!(parse_heap_used(sample), Some(6656 * 1024));
        assert_eq!(parse_heap_used("not jstat output"), None);
        assert_eq!(memory_bytes("4G"), Some(4 * 1024u64.pow(3)));
        assert_eq!(memory_bytes("0"), None);
    }
    #[tokio::test]
    async fn reads_xmx_from_argument_file_and_ignores_jar_arguments() {
        let directory = std::env::temp_dir().join(format!("void-args-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        tokio::fs::write(
            directory.join("user_jvm_args.txt"),
            "# max heap\n-Xms1G\n\"-Xmx3G\"\n",
        )
        .await
        .unwrap();
        let server:Server=serde_json::from_value(serde_json::json!({"name":"Test","host":"localhost","directory":directory,"args":["@user_jvm_args.txt","-jar","server.jar","-Xmx64G"]})).unwrap();
        assert_eq!(configured_heap(&server).await, Some(3 * 1024u64.pow(3)));
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
    #[tokio::test]
    async fn discovers_local_bridge_without_exposing_or_sending_tokens_to_remote_hosts() {
        let directory = std::env::temp_dir().join(format!("void-bridge-{}", uuid::Uuid::new_v4()));
        let plugin = directory.join("plugins/void-dash-bridge");
        tokio::fs::create_dir_all(&plugin).await.unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        tokio::fs::write(
            plugin.join("config.yml"),
            format!("bind: 127.0.0.1\nport: 8788\ntoken: '{token}'\n"),
        )
        .await
        .unwrap();
        let server: Server = serde_json::from_value(
            serde_json::json!({"name":"Test","host":"localhost","directory":directory}),
        )
        .unwrap();
        assert_eq!(
            local_bridge(&server).await,
            Some(("http://127.0.0.1:8788/v1/telemetry".into(), token.into()))
        );
        tokio::fs::write(
            plugin.join("config.yml"),
            format!("bind: example.com\nport: 8788\ntoken: '{token}'\n"),
        )
        .await
        .unwrap();
        assert!(local_bridge(&server).await.is_none());
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
