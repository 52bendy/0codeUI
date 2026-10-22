package com.intimeui.client.app;

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.DisplayMetrics;
import android.view.View;
import android.widget.FrameLayout;

import com.google.gson.JsonObject;
import com.intimeui.client.app.config.ApiClientFactory;
import com.intimeui.client.app.config.ClientConfig;
import com.intimeui.client.app.intent.IntentRepository;
import com.intimeui.client.app.telemetry.SharedPreferencesTelemetryStore;
import com.intimeui.client.app.telemetry.WebViewTelemetryTracker;
import com.intimeui.client.app.ui.AppHeader;
import com.intimeui.client.app.ui.ConfirmationSheet;
import com.intimeui.client.app.ui.ThemeColors;
import com.intimeui.client.app.ui.ThemeController;
import com.intimeui.client.app.ui.UiErrors;
import com.intimeui.client.app.ui.WebViewContainer;
import com.intimeui.client.app.web.WebViewBridge;
import com.intimeui.client.core.api.ApiClient;
import com.intimeui.client.core.api.ApiException;
import com.intimeui.client.core.bridge.BridgeProtocol;
import com.intimeui.client.core.bridge.BridgeRouter;
import com.intimeui.client.core.intent.ConfirmationCard;
import com.intimeui.client.core.intent.ConfirmationDecision;
import com.intimeui.client.core.intent.IntentRequest;
import com.intimeui.client.core.intent.IntentResult;
import com.intimeui.client.core.json.Json;
import com.intimeui.client.core.replay.ServerClock;
import com.intimeui.client.core.telemetry.TelemetryQueue;
import com.intimeui.client.core.telemetry.TelemetryUploader;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

