package com.intimeui.client.core.bridge;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;
import org.junit.Test;

public class BridgeRouterTest {
    @Test
    public void dispatchesToRegisteredHandler() {
        BridgeRouter router = new BridgeRouter()
                .register(BridgeProtocol.TYPE_RENDER_READY, payload -> {
                    JsonObject reply = new JsonObject();
                    reply.addProperty("page", payload.get("page").getAsString());
                    return reply;
                });

        String response = router.handle(
                "{\"v\":1,\"id\":\"m_1\",\"type\":\"RENDER_READY\","
                        + "\"payload\":{\"page\":\"ORDER_DETAIL_PAGE\"}}");
        JsonObject root = Json.parseObject(response);
        assertEquals("ACK", root.get("type").getAsString());
        assertEquals("ORDER_DETAIL_PAGE",
                root.getAsJsonObject("payload").get("page").getAsString());
    }

    @Test
    public void returnsStructuredErrors() {
        BridgeRouter router = new BridgeRouter();
        JsonObject unknown = Json.parseObject(router.handle(
                "{\"v\":1,\"id\":\"m_2\",\"type\":\"UNKNOWN\",\"payload\":{}}"));
        assertEquals("ERROR", unknown.get("type").getAsString());
        assertEquals("UNKNOWN_TYPE",
                unknown.getAsJsonObject("payload").get("code").getAsString());

        JsonObject malformed = Json.parseObject(router.handle("not-json"));
        assertEquals("BAD_MESSAGE",
                malformed.getAsJsonObject("payload").get("code").getAsString());
    }

    @Test
    public void rejectsOversizedMessages() {
        StringBuilder huge = new StringBuilder(
                "{\"v\":1,\"id\":\"m_3\",\"type\":\"RENDER_READY\",\"payload\":{\"x\":\"");
        while (huge.length() < BridgeProtocol.MAX_MESSAGE_BYTES + 10) {
            huge.append('a');
        }
        huge.append("\"}}");
        JsonObject response = Json.parseObject(new BridgeRouter().handle(huge.toString()));
        assertEquals("MESSAGE_TOO_LARGE",
                response.getAsJsonObject("payload").get("code").getAsString());
    }

    @Test
    public void handlerFailureDoesNotLeakDetails() {
        BridgeRouter router = new BridgeRouter().register(
                BridgeProtocol.TYPE_INTENT_TRIGGER,
                payload -> {
                    throw new IllegalStateException("secret-token-value");
                });
        String response = router.handle(
                "{\"v\":1,\"id\":\"m_4\",\"type\":\"INTENT_TRIGGER\",\"payload\":{}}");
        assertTrue(response.contains("HANDLER_ERROR"));
        assertTrue(!response.contains("secret-token-value"));
    }
}
