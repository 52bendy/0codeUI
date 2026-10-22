package com.intimeui.client.core.api;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotEquals;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import com.google.gson.JsonObject;
import com.intimeui.client.core.intent.IntentRequest;
import com.intimeui.client.core.intent.IntentResult;
import com.intimeui.client.core.replay.NonceGenerator;
import com.intimeui.client.core.replay.ReplayHeaders;
import com.intimeui.client.core.replay.ReplaySigner;
import com.intimeui.client.core.replay.ServerClock;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import org.junit.After;
import org.junit.Before;
import org.junit.Test;

import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;

public class ApiClientTest {
    private static final String SECRET = "0123456789abcdef0123456789abcdef";
    private static final String USER_ID = "usr_9527";

    private HttpServer server;
    private final List<Captured> captured = new ArrayList<>();
    private final AtomicInteger calls = new AtomicInteger();

    @Before
    public void startServer() throws IOException {
        server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext(ApiClient.PATH_INTENT_EXECUTE, this::handleExecute);
        server.createContext(ApiClient.PATH_INTENT_CONFIRM, this::handleConfirm);
        server.start();
    }

    @After
    public void stopServer() {
        server.stop(0);
    }

    @Test
    public void signsExactBodyAndRetriesReplayWithFreshNonce() throws Exception {
        ApiClient client = client();
        JsonObject data = new JsonObject();
        data.addProperty("order_id", "ORD_20261014_001");
        IntentRequest request = IntentRequest.uiInteraction(
                USER_ID, null, "ORDER_DETAIL_PAGE", "ORD_20261014_001",
                "btn_save_address", "update_order_address", data);

        IntentResult result = client.executeIntent(request);

        assertEquals("EXECUTE", result.actionType());
        assertEquals(2, calls.get());
        assertEquals(2, captured.size());
        Captured first = captured.get(0);
        Captured second = captured.get(1);
        assertNotEquals(first.nonce, second.nonce);
        assertEquals(first.body, second.body);

        ReplayHeaders expected = new ReplaySigner().sign(
                SECRET, "POST", ApiClient.PATH_INTENT_EXECUTE,
                Long.parseLong(first.timestamp), first.nonce, USER_ID,
                first.body.getBytes(StandardCharsets.UTF_8));
        assertEquals(expected.dynamicToken(), first.dynamicToken);
        assertEquals(expected.signature(), first.signature);
    }

    @Test
    public void confirmationUsesBearerWithoutReplayHeaders() throws Exception {
        ApiClient client = client();
        client.confirm("CNF_1", "cct_token", "sess_1");
        Captured request = captured.get(0);
        assertEquals("Bearer dev:usr_9527", request.authorization);
        assertNull(request.timestamp);
        assertNull(request.signature);
    }

    private ApiClient client() {
        ServerClock clock = new ServerClock();
        HttpTransport transport = new HttpUrlConnectionTransport(
                "http://127.0.0.1:" + server.getAddress().getPort());
        return new ApiClient(
                "dev:usr_9527", USER_ID, () -> SECRET, clock, transport,
                new ReplaySigner(), new NonceGenerator());
    }

    private void handleExecute(HttpExchange exchange) throws IOException {
        Captured request = Captured.from(exchange);
        captured.add(request);
        if (calls.incrementAndGet() == 1) {
            respond(exchange, 409,
                    "{\"code\":40900,\"msg\":\"replay detected\",\"data\":null}");
            return;
        }
        respond(exchange, 200,
                "{\"code\":0,\"msg\":\"ok\",\"data\":{"
                        + "\"session_id\":\"sess_1\",\"action_type\":\"EXECUTE\","
                        + "\"intent_summary\":\"查询订单\",\"proposed_changes\":{},"
                        + "\"context\":{},\"execution_results\":[]}}");
    }

    private void handleConfirm(HttpExchange exchange) throws IOException {
        captured.add(Captured.from(exchange));
        respond(exchange, 200,
                "{\"code\":0,\"msg\":\"ok\",\"data\":{"
                        + "\"confirmation_id\":\"CNF_1\",\"status\":\"EXECUTED\","
                        + "\"results\":[]}}");
    }

    private static void respond(HttpExchange exchange, int status, String body)
            throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().add("Content-Type", "application/json");
        exchange.sendResponseHeaders(status, bytes.length);
        try (OutputStream output = exchange.getResponseBody()) {
            output.write(bytes);
        }
    }

    private static final class Captured {
        private final String body;
        private final String timestamp;
        private final String nonce;
        private final String dynamicToken;
        private final String signature;
        private final String authorization;

        private Captured(
                String body,
                String timestamp,
                String nonce,
                String dynamicToken,
                String signature,
                String authorization) {
            this.body = body;
            this.timestamp = timestamp;
            this.nonce = nonce;
            this.dynamicToken = dynamicToken;
            this.signature = signature;
            this.authorization = authorization;
        }

        private static Captured from(HttpExchange exchange) throws IOException {
            String body = new String(
                    exchange.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
            return new Captured(
                    body,
                    exchange.getRequestHeaders().getFirst("x-timestamp"),
                    exchange.getRequestHeaders().getFirst("x-nonce"),
                    exchange.getRequestHeaders().getFirst("x-dynamic-token"),
                    exchange.getRequestHeaders().getFirst("x-signature"),
                    exchange.getRequestHeaders().getFirst("Authorization"));
        }
    }
}
