package com.intimeui.client.core.telemetry;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

import java.time.Instant;
import java.util.List;

public class TelemetryQueueTest {
    private static TelemetryEvent event(String id) {
        return new TelemetryEvent(
                id, "ORDER_DETAIL_PAGE", 1000, 0.5, 0.1, 0.8, Instant.now().toString());
    }

    @Test
    public void persistsAndRestoresPendingEvents() {
        InMemoryTelemetryStore store = new InMemoryTelemetryStore();
        TelemetryQueue first = new TelemetryQueue(store, 10);
        first.enqueue(event("evt_1"));
        first.enqueue(event("evt_2"));

        TelemetryQueue restored = new TelemetryQueue(store, 10);
        assertEquals(2, restored.size());
        List<TelemetryEvent> batch = restored.drain(1);
        assertEquals("evt_1", batch.get(0).eventId());
        assertEquals(1, restored.size());
    }

    @Test
    public void boundsQueueByDroppingOldest() {
        TelemetryQueue queue = new TelemetryQueue(new InMemoryTelemetryStore(), 2);
        queue.enqueue(event("evt_1"));
        queue.enqueue(event("evt_2"));
        queue.enqueue(event("evt_3"));
        List<TelemetryEvent> batch = queue.drain(10);
        assertEquals(2, batch.size());
        assertEquals("evt_2", batch.get(0).eventId());
        assertEquals("evt_3", batch.get(1).eventId());
    }

    @Test
    public void requeueKeepsOriginalOrder() {
        TelemetryQueue queue = new TelemetryQueue(new InMemoryTelemetryStore(), 10);
        queue.enqueue(event("evt_1"));
        queue.enqueue(event("evt_2"));
        List<TelemetryEvent> batch = queue.drain(2);
        queue.requeue(batch);
        List<TelemetryEvent> restored = queue.drain(2);
        assertEquals("evt_1", restored.get(0).eventId());
        assertEquals("evt_2", restored.get(1).eventId());
    }

    @Test
    public void corruptStoreIsDiscarded() {
        InMemoryTelemetryStore store = new InMemoryTelemetryStore();
        store.write("{not-json");
        TelemetryQueue queue = new TelemetryQueue(store, 10);
        assertEquals(0, queue.size());
        assertTrue(store.read().contains("\"events\""));
    }
}
