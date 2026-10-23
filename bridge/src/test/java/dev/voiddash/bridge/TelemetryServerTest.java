package dev.voiddash.bridge;

import static org.junit.jupiter.api.Assertions.*;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.concurrent.atomic.AtomicReference;
import org.junit.jupiter.api.Test;

final class TelemetryServerTest {
    private static final String TOKEN = "test-only-token-with-at-least-32-characters";
    @Test void authenticatesReadOnlyEndpointAndReportsMissingSamples() throws Exception {
        AtomicReference<String> sample = new AtomicReference<>();
        try (TelemetryServer server = new TelemetryServer("127.0.0.1", 0, TOKEN, sample::get)) {
            server.start();
            URI uri = URI.create("http://127.0.0.1:" + server.port() + "/v1/telemetry");
            HttpClient client = HttpClient.newHttpClient();
            assertEquals(401, client.send(HttpRequest.newBuilder(uri).GET().build(), HttpResponse.BodyHandlers.ofString()).statusCode());
            assertEquals(401, client.send(HttpRequest.newBuilder(uri).header("Authorization", "Bearer wrong").GET().build(), HttpResponse.BodyHandlers.ofString()).statusCode());
            assertEquals(503, client.send(HttpRequest.newBuilder(uri).header("Authorization", "Bearer " + TOKEN).GET().build(), HttpResponse.BodyHandlers.ofString()).statusCode());
            sample.set("{\"heap_used\":1048576,\"heap_max\":67108864,\"uptime_seconds\":10,\"players\":[{\"name\":\"Alex\",\"ping_ms\":42}]}");
            var response = client.send(HttpRequest.newBuilder(uri).header("Authorization", "Bearer " + TOKEN).GET().build(), HttpResponse.BodyHandlers.ofString());
            assertEquals(200, response.statusCode());
            assertEquals(sample.get(), response.body());
            assertEquals(405, client.send(HttpRequest.newBuilder(uri).header("Authorization", "Bearer " + TOKEN).POST(HttpRequest.BodyPublishers.noBody()).build(), HttpResponse.BodyHandlers.ofString()).statusCode());
            assertEquals(404, client.send(HttpRequest.newBuilder(URI.create(uri + "/wrong")).header("Authorization", "Bearer " + TOKEN).GET().build(), HttpResponse.BodyHandlers.ofString()).statusCode());
        }
    }
    @Test void escapesNamesAndRejectsWeakTokens() {
        assertEquals("\"Alex\\\"\\\\\\u000a\"", TelemetryServer.quote("Alex\"\\\n"));
        assertThrows(IllegalArgumentException.class, () -> new TelemetryServer("127.0.0.1", 0, "short", () -> "{}"));
    }
}
