package com.intimeui.client.core.json;

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParseException;
import com.google.gson.JsonParser;

/** Small, dependency-light JSON facade used by the core client. */
public final class Json {
    private static final Gson GSON = new Gson();

    private Json() {
    }

    public static String write(Object value) {
        return GSON.toJson(value);
    }

    public static JsonObject parseObject(String raw) {
        if (raw == null) {
            throw new JsonParseException("json body is null");
        }
        JsonElement element = JsonParser.parseString(raw);
        if (!element.isJsonObject()) {
            throw new JsonParseException("json body must be an object");
        }
        return element.getAsJsonObject();
    }

    public static JsonArray parseArray(String raw) {
        if (raw == null) {
            throw new JsonParseException("json body is null");
        }
        JsonElement element = JsonParser.parseString(raw);
        if (!element.isJsonArray()) {
            throw new JsonParseException("json body must be an array");
        }
        return element.getAsJsonArray();
    }

    public static JsonObject object(Object value) {
        JsonElement element = GSON.toJsonTree(value);
        if (!element.isJsonObject()) {
            throw new JsonParseException("value is not an object");
        }
        return element.getAsJsonObject();
    }

    public static String requireString(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || value.isJsonNull() || !value.isJsonPrimitive()) {
            throw new JsonParseException("missing string field: " + field);
        }
        String text = value.getAsString();
        if (text.isEmpty()) {
            throw new JsonParseException("empty string field: " + field);
        }
        return text;
    }

    public static String optionalString(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || value.isJsonNull()) {
            return null;
        }
        return value.getAsString();
    }

    public static long requireLong(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || value.isJsonNull()) {
            throw new JsonParseException("missing number field: " + field);
        }
        return value.getAsLong();
    }

    public static double requireDouble(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || value.isJsonNull()) {
            throw new JsonParseException("missing number field: " + field);
        }
        return value.getAsDouble();
    }

    public static JsonObject requireObject(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || !value.isJsonObject()) {
            throw new JsonParseException("missing object field: " + field);
        }
        return value.getAsJsonObject();
    }

    public static JsonArray optionalArray(JsonObject object, String field) {
        JsonElement value = object.get(field);
        if (value == null || value.isJsonNull() || !value.isJsonArray()) {
            return new JsonArray();
        }
        return value.getAsJsonArray();
    }
}
