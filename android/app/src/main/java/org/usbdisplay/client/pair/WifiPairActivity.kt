package org.usbdisplay.client.pair

import android.os.Bundle
import android.widget.Button
import android.widget.ImageView
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import org.usbdisplay.client.R

private const val WIFI_STREAM_PORT = 27184

/**
 * WiFi pairing screen that shows the tablet's LAN IP and a QR code.
 * PR-2 shows basic IP/port; PR-3 will add PIN and fingerprint.
 */
class WifiPairActivity : AppCompatActivity() {
    private lateinit var ipTextView: TextView
    private lateinit var qrImageView: ImageView
    private lateinit var rotateButton: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_wifi_pair)

        ipTextView = findViewById(R.id.wifi_ip_text)
        qrImageView = findViewById(R.id.qr_image)
        rotateButton = findViewById(R.id.rotate_button)

        updatePairingInfo()

        rotateButton.setOnClickListener {
            // PR-3: rotate PIN
            // For now, just refresh the QR (same content)
            updatePairingInfo()
        }
    }

    private fun updatePairingInfo() {
        val ip = LanAddress.getLanIp(this) ?: "Unknown"
        ipTextView.text = "IP: $ip:$WIFI_STREAM_PORT"

        val qrContent = PairPayload.encode(ip, WIFI_STREAM_PORT)
        val qrBitmap = QrGenerator.generate(qrContent, QR_SIZE_PX)
        qrImageView.setImageBitmap(qrBitmap)
    }

    private companion object {
        const val QR_SIZE_PX = 512
    }
}
