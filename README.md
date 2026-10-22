# void.dash

A quiet control room for your Minecraft worlds. Dark surfaces, warm amber (`#F5B427`), a live console, and the information you need without getting in the way.

**A single Rust executable with an embedded browser interface.** Run it on your computer and it opens your browser automatically. There is no Node runtime, frontend build, database, or hosted service to deploy. This version uses the system browser, rather than a native desktop window.

## Run

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
| Local process launched here | Yes | stdin; optional RCON | stdout + stderr | Yes | Yes |
| RCON network server | Yes | RCON | Command responses only | Requires agent | Requires agent |
| void.dash agent on server host | Yes | Remote stdin / RCON | Remote stdout + stderr | Yes | Yes |

**Local process:** Add a server name, game address, existing directory, executable, and argument array. For example:

```json
["-Xms1G", "-Xmx4G", "-jar", "server.jar", "nogui"]
```

Arguments are passed directly to the executable. Shell substitutions and shell syntax are not evaluated. On Windows, use a full path to `java.exe` if Java is not on `PATH`. Starting checks for an existing Minecraft listener to avoid launching over a running server. Stop sends Minecraft's `stop` command and waits up to 30 seconds for the world to save. It never silently force-kills a server that is still saving.

Stop managed servers before exiting void.dash. Closing the application does not deliberately stop a world; the process is detached on application exit, and console pipes cannot be reattached after a dashboard restart. For unattended operation, run the agent under your operating system's service manager and keep its process alive. A process started elsewhere can use RCON, but must be stopped before void.dash can launch it as a managed process.

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
- **Console:** bounded 1,000-line buffer per managed process, stdout/stderr, command input, filter, pause, local clear, and log download. Logs are held in memory; Minecraft's own `logs/` remains the durable record.
- **Completion:** select **Discover commands** once the server starts. RCON reads up to 20 help pages; local stdin reads the current help output. Completion lists server-provided command names, including plugin commands that appear in help. It does not invent argument suggestions or inspect a Brigadier command tree. Use Tab to complete, arrows to select suggestions or browse command history, and Enter to send.
- **Resources:** host CPU and core count, host memory, process RSS, disks, process uptime, and a rolling memory graph. RSS is not JVM heap allocation, and host disk totals aggregate mounted disks. Remote telemetry requires an agent.
- **Players:** names and skin faces. Status responses can contain only a sample; the UI labels incomplete lists. RCON `list` (or managed-process `list`) refreshes the full list every roughly 15 seconds. Sampled UUIDs are reused; otherwise faces are requested by name from `mc-heads.net`, with a built-in pixel face fallback if unavailable. Servers may disable or customize help, list, or status responses.
- **Plugin viewers:** add squaremap, BlueMap, Dynmap, Plan, or a custom HTTP(S) site. View inside a sandboxed panel or open in a new browser tab. Install/configure the plugin separately; its site must be reachable from your browser. Plugins with embedding restrictions may need the external-browser option. Preset ports are suggestions and can be changed.

Minecraft **Java Edition** is supported. Bedrock protocol, automatic LAN scanning, SSH process control, JVM heap/TPS collection, installation of server software/plugins, and recovery of console streams from externally launched processes are outside this version.

## Development & verification

The server embeds `web/index.html`, `web/style.css`, and `web/app.js` at compile time. Rebuild the Rust binary after editing them. A recent Chromium is useful for optional browser checks. Application users do not need Node or Chromium specifically; their system browser is sufficient.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Tests cover Minecraft status framing, fragmented RCON responses, help parsing, credential redaction, API session/origin checks and persistence, and a real subprocess's console and lifecycle. They use temporary directories and loopback protocol fixtures, so they require neither a Minecraft installation nor live credentials. Linux has been exercised in the cloud environment. macOS and Windows builds require validation on those systems.

An optional Playwright smoke test exercises the UI against the actual Rust API and a Python Minecraft-wire fixture, including command completion, player display, persisted viewers, restart/stop, and a phone viewport. It requires Node, Python 3, Playwright, Chromium, and the debug binary:

```sh
cargo build --locked
npm install --prefix /tmp/void-browser playwright
PLAYWRIGHT_MODULE=/tmp/void-browser/node_modules/playwright \
  CHROMIUM_PATH=/usr/bin/chromium node tests/browser-smoke.cjs
```

Set `PYTHON` or `VOID_DASH_BINARY` to override those executable paths. The test writes screenshots under the ignored `data/screenshots/` directory, manages its own application instance, and cleans up its temporary server configuration.

In the onboarding cloud machine, Rust is installed under `/workspace/.cargo` and `/workspace/.rustup`:

```sh
export CARGO_HOME=/workspace/.cargo
export RUSTUP_HOME=/workspace/.rustup
export PATH="$CARGO_HOME/bin:$PATH"
cargo run --locked -- --no-open --config /workspace/void.dash/data/config.json
```

Use the existing checkout for cloud tasks; each task already has its own isolated environment. No Git worktree is needed.
