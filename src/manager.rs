use crate::{model::*, protocol};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use sysinfo::{Disks, Pid, System};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{Mutex, RwLock},
};

pub type App = Arc<Manager>;
pub type Result<T> = std::result::Result<T, String>;
pub struct ManagedProcess {
    pub child: Child,
    pub stdin: ChildStdin,
    pub started: Instant,
}
pub struct Manager {
    pub config: Mutex<Config>,
    pub path: PathBuf,
    pub processes: Mutex<HashMap<String, Arc<Mutex<ManagedProcess>>>>,
    pub snapshots: RwLock<HashMap<String, Snapshot>>,
    pub logs: RwLock<HashMap<String, Vec<LogLine>>>,
    pub http: reqwest::Client,
    pub token: String,
    pub agent: bool,
    pub next_log: AtomicU64,
}
impl Manager {
    pub async fn new(path: PathBuf, token: String, agent: bool) -> Result<App> {
        let config: Config = match tokio::fs::read(&path).await {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| format!("Invalid configuration: {e}"))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(format!("Cannot read configuration: {e}")),
        };
        let mut ids = std::collections::HashSet::new();
        for s in &config.servers {
            s.validate()?;
            if s.id.is_empty() || !ids.insert(s.id.clone()) {
                return Err("Server IDs must be unique and nonempty.".into());
            }
        }
        Ok(Arc::new(Self {
            config: Mutex::new(config),
            path,
            processes: Mutex::new(HashMap::new()),
            snapshots: RwLock::new(HashMap::new()),
            logs: RwLock::new(HashMap::new()),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(40))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| e.to_string())?,
            token,
            agent,
            next_log: AtomicU64::new(1),
        }))
    }
    pub async fn server(&self, id: &str) -> Result<Server> {
        self.config
            .lock()
            .await
            .servers
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or("Server not found".into())
    }
    async fn persist(&self, config: &Config) -> Result<()> {
        let parent = self.path.parent().ok_or("Invalid config path")?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
        let temp = parent.join(format!(".void-{}.tmp", uuid::Uuid::new_v4()));
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temp).await.map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(config).map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?;
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        tokio::fs::rename(&temp, &self.path)
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn save(&self, mut server: Server, existing: Option<&str>) -> Result<Server> {
        let mut config = self.config.lock().await;
        let mut next = config.clone();
        if let Some(id) = existing {
            let entry = next
                .servers
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or("Server not found")?;
            if self.processes.lock().await.contains_key(id) {
                return Err("Stop this managed server before editing its configuration.".into());
            }
            server.id = id.into();
            // Blank credential fields preserve existing secrets; they never leave the Rust backend.
            if server.rcon_password.is_empty() {
                server.rcon_password = entry.rcon_password.clone();
            }
            if server.agent_token.is_empty() {
                server.agent_token = entry.agent_token.clone();
            }
            server.validate()?;
            *entry = server.clone();
        } else {
            server.id = uuid::Uuid::new_v4().to_string();
            server.validate()?;
            next.servers.push(server.clone());
        }
        if self.agent && server.mode == "agent" {
            return Err("Agents cannot chain to other agents.".into());
        }
        self.persist(&next).await?;
        *config = next;
        self.snapshots.write().await.remove(&server.id);
        Ok(server)
    }
    pub async fn remove(&self, id: &str) -> Result<()> {
        if self.processes.lock().await.contains_key(id) {
            return Err("Stop the managed server before removing it.".into());
        }
        let mut config = self.config.lock().await;
        let mut next = config.clone();
        let before = next.servers.len();
        next.servers.retain(|s| s.id != id);
        if next.servers.len() == before {
            return Err("Server not found".into());
        }
        self.persist(&next).await?;
        *config = next;
        self.snapshots.write().await.remove(id);
        self.logs.write().await.remove(id);
        Ok(())
    }
    pub async fn save_viewers(&self, id: &str, viewers: Vec<Viewer>) -> Result<()> {
        for viewer in &viewers {
            viewer.validate()?;
        }
        let mut config = self.config.lock().await;
        let mut next = config.clone();
        next.servers
            .iter_mut()
            .find(|s| s.id == id)
            .ok_or("Server not found")?
            .viewers = viewers;
        self.persist(&next).await?;
        *config = next;
        Ok(())
    }
    pub async fn log(&self, id: &str, text: String) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .take(16_384)
            .collect();
        let level = if text.contains("ERROR") || text.contains("Exception") {
            "error"
        } else if text.contains("WARN") {
            "warn"
        } else {
            "info"
        };
        let entry = LogLine {
            id: self.next_log.fetch_add(1, Ordering::Relaxed),
            time: chrono::Local::now().format("%H:%M:%S").to_string(),
            level: level.into(),
            text: protocol::strip_format(&text),
        };
        // A /list response gives the full list, unlike the sampled status ping.
        if let Some((_, list)) = text.split_once("players online:") {
            let mut snapshots = self.snapshots.write().await;
            let snapshot = snapshots.entry(id.into()).or_default();
            let previous = snapshot.players.clone();
            snapshot.players = list
                .trim()
                .split(", ")
                .filter(|s| !s.is_empty())
                .map(|name| Player {
                    name: name.into(),
                    uuid: previous
                        .iter()
                        .find(|p| p.name == name)
                        .map(|p| p.uuid.clone())
                        .unwrap_or_default(),
                })
                .collect();
            snapshot.players_online = snapshot.players.len() as u32;
            snapshot.player_list_complete = true;
        }
        let mut logs = self.logs.write().await;
        let lines = logs.entry(id.into()).or_default();
        lines.push(entry);
        if lines.len() > 1000 {
            lines.drain(..lines.len() - 1000);
        }
    }
    pub async fn proxy(
        &self,
        server: &Server,
        method: reqwest::Method,
        suffix: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let mut base = reqwest::Url::parse(&format!("{}/", server.agent_url.trim_end_matches('/')))
            .map_err(|e| e.to_string())?;
        base.path_segments_mut()
            .map_err(|_| "Invalid agent URL")?
            .extend(["api", "servers", &server.agent_server_id]);
        for segment in suffix.split('/').filter(|s| !s.is_empty()) {
            base.path_segments_mut().unwrap().push(segment);
        }
        let mut request = self
            .http
            .request(method, base)
            .bearer_auth(&server.agent_token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| {
            "Cannot reach agent. Check its URL, TLS, and network access.".to_string()
        })?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > 8_388_608 {
            return Err("Agent response is too large".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "Agent returned an invalid response")?;
        if !status.is_success() {
            return Err(value["error"]
                .as_str()
                .unwrap_or("Agent request failed; check authentication and remote server ID.")
                .into());
        }
        Ok(value)
    }
    pub async fn start(self: &App, server: &Server) -> Result<()> {
        if server.mode == "agent" {
            self.proxy(server, reqwest::Method::POST, "action/start", None)
                .await?;
            return Ok(());
        }
        if server.mode != "local" {
            return Err(
                "Starting a remote server requires the void.dash agent on that host.".into(),
            );
        }
        let mut processes = self.processes.lock().await;
        if processes.contains_key(&server.id) {
            return Err("This server already has a managed process.".into());
        }
        if protocol::status(&server.host, server.port).await.is_ok() {
            return Err("A server is already listening at this address. Stop it before launching another process.".into());
        }
        let mut child = Command::new(&server.executable)
            .args(&server.args)
            .current_dir(&server.directory)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| format!("Cannot start process: {e}"))?;
        let stdin = child.stdin.take().ok_or("Cannot open process input")?;
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        processes.insert(
            server.id.clone(),
            Arc::new(Mutex::new(ManagedProcess {
                child,
                stdin,
                started: Instant::now(),
            })),
        );
        drop(processes);
        let app = self.clone();
        let id = server.id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app.log(&id, line).await;
            }
        });
        let app = self.clone();
        let id = server.id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app.log(&id, line).await;
            }
        });
        self.log(&server.id, "[void.dash] Starting server process…".into())
            .await;
        Ok(())
    }
    pub async fn stop(&self, server: &Server) -> Result<()> {
        if server.mode == "agent" {
            self.proxy(server, reqwest::Method::POST, "action/stop", None)
                .await?;
            return Ok(());
        }
        let process = self.processes.lock().await.get(&server.id).cloned();
        if let Some(handle) = process {
            let mut process = handle.lock().await;
            process
                .stdin
                .write_all(b"stop\n")
                .await
                .map_err(|e| e.to_string())?;
            self.log(
                &server.id,
                "[void.dash] Sent stop. Waiting for the world to save…".into(),
            )
            .await;
            match tokio::time::timeout(std::time::Duration::from_secs(30), process.child.wait()).await {
                Ok(Ok(_)) => (),
                Ok(Err(e)) => return Err(e.to_string()),
                Err(_) => return Err("Server is still saving after 30 seconds. It has not been force-killed; inspect the console.".into()),
            }
            self.remove_process(&server.id, &handle).await;
            Ok(())
        } else if !server.rcon_password.is_empty() {
            protocol::rcon(
                &server.host,
                server.rcon_port,
                &server.rcon_password,
                "stop",
            )
            .await?;
            Ok(())
        } else {
            Err("No process is managed by this dashboard. Configure RCON to stop an externally started server.".into())
        }
    }
    async fn remove_process(&self, id: &str, handle: &Arc<Mutex<ManagedProcess>>) {
        let mut processes = self.processes.lock().await;
        // A monitor holding an old handle must not remove a newly restarted process.
        if processes
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, handle))
        {
            processes.remove(id);
        }
    }
    pub async fn command(&self, server: &Server, command: &str) -> Result<String> {
        let command = command.trim().trim_start_matches('/');
        if command.is_empty() || command.len() > 8192 || command.contains(['\n', '\r', '\0']) {
            return Err("Enter one command, up to 8192 bytes.".into());
        }
        if server.mode == "agent" {
            let value = self
                .proxy(
                    server,
                    reqwest::Method::POST,
                    "command",
                    Some(serde_json::json!({"command":command})),
                )
                .await?;
            return Ok(value["output"].as_str().unwrap_or("").into());
        }
        self.log(&server.id, format!("[void.dash] > {command}"))
            .await;
        let process = self.processes.lock().await.get(&server.id).cloned();
        if let Some(process) = process {
            let mut process = process.lock().await;
            process
                .stdin
                .write_all(format!("{command}\n").as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            process.stdin.flush().await.map_err(|e| e.to_string())?;
            Ok("Command sent to server console.".into())
        } else if !server.rcon_password.is_empty() {
            let output = protocol::rcon(
                &server.host,
                server.rcon_port,
                &server.rcon_password,
                command,
            )
            .await?;
            for line in output.lines() {
                self.log(&server.id, line.into()).await;
            }
            Ok(output)
        } else {
            Err("Start the local process or configure RCON to send commands.".into())
        }
    }
    pub async fn discover(&self, server: &Server) -> Result<Vec<String>> {
        if server.mode == "agent" {
            let value = self
                .proxy(server, reqwest::Method::POST, "commands/refresh", None)
                .await?;
            return serde_json::from_value(value["commands"].clone()).map_err(|e| e.to_string());
        }
        let (mut commands, source) = if !server.rcon_password.is_empty() {
            let mut all = vec![];
            for page in 1..=20 {
                // Vanilla help has no numeric page argument; Bukkit/Paper add paging.
                let command = if page == 1 {
                    "help".to_owned()
                } else {
                    format!("help {page}")
                };
                let help = protocol::rcon(
                    &server.host,
                    server.rcon_port,
                    &server.rcon_password,
                    &command,
                )
                .await?;
                let found = protocol::parse_commands(&help);
                if found.is_empty() || found.iter().all(|c| all.contains(c)) {
                    break;
                }
                all.extend(found);
            }
            (all, "rcon_help")
        } else {
            self.command(server, "help").await?;
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            let logs = self.logs.read().await;
            let text = logs
                .get(&server.id)
                .map(|lines| {
                    lines
                        .iter()
                        .rev()
                        .take(150)
                        .map(|l| l.text.split_once("]: ").map(|(_, s)| s).unwrap_or(&l.text))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            (protocol::parse_commands(&text), "console_help")
        };
        commands.sort();
        commands.dedup();
        let mut snapshots = self.snapshots.write().await;
        let snapshot = snapshots.entry(server.id.clone()).or_default();
        snapshot.commands = commands.clone();
        snapshot.command_source = source.into();
        if commands.is_empty() {
            return Err("The server returned no parseable commands. RCON exposes more complete help on Paper/Bukkit; try refreshing after startup.".into());
        }
        Ok(commands)
    }
    pub async fn background(self: App) {
        let mut system = System::new_all();
        let mut tick = 0u64;
        loop {
            system.refresh_all();
            let disks = Disks::new_with_refreshed_list();
            let host = Resources {
                cpu: system.global_cpu_usage(),
                memory_used: system.used_memory(),
                memory_total: system.total_memory(),
                cpu_count: system.cpus().len(),
                disk_free: disks.iter().map(|d| d.available_space()).sum(),
                disk_total: disks.iter().map(|d| d.total_space()).sum(),
                ..Default::default()
            };
            let servers = self.config.lock().await.servers.clone();
            let mut tasks = tokio::task::JoinSet::new();
            for server in servers {
                let mut resources = host.clone();
                let process = self.processes.lock().await.get(&server.id).cloned();
                if let Some(handle) = process {
                    let mut p = handle.lock().await;
                    match p.child.try_wait() {
                        Ok(Some(code)) => {
                            self.remove_process(&server.id, &handle).await;
                            self.log(
                                &server.id,
                                format!("[void.dash] Server process exited: {code}"),
                            )
                            .await;
                        }
                        Ok(None) => {
                            resources.uptime = p.started.elapsed().as_secs();
                            if let Some(process) = p
                                .child
                                .id()
                                .and_then(|id| system.process(Pid::from_u32(id)))
                            {
                                resources.process_memory = Some(process.memory());
                                resources.process_cpu =
                                    Some(process.cpu_usage() / resources.cpu_count.max(1) as f32);
                            }
                        }
                        Err(e) => {
                            self.log(&server.id, format!("[void.dash] Process check failed: {e}"))
                                .await
                        }
                    }
                }
                let app = self.clone();
                tasks.spawn(async move {
                    app.refresh(server, resources, tick).await;
                });
            }
            while tasks.join_next().await.is_some() {}
            tick += 1;
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }
    async fn refresh(&self, server: Server, resources: Resources, tick: u64) {
        if server.mode == "agent" {
            match self
                .proxy(&server, reqwest::Method::GET, "state", None)
                .await
            {
                Ok(value) => match serde_json::from_value::<Snapshot>(value) {
                    Ok(snapshot) => {
                        self.snapshots.write().await.insert(server.id, snapshot);
                    }
                    Err(_) => {
                        self.snapshots
                            .write()
                            .await
                            .entry(server.id)
                            .or_default()
                            .error = Some("Agent returned an incompatible state.".into());
                    }
                },
                Err(error) => {
                    let mut snapshots = self.snapshots.write().await;
                    let snapshot = snapshots.entry(server.id).or_default();
                    snapshot.online = false;
                    snapshot.resources = None;
                    snapshot.error = Some(error);
                }
            }
            return;
        }
        let result = protocol::status(&server.host, server.port).await;
        let managed = self.processes.lock().await.contains_key(&server.id);
        {
            let mut snapshots = self.snapshots.write().await;
            let snapshot = snapshots.entry(server.id.clone()).or_default();
            snapshot.managed = managed;
            snapshot.resources = if server.mode == "local" {
                Some(resources)
            } else {
                None
            };
            snapshot.updated_at = Some(chrono::Utc::now().to_rfc3339());
            match result {
                Ok(status) => {
                    snapshot.online = true;
                    snapshot.motd = status.motd;
                    snapshot.icon = status.icon;
                    snapshot.version = status.version;
                    snapshot.players_online = status.online;
                    snapshot.players_max = status.max;
                    snapshot.latency_ms = Some(status.latency);
                    snapshot.error = None;
                    // Preserve the complete console/RCON list between polls; merge sampled UUIDs.
                    if snapshot.player_list_complete
                        && snapshot.players.len() == status.online as usize
                    {
                        for player in &mut snapshot.players {
                            if let Some(sample) =
                                status.players.iter().find(|p| p.name == player.name)
                            {
                                player.uuid = sample.uuid.clone();
                            }
                        }
                    } else {
                        snapshot.player_list_complete =
                            status.players.len() == status.online as usize;
                        snapshot.players = status.players;
                    }
                }
                Err(error) => {
                    snapshot.online = false;
                    snapshot.error = Some(error);
                    snapshot.latency_ms = None;
                    snapshot.players.clear();
                    snapshot.players_online = 0;
                    snapshot.player_list_complete = false;
                }
            }
        }
        if tick.is_multiple_of(5) {
            if !server.rcon_password.is_empty() {
                if let Ok(output) = protocol::rcon(
                    &server.host,
                    server.rcon_port,
                    &server.rcon_password,
                    "list",
                )
                .await
                {
                    self.log(&server.id, output).await;
                }
            } else if managed
                && self
                    .snapshots
                    .read()
                    .await
                    .get(&server.id)
                    .is_some_and(|s| s.online)
            {
                let _ = self.command(&server, "list").await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn manages_real_process_logs_commands_viewers_and_graceful_stop() {
        let directory = std::env::temp_dir().join(format!("void-process-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let app = Manager::new(directory.join("config.json"), "test".into(), false)
            .await
            .unwrap();
        let server: Server = serde_json::from_value(serde_json::json!({
            "name":"Process fixture", "host":"127.0.0.1", "port":9, "mode":"local",
            "directory":directory, "executable":"/bin/sh",
            "args":["-c","echo 'Fixture ready'; while IFS= read -r cmd; do case \"$cmd\" in help) printf '/list: Players\\n/stop: Stop server\\n' ;; list) echo 'There are 1 of a max of 20 players online: Alex' ;; stop) echo 'World saved'; exit 0 ;; *) printf 'Executed: %s\\n' \"$cmd\" ;; esac; done"]
        })).unwrap();
        let server = app.save(server, None).await.unwrap();
        app.start(&server).await.unwrap();
        assert!(app.start(&server).await.is_err());
        assert_eq!(app.discover(&server).await.unwrap(), vec!["list", "stop"]);
        app.command(&server, "list").await.unwrap();
        app.command(&server, "say hello").await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            app.snapshots.read().await[&server.id].players[0].name,
            "Alex"
        );
        assert!(app.logs.read().await[&server.id]
            .iter()
            .any(|l| l.text == "Executed: say hello"));
        app.save_viewers(
            &server.id,
            vec![Viewer {
                name: "squaremap".into(),
                url: "http://localhost:8080".into(),
            }],
        )
        .await
        .unwrap();
        assert_eq!(app.server(&server.id).await.unwrap().viewers.len(), 1);
        assert!(app.remove(&server.id).await.is_err());
        app.stop(&server).await.unwrap();
        assert!(!app.processes.lock().await.contains_key(&server.id));
        app.remove(&server.id).await.unwrap();
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
