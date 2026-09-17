package org.usbdisplay.client

import android.app.Activity
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.Window
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.Button
import android.widget.FrameLayout
import org.usbdisplay.client.pair.WifiPairActivity
import org.usbdisplay.client.transport.InputAction
import org.usbdisplay.client.transport.InputEvent
import org.usbdisplay.client.transport.KeyAction
import org.usbdisplay.client.transport.NamedKey
import org.usbdisplay.client.transport.PointerButton

class MainActivity : Activity(), SurfaceHolder.Callback {
    private lateinit var surfaceView: SurfaceView
    private var streamSession: StreamSession? = null
    private var wifiListener: WifiListener? = null
    private var streamPipeline: StreamPipeline? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestWindowFeature(Window.FEATURE_NO_TITLE)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        val layout = FrameLayout(this)
        surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)
        layout.addView(surfaceView, FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT,
            FrameLayout.LayoutParams.MATCH_PARENT
        ))

        val pairButton = Button(this).apply {
            text = "WiFi Pair"
            alpha = 0.6f
            setOnClickListener {
                startActivity(Intent(this@MainActivity, WifiPairActivity::class.java))
            }
        }
        val pairParams = FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.WRAP_CONTENT,
            FrameLayout.LayoutParams.WRAP_CONTENT
        )
        pairParams.setMargins(16, 16, 16, 16)
        layout.addView(pairButton, pairParams)

        setContentView(layout)
        hideSystemBars()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val pipeline = streamPipeline ?: return super.onTouchEvent(event)
        val width = surfaceView.width
        val height = surfaceView.height
        if (width <= 0 || height <= 0) return super.onTouchEvent(event)

        val action = when (event.actionMasked) {
            MotionEvent.ACTION_DOWN, MotionEvent.ACTION_POINTER_DOWN -> InputAction.Down
            MotionEvent.ACTION_MOVE -> InputAction.Move
            MotionEvent.ACTION_UP, MotionEvent.ACTION_POINTER_UP, MotionEvent.ACTION_CANCEL ->
                InputAction.Up
            else -> return super.onTouchEvent(event)
        }

        // A MOVE batches intermediate samples in the historical buffer; replay
        // them in order so fast drags stay smooth on the host cursor.
        if (action == InputAction.Move) {
            for (h in 0 until event.historySize) {
                pipeline.sendInput(
                    pointerEvent(action, event.getHistoricalX(h), event.getHistoricalY(h), width, height)
                )
            }
        }
        pipeline.sendInput(pointerEvent(action, event.x, event.y, width, height))
        return true
    }

    private fun pointerEvent(
        action: InputAction,
        rawX: Float,
        rawY: Float,
        width: Int,
        height: Int,
    ): InputEvent = InputEvent.pointer(
        action = action,
        button = PointerButton.Left,
        pointerId = 0,
        x = InputEvent.normalize(rawX, width),
        y = InputEvent.normalize(rawY, height),
    )

    override fun onKeyDown(keyCode: Int, event: KeyEvent): Boolean =
        if (sendKey(KeyAction.Down, keyCode, event)) true else super.onKeyDown(keyCode, event)

    override fun onKeyUp(keyCode: Int, event: KeyEvent): Boolean =
        if (sendKey(KeyAction.Up, keyCode, event)) true else super.onKeyUp(keyCode, event)

    /**
     * Forward a key event to the host. Named editing keys map to a [NamedKey];
     * anything with a printable Unicode value is sent as a character. Returns
     * false when the key is neither, so the platform keeps its default handling
     * (e.g. Back to leave the activity).
     */
    private fun sendKey(action: KeyAction, keyCode: Int, event: KeyEvent): Boolean {
        val pipeline = streamPipeline ?: return false
        val named = namedKeyFor(keyCode)
        if (named != null) {
            pipeline.sendInput(InputEvent.keyNamed(action, named))
            return true
        }
        val unicode = event.unicodeChar
        if (unicode != 0) {
            pipeline.sendInput(InputEvent.keyChar(action, unicode))
            return true
        }
        return false
    }

    private fun namedKeyFor(keyCode: Int): NamedKey? = when (keyCode) {
        KeyEvent.KEYCODE_ENTER, KeyEvent.KEYCODE_NUMPAD_ENTER -> NamedKey.Enter
        KeyEvent.KEYCODE_DEL -> NamedKey.Backspace
        KeyEvent.KEYCODE_FORWARD_DEL -> NamedKey.Delete
        KeyEvent.KEYCODE_TAB -> NamedKey.Tab
        KeyEvent.KEYCODE_ESCAPE -> NamedKey.Escape
        KeyEvent.KEYCODE_DPAD_LEFT -> NamedKey.ArrowLeft
        KeyEvent.KEYCODE_DPAD_RIGHT -> NamedKey.ArrowRight
        KeyEvent.KEYCODE_DPAD_UP -> NamedKey.ArrowUp
        KeyEvent.KEYCODE_DPAD_DOWN -> NamedKey.ArrowDown
        KeyEvent.KEYCODE_MOVE_HOME -> NamedKey.Home
        KeyEvent.KEYCODE_MOVE_END -> NamedKey.End
        else -> null
    }

    private fun hideSystemBars() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.insetsController?.let { controller ->
                controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                controller.systemBarsBehavior =
                    WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                android.view.View.SYSTEM_UI_FLAG_FULLSCREEN or
                    android.view.View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or
                    android.view.View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        // One shared decode pipeline for USB + WiFi so framing stays identical.
        streamSession?.stop()
        wifiListener?.stop()
        streamPipeline?.release()
        val pipeline = StreamPipeline(holder.surface)
        streamPipeline = pipeline
        streamSession = StreamSession(holder.surface, pipeline).also { it.start() }
        wifiListener = WifiListener(this, pipeline).also { it.start() }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        streamSession?.stop()
        streamSession = null
        wifiListener?.stop()
        wifiListener = null
    }

    override fun onDestroy() {
        streamSession?.stop()
        streamSession = null
        wifiListener?.stop()
        wifiListener = null
        streamPipeline?.release()
        streamPipeline = null
        super.onDestroy()
    }
}
