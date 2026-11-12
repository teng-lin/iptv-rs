package com.hantekllc.iptvrs.host

import android.view.KeyEvent

/** Search belongs to the app; all other keys retain native semantics. */
internal object RemoteKeys {
    fun translate(key: Int): Int =
        if (key == KeyEvent.KEYCODE_SEARCH) KeyEvent.KEYCODE_F12 else key
}
