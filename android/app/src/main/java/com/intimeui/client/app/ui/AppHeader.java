package com.intimeui.client.app.ui;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Typeface;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

import com.intimeui.client.app.R;

/** Editorial header with brand, scene label and the top-right theme toggle. */
@SuppressLint("ViewConstructor")
public final class AppHeader extends LinearLayout {
    private final TextView brand;
    private final TextView scene;
    private final TextView toggle;
    private final View divider;

    public AppHeader(Context context, String sceneCode, boolean dark, Runnable onToggle) {
        super(context);
        setOrientation(VERTICAL);

        LinearLayout row = new LinearLayout(context);
        row.setOrientation(HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(Ui.dp(context, 20), Ui.dp(context, 10), Ui.dp(context, 14), Ui.dp(context, 10));

        LinearLayout titles = new LinearLayout(context);
        titles.setOrientation(VERTICAL);
        brand = new TextView(context);
        brand.setText(R.string.header_brand);
        brand.setTypeface(Typeface.create(Ui.SANS_MEDIUM, Typeface.NORMAL));
        brand.setTextSize(15);
        brand.setLetterSpacing(0.18f);
        scene = new TextView(context);
        scene.setText(context.getString(R.string.header_scene, sceneCode));
        scene.setTypeface(Typeface.create(Ui.MONO, Typeface.NORMAL));
        scene.setTextSize(10);
        scene.setLetterSpacing(0.12f);
        titles.addView(brand);
        titles.addView(scene);

        toggle = new TextView(context);
        toggle.setText(R.string.theme_toggle_glyph);
        toggle.setTextSize(18);
        toggle.setGravity(Gravity.CENTER);
        toggle.setContentDescription(context.getString(R.string.theme_toggle_description));
        toggle.setClickable(true);
        toggle.setFocusable(true);
        toggle.setMinWidth(Ui.dp(context, 44));
        toggle.setMinHeight(Ui.dp(context, 44));
        toggle.setOnClickListener(view -> onToggle.run());

        row.addView(titles, new LinearLayout.LayoutParams(0,
                LinearLayout.LayoutParams.WRAP_CONTENT, 1f));
        row.addView(toggle, new LinearLayout.LayoutParams(
                Ui.dp(context, 44), Ui.dp(context, 44)));

        divider = new View(context);
        addView(row, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, Ui.dp(context, 64)));
        addView(divider, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, Math.max(1, Ui.dp(context, 0.7f))));
        applyTheme(ThemeColors.of(context, dark), dark);
    }

    public void applyTheme(ThemeColors colors, boolean dark) {
        setBackgroundColor(colors.background);
        brand.setTextColor(colors.ink);
        scene.setTextColor(colors.muted);
        toggle.setTextColor(colors.ink);
        toggle.setBackground(Ui.ripple(
                Ui.rounded(getContext(), colors.surface, 22), Ui.withAlpha(colors.ink, 0x22)));
        divider.setBackgroundColor(colors.line);
    }
}
