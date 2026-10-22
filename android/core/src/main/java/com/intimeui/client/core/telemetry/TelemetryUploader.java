package com.intimeui.client.core.telemetry;

import com.intimeui.client.core.api.ApiException;

import java.util.List;

/**
 * Drains the persisted queue in bounded batches. A failed batch is put back at
 * the head of the queue; the next attempt reuses the same event identifiers.
 */
public final class TelemetryUploader {
    private static final int DEFAULT_BATCH_SIZE = 20;
    private final TelemetryQueue queue;
    private final TelemetryReporter reporter;
    private final int batchSize;
    private final Object flushLock = new Object();
    private boolean flushing;

    public TelemetryUploader(TelemetryQueue queue, TelemetryReporter reporter) {
        this(queue, reporter, DEFAULT_BATCH_SIZE);
    }

    public TelemetryUploader(TelemetryQueue queue, TelemetryReporter reporter, int batchSize) {
        if (batchSize < 1) {
            throw new IllegalArgumentException("batchSize must be positive");
        }
        this.queue = queue;
        this.reporter = reporter;
        this.batchSize = batchSize;
    }

    public void enqueue(TelemetryEvent event) {
        queue.enqueue(event);
    }

    /**
     * Attempts one synchronous flush.
     *
     * @return true when the queue is empty or the batch was accepted
     */
    public boolean flushNow() {
        synchronized (flushLock) {
            if (flushing) {
                return false;
            }
            flushing = true;
        }
        try {
            List<TelemetryEvent> batch = queue.drain(batchSize);
            if (batch.isEmpty()) {
                return true;
            }
            try {
                reporter.report(batch);
                return true;
            } catch (ApiException error) {
                queue.requeue(batch);
                return false;
            }
        } finally {
            synchronized (flushLock) {
                flushing = false;
            }
        }
    }

    public int pendingCount() {
        return queue.size();
    }
}
