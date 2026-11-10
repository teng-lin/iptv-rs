package com.hantekllc.iptvrs.host

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import com.longbridge.nsh.HostBridge
import com.longbridge.nsh.NshView

/**
 * The Android host: mirrors NSH's AppActivity for a Rust app, and registers
 * the app's native components before the Rust root mounts.
 */
class TvActivity : Activity() {
    private lateinit var nsh: NshView

    override fun onCreate(state: Bundle?) {
        super.onCreate(state)
        HostBridge.edgeToEdge(this)
        nsh = NshView(this)
        HostBridge.useStandaloneLimits(nsh)
        nsh.registerComponent(PlayerComponent.NAME) { context -> PlayerComponent(this, context) }
        setContentView(nsh)
        nsh.handleNotificationIntent(intent)
        if (state == null) nsh.handleUrlSchemeIntent(intent)
        nsh.runRustApp("nsh")
    }

    override fun onConfigurationChanged(config: Configuration) {
        super.onConfigurationChanged(config)
        HostBridge.systemBars(this)
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
