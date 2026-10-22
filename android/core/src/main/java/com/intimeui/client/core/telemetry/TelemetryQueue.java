package com.intimeui.client.core.telemetry;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Deque;
import java.util.List;

/**
 * Bounded FIFO queue with persistence. Event identifiers are stable across
 * retries, which lets the server deduplicate by {@code event_id}.
 */
public final class TelemetryQueue {
    private final TelemetryStore store;
    private final int maxSize;
    private final Deque<TelemetryEvent> pending = new ArrayDeque<>();

    public TelemetryQueue(TelemetryStore store, int maxSize) {
        if (maxSize < 1) {
            throw new IllegalArgumentException("maxSize must be positive");
        }
        this.store = store;
        this.maxSize = maxSize;
        load();
    }

    public synchronized void enqueue(TelemetryEvent event) {
        while (pending.size() >= maxSize) {
            pending.removeFirst();
        }
        pending.addLast(event);
        persist();
    }

    public synchronized List<TelemetryEvent> drain(int limit) {
        List<TelemetryEvent> batch = new ArrayList<>();
        while (!pending.isEmpty() && batch.size() < limit) {
            batch.add(pending.removeFirst());
        }
        persist();
        return batch;
    }

    public synchronized void requeue(List<TelemetryEvent> events) {
        if (events == null || events.isEmpty()) {
            return;
        }
        for (int index = events.size() - 1; index >= 0; index--) {
            pending.addFirst(events.get(index));
        }
        while (pending.size() > maxSize) {
            pending.removeFirst();
        }
        persist();
    }

    public synchronized int size() {
        return pending.size();
    }

    public synchronized void clear() {
        pending.clear();
        persist();
    }

    private void load() {
        String payload = store.read();
        if (payload == null || payload.trim().isEmpty()) {
            return;
        }
        try {
            JsonObject root = Json.parseObject(payload);
            JsonArray events = Json.optionalArray(root, "events");
            for (JsonElement element : events) {
                if (element.isJsonObject()) {
                    pending.addLast(TelemetryEvent.fromJson(element.getAsJsonObject()));
                }
            }
        } catch (RuntimeException error) {
            // A corrupt local queue must not block app startup; discard it.
            pending.clear();
            persist();
        }
    }

    private void persist() {
        JsonArray events = new JsonArray();
        for (TelemetryEvent event : pending) {
            events.add(event.toJson());
        }
        JsonObject root = new JsonObject();
        root.add("events", events);
        store.write(Json.write(root));
    }

    public static List<TelemetryEvent> emptyBatch() {
        return Collections.emptyList();
    }
}
