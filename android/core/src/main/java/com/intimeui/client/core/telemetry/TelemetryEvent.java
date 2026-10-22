package com.intimeui.client.core.telemetry;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

import java.time.Instant;
import java.time.format.DateTimeParseException;
import java.util.Objects;
import java.util.regex.Pattern;

/**
 * One telemetry event matching the server's {@code TelemetryEventInput} schema.
 * Only the contract fields are serialized; unknown fields are never sent.
 */
public final class TelemetryEvent {
    private static final Pattern EVENT_ID_PATTERN =
            Pattern.compile("^[A-Za-z0-9._:-]{1,64}$");
    private static final Pattern PAGE_ID_PATTERN =
            Pattern.compile("^[A-Za-z0-9._:-]{1,128}$");
    private static final long MAX_DWELL_MS = 86_400_000L;

    private final String eventId;
    private final String pageId;
    private final long dwellTimeMs;
    private final double elementClickRate;
    private final double misclickRate;
    private final double scrollDepth;
    private final String timestamp;

    public TelemetryEvent(
            String eventId,
            String pageId,
            long dwellTimeMs,
            double elementClickRate,
            double misclickRate,
            double scrollDepth,
            String timestamp) {
        this.eventId = requirePattern(eventId, EVENT_ID_PATTERN, "event_id");
        this.pageId = requirePattern(pageId, PAGE_ID_PATTERN, "page_id");
        this.dwellTimeMs = clampLong(dwellTimeMs, 0L, MAX_DWELL_MS);
        this.elementClickRate = clampRate(elementClickRate, "element_click_rate");
        this.misclickRate = clampRate(misclickRate, "misclick_rate");
        this.scrollDepth = clampRate(scrollDepth, "scroll_depth");
        this.timestamp = requireTimestamp(timestamp);
    }

    public static TelemetryEvent fromSnapshot(
            String eventId, TelemetrySnapshot snapshot, Instant occurredAt) {
        return new TelemetryEvent(
                eventId,
                snapshot.pageId(),
                snapshot.dwellTimeMs(),
                snapshot.elementClickRate(),
                snapshot.misclickRate(),
                snapshot.scrollDepth(),
                occurredAt.toString());
    }

    public static TelemetryEvent fromJson(JsonObject object) {
        return new TelemetryEvent(
                Json.requireString(object, "event_id"),
                Json.requireString(object, "page_id"),
                Json.requireLong(object, "dwell_time"),
                Json.requireDouble(object, "element_click_rate"),
                Json.requireDouble(object, "misclick_rate"),
                Json.requireDouble(object, "scroll_depth"),
                Json.requireString(object, "timestamp"));
    }

    public JsonObject toJson() {
        JsonObject object = new JsonObject();
        object.addProperty("event_id", eventId);
        object.addProperty("page_id", pageId);
        object.addProperty("dwell_time", dwellTimeMs);
        object.addProperty("element_click_rate", elementClickRate);
        object.addProperty("misclick_rate", misclickRate);
        object.addProperty("scroll_depth", scrollDepth);
        object.addProperty("timestamp", timestamp);
        return object;
    }

    public String eventId() {
        return eventId;
    }

    public String pageId() {
        return pageId;
    }

    public long dwellTimeMs() {
        return dwellTimeMs;
    }

    public double elementClickRate() {
        return elementClickRate;
    }

    public double misclickRate() {
        return misclickRate;
    }

    public double scrollDepth() {
        return scrollDepth;
    }

    public String timestamp() {
        return timestamp;
    }

    private static String requirePattern(String value, Pattern pattern, String field) {
        if (value == null || !pattern.matcher(value).matches()) {
            throw new IllegalArgumentException(field + " has an invalid format");
        }
        return value;
    }

    private static double clampRate(double value, String field) {
        if (Double.isNaN(value) || Double.isInfinite(value)) {
            throw new IllegalArgumentException(field + " must be finite");
        }
        return Math.max(0.0, Math.min(1.0, value));
    }

    private static long clampLong(long value, long min, long max) {
        return Math.max(min, Math.min(max, value));
    }

    private static String requireTimestamp(String value) {
        if (value == null) {
            throw new IllegalArgumentException("timestamp must not be null");
        }
        try {
            Instant.parse(value);
            return value;
        } catch (DateTimeParseException error) {
            throw new IllegalArgumentException("timestamp must be RFC3339", error);
        }
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) {
            return true;
        }
        if (!(other instanceof TelemetryEvent)) {
            return false;
        }
        TelemetryEvent that = (TelemetryEvent) other;
        return eventId.equals(that.eventId);
    }

    @Override
    public int hashCode() {
        return Objects.hash(eventId);
    }
}
