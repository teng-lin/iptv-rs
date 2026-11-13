package com.hantekllc.iptvrs.host

import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy.LoadErrorInfo

/**
 * How [PlayerComponent]'s loads ride out a provider's hiccups: playlist and
 * segment loads that fail on the network retry more often and a little
 * longer than Media3's default (1, 2, 3, 3, 3, 3 s: about 15 s, while the
 * buffer of a stream played behind live plays on), so a short outage never
 * reaches the app. A refusal (401/403/429/458/509: an expired token, the
 * account's connection limit) is not retried here: the same address would
 * be refused again; the error goes to [PlayerComponent], which reopens the
 * stream through its original address (a fresh redirect and token) or
 * leaves it to the app.
 */
@OptIn(UnstableApi::class)
internal class LiveLoadPolicy : DefaultLoadErrorHandlingPolicy(MIN_RETRIES) {
    companion object {
        /** Retries before a failing load becomes the player's error. */
        const val MIN_RETRIES = 6
        /** The longest wait between two retries of a load, ms. */
        const val MAX_RETRY_DELAY_MS = 3_000L
        private val REFUSALS = setOf(401, 403, 429, 458, 509)

        /** The provider refused the request for now (see the class). */
        fun refused(status: Int?) = status != null && status in REFUSALS

        internal fun retryDelay(count: Int, status: Int?, base: Long): Long {
            if (refused(status) || base == C.TIME_UNSET) return C.TIME_UNSET
            return (count * 1_000L).coerceAtMost(MAX_RETRY_DELAY_MS)
        }
    }

    override fun getRetryDelayMsFor(loadErrorInfo: LoadErrorInfo): Long {
        val error = loadErrorInfo.exception
        val status = (error as? HttpDataSource.InvalidResponseCodeException)?.responseCode
        return retryDelay(loadErrorInfo.errorCount, status, super.getRetryDelayMsFor(loadErrorInfo))
    }
}
