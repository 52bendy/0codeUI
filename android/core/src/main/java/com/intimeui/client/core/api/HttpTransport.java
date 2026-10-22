package com.intimeui.client.core.api;

/** Transport port so the API client stays testable without a real socket. */
public interface HttpTransport {
    HttpResponse execute(HttpRequest request) throws ApiException;
}
