package com.intimeui.client.core.telemetry;

import java.util.function.LongSupplier;
import java.util.regex.Pattern;

/**
 * Aggregates raw page interactions into the four metrics required by the
 * telemetry contract (REQ-202610-1174).
 *
 * <p>Definitions:</p>
 * <ul>
 *   <li>{@code element_click_rate} = taps on actionable elements / all taps;</li>
 *   <li>{@code misclick_rate} = taps outside actionable elements / all taps;</li>
 *   <li>{@code scroll_depth} = deepest scroll position / scrollable height;</li>
 *   <li>{@code dwell_time} = milliseconds since the page became visible.</li>
 * </ul>
 *
 * <p>All values are clamped to the server's accepted ranges. The class is not
 * thread-safe; the WebView tracker confines calls to the main thread.</p>
 */
public final class TelemetrySession {
    private static final Pattern PAGE_ID_PATTERN =
            Pattern.compile("^[A-Za-z0-9._:-]{1,128}$");
    private static final long MAX_DWELL_MS = 86_400_000L;

    private final String pageId;
    private final LongSupplier clockMillis;
    private long startedAtMillis;
    private long totalTaps;
    private long interactiveTaps;
    private long misclicks;
    private double maxScrollDepth;

    public TelemetrySession(String pageId, LongSupplier clockMillis) {
        if (pageId == null || !PAGE_ID_PATTERN.matcher(pageId).matches()) {
            throw new IllegalArgumentException("pageId must match [A-Za-z0-9._:-]{1,128}");
        }
        this.pageId = pageId;
        this.clockMillis = clockMillis;
        start();
    }

    public void start() {
        startedAtMillis = clockMillis.getAsLong();
        totalTaps = 0;
        interactiveTaps = 0;
        misclicks = 0;
        maxScrollDepth = 0.0;
    }

    public void recordTap(boolean interactive) {
        totalTaps++;
        if (interactive) {
            interactiveTaps++;
        } else {
            misclicks++;
        }
    }

    public void recordScroll(double depth) {
        if (Double.isNaN(depth)) {
            return;
        }
        maxScrollDepth = Math.max(maxScrollDepth, clamp(depth, 0.0, 1.0));
    }

    public TelemetrySnapshot snapshot() {
        long elapsed = Math.max(0L, clockMillis.getAsLong() - startedAtMillis);
        long dwell = Math.min(elapsed, MAX_DWELL_MS);
        double clickRate = totalTaps == 0 ? 0.0 : (double) interactiveTaps / totalTaps;
        double misclickRate = totalTaps == 0 ? 0.0 : (double) misclicks / totalTaps;
        return new TelemetrySnapshot(
                pageId,
                dwell,
                clamp(clickRate, 0.0, 1.0),
                clamp(misclickRate, 0.0, 1.0),
                clamp(maxScrollDepth, 0.0, 1.0));
    }

    public String pageId() {
        return pageId;
    }

    private static double clamp(double value, double min, double max) {
        return Math.max(min, Math.min(max, value));
    }
}
