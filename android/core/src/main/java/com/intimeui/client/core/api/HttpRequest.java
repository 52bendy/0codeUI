package com.intimeui.client.core.api;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** Immutable HTTP request used by the transport port. */
public final class HttpRequest {
    private final String method;
    private final String pathAndQuery;
    private final Map<String, String> headers;
    private final byte[] body;

    public HttpRequest(
            String method, String pathAndQuery, Map<String, String> headers, byte[] body) {
        this.method = method;
        this.pathAndQuery = pathAndQuery;
        this.headers = Collections.unmodifiableMap(new LinkedHashMap<>(headers));
        this.body = body == null ? new byte[0] : body.clone();
    }

    public String method() {
        return method;
    }

    public String pathAndQuery() {
        return pathAndQuery;
    }

    public Map<String, String> headers() {
        return headers;
    }

    public byte[] body() {
        return body.clone();
    }
}
