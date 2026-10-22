package com.intimeui.client.app.web;

import android.content.Context;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;

/** Loads the bridge/telemetry SDK once from assets. */
public final class JsSdk {
    private static final String ASSET = "intimeui-bridge.js";
    private static String cached;

    private JsSdk() {
    }

    public static synchronized String load(Context context) {
        if (cached == null) {
            cached = readAsset(context);
        }
        return cached;
    }

    private static String readAsset(Context context) {
        try (InputStream input = context.getAssets().open(ASSET);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[4096];
            int read;
            while ((read = input.read(buffer)) != -1) {
                output.write(buffer, 0, read);
            }
            return output.toString(StandardCharsets.UTF_8.name());
        } catch (IOException error) {
            return "window.__INTIMEUI_SDK_ERROR__ = true;";
        }
    }
}
