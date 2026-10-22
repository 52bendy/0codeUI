package com.intimeui.client.app.ui;

import android.content.Context;
import android.content.SharedPreferences;
import android.content.res.Configuration;
import android.os.Build;
import android.view.View;
import android.view.Window;
import android.view.WindowInsetsController;

/** Persists the light/dark preference and applies it to native chrome and WebView. */
public final class ThemeController {
    private static final String PREFS = "intimeui.ui";
    private static final String KEY_DARK = "dark";
    private final Context context;
    private final SharedPreferences preferences;
    private boolean dark;

    public ThemeController(Context context) {
        this.context = context;
        this.preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE);
        this.dark = preferences.contains(KEY_DARK)
                ? preferences.getBoolean(KEY_DARK, false)
                : isSystemDark(context);
    }

    public boolean isDark() {
        return dark;
    }

    public ThemeColors colors() {
        return ThemeColors.of(context, dark);
    }

    public void toggle() {
        setDark(!dark);
    }

    public void setDark(boolean value) {
        dark = value;
        preferences.edit().putBoolean(KEY_DARK, value).apply();
    }

    /** Applies window bars and notifies the components that own themed views. */
    public void apply(Window window, AppHeader header, WebViewContainer container,
                      ConfirmationSheet sheet) {
        ThemeColors palette = ThemeColors.of(context, dark);
        window.setStatusBarColor(palette.background);
        window.setNavigationBarColor(palette.background);
        setLightBars(window, !dark);
        header.applyTheme(palette, dark);
        container.applyTheme(palette, dark);
        sheet.applyTheme(palette);
    }

    private static void setLightBars(Window window, boolean light) {
        View decor = window.getDecorView();
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            WindowInsetsController controller = window.getInsetsController();
            if (controller != null) {
                int mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                        | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
                controller.setSystemBarsAppearance(light ? mask : 0, mask);
            }
        } else {
            int flags = decor.getSystemUiVisibility();
            if (light) {
                flags |= View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
            } else {
                flags &= ~View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
            }
            decor.setSystemUiVisibility(flags);
        }
    }

    private static boolean isSystemDark(Context context) {
        int mode = context.getResources().getConfiguration().uiMode
                & Configuration.UI_MODE_NIGHT_MASK;
        return mode == Configuration.UI_MODE_NIGHT_YES;
    }

    public String themeName() {
        return dark ? "dark" : "light";
    }
}
