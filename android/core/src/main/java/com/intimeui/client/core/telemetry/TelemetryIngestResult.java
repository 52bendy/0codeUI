package com.intimeui.client.core.telemetry;

import com.google.gson.JsonObject;

/** Parsed {@code data} of the telemetry ingest endpoint. */
public final class TelemetryIngestResult {
    private final int accepted;
    private final int duplicates;

    public TelemetryIngestResult(int accepted, int duplicates) {
        this.accepted = accepted;
        this.duplicates = duplicates;
    }

    public static TelemetryIngestResult fromJson(JsonObject data) {
        int accepted = data.has("accepted") ? data.get("accepted").getAsInt() : 0;
        int duplicates = data.has("duplicates") ? data.get("duplicates").getAsInt() : 0;
        return new TelemetryIngestResult(accepted, duplicates);
    }

    public int accepted() {
        return accepted;
    }

    public int duplicates() {
        return duplicates;
    }
}
