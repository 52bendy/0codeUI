package com.intimeui.client.core.replay;

import java.time.Instant;
import java.time.ZonedDateTime;
import java.time.format.DateTimeFormatter;
import java.time.format.DateTimeParseException;

/**
 * Tracks the offset between the device clock and the server clock.
 *
 * <p>The offset is learned from the HTTP {@code Date} header of any response
 * (including {@code GET /healthz}) and applied before signing.</p>
 */
public final class ServerClock {
    private volatile long offsetSeconds;

    public long nowEpochSeconds() {
        return Instant.now().getEpochSecond() + offsetSeconds;
    }

    public long offsetSeconds() {
        return offsetSeconds;
    }

    public void observeServerEpoch(long serverEpochSeconds) {
        if (serverEpochSeconds > 0) {
            offsetSeconds = serverEpochSeconds - Instant.now().getEpochSecond();
        }
    }

    /**
     * Parses an RFC 1123 {@code Date} header and records the offset.
     *
     * @return true when the header was valid and applied
     */
    public boolean observeHttpDate(String httpDate) {
        if (httpDate == null || httpDate.trim().isEmpty()) {
            return false;
        }
        try {
            ZonedDateTime parsed = ZonedDateTime.parse(httpDate, DateTimeFormatter.RFC_1123_DATE_TIME);
            observeServerEpoch(parsed.toEpochSecond());
            return true;
        } catch (DateTimeParseException error) {
            return false;
        }
    }

    public boolean isSkewed(long toleranceSeconds) {
        return Math.abs(offsetSeconds) > toleranceSeconds;
    }

    public void reset() {
        offsetSeconds = 0;
    }
}
