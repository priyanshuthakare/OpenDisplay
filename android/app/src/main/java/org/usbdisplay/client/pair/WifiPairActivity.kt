package org.usbdisplay.client.pair

import android.app.Activity
import android.os.Bundle
import android.widget.Button
import android.widget.ImageView
import android.widget.TextView
import android.widget.Toast
import org.usbdisplay.client.R

private const val WIFI_STREAM_PORT = 27184

/**
 * WiFi pairing screen (PR-3): tablet LAN IP, TLS fingerprint, 6-digit PIN,
 * QR `{"v":1,"ip":"…","port":27184,"fp":"SHA256:…"}`, Rotate PIN, Forget hosts.
 *
 * No location permission: IP comes from LinkProperties, never WiFi scan.
 */
class WifiPairActivity : Activity() {
    private lateinit var ipTextView: TextView
    private lateinit var fpTextView: TextView
    private lateinit var pinTextView: TextView
    private lateinit var qrImageView: ImageView
    private lateinit var rotateButton: Button
    private lateinit var forgetButton: Button
    private lateinit var identity: TabletIdentity

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_wifi_pair)

        identity = TabletIdentity(this)
        ipTextView = findViewById(R.id.wifi_ip_text)
        fpTextView = findViewById(R.id.wifi_fp_text)
        pinTextView = findViewById(R.id.wifi_pin_text)
        qrImageView = findViewById(R.id.qr_image)
        rotateButton = findViewById(R.id.rotate_button)
        forgetButton = findViewById(R.id.forget_button)

        updatePairingInfo()

        rotateButton.setOnClickListener {
            identity.rotatePin()
            updatePairingInfo()
            Toast.makeText(this, "PIN rotated", Toast.LENGTH_SHORT).show()
        }
        forgetButton.setOnClickListener {
            identity.clearTrusted()
            Toast.makeText(this, "Trusted hosts cleared", Toast.LENGTH_SHORT).show()
        }
    }

    override fun onResume() {
        super.onResume()
        updatePairingInfo()
    }

    private fun updatePairingInfo() {
        val ip = LanAddress.getLanIp(this) ?: "Unknown"
        val fp = try {
            identity.fingerprint()
        } catch (e: Exception) {
            "SHA256:unavailable"
        }
        val pin = try {
            identity.pin()
        } catch (e: Exception) {
            "------"
        }
        ipTextView.text = "IP: $ip:$WIFI_STREAM_PORT"
        fpTextView.text = CertFingerprint.shortDisplay(fp)
        pinTextView.text = "PIN: $pin"

        val qrContent = PairPayload.encode(ip, WIFI_STREAM_PORT, fp)
        try {
            val qrBitmap = QrGenerator.generate(qrContent, QR_SIZE_PX)
            qrImageView.setImageBitmap(qrBitmap)
        } catch (e: Exception) {
            // QR render failure must not crash pairing; IP+PIN still usable.
            qrImageView.setImageDrawable(null)
        }
    }

    private companion object {
        const val QR_SIZE_PX = 512
    }
}
