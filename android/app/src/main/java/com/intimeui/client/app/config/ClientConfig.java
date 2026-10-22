package com.intimeui.client.app.config;

import com.intimeui.client.app.BuildConfig;

/** Runtime configuration injected at build time; no production secret is committed. */
public final class ClientConfig {
    private final String apiBaseUrl;
    private final String authToken;
    private final String userId;
    private final String replaySecret;
    private final boolean demoMode;
    private final String sceneCode;
    private final String demoAsset;

    private ClientConfig(
            String apiBaseUrl,
            String authToken,
            String userId,
            String replaySecret,
            boolean demoMode,
            String sceneCode,
            String demoAsset) {
        this.apiBaseUrl = apiBaseUrl;
        this.authToken = authToken;
        this.userId = userId;
        this.replaySecret = replaySecret;
        this.demoMode = demoMode;
        this.sceneCode = sceneCode;
        this.demoAsset = demoAsset;
    }

    public static ClientConfig fromBuildConfig() {
        return new ClientConfig(
                BuildConfig.API_BASE_URL,
                BuildConfig.AUTH_TOKEN,
                BuildConfig.AUTH_USER_ID,
                BuildConfig.REPLAY_SECRET,
                BuildConfig.DEMO_MODE,
                BuildConfig.SCENE_CODE,
                BuildConfig.DEMO_ASSET);
    }

    public String apiBaseUrl() {
        return apiBaseUrl;
    }

    public String authToken() {
        return authToken;
    }

    public String userId() {
        return userId;
    }

    public String replaySecret() {
        return replaySecret;
    }

    public boolean demoMode() {
        return demoMode;
    }

    public String sceneCode() {
        return sceneCode;
    }

    public String demoAsset() {
        return demoAsset;
    }
}
