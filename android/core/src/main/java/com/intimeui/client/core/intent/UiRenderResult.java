package com.intimeui.client.core.intent;

import com.google.gson.JsonObject;
import com.intimeui.client.core.json.Json;

/** Parsed {@code data} of {@code POST /api/v1/ui/generate}. */
public final class UiRenderResult {
    private final String renderType;
    private final String version;
    private final String htmlContent;

    private UiRenderResult(String renderType, String version, String htmlContent) {
        this.renderType = renderType;
        this.version = version;
        this.htmlContent = htmlContent;
    }

    public static UiRenderResult fromJson(JsonObject data) {
        return new UiRenderResult(
                Json.requireString(data, "render_type"),
                Json.optionalString(data, "version"),
                Json.requireString(data, "html_content"));
    }

    public String renderType() {
        return renderType;
    }

    public String version() {
        return version;
    }

    public String htmlContent() {
        return htmlContent;
    }
}
