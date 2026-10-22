package com.intimeui.client.core.bridge;

/** Message types shared by the injected JS SDK and the native bridge. */
public final class BridgeProtocol {
    public static final int VERSION = 1;
    public static final int MAX_MESSAGE_BYTES = 64 * 1024;

    public static final String TYPE_INTENT_TRIGGER = "INTENT_TRIGGER";
    public static final String TYPE_TELEMETRY_SIGNAL = "TELEMETRY_SIGNAL";
    public static final String TYPE_RENDER_READY = "RENDER_READY";
    public static final String TYPE_RENDER_ERROR = "RENDER_ERROR";
    public static final String TYPE_PAGE_HIDE = "PAGE_HIDE";
    public static final String TYPE_SET_THEME = "SET_THEME";

    public static final String TYPE_ACK = "ACK";
    public static final String TYPE_ERROR = "ERROR";
    public static final String TYPE_INTENT_RESULT = "INTENT_RESULT";
    public static final String TYPE_INTENT_ERROR = "INTENT_ERROR";
    public static final String TYPE_CONFIRMATION_CARD = "CONFIRMATION_CARD";
    public static final String TYPE_EXECUTION_RECEIPT = "EXECUTION_RECEIPT";
    public static final String TYPE_THEME = "THEME";

    private BridgeProtocol() {
    }
}
