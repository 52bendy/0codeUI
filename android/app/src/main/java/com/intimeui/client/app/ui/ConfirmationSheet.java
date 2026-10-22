package com.intimeui.client.app.ui;

import android.content.Context;
import android.graphics.Typeface;
import android.os.Handler;
import android.os.Looper;
import android.os.SystemClock;
import android.view.Gravity;
import android.view.View;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.TextView;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.intimeui.client.app.R;
import com.intimeui.client.core.intent.ConfirmationCard;
import com.intimeui.client.core.intent.ConfirmationDecision;
import com.intimeui.client.core.json.Json;

import java.util.Map;

/** Bottom confirmation sheet for WRITE/DELETE intents, including execution receipt. */
public final class ConfirmationSheet extends FrameLayout {
    public interface Listener {
        void onConfirm(ConfirmationCard card);

        void onReject(ConfirmationCard card);
    }

    private final View scrim;
    private final LinearLayout sheet;
    private final TextView title;
    private final TextView riskBadge;
    private final TextView summary;
    private final TextView prompt;
    private final LinearLayout params;
    private final TextView countdown;
    private final TextView status;
    private final LinearLayout actions;
    private final TextView cancel;
    private final TextView confirm;
    private final LinearLayout receipt;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private ThemeColors colors;
    private ConfirmationCard card;
    private Listener listener;
    private long expiresAtElapsed;

    private final Runnable tick = new Runnable() {
        @Override
        public void run() {
            if (card == null || getVisibility() != View.VISIBLE) {
                return;
            }
            long remaining = Math.max(0L, (expiresAtElapsed - SystemClock.elapsedRealtime()) / 1000);
            countdown.setText(getContext().getString(R.string.confirm_expiry, remaining / 60, remaining % 60));
            if (remaining == 0) {
                confirm.setEnabled(false);
                confirm.setAlpha(0.4f);
                countdown.setTextColor(colors.danger);
            }
            handler.postDelayed(this, 1000L);
        }
    };

    public ConfirmationSheet(Context context) {
        super(context);
        setVisibility(View.GONE);
        scrim = new View(context);
        scrim.setClickable(true);
        addView(scrim, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));

