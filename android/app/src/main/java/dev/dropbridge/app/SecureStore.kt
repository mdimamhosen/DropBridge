package dev.dropbridge.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The device identity (private key) is encrypted with an AES-GCM key that never leaves the
 * Android Keystore. Keystore keys can't be exported to Rust, so we wrap instead.
 */
object SecureStore {
    private const val ALIAS = "dropbridge-identity-wrap"
    private const val PREF = "identity.enc"

    private fun key(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (ks.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        gen.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build()
        )
        return gen.generateKey()
    }

    fun load(ctx: Context): String? = try {
        val raw = ctx.getSharedPreferences("secure", Context.MODE_PRIVATE).getString(PREF, null) ?: null
        raw?.let {
            val b = Base64.decode(it, Base64.NO_WRAP)
            val c = Cipher.getInstance("AES/GCM/NoPadding")
            c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, b, 0, 12))
            String(c.doFinal(b, 12, b.size - 12))
        }
    } catch (e: Exception) {
        null // unreadable (e.g. restored onto a new phone): a new identity is generated
    }

    fun save(ctx: Context, secret: String) {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        val out = c.iv + c.doFinal(secret.toByteArray())
        ctx.getSharedPreferences("secure", Context.MODE_PRIVATE).edit()
            .putString(PREF, Base64.encodeToString(out, Base64.NO_WRAP)).apply()
    }
}
