package com.intimeui.client.core.intent;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

/** Parsed {@code data} of {@code POST /api/v1/intent/execute}. */
public final class IntentResult {
    public static final String ACTION_EXECUTE = "EXECUTE";
    public static final String ACTION_REQUIRE_CONFIRMATION = "REQUIRE_CONFIRMATION";

    private final String sessionId;
    private final String actionType;
    private final String intentSummary;
    private final JsonObject proposedChanges;
    private final String confirmationPrompt;
    private final ConfirmationCard confirmation;
    private final JsonObject context;
    private final JsonArray executionResults;

    private IntentResult(
            String sessionId,
            String actionType,
            String intentSummary,
            JsonObject proposedChanges,
            String confirmationPrompt,
            ConfirmationCard confirmation,
            JsonObject context,
            JsonArray executionResults) {
        this.sessionId = sessionId;
        this.actionType = actionType;
        this.intentSummary = intentSummary;
        this.proposedChanges = proposedChanges;
        this.confirmationPrompt = confirmationPrompt;
        this.confirmation = confirmation;
        this.context = context;
        this.executionResults = executionResults;
    }

    public static IntentResult fromJson(JsonObject data) {
        JsonObject changes = data.has("proposed_changes") && data.get("proposed_changes").isJsonObject()
                ? data.getAsJsonObject("proposed_changes")
                : new JsonObject();
        JsonObject context = data.has("context") && data.get("context").isJsonObject()
                ? data.getAsJsonObject("context")
                : new JsonObject();
        ConfirmationCard card = data.has("confirmation") && data.get("confirmation").isJsonObject()
                ? ConfirmationCard.fromJson(data.getAsJsonObject("confirmation"))
                : null;
        return new IntentResult(
                Json.optionalString(data, "session_id"),
                Json.requireString(data, "action_type"),
                Json.optionalString(data, "intent_summary"),
                changes,
                Json.optionalString(data, "confirmation_prompt"),
                card,
                context,
                Json.optionalArray(data, "execution_results"));
    }

    public JsonObject toJson() {
        JsonObject object = new JsonObject();
        object.addProperty("session_id", sessionId);
        object.addProperty("action_type", actionType);
        object.addProperty("intent_summary", intentSummary);
        object.add("proposed_changes", proposedChanges);
        if (confirmationPrompt != null) {
            object.addProperty("confirmation_prompt", confirmationPrompt);
        }
        if (confirmation != null) {
            object.add("confirmation", confirmation.toDisplayJson());
        }
        object.add("context", context);
        object.add("execution_results", executionResults);
        return object;
    }

    public boolean requiresConfirmation() {
        return ACTION_REQUIRE_CONFIRMATION.equals(actionType) && confirmation != null;
    }

    public String sessionId() {
        return sessionId;
    }

    public String actionType() {
        return actionType;
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

    public ConfirmationCard confirmation() {
        return confirmation;
    }

    public JsonObject context() {
        return context;
    }

    public JsonArray executionResults() {
        return executionResults;
    }
}
