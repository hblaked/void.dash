## Dashboard cleanup and real server telemetry

- Removed the top logo, introduction text, sidebar, and bottom-left slogan. The header now contains server tabs and Add Server.
- Added an active-server uptime timer directly below its player count.
- Server resources show actual JVM heap usage against the server's JVM maximum. JDK tools or the optional bridge supply measurements; host RAM and process RSS are never substituted for heap usage.
- Added a separate host panel for system CPU, RAM, disk space, and OS uptime. The server panel keeps its own process CPU and resident memory.
- Added real per-player ping through the optional read-only Paper/Purpur/Spigot bridge. Missing measurements are marked unavailable.
- Removed periodic console list commands and empty-player-list chatter.
- Full console logging now follows logs/latest.log as well as stdout/stderr, including connection/logout and command output, with rotation handling and matching-output deduplication. The dashboard retains the latest 5,000 lines; Minecraft's files retain full history.
- Added a player-relative teleport helper. Commands reach Minecraft unchanged unless the user explicitly chooses the helper.

## Download and upgrade

Download **void-dash-0.2.0-windows-x86_64.zip**, extract it, and launch **void-dash.exe**. Stop servers managed by the old dashboard and close it before replacing the executable. Saved configurations are retained. Update remote void.dash agents to this version too.

For real player ping and exact JVM telemetry, download **void-dash-bridge.jar**, place it in the Minecraft server's **plugins** directory, and restart Minecraft. It supports Bukkit-compatible Paper/Purpur/Spigot 1.21 servers on Java 21. A local profile detects its default loopback configuration from the configured server directory. Remote/custom connections can use the Telemetry URL/token fields: the default endpoint is http://127.0.0.1:8788/v1/telemetry and the generated token is in plugins/void-dash-bridge/config.yml. No RCON is required.

## Relative teleport coordinates

The server console's relative coordinates use the console/world-spawn position. To move each selected player two blocks up from their own current position:

```mcfunction
execute as @a at @s run tp @s ~ ~2 ~
```

The command field offers **Use player positions** for this form; it does not silently change normal Minecraft commands.

## Validation

Sixteen Rust tests, desktop/mobile browser checks, two bridge HTTP/authentication tests, and a real JVM integration check pass. The JVM check verifies the Rust API reports heap usage within an actual 96 MiB JVM maximum independently of host memory. GitHub Actions compiles the bridge against the Spigot API. A live Minecraft server has not been exercised here. The Windows executable was cross-compiled and its imports/archive checked; it has not been run on Windows.
