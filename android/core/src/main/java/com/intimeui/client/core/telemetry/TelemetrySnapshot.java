package com.intimeui.client.core.telemetry;

/** Immutable point-in-time aggregation of one page visit. */
public final class TelemetrySnapshot {
    private final String pageId;
    private final long dwellTimeMs;
    private final double elementClickRate;
    private final double misclickRate;
    private final double scrollDepth;

    TelemetrySnapshot(
            String pageId,
            long dwellTimeMs,
            double elementClickRate,
            double misclickRate,
            double scrollDepth) {
        this.pageId = pageId;
        this.dwellTimeMs = dwellTimeMs;
        this.elementClickRate = elementClickRate;
        this.misclickRate = misclickRate;
        this.scrollDepth = scrollDepth;
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
}
