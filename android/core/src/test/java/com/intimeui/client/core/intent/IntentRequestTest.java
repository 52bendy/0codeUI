package com.intimeui.client.core.intent;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import com.google.gson.JsonObject;
import org.junit.Test;

public class IntentRequestTest {
    @Test
    public void buildsUiInteractionEventTree() {
        JsonObject data = new JsonObject();
        data.addProperty("order_id", "ORD_20261014_001");
        IntentRequest request = IntentRequest.uiInteraction(
                "usr_9527",
                "sess_1",
                "ORDER_DETAIL_PAGE",
                "ORD_20261014_001",
                "btn_save_address",
                "update_order_address",
                data);

        JsonObject body = request.toJson();
        assertEquals("UI_INTERACTION", body.get("input_type").getAsString());
        JsonObject raw = body.getAsJsonObject("raw_input");
        assertEquals("TAP", raw.getAsJsonObject("event").get("type").getAsString());
        assertEquals("update_order_address",
                raw.getAsJsonArray("path").get(1).getAsJsonObject().get("action").getAsString());
        assertEquals("ORDER_DETAIL_PAGE",
                body.getAsJsonObject("current_context").get("active_page").getAsString());
        assertTrue(raw.getAsJsonObject("data").has("order_id"));
    }

    @Test
    public void omitsEmptyOptionalFields() {
        IntentRequest request = new IntentRequest(
                "", "", "NATURAL_LANGUAGE", new JsonObject(), new JsonObject());
        JsonObject body = request.toJson();
        assertTrue(!body.has("user_id"));
        assertTrue(!body.has("session_id"));
        assertTrue(!body.has("current_context"));
    }
}