        sheet = new LinearLayout(context);
        sheet.setOrientation(LinearLayout.VERTICAL);
        sheet.setPadding(Ui.dp(context, 24), Ui.dp(context, 10), Ui.dp(context, 24), Ui.dp(context, 24));
        sheet.setTranslationY(Ui.dp(context, 36));
        FrameLayout.LayoutParams sheetParams = new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.WRAP_CONTENT);
        sheetParams.gravity = Gravity.BOTTOM;
        addView(sheet, sheetParams);

        View handle = new View(context);
        LinearLayout.LayoutParams handleParams = new LinearLayout.LayoutParams(
                Ui.dp(context, 36), Ui.dp(context, 3));
        handleParams.gravity = Gravity.CENTER_HORIZONTAL;
        handleParams.bottomMargin = Ui.dp(context, 22);
        sheet.addView(handle, handleParams);

        riskBadge = Ui.label(context, "", 0, 10);
        riskBadge.setPadding(Ui.dp(context, 8), Ui.dp(context, 4), Ui.dp(context, 8), Ui.dp(context, 4));
        sheet.addView(riskBadge, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT, LinearLayout.LayoutParams.WRAP_CONTENT));

        title = new TextView(context);
        title.setText(R.string.confirm_title);
        title.setTextSize(22);
        title.setTypeface(Typeface.create(Ui.SERIF, Typeface.NORMAL));
        title.setPadding(0, Ui.dp(context, 14), 0, Ui.dp(context, 6));
        sheet.addView(title);

        summary = new TextView(context);
        summary.setTextSize(14);
        summary.setTypeface(Typeface.create(Ui.SANS_MEDIUM, Typeface.NORMAL));
        sheet.addView(summary);

        prompt = new TextView(context);
        prompt.setTextSize(14);
        prompt.setLineSpacing(Ui.dp(context, 5), 1f);
        prompt.setPadding(0, Ui.dp(context, 10), 0, Ui.dp(context, 14));
        sheet.addView(prompt);

        params = new LinearLayout(context);
        params.setOrientation(LinearLayout.VERTICAL);
        sheet.addView(params);

        countdown = Ui.label(context, "", 0, 10);
        countdown.setPadding(0, Ui.dp(context, 14), 0, Ui.dp(context, 4));
        sheet.addView(countdown);

        status = new TextView(context);
        status.setTextSize(13);
        status.setVisibility(View.GONE);
        status.setPadding(0, Ui.dp(context, 8), 0, 0);
        sheet.addView(status);

        actions = new LinearLayout(context);
        actions.setOrientation(LinearLayout.HORIZONTAL);
        actions.setPadding(0, Ui.dp(context, 18), 0, 0);
        cancel = Ui.button(context, context.getString(R.string.confirm_cancel),
                ThemeColors.of(context, false), false);
        confirm = Ui.button(context, context.getString(R.string.confirm_execute),
                ThemeColors.of(context, false), true);
        LinearLayout.LayoutParams half = new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
        actions.addView(cancel, half);
        LinearLayout.LayoutParams confirmParams = new LinearLayout.LayoutParams(
                0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f);
        confirmParams.leftMargin = Ui.dp(context, 10);
        actions.addView(confirm, confirmParams);
        sheet.addView(actions);

        receipt = new LinearLayout(context);
        receipt.setOrientation(LinearLayout.VERTICAL);
        receipt.setVisibility(View.GONE);
        receipt.setPadding(0, Ui.dp(context, 12), 0, 0);
        sheet.addView(receipt);

        cancel.setOnClickListener(view -> {
            if (listener != null && card != null) {
                listener.onReject(card);
            }
        });
        confirm.setOnClickListener(view -> {
            if (listener != null && card != null) {
                listener.onConfirm(card);
            }
        });
        applyTheme(ThemeColors.of(context, false));
    }

    public void applyTheme(ThemeColors palette) {
        this.colors = palette;
        setBackgroundColor(0);
        scrim.setBackgroundColor(Ui.withAlpha(palette.scrim, 0x99));
        sheet.setBackground(Ui.rounded(getContext(), palette.surface, 18));
        handleBackground(palette);
        title.setTextColor(palette.ink);
        summary.setTextColor(palette.ink);
        prompt.setTextColor(palette.muted);
        countdown.setTextColor(palette.muted);
        status.setTextColor(palette.muted);
        cancel.setTextColor(palette.ink);
        cancel.setBackground(Ui.ripple(
                Ui.outlined(getContext(), palette.surface, palette.line, 2),
                Ui.withAlpha(palette.ink, 0x22)));
        confirm.setTextColor(palette.accentInk);
        confirm.setBackground(Ui.ripple(
                Ui.rounded(getContext(), palette.accent, 2),
                Ui.withAlpha(palette.ink, 0x22)));
        if (card != null) {
            bindRisk(card.riskLevel());
        }
    }

    private void handleBackground(ThemeColors palette) {
        View handle = sheet.getChildAt(0);
        handle.setBackground(Ui.rounded(getContext(), palette.line, 2));
    }

    public void show(ConfirmationCard card, Listener listener) {
        this.card = card;
        this.listener = listener;
        bindRisk(card.riskLevel());
        summary.setText(card.intentSummary() == null
                ? getContext().getString(R.string.confirm_untitled)
                : card.intentSummary());
        prompt.setText(card.confirmationPrompt() == null ? "" : card.confirmationPrompt());
        bindParams(card.proposedChanges());
        long seconds = Math.max(0L, card.expiresInSecs());
        expiresAtElapsed = SystemClock.elapsedRealtime() + seconds * 1000L;
        countdown.setVisibility(seconds > 0 ? View.VISIBLE : View.GONE);
        status.setVisibility(View.GONE);
        receipt.setVisibility(View.GONE);
        receipt.removeAllViews();
        actions.setVisibility(View.VISIBLE);
        setEnabledState(true);
        setVisibility(View.VISIBLE);
        scrim.setAlpha(0f);
        sheet.setAlpha(0f);
        sheet.animate().translationY(0f).alpha(1f).setDuration(240L).start();
        scrim.animate().alpha(1f).setDuration(200L).start();
        handler.removeCallbacks(tick);
        handler.post(tick);
    }

    public void showProgress() {
        status.setText(R.string.confirm_processing);
        status.setTextColor(colors.muted);
        status.setVisibility(View.VISIBLE);
        setEnabledState(false);
    }

    public void showError(String message) {
        status.setText(message);
        status.setTextColor(colors.danger);
        status.setVisibility(View.VISIBLE);
        setEnabledState(true);
    }

    public void showReceipt(ConfirmationDecision decision) {
        actions.setVisibility(View.GONE);
        status.setVisibility(View.GONE);
        receipt.setVisibility(View.VISIBLE);
        receipt.removeAllViews();
        TextView heading = new TextView(getContext());
        heading.setText(getContext().getString(
                R.string.confirm_receipt_title, decision.status()));
        heading.setTextSize(16);
        heading.setTypeface(Typeface.create(Ui.SANS_MEDIUM, Typeface.NORMAL));
        heading.setTextColor(colors.ink);
        receipt.addView(heading);

        JsonArray results = decision.results();
        if (results.size() == 0) {
            TextView empty = new TextView(getContext());
            empty.setText(R.string.confirm_receipt_empty);
            empty.setTextColor(colors.muted);
            empty.setTextSize(13);
            empty.setPadding(0, Ui.dp(getContext(), 8), 0, 0);
            receipt.addView(empty);
        }
        for (JsonElement element : results) {
            if (element.isJsonObject()) {
                receipt.addView(resultRow(element.getAsJsonObject()));
            }
        }
        TextView close = Ui.button(getContext(), getContext().getString(R.string.confirm_close),
                colors, false);
        close.setOnClickListener(view -> dismiss());
        LinearLayout.LayoutParams closeParams = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT);
        closeParams.topMargin = Ui.dp(getContext(), 18);
        receipt.addView(close, closeParams);
    }

    private TextView resultRow(JsonObject result) {
        TextView row = new TextView(getContext());
        String capability = Json.optionalString(result, "target_capability");
        String statusText = Json.optionalString(result, "status");
        row.setText(getContext().getString(
                R.string.confirm_receipt_row,
                capability == null ? getContext().getString(R.string.confirm_receipt_action) : capability,
                statusText == null ? getContext().getString(R.string.confirm_receipt_unknown) : statusText));
        row.setTextSize(13);
        row.setTypeface(Typeface.create(Ui.MONO, Typeface.NORMAL));
        row.setTextColor(colors.ink);
        row.setPadding(0, Ui.dp(getContext(), 8), 0, 0);
        return row;
    }

    private void bindRisk(String risk) {
        String normalized = risk == null ? "WRITE" : risk.toUpperCase(java.util.Locale.ROOT);
        int label = "DELETE".equals(normalized)
                ? R.string.confirm_risk_delete
                : R.string.confirm_risk_write;
        riskBadge.setText(label);
        int color = "DELETE".equals(normalized) ? colors.danger : colors.muted;
        riskBadge.setTextColor(color);
        riskBadge.setBackground(Ui.outlined(
                getContext(), colors.surface, color, 2));
    }

    private void bindParams(JsonObject changes) {
        params.removeAllViews();
        int shown = 0;
        for (Map.Entry<String, JsonElement> entry : changes.entrySet()) {
            if (shown >= 6) {
                break;
            }
            TextView row = new TextView(getContext());
            row.setText(getContext().getString(
                    R.string.confirm_param_row, entry.getKey(), valueOf(entry.getValue())));
            row.setTextSize(12);
            row.setTypeface(Typeface.create(Ui.MONO, Typeface.NORMAL));
            row.setTextColor(colors.muted);
            row.setPadding(0, Ui.dp(getContext(), 5), 0, 0);
            params.addView(row);
            shown++;
        }
    }

    private static String valueOf(JsonElement element) {
        if (element == null || element.isJsonNull()) {
            return "—";
        }
        String text = element.isJsonPrimitive()
                ? element.getAsString()
                : Json.write(element);
        return text.length() > 80 ? text.substring(0, 77) + "..." : text;
    }

    private void setEnabledState(boolean enabled) {
        cancel.setEnabled(enabled);
        confirm.setEnabled(enabled);
        cancel.setAlpha(enabled ? 1f : 0.4f);
        confirm.setAlpha(enabled ? 1f : 0.4f);
    }

    public void dismiss() {
        handler.removeCallbacks(tick);
        sheet.animate().translationY(Ui.dp(getContext(), 36)).alpha(0f)
                .setDuration(180L).start();
        scrim.animate().alpha(0f).setDuration(160L).withEndAction(() -> {
            setVisibility(View.GONE);
            card = null;
            listener = null;
        }).start();
    }

    public boolean isShowing() {
        return getVisibility() == View.VISIBLE;
    }

    @Override
    protected void onDetachedFromWindow() {
        handler.removeCallbacks(tick);
        super.onDetachedFromWindow();
    }
}
