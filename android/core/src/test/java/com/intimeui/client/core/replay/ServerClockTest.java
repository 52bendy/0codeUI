package com.intimeui.client.core.replay;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.time.Instant;

public class ServerClockTest {
    @Test
    public void appliesObservedOffset() {
        ServerClock clock = new ServerClock();
        long serverNow = Instant.now().getEpochSecond() + 120;
        clock.observeServerEpoch(serverNow);
        long drift = Math.abs(clock.nowEpochSeconds() - serverNow);
        assertTrue("drift should stay below 2 seconds", drift <= 2);
        assertEquals(120, clock.offsetSeconds(), 2);
    }

    @Test
    public void parsesRfc1123DateHeader() {
        ServerClock clock = new ServerClock();
        assertTrue(clock.observeHttpDate("Thu, 01 Jan 2026 00:00:00 GMT"));
        assertTrue(clock.isSkewed(300));
        clock.reset();
        assertFalse(clock.isSkewed(300));
        assertFalse(clock.observeHttpDate("not-a-date"));
    }
}
