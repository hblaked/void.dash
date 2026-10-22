Fixes misleading RCON errors when restarting local Minecraft servers started from a separate batch file or terminal.

- Local Restart now operates only on the process owned by the current void.dash session. It never silently falls back to RCON.
- An externally started server is labeled in the dashboard, with steps for taking control. Restart is disabled until the server is launched here.
- Stop is disabled for an externally started local server unless RCON is configured.
- RCON connection failures identify the RCON host and port and explain which settings to check.
- A locally managed server can restart without RCON, even if an old optional RCON password remains configured.

### Download and run on Windows

Download **void-dash-0.1.1-windows-x86_64.zip**, extract it, and double-click **void-dash.exe**. Your default browser opens the dashboard. Leave the console window open while using it. Rust and Node.js are not required. Stop any worlds managed by void.dash and close the old application before replacing its executable. Existing saved configuration is retained.

If Minecraft was started from a `.bat` file, type `stop` in that Minecraft console and wait for saving/shutdown. In void.dash choose **Local process**, use `127.0.0.1` and the directory containing the server jar and `server.properties`, and copy the Java executable/arguments from your working batch file. Use **Start** here; Restart and console controls then work without RCON. An already-open Minecraft console cannot be attached to the dashboard.

The standalone **void-dash.exe** is also attached if you only want the executable. `SHA256SUMS.txt` contains checksums for both downloads.

The Windows x64 executable was cross-compiled on Linux. Its format and imported DLLs were checked, but it has not been run on Windows. Ten Rust tests and desktop/mobile browser smoke tests pass, including a real externally launched process, the ownership error, taking control without RCON, and graceful managed restart.

Actual Minecraft servers need to be configured separately. Remote hosts need a void.dash agent for full logs, host telemetry, and startup/restart controls; RCON alone provides commands and stopping. See the repository README for setup.
