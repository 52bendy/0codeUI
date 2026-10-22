package com.intimeui.client.core.telemetry;

import com.intimeui.client.core.api.ApiException;

import java.util.List;

/** Upload port implemented by the signed API client. */
public interface TelemetryReporter {
    void report(List<TelemetryEvent> events) throws ApiException;
}
