package com.intimeui.client.core.telemetry;

/** Persistence port for the pending telemetry queue. */
public interface TelemetryStore {
    /** Returns the last persisted payload, or an empty string when nothing was stored. */
    String read();

    /** Persists the queue payload. Implementations must be synchronous. */
    void write(String payload);
}
