package com.alvit.inverter_dashboard

import android.app.Activity
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Rust-only bridge. The Rust plugin rejects every webview invocation. */
@TauriPlugin
class SecureStorePlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun encryptionKey(invoke: Invoke) {
        try {
            val key = synchronized(storageLock) { readOrCreateKey() }
            try {
                val values = JSArray()
                key.forEach { values.put(it.toInt() and 0xff) }
                val response = JSObject()
                response.put("key", values)
                invoke.resolve(response)
            } finally {
                key.fill(0)
            }
        } catch (_: Exception) {
            // Do not log keys, ciphertext, or native exception payloads.
            invoke.reject("Android Keystore could not unlock credential storage; saved credentials were not overwritten")
        }
    }

    private fun readOrCreateKey(): ByteArray {
        val preferences = activity.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val wrapped = preferences.getString(WRAPPED_KEY, null)
        if (wrapped != null) {
            // Never replace a missing/invalidated wrapping key: an existing
            // encrypted configuration must fail visibly instead of being lost.
            val wrappingKey = store.getKey(KEY_ALIAS, null) as? SecretKey
                ?: error("Credential wrapping key is unavailable")
            val bytes = Base64.decode(wrapped, Base64.NO_WRAP)
            require(bytes.size == IV_BYTES + KEY_BYTES + TAG_BYTES)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, wrappingKey, GCMParameterSpec(TAG_BYTES * 8, bytes.copyOfRange(0, IV_BYTES)))
            cipher.updateAAD(AAD)
            val key = cipher.doFinal(bytes, IV_BYTES, bytes.size - IV_BYTES)
            check(key.size == KEY_BYTES)
            return key
        }

        val wrappingKey = if (store.containsAlias(KEY_ALIAS)) {
            store.getKey(KEY_ALIAS, null) as? SecretKey
                ?: error("Credential wrapping key is unavailable")
        } else {
            val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            generator.init(
                KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                    .setKeySize(256)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .setRandomizedEncryptionRequired(true)
                    // Config is needed by background connections after startup.
                    .setUserAuthenticationRequired(false)
                    .build()
            )
            generator.generateKey()
        }

        val key = ByteArray(KEY_BYTES).also { SecureRandom().nextBytes(it) }
        try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, wrappingKey)
            cipher.updateAAD(AAD)
            check(cipher.iv.size == IV_BYTES)
            val ciphertext = cipher.iv + cipher.doFinal(key)
            // Preferences contain authenticated ciphertext only; the wrapping
            // key is non-exportable and remains inside AndroidKeyStore.
            check(preferences.edit().putString(WRAPPED_KEY, Base64.encodeToString(ciphertext, Base64.NO_WRAP)).commit())
            return key
        } catch (error: Exception) {
            key.fill(0)
            throw error
        }
    }

    companion object {
        private val storageLock = Any()
        private const val KEY_ALIAS = "inverter-desktop-config-wrap-v1"
        private const val PREFERENCES = "inverter-desktop-credential-key"
        private const val WRAPPED_KEY = "wrapped-key-v1"
        private const val KEY_BYTES = 32
        private const val IV_BYTES = 12
        private const val TAG_BYTES = 16
        private val AAD = "inverter-desktop-config-key-v1".toByteArray(Charsets.UTF_8)
    }
}
