package com.intimeui.client.app.telemetry;

import android.content.Context;
import android.content.SharedPreferences;

import com.intimeui.client.core.telemetry.TelemetryStore;

/** Small durable queue store; telemetry contains no page content or identifiers. */
public final class SharedPreferencesTelemetryStore implements TelemetryStore {
    private static final String PREFS = "intimeui.telemetry";
    private static final String KEY_PENDING = "pending";
    private final SharedPreferences preferences;

    public SharedPreferencesTelemetryStore(Context context) {
        this.preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
    }

    @Override
    public String read() {
        return preferences.getString(KEY_PENDING, "");
    }

    @Override
    public void write(String payload) {
        preferences.edit().putString(KEY_PENDING, payload).apply();
    }
}
