package com.intimeui.client.core.replay;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** Immutable set of replay-protection headers for one request. */
public final class ReplayHeaders {
    private final String timestamp;
    private final String nonce;
    private final String dynamicToken;
    private final String signature;

    ReplayHeaders(String timestamp, String nonce, String dynamicToken, String signature) {
        this.timestamp = timestamp;
        this.nonce = nonce;
        this.dynamicToken = dynamicToken;
        this.signature = signature;
    }

    public String timestamp() {
        return timestamp;
    }

    public String nonce() {
        return nonce;
    }

    public String dynamicToken() {
        return dynamicToken;
    }

    public String signature() {
        return signature;
    }

    public Map<String, String> asMap() {
        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("x-timestamp", timestamp);
        headers.put("x-nonce", nonce);
        headers.put("x-dynamic-token", dynamicToken);
        headers.put("x-signature", signature);
        return Collections.unmodifiableMap(headers);
    }
}
