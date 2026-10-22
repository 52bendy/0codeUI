package com.intimeui.client.core.telemetry;

/** In-memory store used by tests and previews. */
public final class InMemoryTelemetryStore implements TelemetryStore {
    private String payload = "";

    @Override
    public String read() {
        return payload;
    }

    @Override
    public void write(String payload) {
        this.payload = payload == null ? "" : payload;
    }
}
