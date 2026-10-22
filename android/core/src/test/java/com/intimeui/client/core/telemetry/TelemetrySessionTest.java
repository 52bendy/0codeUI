package com.intimeui.client.core.telemetry;

import static org.junit.Assert.assertEquals;

import org.junit.Test;

import java.util.concurrent.atomic.AtomicLong;

public class TelemetrySessionTest {
    @Test
    public void aggregatesContractMetrics() {
        AtomicLong clock = new AtomicLong(1_000L);
        TelemetrySession session = new TelemetrySession("ORDER_DETAIL_PAGE", clock::get);
        clock.set(2_500L);
        session.recordTap(true);
        session.recordTap(true);
        session.recordTap(false);
        session.recordScroll(0.4);
        session.recordScroll(0.8);
        session.recordScroll(1.4);

        TelemetrySnapshot snapshot = session.snapshot();
        assertEquals(1_500L, snapshot.dwellTimeMs());
        assertEquals(2.0 / 3.0, snapshot.elementClickRate(), 0.0001);
        assertEquals(1.0 / 3.0, snapshot.misclickRate(), 0.0001);
        assertEquals(1.0, snapshot.scrollDepth(), 0.0001);
    }

    @Test
    public void clampsDwellAndHandlesNoTaps() {
        AtomicLong clock = new AtomicLong(0L);
        TelemetrySession session = new TelemetrySession("page:home", clock::get);
        clock.set(100_000_000L);
        TelemetrySnapshot snapshot = session.snapshot();
        assertEquals(86_400_000L, snapshot.dwellTimeMs());
        assertEquals(0.0, snapshot.elementClickRate(), 0.0001);
        assertEquals(0.0, snapshot.misclickRate(), 0.0001);
        assertEquals(0.0, snapshot.scrollDepth(), 0.0001);
    }

    @Test(expected = IllegalArgumentException.class)
    public void rejectsUnsafePageIds() {
        new TelemetrySession("https://evil.example/page", System::currentTimeMillis);
    }
}
