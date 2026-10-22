package com.intimeui.client.core.intent;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

/**
 * Confirmation card returned by {@code /intent/execute} for WRITE/DELETE
 * capabilities. The token is shown exactly once and must never be logged.
 */
public final class ConfirmationCard {
    private final String confirmationId;
    private final String confirmationToken;
    private final String riskLevel;
    private final String intentSummary;
    private final JsonObject proposedChanges;
    private final String confirmationPrompt;
    private final String expiresAt;
    private final long expiresInSecs;

    private ConfirmationCard(
            String confirmationId,
            String confirmationToken,
            String riskLevel,
            String intentSummary,
            JsonObject proposedChanges,
            String confirmationPrompt,
            String expiresAt,
            long expiresInSecs) {
        this.confirmationId = confirmationId;
        this.confirmationToken = confirmationToken;
        this.riskLevel = riskLevel;
        this.intentSummary = intentSummary;
        this.proposedChanges = proposedChanges;
        this.confirmationPrompt = confirmationPrompt;
        this.expiresAt = expiresAt;
        this.expiresInSecs = expiresInSecs;
    }

    public static ConfirmationCard fromJson(JsonObject object) {
        JsonObject changes = object.has("proposed_changes") && object.get("proposed_changes").isJsonObject()
                ? object.getAsJsonObject("proposed_changes")
                : new JsonObject();
        return new ConfirmationCard(
                Json.requireString(object, "confirmation_id"),
                Json.requireString(object, "confirmation_token"),
                Json.optionalString(object, "risk_level"),
                Json.optionalString(object, "intent_summary"),
                changes,
                Json.optionalString(object, "confirmation_prompt"),
                Json.optionalString(object, "expires_at"),
                object.has("expires_in_secs") ? object.get("expires_in_secs").getAsLong() : 0L);
    }

    public JsonObject toJson() {
        JsonObject object = new JsonObject();
        object.addProperty("confirmation_id", confirmationId);
        object.addProperty("confirmation_token", confirmationToken);
        object.addProperty("risk_level", riskLevel);
        object.addProperty("intent_summary", intentSummary);
        object.add("proposed_changes", proposedChanges);
        object.addProperty("confirmation_prompt", confirmationPrompt);
        object.addProperty("expires_at", expiresAt);
        object.addProperty("expires_in_secs", expiresInSecs);
        return object;
    }

    /** Copy for UI/JS display with the one-time token redacted. */
    public JsonObject toDisplayJson() {
        JsonObject copy = toJson();
        copy.remove("confirmation_token");
        return copy;
    }

    public String confirmationId() {
        return confirmationId;
    }

    public String confirmationToken() {
        return confirmationToken;
    }

    public String riskLevel() {
        return riskLevel;
    }

    public String intentSummary() {
        return intentSummary;
    }

    public JsonObject proposedChanges() {
        return proposedChanges;
    }

    public String confirmationPrompt() {
        return confirmationPrompt;
    }

    public String expiresAt() {
        return expiresAt;
    }

    public long expiresInSecs() {
        return expiresInSecs;
    }
}
