package com.intimeui.client.app.ui;

import android.content.Context;

import com.intimeui.client.app.R;

/** Resolved light/dark tokens for the black-white-gray theme. */
public final class ThemeColors {
    public final int background;
    public final int surface;
    public final int ink;
    public final int muted;
    public final int line;
    public final int accent;
    public final int accentInk;
    public final int danger;
    public final int success;
    public final int scrim;

    private ThemeColors(
            int background,
            int surface,
            int ink,
            int muted,
            int line,
            int accent,
            int accentInk,
            int danger,
            int success,
            int scrim) {
        this.background = background;
        this.surface = surface;
        this.ink = ink;
        this.muted = muted;
        this.line = line;
        this.accent = accent;
        this.accentInk = accentInk;
        this.danger = danger;
        this.success = success;
        this.scrim = scrim;
    }

    public static ThemeColors of(Context context, boolean dark) {
        if (dark) {
            return new ThemeColors(
                    context.getColor(R.color.dark_bg),
                    context.getColor(R.color.dark_surface),
                    context.getColor(R.color.dark_ink),
                    context.getColor(R.color.dark_muted),
                    context.getColor(R.color.dark_line),
                    context.getColor(R.color.dark_accent),
                    context.getColor(R.color.dark_accent_ink),
                    context.getColor(R.color.dark_danger),
                    context.getColor(R.color.dark_success),
                    context.getColor(R.color.dark_scrim));
        }
        return new ThemeColors(
                context.getColor(R.color.light_bg),
                context.getColor(R.color.light_surface),
                context.getColor(R.color.light_ink),
                context.getColor(R.color.light_muted),
                context.getColor(R.color.light_line),
                context.getColor(R.color.light_accent),
                context.getColor(R.color.light_accent_ink),
                context.getColor(R.color.light_danger),
                context.getColor(R.color.light_success),
                context.getColor(R.color.light_scrim));
    }
}
