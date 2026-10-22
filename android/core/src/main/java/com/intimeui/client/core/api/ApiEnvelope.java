package com.intimeui.client.core.api;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

/** Parses the project-wide {@code {code, msg, data}} envelope. */
public final class ApiEnvelope {
    private final int code;
    private final String message;
    private final JsonObject data;

    private ApiEnvelope(int code, String message, JsonObject data) {
        this.code = code;
        this.message = message;
        this.data = data;
    }

    public static ApiEnvelope parse(String rawBody) throws ApiException {
        final JsonObject root;
        try {
            root = Json.parseObject(rawBody);
        } catch (RuntimeException error) {
            throw ApiException.invalidResponse("response is not a JSON object");
        }
        JsonElement codeElement = root.get("code");
        if (codeElement == null || !codeElement.isJsonPrimitive()) {
            throw ApiException.invalidResponse("response is missing `code`");
        }
        int code;
        try {
            code = codeElement.getAsInt();
        } catch (RuntimeException error) {
            throw ApiException.invalidResponse("response `code` is not an integer");
        }
        String message = root.has("msg") && !root.get("msg").isJsonNull()
                ? root.get("msg").getAsString()
                : "";
        JsonElement dataElement = root.get("data");
        JsonObject data = dataElement != null && dataElement.isJsonObject()
                ? dataElement.getAsJsonObject()
                : null;
        return new ApiEnvelope(code, message, data);
    }

    public int code() {
        return code;
    }

    public String message() {
        return message;
    }

    public boolean isSuccess() {
        return code == 0;
    }

    public JsonObject data() {
        return data;
    }

    public JsonObject requireData() throws ApiException {
        if (data == null) {
            throw ApiException.invalidResponse("successful response has no data object");
        }
        return data;
    }
}
