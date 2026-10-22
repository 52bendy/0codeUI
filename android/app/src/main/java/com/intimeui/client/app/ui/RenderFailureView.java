package com.intimeui.client.app.ui;

import android.content.Context;
import android.graphics.Typeface;
import android.view.Gravity;
import android.view.View;
import android.widget.LinearLayout;
import android.widget.TextView;

import com.intimeui.client.app.R;

/** Full-bleed fallback shown when HTML fails to load, renders blank or times out. */
public final class RenderFailureView extends LinearLayout {
    private final TextView code;
    private final TextView title;
    private final TextView detail;
    private final TextView retry;
    private Runnable retryAction;

    public RenderFailureView(Context context) {
        super(context);
        setOrientation(VERTICAL);
        setGravity(Gravity.CENTER_VERTICAL);
        setPadding(Ui.dp(context, 28), Ui.dp(context, 28), Ui.dp(context, 28), Ui.dp(context, 28));

        code = Ui.label(context, context.getString(R.string.render_failed_code), 0, 10);
        title = new TextView(context);
        title.setTextSize(24);
        title.setTypeface(Typeface.create(Ui.SERIF, Typeface.NORMAL));
        title.setPadding(0, Ui.dp(context, 14), 0, Ui.dp(context, 8));
        detail = new TextView(context);
        detail.setTextSize(13);
        detail.setLineSpacing(Ui.dp(context, 4), 1f);
        retry = Ui.button(
                context,
                context.getString(R.string.render_retry),
                ThemeColors.of(context, false),
                true);
        retry.setOnClickListener(view -> {
            if (retryAction != null) {
                retryAction.run();
            }
        });

        addView(code, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(title, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(detail, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        addView(retry, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        ((LinearLayout.LayoutParams) retry.getLayoutParams()).topMargin = Ui.dp(context, 24);
        setVisibility(View.GONE);
    }

    public void applyTheme(ThemeColors colors) {
        setBackgroundColor(colors.background);
        code.setTextColor(colors.muted);
        title.setTextColor(colors.ink);
        detail.setTextColor(colors.muted);
        retry.setTextColor(colors.accentInk);
        retry.setBackground(Ui.ripple(
                Ui.rounded(getContext(), colors.accent, 2), Ui.withAlpha(colors.ink, 0x22)));
    }

    public void show(String heading, String message, Runnable onRetry) {
        title.setText(heading);
        detail.setText(message);
        retryAction = onRetry;
        setVisibility(View.VISIBLE);
    }

    public void hide() {
        setVisibility(View.GONE);
    }
}
