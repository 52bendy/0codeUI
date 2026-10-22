package com.intimeui.client.app.ui;

import android.content.Context;
import android.content.res.ColorStateList;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

/** Small view factory with the app's typography and stateful shapes. */
public final class Ui {
    public static final String SANS = "sans-serif";
    public static final String SANS_MEDIUM = "sans-serif-medium";
    public static final String MONO = "monospace";
    public static final String SERIF = "serif";

    private Ui() {
    }

    public static int dp(Context context, float value) {
        return Math.round(TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP, value, context.getResources().getDisplayMetrics()));
    }

    public static Drawable rounded(Context context, int color, float radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(color);
        drawable.setCornerRadius(dp(context, radiusDp));
        return drawable;
    }

    public static Drawable outlined(Context context, int fill, int stroke, float radiusDp) {
        GradientDrawable drawable = new GradientDrawable();
        drawable.setColor(fill);
        drawable.setCornerRadius(dp(context, radiusDp));
        drawable.setStroke(dp(context, 1), stroke);
        return drawable;
    }

    public static Drawable ripple(Drawable content, int rippleColor) {
        return new RippleDrawable(
                ColorStateList.valueOf(rippleColor), content, null);
    }

    public static TextView label(Context context, String text, int color, float sizeSp) {
        TextView view = new TextView(context);
        view.setText(text);
        view.setTextColor(color);
        view.setTextSize(sizeSp);
        view.setTypeface(Typeface.create(MONO, Typeface.NORMAL));
        view.setLetterSpacing(0.12f);
        return view;
    }

    public static TextView button(
            Context context, String text, ThemeColors colors, boolean primary) {
        TextView view = new TextView(context);
        view.setText(text);
        view.setTextSize(15);
        view.setTypeface(Typeface.create(SANS_MEDIUM, Typeface.NORMAL));
        view.setGravity(Gravity.CENTER);
        view.setMinHeight(dp(context, 48));
        view.setPadding(dp(context, 20), dp(context, 12), dp(context, 20), dp(context, 12));
        int fill = primary ? colors.accent : colors.surface;
        int stroke = primary ? colors.accent : colors.line;
        int textColor = primary ? colors.accentInk : colors.ink;
        view.setTextColor(textColor);
        view.setBackground(ripple(
                outlined(context, fill, stroke, 2), withAlpha(colors.ink, 0x22)));
        view.setClickable(true);
        view.setFocusable(true);
        return view;
    }

    public static View divider(Context context, ThemeColors colors) {
        View line = new View(context);
        line.setBackgroundColor(colors.line);
        line.setLayoutParams(new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, Math.max(1, dp(context, 0.7f))));
        return line;
    }

    public static int withAlpha(int color, int alpha) {
        return (color & 0x00ffffff) | ((alpha & 0xff) << 24);
    }
}