/** Single-screen WebView host wiring rendering, bridge, telemetry and confirmation flow. */
public final class MainActivity extends Activity {
    private final Handler mainHandler = new Handler(Looper.getMainLooper());
    private final ExecutorService ioExecutor = Executors.newSingleThreadExecutor();
    private ClientConfig config;
    private ThemeController theme;
    private AppHeader header;
    private WebViewContainer container;
    private ConfirmationSheet sheet;
    private WebViewBridge bridge;
    private ApiClient apiClient;
    private IntentRepository intentRepository;
    private WebViewTelemetryTracker telemetry;
    private volatile String sessionId;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        config = ClientConfig.fromBuildConfig();
        theme = new ThemeController(this);
        buildViews();
        buildServices();
        buildBridge();
        applyTheme();
        loadContent();
    }

    private void buildViews() {
        FrameLayout root = new FrameLayout(this);
        android.widget.LinearLayout column = new android.widget.LinearLayout(this);
        column.setOrientation(android.widget.LinearLayout.VERTICAL);

        header = new AppHeader(this, config.sceneCode(), theme.isDark(), () -> {
            theme.toggle();
            applyTheme();
        });
        container = new WebViewContainer(this);
        column.addView(header, new android.widget.LinearLayout.LayoutParams(
                android.widget.LinearLayout.LayoutParams.MATCH_PARENT,
                android.widget.LinearLayout.LayoutParams.WRAP_CONTENT));
        column.addView(container, new android.widget.LinearLayout.LayoutParams(
                android.widget.LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f));
        root.addView(column, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));

        sheet = new ConfirmationSheet(this);
        root.addView(sheet, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
        setContentView(root);
        container.setRetryListener(this::loadContent);
    }

    private void buildServices() {
        apiClient = ApiClientFactory.create(config, new ServerClock());
        intentRepository = new IntentRepository(apiClient);
        TelemetryQueue queue = new TelemetryQueue(
                new SharedPreferencesTelemetryStore(this), 500);
        TelemetryUploader uploader = new TelemetryUploader(
                queue, apiClient::reportTelemetry, 20);
        telemetry = new WebViewTelemetryTracker(mainHandler, uploader, ioExecutor);
        sheet.setVisibility(View.GONE);
    }

    private void buildBridge() {
        BridgeRouter router = new BridgeRouter()
                .register(BridgeProtocol.TYPE_RENDER_READY, payload -> {
                    String page = Json.optionalString(payload, "page");
                    mainHandler.post(() -> {
                        telemetry.start(page == null ? config.sceneCode() : page);
                        container.markReady();
                    });
                    return ack();
                })
                .register(BridgeProtocol.TYPE_TELEMETRY_SIGNAL, payload -> {
                    mainHandler.post(() -> telemetry.onSignal(payload));
                    return ack();
                })
                .register(BridgeProtocol.TYPE_PAGE_HIDE, payload -> {
                    mainHandler.post(() -> telemetry.onSignal(payload));
                    return ack();
                })
                .register(BridgeProtocol.TYPE_SET_THEME, payload -> {
                    String requested = Json.optionalString(payload, "theme");
                    mainHandler.post(() -> {
                        theme.setDark("dark".equals(requested));
                        applyTheme();
                    });
                    return ack();
                })
                .register(BridgeProtocol.TYPE_RENDER_ERROR, payload -> {
                    mainHandler.post(() -> container.showFailure(
                            getString(R.string.render_failed_title),
                            getString(R.string.render_failed_detail)));
                    return ack();
                })
                .register(BridgeProtocol.TYPE_INTENT_TRIGGER, payload -> {
                    mainHandler.post(() -> triggerIntent(payload));
                    return ack();
                });
        bridge = new WebViewBridge(container.webView(), mainHandler, router);
        container.setBridge(bridge);
    }

    private static JsonObject ack() {
        JsonObject payload = new JsonObject();
        payload.addProperty("accepted", true);
        return payload;
    }

    private void applyTheme() {
        ThemeColors colors = theme.colors();
        theme.apply(getWindow(), header, container, sheet);
        getWindow().getDecorView().setBackgroundColor(colors.background);
    }

    private void loadContent() {
        if (config.demoMode()) {
            String html = readAsset(config.demoAsset());
            if (html == null) {
                container.showFailure(
                        getString(R.string.render_failed_title),
                        getString(R.string.demo_asset_missing));
                return;
            }
            container.render(html, "demo:" + config.sceneCode() + ":v1");
            return;
        }
        if (config.replaySecret() == null || config.replaySecret().isEmpty()) {
            container.showFailure(
                    getString(R.string.render_failed_title),
                    getString(R.string.replay_secret_missing));
            return;
        }
        container.showLoading();
        ioExecutor.execute(() -> {
            try {
                apiClient.syncClock();
                com.intimeui.client.core.intent.UiRenderResult result = apiClient.generateUi(
                        config.userId(), config.sceneCode(), deviceInfo(), demoBusinessData());
                mainHandler.post(() -> container.render(result.htmlContent(), result.version()));
            } catch (ApiException error) {
                mainHandler.post(() -> container.showFailure(
                        getString(R.string.render_failed_title),
                        UiErrors.describe(MainActivity.this, error)));
            }
        });
    }

    private JsonObject deviceInfo() {
        DisplayMetrics metrics = getResources().getDisplayMetrics();
        float density = metrics.density <= 0 ? 1f : metrics.density;
        JsonObject info = new JsonObject();
        info.addProperty("screen_width", Math.round(metrics.widthPixels / density));
        info.addProperty("screen_height", Math.round(metrics.heightPixels / density));
        info.addProperty("platform", "android");
        return info;
    }

    private JsonObject demoBusinessData() {
        JsonObject item = new JsonObject();
        item.addProperty("name", "智能降噪耳机");
        item.addProperty("price", 299.00);
        item.addProperty("qty", 1);
        com.google.gson.JsonArray items = new com.google.gson.JsonArray();
        items.add(item);
        JsonObject data = new JsonObject();
        data.addProperty("order_id", "ORD_20261014_001");
        data.addProperty("status", "PAID");
        data.add("items", items);
        return data;
    }

    private void triggerIntent(JsonObject payload) {
        final String action;
        try {
            action = Json.requireString(payload, "action");
        } catch (RuntimeException error) {
            sendIntentError("BAD_PAYLOAD", getString(R.string.error_generic));
            return;
        }
        JsonObject data = payload.has("data") && payload.get("data").isJsonObject()
                ? payload.getAsJsonObject("data") : new JsonObject();
        JsonObject target = payload.has("target") && payload.get("target").isJsonObject()
                ? payload.getAsJsonObject("target") : new JsonObject();
        String targetId = Json.optionalString(target, "id");
        String page = Json.optionalString(payload, "page");
        String entity = Json.optionalString(payload, "entity");
        IntentRequest request = IntentRequest.uiInteraction(
                config.userId(),
                sessionId,
                page == null ? config.sceneCode() : page,
                entity == null ? "" : entity,
                targetId == null ? action : targetId,
                action,
                data);

        ioExecutor.execute(() -> {
            try {
                IntentResult result = intentRepository.execute(request);
                if (result.sessionId() != null) {
                    sessionId = result.sessionId();
                }
                mainHandler.post(() -> onIntentResult(result));
            } catch (ApiException error) {
                mainHandler.post(() -> sendIntentError(
                        String.valueOf(error.code()), UiErrors.describe(this, error)));
            }
        });
    }

    private void onIntentResult(IntentResult result) {
        if (result.requiresConfirmation()) {
            ConfirmationCard card = result.confirmation();
            sheet.show(card, sheetListener());
            bridge.send(BridgeProtocol.TYPE_CONFIRMATION_CARD, card.toDisplayJson());
            return;
        }
        bridge.send(BridgeProtocol.TYPE_INTENT_RESULT, result.toJson());
    }

    private ConfirmationSheet.Listener sheetListener() {
        return new ConfirmationSheet.Listener() {
            @Override
            public void onConfirm(ConfirmationCard card) {
                confirmCard(card);
            }

            @Override
            public void onReject(ConfirmationCard card) {
                rejectCard(card);
            }
        };
    }

    private void confirmCard(ConfirmationCard card) {
        sheet.showProgress();
        ioExecutor.execute(() -> {
            try {
                ConfirmationDecision decision = intentRepository.confirm(card, sessionId);
                mainHandler.post(() -> {
                    sheet.showReceipt(decision);
                    bridge.send(BridgeProtocol.TYPE_EXECUTION_RECEIPT, decision.toReceiptJson());
                });
            } catch (ApiException error) {
                mainHandler.post(() -> sheet.showError(UiErrors.describe(this, error)));
            }
        });
    }

    private void rejectCard(ConfirmationCard card) {
        sheet.showProgress();
        ioExecutor.execute(() -> {
            try {
                ConfirmationDecision decision = intentRepository.reject(
                        card, getString(R.string.confirm_reject_reason));
                mainHandler.post(() -> {
                    sheet.showReceipt(decision);
                    bridge.send(BridgeProtocol.TYPE_EXECUTION_RECEIPT, decision.toReceiptJson());
                });
            } catch (ApiException error) {
                mainHandler.post(() -> sheet.showError(UiErrors.describe(this, error)));
            }
        });
    }

    private void sendIntentError(String code, String message) {
        JsonObject payload = new JsonObject();
        payload.addProperty("code", code);
        payload.addProperty("message", message);
        bridge.send(BridgeProtocol.TYPE_INTENT_ERROR, payload);
    }

    private String readAsset(String name) {
        try (InputStream input = getAssets().open(name);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192];
            int read;
            while ((read = input.read(buffer)) != -1) {
                output.write(buffer, 0, read);
            }
            return output.toString(StandardCharsets.UTF_8.name());
        } catch (IOException error) {
            return null;
        }
    }

    @Override
    protected void onPause() {
        telemetry.flush();
        container.onPause();
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        container.onResume();
    }

    @Override
    public void onBackPressed() {
        if (sheet.isShowing()) {
            sheet.dismiss();
            return;
        }
        super.onBackPressed();
    }

    @Override
    protected void onDestroy() {
        telemetry.stop();
        ioExecutor.shutdownNow();
        container.destroy();
        super.onDestroy();
    }
}
