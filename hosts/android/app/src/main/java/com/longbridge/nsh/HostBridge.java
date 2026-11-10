package com.longbridge.nsh;

import android.app.Activity;

/**
 * Exposes the package-private helpers NSH's own AppActivity uses, so a custom
 * host activity can behave the same. Candidate for the upstream PR: make these
 * public on NshView / a public helper instead of needing a same-package shim.
 */
public final class HostBridge {
    private HostBridge() {}

    public static void edgeToEdge(Activity activity) {
        DevActivity.edgeToEdge(activity);
    }

    public static void systemBars(Activity activity) {
        DevActivity.systemBars(activity);
    }

    public static void useStandaloneLimits(NshView view) {
        view.useStandaloneLimits();
    }
}
