package com.intimeui.client.core.bridge;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

import java.util.regex.Pattern;

/** Validated inbound JS-to-native message. */
public final class BridgeMessage {
    private static final Pattern ID_PATTERN = Pattern.compile("^[A-Za-z0-9._:-]{1,64}$");
    private static final Pattern TYPE_PATTERN = Pattern.compile("^[A-Z_]{1,32}$");

    private final String id;
    private final String type;
    private final JsonObject payload;

    private BridgeMessage(String id, String type, JsonObject payload) {
        this.id = id;
        this.type = type;
        this.payload = payload;
    }

    public static BridgeMessage parse(String raw) throws BridgeException {
        if (raw == null || raw.trim().isEmpty()) {
            throw new BridgeException("BAD_MESSAGE", "message is empty");
        }
        if (raw.getBytes(java.nio.charset.StandardCharsets.UTF_8).length
                > BridgeProtocol.MAX_MESSAGE_BYTES) {
            throw new BridgeException("MESSAGE_TOO_LARGE", "message exceeds 64 KiB");
        }
        final JsonObject root;
        try {
            root = Json.parseObject(raw);
        } catch (RuntimeException error) {
            throw new BridgeException("BAD_MESSAGE", "message is not valid JSON");
        }
        JsonElement version = root.get("v");
        if (version == null || !version.isJsonPrimitive()
                || !version.getAsJsonPrimitive().isNumber()
                || version.getAsInt() != BridgeProtocol.VERSION) {
            throw new BridgeException("BAD_VERSION", "unsupported bridge version");
        }
        String id = Json.optionalString(root, "id");
        if (id == null || !ID_PATTERN.matcher(id).matches()) {
            throw new BridgeException("BAD_MESSAGE", "missing or invalid message id");
        }
        String type = Json.optionalString(root, "type");
        if (type == null || !TYPE_PATTERN.matcher(type).matches()) {
            throw new BridgeException("BAD_MESSAGE", "missing or invalid message type");
        }
        JsonObject payload = root.has("payload") && root.get("payload").isJsonObject()
                ? root.getAsJsonObject("payload")
                : new JsonObject();
        return new BridgeMessage(id, type, payload);
    }

    public String id() {
        return id;
    }

    public String type() {
        return type;
    }

    public JsonObject payload() {
        return payload;
    }
}
