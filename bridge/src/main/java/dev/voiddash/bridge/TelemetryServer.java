package dev.voiddash.bridge;

import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.function.Supplier;

/** Read-only endpoint; Minecraft API access occurs on the Bukkit thread, never here. */
final class TelemetryServer implements AutoCloseable {
    private final HttpServer server;
    private final ExecutorService executor;

    TelemetryServer(String bind, int port, String token, Supplier<String> sample) throws IOException {
        if (token.length() < 32) throw new IllegalArgumentException("Telemetry token must be at least 32 characters");
        server = HttpServer.create(new InetSocketAddress(bind, port), 16);
        executor = Executors.newFixedThreadPool(2, task -> {
            Thread thread = new Thread(task, "void-dash-telemetry");
            thread.setDaemon(true);
            return thread;
        });
        server.setExecutor(executor);
        byte[] expected = ("Bearer " + token).getBytes(StandardCharsets.UTF_8);
        server.createContext("/", exchange -> {
            try {
                int status;
                String body;
                String authorization = exchange.getRequestHeaders().getFirst("Authorization");
                if (!exchange.getRequestURI().getPath().equals("/v1/telemetry")) {
                    status = 404; body = "{\"error\":\"Not found\"}";
                } else if (!exchange.getRequestMethod().equals("GET")) {
                    status = 405; body = "{\"error\":\"Read-only endpoint\"}";
                } else if (authorization == null || !MessageDigest.isEqual(expected, authorization.getBytes(StandardCharsets.UTF_8))) {
                    status = 401; body = "{\"error\":\"Authentication required\"}";
                } else {
                    body = sample.get();
                    status = body == null ? 503 : 200;
                    if (body == null) body = "{\"error\":\"Waiting for a server tick\"}";
                }
                byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
                exchange.getResponseHeaders().set("Content-Type", "application/json; charset=utf-8");
                exchange.getResponseHeaders().set("Cache-Control", "no-store");
                exchange.getResponseHeaders().set("X-Content-Type-Options", "nosniff");
                exchange.sendResponseHeaders(status, bytes.length);
                exchange.getResponseBody().write(bytes);
            } finally {
                exchange.close();
            }
        });
    }
    void start() { server.start(); }
    int port() { return server.getAddress().getPort(); }
    @Override public void close() { server.stop(0); executor.shutdownNow(); }

    static String quote(String value) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            if (c == '\\' || c == '"') out.append('\\').append(c);
            else if (c < 32) out.append(String.format("\\u%04x", (int)c));
            else out.append(c);
        }
        return out.append('"').toString();
    }
}
