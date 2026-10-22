package com.intimeui.client.core.bridge;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Dispatches inbound bridge messages and always returns a JSON envelope so the
 * JavaScript caller gets a deterministic acknowledgement.
 */
public final class BridgeRouter {
    private final Map<String, BridgeHandler> handlers = new ConcurrentHashMap<>();

    public BridgeRouter register(String type, BridgeHandler handler) {
        if (type == null || handler == null) {
            throw new IllegalArgumentException("type and handler are required");
        }
        handlers.put(type, handler);
        return this;
    }

    public String handle(String raw) {
        String id = extractId(raw);
        try {
            BridgeMessage message = BridgeMessage.parse(raw);
            BridgeHandler handler = handlers.get(message.type());
            if (handler == null) {
                throw new BridgeException("UNKNOWN_TYPE", "unsupported message type");
            }
            JsonObject payload = handler.handle(message.payload());
            return response(message.id(), BridgeProtocol.TYPE_ACK,
                    payload == null ? new JsonObject() : payload);
        } catch (BridgeException error) {
            return errorResponse(id, error.code(), error.getMessage());
        } catch (Exception error) {
            return errorResponse(id, "HANDLER_ERROR", "request could not be processed");
        }
    }

    private static String response(String id, String type, JsonObject payload) {
        JsonObject envelope = new JsonObject();
        envelope.addProperty("v", BridgeProtocol.VERSION);
        envelope.addProperty("id", id);
        envelope.addProperty("type", type);
        envelope.add("payload", payload);
        return Json.write(envelope);
    }

    private static String errorResponse(String id, String code, String message) {
        JsonObject payload = new JsonObject();
        payload.addProperty("code", code);
        payload.addProperty("message", message == null ? "bridge error" : message);
        return response(id == null ? "unknown" : id, BridgeProtocol.TYPE_ERROR, payload);
    }

    private static String extractId(String raw) {
        try {
            JsonObject root = Json.parseObject(raw);
            String id = Json.optionalString(root, "id");
            return id != null && id.matches("^[A-Za-z0-9._:-]{1,64}$") ? id : null;
        } catch (RuntimeException error) {
            return null;
        }
    }
}
