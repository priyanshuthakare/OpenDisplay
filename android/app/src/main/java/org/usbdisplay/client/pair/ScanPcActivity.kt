package org.usbdisplay.client.pair

import android.app.Activity
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.media.ImageReader
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import org.usbdisplay.client.R
import java.util.EnumMap
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Scans the pairing code shown by the Windows control app
 * (`{"v":1,"host_id":"host-…"}`) and trusts that host id, so the next
 * connect from that PC skips the PIN (TLS fingerprint still enforced).
 *
 * Dependency-free scanning: framework Camera2 + the already-bundled
 * `zxing:core` QR decoder. No preview surface — the ImageReader target is
 * enough, which keeps this to one small activity.
 */
class ScanPcActivity : Activity() {
    private lateinit var statusView: TextView
    private var bgThread: HandlerThread? = null
    private var bgHandler: Handler? = null
    private var camera: CameraDevice? = null
    private var session: CameraCaptureSession? = null
    private var reader: ImageReader? = null
    private val decoding = AtomicBoolean(false)
    private val done = AtomicBoolean(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_scan_pc)
        statusView = findViewById(R.id.scan_status_text)
        findViewById<Button>(R.id.scan_cancel_button).setOnClickListener { finish() }
        if (checkSelfPermission(android.Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            startCamera()
        } else {
            requestPermissions(arrayOf(android.Manifest.permission.CAMERA), REQUEST_CAMERA)
        }
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        if (requestCode == REQUEST_CAMERA &&
            grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED
        ) {
            startCamera()
        } else {
            Toast.makeText(this, "Camera permission is needed to scan the PC code.", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    override fun onPause() {
        closeCamera()
        super.onPause()
    }

    private fun startCamera() {
        bgThread = HandlerThread("usbdisplay-scan").also { it.start() }
        bgHandler = Handler(bgThread!!.looper)
        try {
            val manager = getSystemService(CAMERA_SERVICE) as CameraManager
            val id = manager.cameraIdList.firstOrNull {
                manager.getCameraCharacteristics(it)
                    .get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK
            } ?: manager.cameraIdList.firstOrNull()
            if (id == null) {
                toast("No camera found on this tablet.")
                finish()
                return
            }
            reader = ImageReader.newInstance(PREVIEW_W, PREVIEW_H, ImageFormat.YUV_420_888, 2).also {
                it.setOnImageAvailableListener({ r -> onFrame(r) }, bgHandler)
            }
            @Suppress("MissingPermission")
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    camera = device
                    createSession()
                }

                override fun onDisconnected(device: CameraDevice) = device.close()
                override fun onError(device: CameraDevice, error: Int) {
                    toast("Camera error ($error).")
                    finish()
                }
            }, bgHandler)
        } catch (e: Exception) {
            toast("Could not open camera: ${e.message}")
            finish()
        }
    }

    private fun createSession() {
        val device = camera ?: return
        val surface = reader?.surface ?: return
        try {
            @Suppress("Deprecation")
            device.createCaptureSession(
                listOf(surface),
                object : CameraCaptureSession.StateCallback() {
                    override fun onConfigured(s: CameraCaptureSession) {
                        session = s
                        try {
                            val req = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                            req.addTarget(surface)
                            s.setRepeatingRequest(req.build(), null, bgHandler)
                        } catch (e: Exception) {
                            toast("Could not start preview: ${e.message}")
                            finish()
                        }
                    }

                    override fun onConfigureFailed(s: CameraCaptureSession) {
                        toast("Could not configure camera.")
                        finish()
                    }
                },
                bgHandler,
            )
        } catch (e: Exception) {
            toast("Could not configure camera: ${e.message}")
            finish()
        }
    }

    private fun onFrame(reader: ImageReader) {
        if (done.get() || !decoding.compareAndSet(false, true)) {
            return
        }
        try {
            val image = try {
                reader.acquireLatestImage()
            } catch (_: Exception) {
                null
            } ?: return
            try {
                val text = decodeQr(image)
                if (text != null) onCode(text)
            } finally {
                image.close()
            }
        } finally {
            decoding.set(false)
        }
    }

    private fun decodeQr(image: android.media.Image): String? {
        // Luminance-only decode: copy the Y plane into a tight buffer
        // (rowStride may exceed width) for PlanarYUVLuminanceSource.
        val y = image.planes[0]
        val buffer = y.buffer
        val rowStride = y.rowStride
        val tight = ByteArray(PREVIEW_W * PREVIEW_H)
        if (rowStride == PREVIEW_W) {
            buffer.get(tight, 0, minOf(buffer.remaining(), tight.size))
        } else {
            var offset = 0
            val row = ByteArray(rowStride)
            repeat(PREVIEW_H) {
                val n = minOf(rowStride, buffer.remaining())
                buffer.get(row, 0, n)
                val copy = minOf(PREVIEW_W, n)
                System.arraycopy(row, 0, tight, offset, copy)
                offset += PREVIEW_W
            }
        }
        return try {
            val source = PlanarYUVLuminanceSource(tight, PREVIEW_W, PREVIEW_H, 0, 0, PREVIEW_W, PREVIEW_H, false)
            val hints = EnumMap<DecodeHintType, Any>(DecodeHintType::class.java)
            hints[DecodeHintType.TRY_HARDER] = true
            QRCodeReader().decode(BinaryBitmap(HybridBinarizer(source)), hints).text
        } catch (_: Exception) {
            null
        }
    }

    private fun onCode(text: String) {
        val hostId = PcPairPayload.parseHostId(text)
        if (hostId == null) {
            runOnUiThread {
                statusView.text = "That is not a USBDisplay pairing code — keep aiming at the PC screen."
            }
            return
        }
        if (!done.compareAndSet(false, true)) return
        TabletIdentity(this).trust(hostId)
        runOnUiThread {
            Toast.makeText(this, "PC trusted ($hostId). Connect from the PC app — no PIN needed.", Toast.LENGTH_LONG).show()
        }
        setResult(RESULT_OK)
        finish()
    }

    private fun closeCamera() {
        try {
            session?.close()
        } catch (_: Exception) {
        }
        session = null
        try {
            camera?.close()
        } catch (_: Exception) {
        }
        camera = null
        try {
            reader?.close()
        } catch (_: Exception) {
        }
        reader = null
        bgThread?.quitSafely()
        bgThread = null
        bgHandler = null
    }

    private fun toast(msg: String) = runOnUiThread {
        Toast.makeText(this, msg, Toast.LENGTH_LONG).show()
    }

    private companion object {
        const val REQUEST_CAMERA = 41
        const val PREVIEW_W = 1280
        const val PREVIEW_H = 720
    }
}
