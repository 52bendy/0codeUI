package com.intimeui.client.app.config;

import com.intimeui.client.core.api.ApiClient;
import com.intimeui.client.core.api.HttpUrlConnectionTransport;
import com.intimeui.client.core.replay.ServerClock;

/** Wires the core API client with the app configuration. */
public final class ApiClientFactory {
    private ApiClientFactory() {
    }

    public static ApiClient create(ClientConfig config, ServerClock clock) {
        return new ApiClient(
                config.authToken(),
                config.userId(),
                config::replaySecret,
                clock,
                new HttpUrlConnectionTransport(config.apiBaseUrl()));
    }
}
