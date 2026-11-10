package com.hantekllc.iptvrs.host

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import com.longbridge.nsh.DevActivity
import com.longbridge.nsh.NshView

/**
 * The Android host: mirrors NSH's AppActivity for a Rust app, and registers
 * the app's native components before the Rust root mounts. Its players stop
 * while it is stopped (Home, another app, the screen off) and reopen when
 * it starts again ([PlayerComponent.background]); nothing plays in the
 * background.
 */
class TvActivity : Activity() {
    private lateinit var nsh: NshView
    /** The mounted players (multiview mounts more than one). */
    private val players = LinkedHashSet<PlayerComponent>()
    /** Between onStart and onStop. */
    private var started = false

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        DevActivity.edgeToEdge(this)
        nsh = NshView(this)
        nsh.useStandaloneLimits()
        nsh.registerComponent(PlayerComponent.NAME) { context ->
            PlayerComponent(this, context) { players.remove(it) }.also {
                players.add(it)
                if (!started) it.background()
            }
        }
        setContentView(nsh)
        nsh.handleNotificationIntent(intent)
        if (state == null) nsh.handleUrlSchemeIntent(intent)
        nsh.runRustApp("nsh")
    }

    override fun onStart() {
        super.onStart()
        started = true
        for (p in players.toList()) p.foreground()
    }

    override fun onStop() {
        started = false
        for (p in players.toList()) p.background()
        super.onStop()
    }

    override fun onConfigurationChanged(config: Configuration) {
        super.onConfigurationChanged(config)
        DevActivity.systemBars(this)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        nsh.handleNotificationIntent(intent)
        nsh.handleUrlSchemeIntent(intent)
    }

    @Deprecated("Back is also handled by NshView's OnBackInvokedCallback on API 33+")
    override fun onBackPressed() {
        if (!nsh.handleBack()) super.onBackPressed()
    }

    override fun onDestroy() {
        nsh.destroy()
        super.onDestroy()
    }
}
