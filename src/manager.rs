use crate::{logtail, model::*, protocol, telemetry};
use std::{
    collections::{HashMap, VecDeque},
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
    pub heap_max: Option<u64>,
    pub heap_checked: bool,
}
#[derive(Clone, Copy, PartialEq)]
enum LogSource {
    Dashboard,
    Console,
    File,
}
struct SeenLog {
    source: LogSource,
    body: String,
    stamp: Option<String>,
    seen: Instant,
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
    recent_logs: Mutex<HashMap<String, VecDeque<SeenLog>>>,
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
            recent_logs: Mutex::new(HashMap::new()),
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
            if server.telemetry_token.is_empty() {
                server.telemetry_token = entry.telemetry_token.clone();
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
        self.ingest_log(id, text, LogSource::Dashboard).await;
    }
    async fn update_player_list(&self, id: &str, line: &str) -> Option<u32> {
        let (count, names) = logtail::list_report(line)?;
        let mut snapshots = self.snapshots.write().await;
        let snapshot = snapshots.entry(id.into()).or_default();
        let previous = std::mem::take(&mut snapshot.players);
        snapshot.players = names
            .into_iter()
            .map(|name| {
                previous
                    .iter()
                    .find(|p| p.name == name)
                    .cloned()
                    .unwrap_or(Player {
                        name,
                        uuid: String::new(),
                        ping_ms: None,
                    })
            })
            .collect();
        snapshot.players_online = count;
        snapshot.player_list_complete = snapshot.players.len() == count as usize;
        Some(count)
    }
    async fn ingest_log(&self, id: &str, text: String, source: LogSource) {
        let text = logtail::clean_message(&text);
        // Do not display the old automatic empty-list chatter; still update metadata.
        if self.update_player_list(id, &text).await == Some(0) {
            return;
        }
        let body = logtail::message_body(&text);
        for (suffix, joined) in [(" joined the game", true), (" left the game", false)] {
            if let Some(name) = body
                .strip_suffix(suffix)
                .filter(|n| !n.is_empty() && !n.contains(' '))
            {
                let mut snapshots = self.snapshots.write().await;
                let snapshot = snapshots.entry(id.into()).or_default();
                if joined && !snapshot.players.iter().any(|p| p.name == name) {
                    snapshot.players.push(Player {
                        name: name.into(),
                        uuid: String::new(),
                        ping_ms: None,
                    });
                } else if !joined {
                    snapshot.players.retain(|p| p.name != name);
                }
            }
        }
        let stamp = text
            .get(1..9)
            .filter(|_| text.starts_with('['))
            .filter(|s| {
                s.bytes().all(|b| b.is_ascii_digit() || b == b':')
                    && s.as_bytes().get(2) == Some(&b':')
            })
            .map(str::to_owned);
        if source != LogSource::Dashboard {
            let mut recent = self.recent_logs.lock().await;
            let seen = recent.entry(id.into()).or_default();
            seen.retain(|s| s.seen.elapsed().as_secs() < 5);
            if let Some(index) = seen.iter().position(|s| {
                s.source != source
                    && s.body == body
                    && (s.stamp.is_none() || stamp.is_none() || s.stamp == stamp)
            }) {
                seen.remove(index);
                return;
            }
            seen.push_back(SeenLog {
                source,
                body: body.into(),
                stamp: stamp.clone(),
                seen: Instant::now(),
            });
            if seen.len() > 5000 {
                seen.pop_front();
            }
        }
        let level = if text.contains("ERROR") || text.contains("Exception") {
            "error"
        } else if text.contains("WARN") {
            "warn"
        } else {
            "info"
        };
        let entry = LogLine {
            id: self.next_log.fetch_add(1, Ordering::Relaxed),
            time: stamp.unwrap_or_else(|| chrono::Local::now().format("%H:%M:%S").to_string()),
            level: level.into(),
            text: protocol::strip_format(&text),
        };
        let mut logs = self.logs.write().await;
        let lines = logs.entry(id.into()).or_default();
        lines.push(entry);
        if lines.len() > 5000 {
            lines.drain(..lines.len() - 5000);
        }
    }
    async fn follow_logs(self: App) {
        let mut tails = HashMap::<String, logtail::LogTail>::new();
        loop {
            let servers = self.config.lock().await.servers.clone();
            tails.retain(|id, _| servers.iter().any(|s| s.id == *id && s.mode == "local"));
            for server in servers.into_iter().filter(|s| s.mode == "local") {
                let path = std::path::Path::new(&server.directory).join("logs/latest.log");
                let tail = tails.entry(server.id.clone()).or_default();
                match tail.read(&path).await {
                    Ok(lines) => {
                        self.snapshots
                            .write()
                            .await
                            .entry(server.id.clone())
                            .or_default()
                            .log_source = "latest.log + console".into();
                        for line in lines {
                            self.ingest_log(&server.id, line, LogSource::File).await;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => {
                        let mut snapshots = self.snapshots.write().await;
                        snapshots.entry(server.id.clone()).or_default().log_source =
                            format!("console (latest.log unavailable: {error})");
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
        let configured_heap = telemetry::configured_heap(server).await;
        processes.insert(
            server.id.clone(),
            Arc::new(Mutex::new(ManagedProcess {
                child,
                stdin,
                started: Instant::now(),
                heap_max: configured_heap,
                heap_checked: false,
            })),
        );
        drop(processes);
        let app = self.clone();
        let id = server.id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app.ingest_log(&id, line, LogSource::Console).await;
            }
        });
        let app = self.clone();
        let id = server.id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                app.ingest_log(&id, line, LogSource::Console).await;
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
            self.stop_managed(server, &handle).await
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
    pub async fn restart(self: &App, server: &Server) -> Result<()> {
        if server.mode == "rcon" {
            return Err(
                "Restarting remote servers requires an agent; RCON cannot start a stopped process."
                    .into(),
            );
        }
        if server.mode == "agent" {
            self.proxy(server, reqwest::Method::POST, "action/restart", None)
                .await?;
            return Ok(());
        }
        let handle = self.processes.lock().await.get(&server.id).cloned().ok_or(
            "This server was not started by this void.dash session. Stop it in its current Minecraft console, then use Start here before using Restart. Local process control does not require RCON."
        )?;
        // Restart must operate on the process we own, never silently fall back to RCON.
        self.stop_managed(server, &handle).await?;
        self.start(server).await
    }
    async fn stop_managed(
        &self,
        server: &Server,
        handle: &Arc<Mutex<ManagedProcess>>,
    ) -> Result<()> {
        let mut process = handle.lock().await;
        process.stdin.write_all(b"stop\n").await.map_err(|e| {
            format!("Cannot send stop to the managed server process: {e}. It may have exited; inspect the console and use Start to launch it again.")
        })?;
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
        self.remove_process(&server.id, handle).await;
        Ok(())
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
        tokio::spawn(self.clone().follow_logs());
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
                host_uptime: System::uptime(),
                ..Default::default()
            };
            let servers = self.config.lock().await.servers.clone();
            let mut tasks = tokio::task::JoinSet::new();
            for server in servers {
                let mut resources = host.clone();
                let mut jvm = None;
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
                            if let Some(pid) = p.child.id() {
                                jvm = Some((pid, p.heap_max, p.heap_checked, handle.clone()));
                            }
                            if let Some(process) = p
                                .child
                                .id()
                                .and_then(|id| system.process(Pid::from_u32(id)))
                            {
                                resources.process_memory = Some(process.memory());
                                resources.process_cpu = Some(process.cpu_usage());
                            }
                        }
                        Err(e) => {
                            self.log(&server.id, format!("[void.dash] Process check failed: {e}"))
                                .await
                        }
                    }
                }
                if let Some((pid, limit, checked, handle)) = jvm {
                    let (heap_used, observed_max) =
                        tokio::join!(telemetry::heap_used(&server.executable, pid), async {
                            if checked {
                                None
                            } else {
                                telemetry::heap_max(&server.executable, pid).await
                            }
                        });
                    resources.heap_used = heap_used;
                    resources.heap_max = observed_max.or(limit);
                    resources.memory_limit = resources.heap_max;
                    if !checked {
                        let mut p = handle.lock().await;
                        if p.child.id() == Some(pid) {
                            // Retry attachment while a newly launched JVM is still initializing.
                            p.heap_checked =
                                resources.heap_used.is_some() || observed_max.is_some();
                            p.heap_max = resources.heap_max;
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
    async fn refresh(&self, server: Server, mut resources: Resources, tick: u64) {
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
                    snapshot.uptime_seconds = None;
                    snapshot.error = Some(error);
                }
            }
            return;
        }
        let (result, bridge) = tokio::join!(
            protocol::status(&server.host, server.port),
            self.bridge_sample(&server)
        );
        let managed = self.processes.lock().await.contains_key(&server.id);
        {
            let mut snapshots = self.snapshots.write().await;
            let snapshot = snapshots.entry(server.id.clone()).or_default();
            snapshot.managed = managed;
            snapshot.uptime_seconds = managed.then_some(resources.uptime);
            let bridge = match bridge {
                Ok(sample) => {
                    snapshot.telemetry_error = None;
                    sample
                }
                Err(error) => {
                    snapshot.telemetry_error = Some(error);
                    None
                }
            };
            if let Some(sample) = &bridge {
                resources.heap_used = Some(sample.heap_used);
                resources.heap_max = Some(sample.heap_max);
                resources.memory_limit = Some(sample.heap_max);
                snapshot.uptime_seconds = Some(sample.uptime_seconds);
            }
            snapshot.resources = if server.mode == "local" {
                Some(resources)
            } else {
                bridge.as_ref().map(|sample| Resources {
                    heap_used: Some(sample.heap_used),
                    heap_max: Some(sample.heap_max),
                    memory_limit: Some(sample.heap_max),
                    ..Default::default()
                })
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
                    } else if status.players.len() == status.online as usize
                        || snapshot.players.len() > status.online as usize
                    {
                        snapshot.player_list_complete =
                            status.players.len() == status.online as usize;
                        snapshot.players = status.players;
                    } else {
                        for player in status.players {
                            if let Some(existing) =
                                snapshot.players.iter_mut().find(|p| p.name == player.name)
                            {
                                existing.uuid = player.uuid;
                            } else {
                                snapshot.players.push(player);
                            }
                        }
                        snapshot.player_list_complete =
                            snapshot.players.len() == status.online as usize;
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
            if let Some(sample) = bridge {
                snapshot.players = sample.players;
                snapshot.players_online = snapshot.players.len() as u32;
                snapshot.player_list_complete = true;
            }
        }
        if tick.is_multiple_of(5) && !server.rcon_password.is_empty() {
            if let Ok(output) = protocol::rcon(
                &server.host,
                server.rcon_port,
                &server.rcon_password,
                "list",
            )
            .await
            {
                self.update_player_list(&server.id, &output).await;
            }
        }
    }
    async fn bridge_sample(&self, server: &Server) -> Result<Option<telemetry::BridgeSample>> {
        let (url, token) = if server.telemetry_url.is_empty() {
            match telemetry::local_bridge(server).await {
                Some(connection) => connection,
                None => return Ok(None),
            }
        } else {
            (server.telemetry_url.clone(), server.telemetry_token.clone())
        };
        let response = self
            .http
            .get(&url)
            .bearer_auth(&token)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|_| {
                "Cannot reach the telemetry bridge. Check its URL and token.".to_string()
            })?;
        if !response.status().is_success() {
            return Err(format!(
                "Telemetry bridge returned HTTP {}. Check its token and URL.",
                response.status().as_u16()
            ));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "Cannot read the telemetry bridge response.")?;
        if bytes.len() > 2_097_152 {
            return Err("Telemetry bridge response is too large.".into());
        }
        let sample: telemetry::BridgeSample = serde_json::from_slice(&bytes)
            .map_err(|_| "Telemetry bridge returned an incompatible response.")?;
        sample.validate()?;
        Ok(Some(sample))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn full_logs_deduplicate_sources_preserve_command_output_and_hide_empty_list_noise() {
        let app = Manager::new(
            std::env::temp_dir().join(format!("void-logs-{}.json", uuid::Uuid::new_v4())),
            "test".into(),
            false,
        )
        .await
        .unwrap();
        app.ingest_log(
            "world",
            "[12:00:01 INFO]: Alex joined the game".into(),
            LogSource::Console,
        )
        .await;
        app.ingest_log(
            "world",
            "[12:00:01] [Server thread/INFO]: Alex joined the game".into(),
            LogSource::File,
        )
        .await;
        app.ingest_log(
            "world",
            "[12:00:02 INFO]: Teleported Alex to 0, 66, 0".into(),
            LogSource::File,
        )
        .await;
        app.ingest_log(
            "world",
            "[12:00:03 INFO]: Alex left the game".into(),
            LogSource::File,
        )
        .await;
        app.ingest_log(
            "world",
            "[12:00:04 INFO]: There are 0 of a max of 20 players online:&#x20;".into(),
            LogSource::File,
        )
        .await;
        let logs = app.logs.read().await;
        assert_eq!(logs["world"].len(), 3);
        assert_eq!(logs["world"][0].time, "12:00:01");
        assert!(logs["world"][1].text.contains("Teleported Alex"));
        assert!(logs["world"][2].text.contains("left the game"));
        assert!(app.snapshots.read().await["world"].players.is_empty());
    }
    #[tokio::test]
    async fn restart_of_external_local_server_never_connects_to_rcon() {
        let app = Manager::new(
            std::env::temp_dir().join(format!("void-unowned-{}.json", uuid::Uuid::new_v4())),
            "test".into(),
            false,
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server: Server = serde_json::from_value(serde_json::json!({
            "name":"External local server", "host":"127.0.0.1", "mode":"local",
            "directory":"C:\\Minecraft\\Survival", "rcon_password":"unused-password",
            "rcon_port":listener.local_addr().unwrap().port()
        }))
        .unwrap();
        let error =
            tokio::time::timeout(std::time::Duration::from_millis(500), app.restart(&server))
                .await
                .unwrap()
                .unwrap_err();
        assert!(error.contains("not started by this void.dash session"));
        assert!(error.contains("Local process control does not require RCON"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
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
        // A stale optional RCON password must not affect restart of our own process.
        let mut with_stale_rcon = server.clone();
        with_stale_rcon.rcon_password = "unused-password".into();
        with_stale_rcon.rcon_port = 9;
        app.restart(&with_stale_rcon).await.unwrap();
        app.command(&server, "say restarted without RCON")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(app.logs.read().await[&server.id]
            .iter()
            .any(|l| l.text == "Executed: say restarted without RCON"));
        app.stop(&server).await.unwrap();
        assert!(!app.processes.lock().await.contains_key(&server.id));
        app.remove(&server.id).await.unwrap();
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
