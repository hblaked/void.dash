# void.dash

A quiet control room for your Minecraft worlds.

**A single Rust executable with an embedded browser interface.** Run it on your computer and it opens your browser automatically. There is no Node runtime, frontend build, database, or hosted service to deploy. This version uses the system browser, rather than a native desktop window.

## Run

**Windows download:** Get the [Windows ZIP from GitHub Releases](https://github.com/hblaked/void.dash/releases/latest), extract it, and double-click **void-dash.exe**. The executable is also available as a separate release asset. This Windows x64 build was cross-compiled and has not yet been run on Windows.

To build from source:

Install a current stable [Rust toolchain](https://rustup.rs/) and your platform's C/C++ linker. Then:

```sh
cargo run --locked --release
```

Or build once and run the standalone binary:

```sh
cargo build --locked --release
./target/release/void-dash
# Windows: .\target\release\void-dash.exe
```

The dashboard listens on `127.0.0.1:8787`. Use `--port 9000` to choose another port, `--no-open` for headless development, or `--config /path/to/config.json` to choose a configuration file. An empty configuration opens a clearly marked interactive preview with sample servers; connect a real server with **+** or **Connect your server**.

Configuration defaults to the operating system's configuration directory under `void.dash/config.json`. Server definitions and viewer URLs persist there. Passwords and agent tokens stay in that file and are redacted from API responses. Unix config writes use owner-only permissions. Protect the configuration and its parent directory on shared computers, including Windows ACLs.

Java and a Minecraft server are needed only on machines running a Minecraft process, not to launch the dashboard. Install the Java version required by your chosen Minecraft release. Prepare the server directory, server jar, and `eula.txt` yourself. void.dash does not download a Minecraft distribution or accept Mojang's EULA on your behalf.

## Connect your servers

| Connection | Status, icon, MOTD | Commands | Full server logs | Host resources | Start / restart |
| --- | --- | --- | --- | --- | --- |
| Local process launched here | Yes | stdin; optional RCON | stdout + stderr + latest.log | Yes | Yes |
| RCON network server | Yes | RCON | Command responses only | Requires agent | Requires agent |
| void.dash agent on server host | Yes | Remote stdin / RCON | Remote console + latest.log | Yes | Yes |

**Local process:** Add a server name, game address, existing directory, executable, and argument array. For example:

```json
["-Xms1G", "-Xmx4G", "-jar", "server.jar", "nogui"]
```

Arguments are passed directly to the executable. Shell substitutions and shell syntax are not evaluated. On Windows, use a full path to `java.exe` if Java is not on `PATH`. Starting checks for an existing Minecraft listener to avoid launching over a running server. Stop sends Minecraft's `stop` command and waits up to 30 seconds for the world to save. It never silently force-kills a server that is still saving.

Stop managed servers before exiting void.dash. Closing the application does not deliberately stop a world; the process is detached on application exit, and console pipes cannot be reattached after a dashboard restart. For unattended operation, run the agent under your operating system's service manager and keep its process alive. A process started elsewhere can use RCON, but must be stopped before void.dash can launch it as a managed process.

**Already started Minecraft with a `.bat` file?** The dashboard can display its status, but Restart and live console logs require a process started by the current void.dash session. Type `stop` in the existing Minecraft console, wait for saving/shutdown, then use **Start** in void.dash. Choose **Local process**, use `127.0.0.1` for the hostname when both applications run on the same machine, and set the server directory to the folder containing the jar and `server.properties` (for example, `C:\Minecraft\Survival`). Copy the Java executable and arguments from your working batch file. RCON is not required for this workflow. The dashboard labels externally started servers and disables Restart rather than silently attempting RCON.

**Network RCON:** Enable RCON in `server.properties`, restart the Minecraft server, then enter its host, RCON port, and password in the dashboard:

```properties
enable-rcon=true
rcon.port=25575
rcon.password=YOUR_UNIQUE_PASSWORD
enable-status=true
```

RCON is unencrypted. Restrict it to your trusted LAN, a VPN, or a secure tunnel. It cannot start a stopped process, expose OS metrics, or stream all logs; the UI explains those limits and disables unsupported lifecycle actions.

**Remote agent:** Run the same executable on the Minecraft host. First create its configuration with a local server entry. You can use the dashboard on that machine to create the entry, or create a JSON file using this example (adjust all paths):

```json
{
  "servers": [
    {
      "id": "survival",
      "name": "Survival",
      "host": "127.0.0.1",
      "port": 25565,
      "mode": "local",
      "directory": "/srv/minecraft/survival",
      "executable": "java",
      "args": ["-Xms1G", "-Xmx4G", "-jar", "server.jar", "nogui"],
      "viewers": []
    }
  ]
}
```

Set `VOID_DASH_TOKEN` through your shell or service manager to a unique random token of at least 32 characters, and launch:

```sh
void-dash --agent --bind 0.0.0.0 --config /path/to/config.json
```

Add an **agent** server in your local dashboard with the remote agent URL (for example, `http://192.168.1.20:8787`), that token, and remote server ID `survival`. The game hostname is the Minecraft host reachable from your computer. Agents never auto-start configured servers; use Start from the dashboard. Agents cannot chain to other agents.

An agent token grants process-management access to that host. Restrict the listening interface/firewall to trusted machines. Use a VPN or an HTTPS reverse proxy across untrusted networks; native agent HTTP does not encrypt the token or console traffic. TLS certificates are verified for HTTPS agent connections, and redirects are not followed. The local dashboard always binds loopback and rejects cross-origin API calls and unrecognized hosts.

## What the dashboard shows

- **Server tabs:** switch between all configured servers, with status and player count.
- **Server identity:** real icon and MOTD from Minecraft Java's server-list status protocol. Missing icons use a built-in block icon. Formatting codes are reduced to readable text; the server's message and line breaks are retained.
- **Console:** the latest 5,000 lines from stdout/stderr and `logs/latest.log`, including file-only connection/logout messages and command output. The follower handles appends, truncation, and rotation, and deduplicates matching file/console output. Automatic stdin `list` polling is removed and empty-player-list reports are omitted. Other command results remain visible. Local profiles can view the log file even when the server was launched elsewhere; commands/lifecycle still need a managed process or RCON. Minecraft's `logs/` is the durable full history; export downloads the current dashboard buffer.
- **Completion:** select **Discover commands** once the server starts. RCON reads up to 20 help pages; local stdin reads the current help output. Completion lists server-provided command names, including plugin commands that appear in help. It does not invent argument suggestions or inspect a Brigadier command tree. Use Tab to complete, arrows to select suggestions or browse command history, and Enter to send.
- **Server resources:** measured JVM heap used versus its real maximum, server-process CPU (100% = one core), process RSS separately, and a rolling heap graph. The heap maximum comes from the JVM/bridge or the configured `-Xmx`, including Java `@argument` files. If heap measurement is unavailable, the UI says so; it never substitutes host RAM or RSS for JVM heap. Uptime appears below the active server's player count and ticks every second. Local measurements use JDK `jstat`/`jcmd` tools when installed, or the bridge below.
- **Host computer:** a separate panel shows system CPU/core count, total host RAM, mounted disk capacity/free space, and OS uptime. Remote host metrics require a current void.dash agent.
- **Players:** names, skin faces, and real per-player ping from the optional bridge. Without the bridge, ping is marked unavailable because the Java status protocol/RCON do not expose it. Status samples, join/leave logs, and quiet RCON list requests supply names; incomplete lists are labeled. UUIDs from status/bridge are reused for faces from `mc-heads.net`, with a built-in fallback if unavailable.
- **Plugin viewers:** add squaremap, BlueMap, Dynmap, Plan, or a custom HTTP(S) site. View inside a sandboxed panel or open in a new browser tab. Install/configure the plugin separately; its site must be reachable from your browser. Plugins with embedding restrictions may need the external-browser option. Preset ports are suggestions and can be changed.

Minecraft **Java Edition** is supported. Bedrock protocol, automatic LAN scanning, SSH process control, TPS collection, installation of server software/plugins, and recovery of console streams from externally launched processes are outside this version.

## Player ping and exact JVM memory (Paper/Purpur/Spigot)

1. Stop Minecraft and download **void-dash-bridge.jar** from the same GitHub release as the app.
2. Put it in the Minecraft server's `plugins` directory and start the server again. Java 21 and a Bukkit-compatible 1.21 server are required.
3. A **Local process** profile automatically reads the bridge's generated default configuration from `plugins/void-dash-bridge/config.yml` within its configured server directory. This also works for a current void.dash agent's local server profile. No RCON is required.
4. For a custom/remote connection, use Configuration → Optional server telemetry. The default URL is `http://127.0.0.1:8788/v1/telemetry`; enter the generated `token` from the plugin configuration. Configure remote-agent telemetry on the agent's server profile, and update the agent to 0.2.0 too.

The bridge is read-only. It samples Bukkit's real player ping and Java's heap MemoryMXBean/RuntimeMXBean on the main server thread once per second. HTTP only reads the last sample and requires the generated bearer token. The endpoint binds loopback by default. For another LAN computer, configure an appropriate bind address/port and firewall; use a VPN or HTTPS proxy across untrusted networks. Tokens stay in local configuration and are redacted from dashboard API responses. Do not commit server plugin configuration or tokens.

A local Java 21 JDK can supply exact heap usage through `jstat` and `jcmd` even without the plugin. Set the Java executable to the JDK's `java.exe`, or make its tools available through `JAVA_HOME`/`PATH`. Some JVMs or `-XX:-UsePerfData` disable these facilities; use the bridge in that case. Process resident memory includes native memory and is intentionally separate from heap allocation.

## Relative coordinates in the console

Minecraft executes console commands at the console's world position (normally spawn), not at each targeted player's position. `tp @a ~ ~2 ~` is sent exactly as typed and follows that server-console behavior. To move every player two blocks up from their own location:

```mcfunction
execute as @a at @s run tp @s ~ ~2 ~
```

The console detects relevant relative-teleport commands and offers **Use player positions** to insert this form explicitly. It never silently rewrites commands.

## Development & verification

The server embeds `web/index.html`, `web/style.css`, and `web/app.js` at compile time. Rebuild the Rust binary after editing them. A recent Chromium is useful for optional browser checks. Application users do not need Node or Chromium specifically; their system browser is sufficient.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Tests cover Minecraft status framing, fragmented RCON responses, help parsing, credential redaction, API session/origin checks and persistence, real subprocess console/lifecycle, log rotation/deduplication, quiet player updates, JVM memory parsing, and bridge discovery. They use temporary directories and loopback protocol fixtures, so they require neither a Minecraft installation nor live credentials. Linux has been exercised in the cloud environment. macOS and Windows builds require validation on those systems.

An optional Playwright smoke test exercises the UI against the actual Rust API and a Python Minecraft-wire fixture, including command completion, player display, persisted viewers, restart/stop, and a phone viewport. It requires Node, Python 3, Playwright, Chromium, and the debug binary:

```sh
cargo build --locked
npm install --prefix /tmp/void-browser playwright
PLAYWRIGHT_MODULE=/tmp/void-browser/node_modules/playwright \
  CHROMIUM_PATH=/usr/bin/chromium node tests/browser-smoke.cjs
```

Set `PYTHON` or `VOID_DASH_BINARY` to override those executable paths. The test writes screenshots under the ignored `data/screenshots/` directory, manages its own application instance, and cleans up its temporary server configuration.

The optional real-JVM integration check launches a Java fixture with a known 96 MiB heap and verifies the Rust API reports that heap independently of host RAM:

```sh
python3 tests/jvm-smoke.py
# Set VOID_JDK_BIN=/path/to/jdk/bin when the JDK is not on PATH.
```

The optional bridge is built/tested with Java 21 and Maven (`mvn --file bridge/pom.xml verify`). GitHub Actions compiles against the Spigot API, runs its HTTP/authentication tests, and attaches the jar to tagged releases. No Minecraft EULA or server installation is needed for these checks.

In the onboarding cloud machine, Rust is installed under `/workspace/.cargo` and `/workspace/.rustup`:

```sh
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export PATH="$CARGO_HOME/bin:$PATH"
cargo run --locked -- --no-open --config /workspace/void.dash/data/config.json
```

Use the existing checkout for cloud tasks; each task already has its own isolated environment. No Git worktree is needed.
