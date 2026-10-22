package com.intimeui.client.core.intent;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

/** Parsed result of {@code /intent/confirm} or {@code /intent/reject}. */
public final class ConfirmationDecision {
    private final String confirmationId;
    private final String status;
    private final String decidedAt;
    private final String executedAt;
    private final JsonArray results;
    private final String reason;

    private ConfirmationDecision(
            String confirmationId,
            String status,
            String decidedAt,
            String executedAt,
            JsonArray results,
            String reason) {
        this.confirmationId = confirmationId;
        this.status = status;
        this.decidedAt = decidedAt;
        this.executedAt = executedAt;
        this.results = results;
        this.reason = reason;
    }

    public static ConfirmationDecision fromJson(JsonObject data) {
        return new ConfirmationDecision(
                Json.optionalString(data, "confirmation_id"),
                Json.requireString(data, "status"),
                Json.optionalString(data, "decided_at"),
                Json.optionalString(data, "executed_at"),
                Json.optionalArray(data, "results"),
                Json.optionalString(data, "reason"));
    }

    public JsonObject toReceiptJson() {
        JsonObject object = new JsonObject();
        object.addProperty("confirmation_id", confirmationId);
        object.addProperty("status", status);
        object.addProperty("decided_at", decidedAt);
        object.addProperty("executed_at", executedAt);
        object.add("results", results);
        if (reason != null) {
            object.addProperty("reason", reason);
        }
        return object;
    }

    public String confirmationId() {
        return confirmationId;
    }

    public String status() {
        return status;
    }

    public String decidedAt() {
        return decidedAt;
    }

    public String executedAt() {
        return executedAt;
    }

    public JsonArray results() {
        return results;
    }

    public String reason() {
        return reason;
    }
}
