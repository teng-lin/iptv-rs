package com.hantekllc.iptvrs.host

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.AtomicFile
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Only authenticated ciphertext is stored beside the catalog; the wrapping
 * key is non-exportable and owned by Android Keystore for this application's UID. */
object CredentialKey {
    private const val ALIAS = "iptv-rs.catalog.v1"
    @JvmStatic private external fun installDatabaseKey(key: ByteArray): Boolean

    @Synchronized fun initialize(context: Context): Boolean = try {
        val wrapped = AtomicFile(File(context.noBackupFilesDir, "catalog-key.v1"))
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val exists = wrapped.baseFile.exists() || File(wrapped.baseFile.path + ".bak").exists()
        val catalog = File(context.filesDir, "iptv.sqlite")
        val encryptedCatalog = catalog.exists() && catalog.length() > 0 && catalog.inputStream().use {
            val header = ByteArray(16)
            it.read(header) != 16 || !header.contentEquals("SQLite format 3\u0000".toByteArray(Charsets.US_ASCII))
        }
        if ((exists && !store.containsAlias(ALIAS)) || (!exists && encryptedCatalog)) {
            false // Never replace a lost wrapping key or overwrite its ciphertext.
        } else {
            val wrapping = if (store.containsAlias(ALIAS)) {
                store.getKey(ALIAS, null) as SecretKey
            } else {
                KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
                    init(KeyGenParameterSpec.Builder(ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256).build())
                }.generateKey()
            }
            val key = if (exists) {
                val payload = wrapped.openRead().use { input ->
                    val data = ByteArray(61)
                    var count = 0
                    while (count < data.size) {
                        val n = input.read(data, count, data.size - count)
                        if (n < 0) break
                        count += n
                    }
                    require(count == 60 && input.read() == -1)
                    data.copyOf(count)
                }
                Cipher.getInstance("AES/GCM/NoPadding").run {
                    init(Cipher.DECRYPT_MODE, wrapping, GCMParameterSpec(128, payload.copyOfRange(0, 12)))
                    doFinal(payload, 12, payload.size - 12)
                }
            } else {
                ByteArray(32).also { bytes ->
                    SecureRandom().nextBytes(bytes)
                    try {
                        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                        cipher.init(Cipher.ENCRYPT_MODE, wrapping)
                        require(cipher.iv.size == 12)
                        val payload = cipher.iv + cipher.doFinal(bytes)
                        val output = wrapped.startWrite()
                        try {
                            output.write(payload)
                            wrapped.finishWrite(output)
                        } catch (error: Exception) {
                            wrapped.failWrite(output)
                            throw error
                        }
                    } catch (error: Exception) {
                        bytes.fill(0)
                        throw error
                    }
                }
            }
            try {
                require(key.size == 32)
                installDatabaseKey(key)
            } finally { key.fill(0) }
        }
    } catch (_: Exception) {
        // No exception message or credential material enters Android logs.
        false
    }
}
