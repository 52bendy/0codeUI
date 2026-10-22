package com.intimeui.client.core.bridge;

import com.google.gson.JsonObject;

/** Handles one bridge message type. Implementations must be fast and non-blocking. */
public interface BridgeHandler {
    JsonObject handle(JsonObject payload) throws Exception;
}
