package com.intimeui.client.app.ui;

import android.annotation.SuppressLint;
import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Color;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.View;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.ValueCallback;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.ProgressBar;

import com.intimeui.client.app.BuildConfig;
import com.intimeui.client.app.R;
import com.intimeui.client.app.web.JsSdk;
import com.intimeui.client.app.web.WebViewBridge;

/** Reusable WebView host with loading, blank detection, timeout and retry fallback. */
public final class WebViewContainer extends FrameLayout {
    public static final String BASE_URL = "https://intimeui.local/";
    private static final long RENDER_TIMEOUT_MS = 12_000L;
    private static final long BLANK_CHECK_DELAY_MS = 450L;

    private WebView webView;
    private final ProgressBar progress;
    private final RenderFailureView failureView;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private WebViewBridge bridge;
    private Runnable retryListener;
    private String currentHtml;
    private String currentTheme = "light";
    private boolean ready;
    private final Runnable timeout = () -> {
        if (!ready) {
            showFailure(
                    getContext().getString(R.string.render_timeout_title),
                    getContext().getString(R.string.render_timeout_detail));
        }
    };

    @SuppressLint("SetJavaScriptEnabled")
    public WebViewContainer(Context context) {
        super(context);
        setBackgroundColor(Color.TRANSPARENT);
        WebView.setWebContentsDebuggingEnabled(BuildConfig.DEBUG);
        webView = createWebView();
        addView(webView, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));

        progress = new ProgressBar(context, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        progress.setVisibility(View.GONE);
        FrameLayout.LayoutParams progressParams = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, Ui.dp(context, 2));
        progressParams.gravity = android.view.Gravity.TOP;
        addView(progress, progressParams);

        failureView = new RenderFailureView(context);
        addView(failureView, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
    }

    @SuppressLint("SetJavaScriptEnabled")
    private static void configure(WebSettings settings) {
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setGeolocationEnabled(false);
        settings.setSupportZoom(false);
        settings.setBuiltInZoomControls(false);
        settings.setDisplayZoomControls(false);
        settings.setMediaPlaybackRequiresUserGesture(true);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        settings.setCacheMode(WebSettings.LOAD_NO_CACHE);
        settings.setSafeBrowsingEnabled(true);
    }

    private WebView createWebView() {
        WebView view = new WebView(getContext());
        view.setBackgroundColor(Color.TRANSPARENT);
        configure(view.getSettings());
        view.setWebViewClient(new ContainerClient());
        view.setWebChromeClient(new ContainerChromeClient());
        return view;
    }

    private void replaceWebView() {
        WebView old = webView;
        if (old != null) {
            old.removeJavascriptInterface(WebViewBridge.JS_INTERFACE);
            removeView(old);
            old.stopLoading();
            old.destroy();
        }
        webView = createWebView();
        addView(webView, 0, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
        if (bridge != null) {
            webView.addJavascriptInterface(bridge, WebViewBridge.JS_INTERFACE);
            bridge.attach(webView);
        }
    }

    public void setBridge(WebViewBridge bridge) {
        this.bridge = bridge;
        bridge.attach(webView);
        webView.addJavascriptInterface(bridge, WebViewBridge.JS_INTERFACE);
    }

    public WebView webView() {
        return webView;
    }

    public void setRetryListener(Runnable onRetry) {
        this.retryListener = onRetry;
    }

    public void showLoading() {
        failureView.hide();
        progress.setVisibility(View.VISIBLE);
    }

    public void render(String html, String version) {
        currentHtml = html;
        ready = false;
        if (bridge != null) {
            bridge.setAllowed(true);
        }
        failureView.hide();
        progress.setVisibility(View.VISIBLE);
        handler.removeCallbacks(timeout);
        handler.postDelayed(timeout, RENDER_TIMEOUT_MS);
        webView.loadDataWithBaseURL(BASE_URL, html, "text/html", "UTF-8", null);
    }

    public void applyTheme(ThemeColors colors, boolean dark) {
        currentTheme = dark ? "dark" : "light";
        setBackgroundColor(colors.background);
        webView.setBackgroundColor(colors.background);
        failureView.applyTheme(colors);
        progress.setProgressTintList(ColorStateList.valueOf(colors.ink));
        if (bridge != null) {
            bridge.sendTheme(currentTheme);
        }
    }

    public void markReady() {
        ready = true;
        handler.removeCallbacks(timeout);
        progress.setVisibility(View.GONE);
        failureView.hide();
    }

    public void showFailure(String title, String detail) {
        ready = false;
        handler.removeCallbacks(timeout);
        progress.setVisibility(View.GONE);
        failureView.show(title, detail, () -> {
            if (retryListener != null) {
                retryListener.run();
            } else if (currentHtml != null) {
                render(currentHtml, null);
            }
        });
    }

    public void evaluate(String javascript) {
        webView.evaluateJavascript(javascript, null);
    }

    public void onPause() {
        webView.onPause();
    }

    public void onResume() {
        webView.onResume();
    }

    public void destroy() {
        handler.removeCallbacksAndMessages(null);
        if (bridge != null) {
            bridge.setAllowed(false);
        }
        webView.removeJavascriptInterface(WebViewBridge.JS_INTERFACE);
        webView.stopLoading();
        webView.destroy();
    }

    private void injectSdk() {
        if (bridge == null) {
            return;
        }
        String sdk = JsSdk.load(getContext());
        webView.evaluateJavascript(sdk, null);
    }

    private void checkContent() {
        if (!ready) {
            webView.evaluateJavascript(
                    "(function(){var b=document.body;if(!b){return 'empty';}"
                            + "var t=(b.innerText||'').trim();"
                            + "var v=b.querySelector('img,svg,canvas,video,button,input');"
                            + "return (t.length>0||v)?'ok':'blank';})()",
                    new ValueCallback<String>() {
                        @Override
                        public void onReceiveValue(String value) {
                            if (value != null && value.contains("ok")) {
                                markReady();
                            } else if (value != null && (value.contains("blank") || value.contains("empty"))) {
                                showFailure(
                                        getContext().getString(R.string.render_blank_title),
                                        getContext().getString(R.string.render_blank_detail));
                            }
                        }
                    });
        }
    }

    private final class ContainerClient extends WebViewClient {
        @Override
        public void onPageStarted(WebView view, String url, android.graphics.Bitmap favicon) {
            injectSdk();
        }

        @Override
        public void onPageFinished(WebView view, String url) {
            injectSdk();
            if (bridge != null) {
                bridge.sendTheme(currentTheme);
            }
            handler.postDelayed(WebViewContainer.this::checkContent, BLANK_CHECK_DELAY_MS);
        }

        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            String url = request.getUrl().toString();
            return !(url.startsWith(BASE_URL) || url.startsWith("about:blank"));
        }

        @Override
        public void onReceivedError(
                WebView view, WebResourceRequest request, WebResourceError error) {
            if (request.isForMainFrame()) {
                showFailure(
                        getContext().getString(R.string.render_failed_title),
                        getContext().getString(R.string.render_failed_detail));
            }
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            replaceWebView();
            showFailure(
                    getContext().getString(R.string.render_crashed_title),
                    getContext().getString(R.string.render_crashed_detail));
            return true;
        }
    }

    private final class ContainerChromeClient extends WebChromeClient {
        @Override
        public void onProgressChanged(WebView view, int newProgress) {
            progress.setProgress(newProgress);
            progress.setVisibility(newProgress >= 100 ? View.GONE : View.VISIBLE);
        }
    }
}
