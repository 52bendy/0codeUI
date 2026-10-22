package com.intimeui.client.core.bridge;

/** Protocol-level bridge failure with a stable machine-readable code. */
public final class BridgeException extends Exception {
    private final String code;

    public BridgeException(String code, String message) {
        super(message);
        this.code = code;
    }

    public String code() {
        return code;
    }
}
