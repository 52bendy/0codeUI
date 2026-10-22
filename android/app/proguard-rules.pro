# Keep the JavaScript interface names: @JavascriptInterface methods are resolved by name.
-keepclassmembers class com.intimeui.client.app.web.WebViewBridge {
    public *;
}
-keepattributes JavascriptInterface
