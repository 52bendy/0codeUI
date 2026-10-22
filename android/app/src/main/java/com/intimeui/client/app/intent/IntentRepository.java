package com.intimeui.client.app.intent;

import com.intimeui.client.core.api.ApiClient;
import com.intimeui.client.core.api.ApiException;
import com.intimeui.client.core.intent.ConfirmationCard;
import com.intimeui.client.core.intent.ConfirmationDecision;
import com.intimeui.client.core.intent.IntentRequest;
import com.intimeui.client.core.intent.IntentResult;

/** Thin domain facade over the signed intent endpoints. */
public final class IntentRepository {
    private final ApiClient apiClient;

    public IntentRepository(ApiClient apiClient) {
        this.apiClient = apiClient;
    }

    public IntentResult execute(IntentRequest request) throws ApiException {
        return apiClient.executeIntent(request);
    }

    public ConfirmationDecision confirm(ConfirmationCard card, String sessionId)
            throws ApiException {
        return apiClient.confirm(card.confirmationId(), card.confirmationToken(), sessionId);
    }

    public ConfirmationDecision reject(ConfirmationCard card, String reason)
            throws ApiException {
        return apiClient.reject(card.confirmationId(), card.confirmationToken(), reason);
    }
}
