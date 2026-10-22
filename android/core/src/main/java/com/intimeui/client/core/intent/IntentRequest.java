package com.intimeui.client.core.intent;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;

/**
 * Request body for {@code POST /api/v1/intent/execute}.
 *
 * <p>Use {@link #uiInteraction} to build the {@code UI_INTERACTION} event tree
 * defined by the #09 contract; the tree carries the page/entity context so the
 * server can resolve the target capability without extra hints.</p>
 */
public final class IntentRequest {
    private final String userId;
    private final String sessionId;
    private final String inputType;
    private final JsonObject rawInput;
    private final JsonObject currentContext;

    public IntentRequest(
            String userId,
            String sessionId,
            String inputType,
            JsonObject rawInput,
            JsonObject currentContext) {
        this.userId = userId;
        this.sessionId = sessionId;
        this.inputType = inputType;
        this.rawInput = rawInput;
        this.currentContext = currentContext == null ? new JsonObject() : currentContext;
    }

    public static IntentRequest uiInteraction(
            String userId,
            String sessionId,
            String pageId,
            String entityId,
            String targetId,
            String action,
            JsonObject data) {
        JsonObject event = new JsonObject();
        event.addProperty("type", "TAP");
        event.addProperty("target_id", targetId);

        JsonArray path = new JsonArray();
        JsonObject pageNode = new JsonObject();
        pageNode.addProperty("id", pageId);
        pageNode.addProperty("type", "page");
        path.add(pageNode);

        JsonObject targetNode = new JsonObject();
        targetNode.addProperty("id", targetId);
        targetNode.addProperty("type", "button");
        targetNode.addProperty("action", action);
        path.add(targetNode);

        JsonObject raw = new JsonObject();
        raw.add("event", event);
        raw.add("path", path);
        raw.add("data", data == null ? new JsonObject() : data);
        raw.addProperty("page", pageId);
        raw.addProperty("entity_id", entityId);

        JsonObject context = new JsonObject();
        context.addProperty("active_page", pageId);
        context.addProperty("entity_id", entityId);
        return new IntentRequest(userId, sessionId, "UI_INTERACTION", raw, context);
    }

    public JsonObject toJson() {
        JsonObject body = new JsonObject();
        if (userId != null && !userId.isEmpty()) {
            body.addProperty("user_id", userId);
        }
        if (sessionId != null && !sessionId.isEmpty()) {
            body.addProperty("session_id", sessionId);
        }
        body.addProperty("input_type", inputType);
        body.add("raw_input", rawInput);
        if (currentContext.size() > 0) {
            body.add("current_context", currentContext);
        }
        return body;
    }

    public String sessionId() {
        return sessionId;
    }

    public String inputType() {
        return inputType;
    }

    public JsonObject rawInput() {
        return rawInput;
    }

    public JsonObject currentContext() {
        return currentContext;
    }
}
