package dev.voiddash.bridge;

import java.lang.management.ManagementFactory;
import java.lang.management.MemoryUsage;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicReference;
import org.bukkit.entity.Player;
import org.bukkit.plugin.java.JavaPlugin;

public final class VoidDashBridge extends JavaPlugin {
    private TelemetryServer endpoint;
    @Override public void onEnable() {
        saveDefaultConfig();
        String token = getConfig().getString("token", "");
        if (token.isBlank()) {
            token = UUID.randomUUID().toString().replace("-", "") + UUID.randomUUID().toString().replace("-", "");
            getConfig().set("token", token);
            saveConfig();
        }
        AtomicReference<String> sample = new AtomicReference<>();
        try {
            endpoint = new TelemetryServer(getConfig().getString("bind", "127.0.0.1"), getConfig().getInt("port", 8788), token, sample::get);
            endpoint.start();
            getServer().getScheduler().runTaskTimer(this, () -> sample.set(snapshot()), 1L, 20L);
            getLogger().info("Read-only telemetry is listening on port " + endpoint.port() + ". Its token is in this plugin's config.yml.");
        } catch (Exception error) {
            getLogger().severe("Cannot start telemetry: " + error.getMessage());
            getServer().getPluginManager().disablePlugin(this);
        }
    }
    private String snapshot() {
        MemoryUsage heap = ManagementFactory.getMemoryMXBean().getHeapMemoryUsage();
        long max = heap.getMax() > 0 ? heap.getMax() : Runtime.getRuntime().maxMemory();
        StringBuilder json = new StringBuilder("{\"heap_used\":").append(heap.getUsed())
            .append(",\"heap_max\":").append(max)
            .append(",\"uptime_seconds\":").append(ManagementFactory.getRuntimeMXBean().getUptime() / 1000)
            .append(",\"sampled_at_ms\":").append(System.currentTimeMillis())
            .append(",\"players\":[");
        boolean first = true;
        for (Player player : getServer().getOnlinePlayers()) {
            if (!first) json.append(',');
            first = false;
            json.append("{\"name\":").append(TelemetryServer.quote(player.getName()))
                .append(",\"uuid\":").append(TelemetryServer.quote(player.getUniqueId().toString()))
                .append(",\"ping_ms\":").append(player.getPing() < 0 ? "null" : Integer.toString(player.getPing())).append('}');
        }
        return json.append("]}").toString();
    }
    @Override public void onDisable() {
        if (endpoint != null) endpoint.close();
    }
}
