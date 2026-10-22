package com.intimeui.client.core.api;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.intimeui.client.core.intent.ConfirmationDecision;
import com.intimeui.client.core.intent.IntentRequest;
import com.intimeui.client.core.intent.IntentResult;
import com.intimeui.client.core.intent.UiRenderResult;
import com.intimeui.client.core.json.Json;
import com.intimeui.client.core.replay.NonceGenerator;
import com.intimeui.client.core.replay.ReplaySigner;
import com.intimeui.client.core.replay.ServerClock;
import com.intimeui.client.core.telemetry.TelemetryEvent;
import com.intimeui.client.core.telemetry.TelemetryIngestResult;

import java.nio.charset.StandardCharsets;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/**
 * Signed client for the InTimeUI backend.
 *
 * <p>Only the three endpoints listed in INTIMEUI-REPLAY-V1 are signed:
 * {@code /ui/generate}, {@code /intent/execute} and {@code /telemetry/event}.
 * Confirmation endpoints authenticate with Bearer only, matching the server
 * routing introduced by REQ-202610-1179.</p>
 */
public final class ApiClient {
    public static final String PATH_UI_GENERATE = "/api/v1/ui/generate";
    public static final String PATH_INTENT_EXECUTE = "/api/v1/intent/execute";
    public static final String PATH_INTENT_CONFIRM = "/api/v1/intent/confirm";
    public static final String PATH_INTENT_REJECT = "/api/v1/intent/reject";
    public static final String PATH_TELEMETRY_EVENT = "/api/v1/telemetry/event";
    public static final String PATH_HEALTHZ = "/healthz";

    private final String authToken;
    private final String signingUserId;
    private final ReplaySecretProvider secretProvider;
    private final ServerClock clock;
    private final HttpTransport transport;
    private final ReplaySigner signer;
    private final NonceGenerator nonces;

    public ApiClient(
            String authToken,
            String signingUserId,
            ReplaySecretProvider secretProvider,
            ServerClock clock,
            HttpTransport transport) {
        this(authToken, signingUserId, secretProvider, clock, transport,
                new ReplaySigner(), new NonceGenerator());
    }

    public ApiClient(
            String authToken,
            String signingUserId,
            ReplaySecretProvider secretProvider,
            ServerClock clock,
            HttpTransport transport,
            ReplaySigner signer,
            NonceGenerator nonces) {
        if (authToken == null || authToken.trim().isEmpty()) {
            throw new IllegalArgumentException("authToken must not be blank");
        }
        this.authToken = authToken;
        this.signingUserId = signingUserId == null ? "" : signingUserId;
        this.secretProvider = secretProvider == null ? () -> null : secretProvider;
        this.clock = clock;
        this.transport = transport;
        this.signer = signer;
        this.nonces = nonces;
    }

    public UiRenderResult generateUi(
            String userId,
            String sceneCode,
            JsonObject deviceInfo,
            JsonObject businessData) throws ApiException {
        JsonObject body = new JsonObject();
        body.addProperty("user_id", userId);
        body.addProperty("scene_code", sceneCode);
        body.add("device_info", deviceInfo);
        body.add("business_data", businessData);
        return UiRenderResult.fromJson(post(PATH_UI_GENERATE, body, true));
    }

    public IntentResult executeIntent(IntentRequest request) throws ApiException {
        return IntentResult.fromJson(post(PATH_INTENT_EXECUTE, request.toJson(), true));
    }

    public ConfirmationDecision confirm(
            String confirmationId, String confirmationToken, String sessionId)
            throws ApiException {
        JsonObject body = new JsonObject();
        body.addProperty("confirmation_id", confirmationId);
        body.addProperty("confirmation_token", confirmationToken);
        if (sessionId != null && !sessionId.isEmpty()) {
            body.addProperty("session_id", sessionId);
        }
        return ConfirmationDecision.fromJson(post(PATH_INTENT_CONFIRM, body, false));
    }

    public ConfirmationDecision reject(
            String confirmationId, String confirmationToken, String reason)
            throws ApiException {
        JsonObject body = new JsonObject();
        body.addProperty("confirmation_id", confirmationId);
        body.addProperty("confirmation_token", confirmationToken);
        if (reason != null && !reason.isEmpty()) {
            body.addProperty("reason", reason);
        }
        return ConfirmationDecision.fromJson(post(PATH_INTENT_REJECT, body, false));
    }

    public TelemetryIngestResult reportTelemetry(List<TelemetryEvent> events)
            throws ApiException {
        JsonArray array = new JsonArray();
        for (TelemetryEvent event : events) {
            array.add(event.toJson());
        }
        JsonObject body = new JsonObject();
        body.add("events", array);
        return TelemetryIngestResult.fromJson(post(PATH_TELEMETRY_EVENT, body, true));
    }

    /** Fetches {@code /healthz} and learns the server clock from its Date header. */
    public void syncClock() throws ApiException {
        HttpResponse response = transport.execute(
                new HttpRequest("GET", PATH_HEALTHZ, new LinkedHashMap<>(), new byte[0]));
        clock.observeHttpDate(response.header("Date"));
    }

    private JsonObject post(String path, JsonObject body, boolean signed) throws ApiException {
        byte[] rawBody = Json.write(body).getBytes(StandardCharsets.UTF_8);
        ApiException lastError = null;
        for (int attempt = 0; attempt < 2; attempt++) {
            Map<String, String> headers = baseHeaders();
            String secret = signed ? secretProvider.secret() : null;
            if (secret != null && !secret.isEmpty()) {
                addReplayHeaders(headers, path, rawBody, secret);
            }
            HttpResponse response = transport.execute(
                    new HttpRequest("POST", path, headers, rawBody));
            clock.observeHttpDate(response.header("Date"));
            ApiEnvelope envelope = parseEnvelope(response);
            if (envelope.isSuccess()) {
                return envelope.requireData();
            }
            ApiException error = new ApiException(
                    envelope.code(), response.status(), envelope.message());
            if (attempt == 0 && signed && isReplayRetryable(error)) {
                lastError = error;
                continue;
            }
            throw error;
        }
        throw lastError == null
                ? ApiException.invalidResponse("request failed")
                : lastError;
    }

    private Map<String, String> baseHeaders() {
        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("Authorization", "Bearer " + authToken);
        headers.put("Content-Type", "application/json");
        headers.put("Accept", "application/json");
        return headers;
    }

    private void addReplayHeaders(
            Map<String, String> headers, String path, byte[] rawBody, String secret)
            throws ApiException {
        if (signingUserId.isEmpty()) {
            throw ApiException.invalidResponse("signing user id is not configured");
        }
        long timestamp = clock.nowEpochSeconds();
        String nonce = nonces.newNonce();
        try {
            headers.putAll(signer.sign(
                    secret, "POST", path, timestamp, nonce, signingUserId, rawBody).asMap());
        } catch (IllegalArgumentException error) {
            throw ApiException.invalidResponse("replay signing configuration is invalid");
        }
    }

    private static ApiEnvelope parseEnvelope(HttpResponse response) throws ApiException {
        try {
            return ApiEnvelope.parse(response.body());
        } catch (ApiException error) {
            if (response.status() >= 400) {
                throw new ApiException(50000, response.status(), "HTTP " + response.status());
            }
            throw error;
        }
    }

    private static boolean isReplayRetryable(ApiException error) {
        return error.isNonceReplay() || error.isClockRejected();
    }
}
