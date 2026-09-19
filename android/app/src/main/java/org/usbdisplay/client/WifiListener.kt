package org.usbdisplay.client

import android.content.Context
import android.util.Log
import org.usbdisplay.client.pair.CertFingerprint
import org.usbdisplay.client.pair.Handshake
import org.usbdisplay.client.pair.PinVerifier
import org.usbdisplay.client.pair.TabletIdentity
import org.usbdisplay.client.transport.PacketKind
import org.usbdisplay.client.transport.TransportPacket
import java.io.BufferedInputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.KeyStore
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket

private const val TAG = "USBDisplay-WiFi"
const val WIFI_STREAM_PORT = 27184

/**
 * WiFi TLS-1.3 listener on all interfaces (PR-3).
 *
 * Flow per connection:
 *  1. TLS-1.3 handshake (self-signed ECDSA cert, fingerprint pinned via QR).
 *  2. Read one framed Handshake packet (Hello JSON with pin/host_id).
 *  3. Verify PIN constant-time (skip if host_id already trusted), 3 strikes
 *     -> 30s lockout; reply Welcome Handshake packet.
 *  4. Hand the socket streams to the shared [StreamPipeline] (same decode
 *     path as USB — no forked decoder).
 */
class WifiListener(
    private val context: Context,
    private val pipeline: StreamPipeline,
) {
    private val running = AtomicBoolean(false)
    private var serverSocket: ServerSocket? = null
    private var thread: Thread? = null
    private val identity by lazy { TabletIdentity(context) }
    private val pinVerifier = PinVerifier()

    fun start() {
        if (!running.compareAndSet(false, true)) return
        thread = Thread(::runLoop, "usbdisplay-wifi-listener").also { it.start() }
    }

    fun stop() {
        running.set(false)
        try {
            serverSocket?.close()
        } catch (_: Exception) {
        }
        thread?.join(2000)
        thread = null
    }

    private fun runLoop() {
        try {
            serverSocket = createTlsServerSocket()
            Log.i(TAG, "WiFi TLS listener on 0.0.0.0:$WIFI_STREAM_PORT fp=${identity.fingerprintShort()}")
            while (running.get()) {
                val client = try {
                    serverSocket?.accept() ?: break
                } catch (e: Exception) {
                    if (!running.get()) break
                    throw e
                }
                Log.i(TAG, "WiFi client connected from ${client.inetAddress}")
                try {
                    handleClient(client)
                } catch (e: Exception) {
                    Log.e(TAG, "WiFi client error", e)
                } finally {
                    try {
                        client.close()
                    } catch (_: Exception) {
                    }
                }
            }
        } catch (e: Exception) {
            if (running.get()) {
                Log.e(TAG, "WiFi listener error", e)
            }
        }
    }

    private fun createTlsServerSocket(): SSLServerSocket {
        val certDer = identity.certDer()
        val priv = identity.privateKey()
            ?: throw IllegalStateException("tablet private key unavailable")
        // In-memory KeyStore for the KeyManager (private prefs hold the key).
        val ks = KeyStore.getInstance(KeyStore.getDefaultType())
        ks.load(null, null)
        val certFactory = java.security.cert.CertificateFactory.getInstance("X.509")
        val cert = certFactory.generateCertificate(certDer.inputStream())
        ks.setKeyEntry("tablet", priv, CharArray(0), arrayOf(cert))
        val kmf = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm())
        kmf.init(ks, CharArray(0))
        val ctx = SSLContext.getInstance("TLSv1.3")
        ctx.init(kmf.keyManagers, null, SecureRandom())
        val factory = ctx.serverSocketFactory
        val ss = factory.createServerSocket(
            WIFI_STREAM_PORT, 0, InetAddress.getByName("0.0.0.0"),
        ) as SSLServerSocket
        ss.enabledProtocols = arrayOf("TLSv1.3")
        return ss
    }

    private fun handleClient(socket: Socket) {
        socket.soTimeout = 10_000
        val input = BufferedInputStream(socket.getInputStream())
        val output = socket.getOutputStream()
        // 1 framed Hello.
        val helloPacket = readFramed(input) ?: run {
            Log.w(TAG, "TLS client closed before Hello")
            return
        }
        if (helloPacket.header.kind != PacketKind.Handshake) {
            writeFramed(output, TransportPacket.handshake(1, Handshake.buildWelcomeReject("expected-handshake").toByteArray()))
            return
        }
        val helloJson = String(helloPacket.payload, Charsets.UTF_8)
        val hello = Handshake.parseHello(helloJson)
        if (hello == null) {
            writeFramed(output, TransportPacket.handshake(1, Handshake.buildWelcomeReject("version").toByteArray()))
            return
        }
        // Trusted host skips PIN (TLS fp still enforced by the handshake).
        if (hello.hostId.isNotBlank() && identity.isTrusted(hello.hostId)) {
            Log.i(TAG, "Known host ${hello.hostId} — PIN skipped")
            writeFramed(
                output,
                TransportPacket.handshake(
                    1,
                    Handshake.buildWelcomeAccept(identity.tabletId(), identity.fingerprint()).toByteArray(),
                ),
            )
            socket.soTimeout = 0
            // Reuse `input`: it may already hold TLS bytes coalesced past the
            // Hello, which a fresh BufferedInputStream would silently drop.
            pipeline.handleClient(input, output)
            return
        }
        // PIN check with lockout.
        when (val r = pinVerifier.verify(hello.pin, identity.pin())) {
            is PinVerifier.Result.Ok -> {
                identity.trust(hello.hostId.ifBlank { socket.inetAddress.hostAddress ?: "unknown" })
                writeFramed(
                    output,
                    TransportPacket.handshake(
                        1,
                        Handshake.buildWelcomeAccept(identity.tabletId(), identity.fingerprint()).toByteArray(),
                    ),
                )
                socket.soTimeout = 0
                // Reuse `input` (see above): never re-wrap the socket stream.
                pipeline.handleClient(input, output)
            }
            is PinVerifier.Result.Wrong -> {
                Log.w(TAG, "Wrong PIN (${r.retriesLeft} retries left)")
                writeFramed(output, TransportPacket.handshake(1, Handshake.buildWelcomeReject("bad-pin").toByteArray()))
            }
            is PinVerifier.Result.Locked -> {
                Log.w(TAG, "PIN lockout ${r.retryAfterMs}ms")
                writeFramed(output, TransportPacket.handshake(1, Handshake.buildWelcomeReject("lockout").toByteArray()))
            }
        }
    }

    private fun readFramed(input: BufferedInputStream): TransportPacket? {
        val lenBytes = ByteArray(4)
        if (!readExactly(input, lenBytes)) return null
        val len = ByteBuffer.wrap(lenBytes).order(ByteOrder.LITTLE_ENDIAN).int
        if (len <= 0 || len > 64 * 1024 + 64) throw IllegalArgumentException("bad frame len $len")
        val body = ByteArray(len)
        if (!readExactly(input, body)) return null
        return TransportPacket.decode(body)
    }

    private fun writeFramed(output: java.io.OutputStream, packet: TransportPacket) {
        val bytes = packet.encode()
        val framed = ByteBuffer.allocate(4 + bytes.size).order(ByteOrder.LITTLE_ENDIAN)
        framed.putInt(bytes.size)
        framed.put(bytes)
        output.write(framed.array())
        output.flush()
    }

    private fun readExactly(input: BufferedInputStream, buf: ByteArray): Boolean {
        var off = 0
        while (off < buf.size) {
            val n = input.read(buf, off, buf.size - off)
            if (n == -1) return false
            off += n
        }
        return true
    }

    private fun ByteArray.inputStream() = java.io.ByteArrayInputStream(this)
}
