package com.intimeui.client.app.web;

import android.os.Handler;
import android.webkit.JavascriptInterface;
import android.webkit.WebView;

import com.google.gson.JsonObject;
import com.intimeui.client.core.bridge.BridgeProtocol;
import com.intimeui.client.core.bridge.BridgeRouter;
import com.intimeui.client.core.json.Json;

/** JS-native bridge exposed as {@code window.IntimeUINative}. */
public final class WebViewBridge {
    public static final String JS_INTERFACE = "IntimeUINative";
    private volatile WebView webView;
    private final Handler mainHandler;
    private final BridgeRouter router;
    private volatile boolean allowed;

    public WebViewBridge(WebView webView, Handler mainHandler, BridgeRouter router) {
        this.webView = webView;
        this.mainHandler = mainHandler;
        this.router = router;
    }

    public void attach(WebView target) {
        this.webView = target;
    }

    public void setAllowed(boolean value) {
        this.allowed = value;
    }

    @JavascriptInterface
    public String postMessage(String raw) {
        if (!allowed) {
            return "{\"v\":1,\"id\":\"unknown\",\"type\":\"ERROR\","
                    + "\"payload\":{\"code\":\"NOT_READY\",\"message\":\"bridge is not ready\"}}";
        }
        return router.handle(raw);
    }

    public void send(String type, JsonObject payload) {
        JsonObject envelope = new JsonObject();
        envelope.addProperty("v", BridgeProtocol.VERSION);
        envelope.addProperty("type", type);
        envelope.add("payload", payload == null ? new JsonObject() : payload);
        String literal = Json.write(envelope);
        mainHandler.post(() -> webView.evaluateJavascript(
                "window.__INTIMEUI_BRIDGE__&&window.__INTIMEUI_BRIDGE__.receive("
                        + literal + ");", null));
    }

    public void sendTheme(String theme) {
        JsonObject payload = new JsonObject();
        payload.addProperty("theme", theme);
        send(BridgeProtocol.TYPE_THEME, payload);
    }
}
