package com.intimeui.client.core.api;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** Immutable HTTP response. */
public final class HttpResponse {
    private final int status;
    private final Map<String, String> headers;
    private final String body;

    public HttpResponse(int status, Map<String, String> headers, String body) {
        this.status = status;
        Map<String, String> normalized = new LinkedHashMap<>();
        for (Map.Entry<String, String> entry : headers.entrySet()) {
            normalized.put(entry.getKey().toLowerCase(java.util.Locale.ROOT), entry.getValue());
        }
        this.headers = Collections.unmodifiableMap(normalized);
        this.body = body == null ? "" : body;
    }

    public int status() {
        return status;
    }

    public String header(String name) {
        return headers.get(name.toLowerCase(java.util.Locale.ROOT));
    }

    public Map<String, String> headers() {
        return headers;
    }

    public String body() {
        return body;
    }
}
