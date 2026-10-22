The first void.dash release: a standalone Rust Minecraft Java server dashboard with an embedded browser interface.

- Dark amber interface with server tabs and a server icon/MOTD header.
- Live process logs, command input, server-derived command completion, and console filtering/export.
- Host and process resources, player names and skin faces, and start/stop/restart controls.
- Local process management, RCON connections, and a remote-agent mode.
- Configurable plugin website viewers for squaremap, BlueMap, Dynmap, Plan, and custom sites.

### Download and run on Windows

Download **void-dash-0.1.0-windows-x86_64.zip**, extract it, and double-click **void-dash.exe**. Your default browser opens the dashboard. Leave the console window open while using it. Rust and Node.js are not required.

The standalone **void-dash.exe** is also attached if you only want the executable. `SHA256SUMS.txt` contains checksums for both downloads.

The Windows x64 executable was cross-compiled on Linux. Its format and imported DLLs were checked, but it has not been run on Windows. The Linux application passed eight Rust tests and desktop/mobile browser smoke tests against protocol fixtures.

Actual Minecraft servers need to be configured separately. Remote hosts need a void.dash agent for full logs, host telemetry, and startup/restart controls; RCON alone provides commands and stopping. See the repository README for setup.
