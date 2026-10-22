package com.intimeui.client.core.telemetry;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertTrue;

import com.intimeui.client.core.api.ApiException;
import org.junit.Test;

import java.time.Instant;
import java.util.ArrayList;
import java.util.List;

public class TelemetryUploaderTest {
    private static TelemetryEvent event(String id) {
        return new TelemetryEvent(
                id, "ORDER_DETAIL_PAGE", 1000, 0.5, 0.1, 0.8, Instant.now().toString());
    }

    @Test
    public void drainsQueueInBatches() {
        TelemetryQueue queue = new TelemetryQueue(new InMemoryTelemetryStore(), 100);
        List<TelemetryEvent> uploaded = new ArrayList<>();
        TelemetryUploader uploader = new TelemetryUploader(queue, uploaded::addAll, 2);
        queue.enqueue(event("evt_1"));
        queue.enqueue(event("evt_2"));
        queue.enqueue(event("evt_3"));

        assertTrue(uploader.flushNow());
        assertEquals(2, uploaded.size());
        assertEquals(1, uploader.pendingCount());
        assertTrue(uploader.flushNow());
        assertEquals(3, uploaded.size());
    }

    @Test
    public void failedBatchReturnsToQueueWithStableIds() {
        TelemetryQueue queue = new TelemetryQueue(new InMemoryTelemetryStore(), 100);
        queue.enqueue(event("evt_1"));
        queue.enqueue(event("evt_2"));
        TelemetryUploader uploader = new TelemetryUploader(queue, events -> {
            throw new ApiException(ApiException.NETWORK_ERROR, "offline");
        }, 10);

        assertFalse(uploader.flushNow());
        assertEquals(2, uploader.pendingCount());
        List<TelemetryEvent> pending = queue.drain(10);
        assertEquals("evt_1", pending.get(0).eventId());
        assertEquals("evt_2", pending.get(1).eventId());
    }
}
