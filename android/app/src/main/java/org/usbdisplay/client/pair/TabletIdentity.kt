package org.usbdisplay.client.pair

import android.content.Context
import android.util.Base64
import android.util.Log
import org.bouncycastle.asn1.x500.X500Name
import org.bouncycastle.cert.jcajce.JcaX509CertificateConverter
import org.bouncycastle.cert.jcajce.JcaX509v3CertificateBuilder
import org.bouncycastle.jce.provider.BouncyCastleProvider
import org.bouncycastle.operator.jcajce.JcaContentSignerBuilder
import java.math.BigInteger
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.PrivateKey
import java.security.PublicKey
import java.security.SecureRandom
import java.security.Security
import java.security.spec.ECGenParameterSpec
import java.security.spec.PKCS8EncodedKeySpec
import java.security.spec.X509EncodedKeySpec
import java.util.Date
import java.util.UUID

/**
 * Tablet TLS identity + PIN + trusted hosts (PR-3).
 *
 * Choice documented in docs/wifi.md: AndroidKeyStore cannot mint TLS server
 * certs directly without attestation infra, so we generate an ECDSA P-256
 * keypair in-memory and self-sign via BouncyCastle, persisting PKCS8 + DER
 * in private SharedPreferences (MODE_PRIVATE, no backup). TOFU pinning makes
 * this acceptable for LAN pairing; "Forget hosts" + cert regen rotates.
 */
class TabletIdentity(private val context: Context) {
    private val prefs =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    fun tabletId(): String {
        var id = prefs.getString(KEY_TABLET_ID, null)
        if (id == null) {
            id = "tablet-" + UUID.randomUUID().toString().take(8)
            prefs.edit().putString(KEY_TABLET_ID, id).apply()
        }
        return id!!
    }

    fun pin(): String {
        var pin = prefs.getString(KEY_PIN, null)
        if (pin == null || !PinVerifier.isValidFormat(pin)) {
            pin = PinVerifier.generate()
            prefs.edit().putString(KEY_PIN, pin).apply()
        }
        return pin!!
    }

    fun rotatePin(): String {
        val pin = PinVerifier.generate()
        prefs.edit().putString(KEY_PIN, pin).apply()
        return pin
    }

    fun certDer(): ByteArray {
        val b64 = prefs.getString(KEY_CERT_DER, null)
        if (b64 != null) {
            try {
                return Base64.decode(b64, Base64.NO_WRAP)
            } catch (_: Exception) {
            }
        }
        return generateAndStore()
    }

    fun privateKey(): PrivateKey? {
        val b64 = prefs.getString(KEY_PRIV, null) ?: return null
        return try {
            val spec = PKCS8EncodedKeySpec(Base64.decode(b64, Base64.NO_WRAP))
            KeyFactory.getInstance("EC").generatePrivate(spec)
        } catch (e: Exception) {
            Log.w(TAG, "Failed to load private key", e)
            null
        }
    }

    fun publicKey(): PublicKey? {
        val b64 = prefs.getString(KEY_PUB, null) ?: return null
        return try {
            val spec = X509EncodedKeySpec(Base64.decode(b64, Base64.NO_WRAP))
            KeyFactory.getInstance("EC").generatePublic(spec)
        } catch (e: Exception) {
            Log.w(TAG, "Failed to load public key", e)
            null
        }
    }

    fun fingerprint(): String = CertFingerprint.ofDer(certDer())

    fun fingerprintShort(): String = CertFingerprint.shortDisplay(fingerprint())

    fun trustedHosts(): MutableSet<String> =
        prefs.getStringSet(KEY_TRUSTED, emptySet())?.toMutableSet() ?: mutableSetOf()

    fun isTrusted(hostId: String): Boolean = trustedHosts().contains(hostId)

    fun trust(hostId: String) {
        if (hostId.isBlank()) return
        val set = trustedHosts()
        set.add(hostId)
        prefs.edit().putStringSet(KEY_TRUSTED, set).apply()
    }

    fun clearTrusted() {
        prefs.edit().remove(KEY_TRUSTED).apply()
    }

    fun regenerateCert(): ByteArray {
        prefs.edit().remove(KEY_CERT_DER).remove(KEY_PRIV).remove(KEY_PUB).apply()
        return generateAndStore()
    }

    private fun generateAndStore(): ByteArray {
        ensureBc()
        val kpg = KeyPairGenerator.getInstance("EC")
        kpg.initialize(ECGenParameterSpec("secp256r1"), SecureRandom())
        val kp = kpg.generateKeyPair()
        val now = Date()
        val expiry = Date(now.time + 10L * 365 * 24 * 3600 * 1000)
        val serial = BigInteger(64, SecureRandom())
        val builder = JcaX509v3CertificateBuilder(
            X500Name("CN=USBDisplay-Tablet"),
            serial,
            now,
            expiry,
            X500Name("CN=USBDisplay-Tablet"),
            kp.public,
        )
        val signer = JcaContentSignerBuilder("SHA256withECDSA").build(kp.private)
        val holder = builder.build(signer)
        val cert = JcaX509CertificateConverter().getCertificate(holder)
        val der = cert.encoded
        prefs.edit()
            .putString(KEY_CERT_DER, Base64.encodeToString(der, Base64.NO_WRAP))
            .putString(KEY_PRIV, Base64.encodeToString(kp.private.encoded, Base64.NO_WRAP))
            .putString(KEY_PUB, Base64.encodeToString(kp.public.encoded, Base64.NO_WRAP))
            .apply()
        Log.i(TAG, "Generated new ECDSA P-256 self-signed cert fp=${CertFingerprint.shortDisplay(CertFingerprint.ofDer(der))}")
        return der
    }

    private fun ensureBc() {
        try {
            if (Security.getProvider(BouncyCastleProvider.PROVIDER_NAME) == null) {
                Security.addProvider(BouncyCastleProvider())
            }
        } catch (e: Exception) {
            Log.w(TAG, "BC provider install failed", e)
        }
    }

    companion object {
        private const val TAG = "USBDisplay-Identity"
        private const val PREFS = "wifi_pair"
        private const val KEY_TABLET_ID = "tablet_id"
        private const val KEY_PIN = "pin"
        private const val KEY_CERT_DER = "cert_der_b64"
        private const val KEY_PRIV = "key_pkcs8_b64"
        private const val KEY_PUB = "key_pub_b64"
        private const val KEY_TRUSTED = "trusted_hosts"
    }
}
