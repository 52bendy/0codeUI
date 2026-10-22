package com.intimeui.client.app.telemetry;

import android.os.Handler;
import android.os.SystemClock;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;
import com.intimeui.client.core.telemetry.TelemetryEvent;
import com.intimeui.client.core.telemetry.TelemetrySession;
import com.intimeui.client.core.telemetry.TelemetrySnapshot;
import com.intimeui.client.core.telemetry.TelemetryUploader;

import java.time.Instant;
import java.util.UUID;
import java.util.concurrent.ExecutorService;

/** Native telemetry aggregation driven by the injected JS SDK signals. */
public final class WebViewTelemetryTracker {
    private static final long FLUSH_INTERVAL_MS = 15_000L;
    private final Handler mainHandler;
    private final TelemetryUploader uploader;
    private final ExecutorService ioExecutor;
    private TelemetrySession session;
    private boolean started;

    private final Runnable periodicFlush = new Runnable() {
        @Override
        public void run() {
            if (!started) {
                return;
            }
            flush();
            mainHandler.postDelayed(this, FLUSH_INTERVAL_MS);
        }
    };

    public WebViewTelemetryTracker(
            Handler mainHandler, TelemetryUploader uploader, ExecutorService ioExecutor) {
        this.mainHandler = mainHandler;
        this.uploader = uploader;
        this.ioExecutor = ioExecutor;
    }

    public void start(String pageId) {
        if (started) {
            flush();
        }
        session = new TelemetrySession(safePageId(pageId), SystemClock::elapsedRealtime);
        started = true;
        mainHandler.removeCallbacks(periodicFlush);
        mainHandler.postDelayed(periodicFlush, FLUSH_INTERVAL_MS);
    }

    public void onSignal(JsonObject payload) {
        if (!started || session == null) {
            return;
        }
        String kind = Json.optionalString(payload, "kind");
        if ("TAP".equals(kind)) {
            boolean interactive = payload.has("interactive")
                    && payload.get("interactive").getAsBoolean();
            session.recordTap(interactive);
        } else if ("SCROLL".equals(kind) && payload.has("depth")) {
            session.recordScroll(payload.get("depth").getAsDouble());
        } else if ("PAGE_HIDE".equals(kind)
                || (kind == null && payload.has("depth"))) {
            if (payload.has("depth")) {
                session.recordScroll(payload.get("depth").getAsDouble());
            }
            flush();
        }
    }

    public void flush() {
        if (!started || session == null) {
            return;
        }
        TelemetrySnapshot snapshot = session.snapshot();
        if (snapshot.dwellTimeMs() <= 0) {
            return;
        }
        String eventId = "evt_" + UUID.randomUUID().toString().replace("-", "");
        TelemetryEvent event = TelemetryEvent.fromSnapshot(
                eventId, snapshot, Instant.now());
        uploader.enqueue(event);
        session.start();
        ioExecutor.execute(uploader::flushNow);
    }

    public void stop() {
        if (started) {
            flush();
        }
        started = false;
        mainHandler.removeCallbacks(periodicFlush);
    }

    public int pendingCount() {
        return uploader.pendingCount();
    }

    private static String safePageId(String pageId) {
        return pageId != null && pageId.matches("^[A-Za-z0-9._:-]{1,128}$")
                ? pageId
                : "UNKNOWN";
    }
}
