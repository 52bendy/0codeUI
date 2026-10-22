package com.intimeui.client.app.ui;

import android.content.Context;

import com.intimeui.client.app.R;
import com.intimeui.client.core.api.ApiException;

/** Maps transport/business failures to user-facing strings without leaking internals. */
public final class UiErrors {
    private UiErrors() {
    }

    public static String describe(Context context, ApiException error) {
        if (error.code() == ApiException.NETWORK_ERROR) {
            return context.getString(R.string.error_network);
        }
        if (error.isNonceReplay()) {
            return context.getString(R.string.error_replay);
        }
        if (error.isUnauthorized()) {
            return context.getString(R.string.error_auth);
        }
        if (error.isClockRejected()) {
            return context.getString(R.string.error_clock);
        }
        return context.getString(R.string.error_generic);
    }
}
