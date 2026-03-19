package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import android.graphics.Bitmap
import android.util.Base64
import android.util.Log
import java.io.ByteArrayOutputStream
import java.util.Timer
import java.util.TimerTask
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okio.ByteString
import okio.ByteString.Companion.toByteString
import org.json.JSONObject
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager

/**
 * HiveLiveService — Real-time voice + vision bridge to Apis.
 *
 * Replaces GeminiLiveService. Key differences:
 * - Audio is sent as raw binary PCM frames (not base64 in JSON)
 * - Video frames are sent as JSON {"type": "frame", "jpeg": "<base64>"}
 * - End-of-speech is detected client-side (VAD) and signaled via JSON
 * - Apis handles STT/TTS server-side; we receive raw PCM audio back
 * - No tool call routing needed — Apis has native tool support
 *
 * Protocol:
 *   Client → Server:
 *     Binary:  Raw PCM audio (16kHz, 16-bit, mono)
 *     JSON:    {"type": "frame", "jpeg": "<base64>"}
 *     JSON:    {"type": "end_of_speech"}
 *     JSON:    {"type": "ping"}
 *
 *   Server → Client:
 *     Binary:  Raw PCM audio response (24kHz, 16-bit, mono)
 *     JSON:    {"type": "text", "content": "..."}
 *     JSON:    {"type": "thinking"}
 *     JSON:    {"type": "done"}
 *     JSON:    {"type": "connected", ...}
 *     JSON:    {"type": "pong"}
 *     JSON:    {"type": "error", "message": "..."}
 */

sealed class ApisConnectionState {
    data object Disconnected : ApisConnectionState()
    data object Connecting : ApisConnectionState()
    data object Ready : ApisConnectionState()
    data class Error(val message: String) : ApisConnectionState()
}

class HiveLiveService {
    companion object {
        private const val TAG = "HiveLiveService"
        private const val SILENCE_THRESHOLD = 500   // amplitude threshold for silence
        private const val SILENCE_DURATION_MS = 800  // ms of silence before end-of-speech
        private const val MIN_SPEECH_DURATION_MS = 300 // minimum speech before end-of-speech triggers
    }

    private val _connectionState = MutableStateFlow<ApisConnectionState>(ApisConnectionState.Disconnected)
    val connectionState: StateFlow<ApisConnectionState> = _connectionState.asStateFlow()

    private val _isModelSpeaking = MutableStateFlow(false)
    val isModelSpeaking: StateFlow<Boolean> = _isModelSpeaking.asStateFlow()

    private val _isInferring = MutableStateFlow(false)
    val isInferring: StateFlow<Boolean> = _isInferring.asStateFlow()

    // Callbacks
    var onAudioReceived: ((ByteArray) -> Unit)? = null
    var onTurnComplete: (() -> Unit)? = null
    var onInterrupted: (() -> Unit)? = null
    var onDisconnected: ((String?) -> Unit)? = null
    var onInputTranscription: ((String) -> Unit)? = null
    var onOutputTranscription: ((String) -> Unit)? = null
    var onWakeWordDetected: ((String) -> Unit)? = null

    // Wake word mode: when true, VAD triggers wake word check instead of full processing
    @Volatile
    var wakeWordMode: Boolean = false

    // Latency tracking
    private var lastUserSpeechEnd: Long = 0
    private var responseLatencyLogged = false

    // VAD (Voice Activity Detection) state
    private var speechStartTime: Long = 0
    private var lastSpeechTime: Long = 0
    private var isSpeaking = false

    private var webSocket: WebSocket? = null
    private val sendExecutor = Executors.newSingleThreadExecutor()
    private var connectCallback: ((Boolean) -> Unit)? = null
    private var timeoutTimer: Timer? = null

    private val client = OkHttpClient.Builder()
        .readTimeout(0, TimeUnit.MILLISECONDS)
        .build()

    fun connect(callback: (Boolean) -> Unit) {
        val url = HiveConfig.websocketURL()
        if (url == null) {
            _connectionState.value = ApisConnectionState.Error("HIVE server not configured")
            callback(false)
            return
        }

        _connectionState.value = ApisConnectionState.Connecting
        connectCallback = callback

        Log.d(TAG, "Connecting to Apis: $url")

        val request = Request.Builder().url(url).build()
        webSocket = client.newWebSocket(request, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                Log.d(TAG, "WebSocket opened to Apis")
                // No setup message needed — server handles auth via JWT in URL
            }

            override fun onMessage(webSocket: WebSocket, text: String) {
                handleTextMessage(text)
            }

            override fun onMessage(webSocket: WebSocket, bytes: ByteString) {
                // Binary frame = PCM audio response from HIVE TTS
                val audioData = bytes.toByteArray()
                if (!_isModelSpeaking.value) {
                    _isModelSpeaking.value = true
                    if (lastUserSpeechEnd > 0 && !responseLatencyLogged) {
                        val latency = System.currentTimeMillis() - lastUserSpeechEnd
                        Log.d(TAG, "[Latency] ${latency}ms (speech end → first audio)")
                        responseLatencyLogged = true
                    }
                }
                onAudioReceived?.invoke(audioData)
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                val msg = t.message ?: "Unknown error"
                Log.e(TAG, "WebSocket failure: $msg")
                _connectionState.value = ApisConnectionState.Error(msg)
                _isModelSpeaking.value = false
                resolveConnect(false)
                onDisconnected?.invoke(msg)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                Log.d(TAG, "WebSocket closing: $code $reason")
                _connectionState.value = ApisConnectionState.Disconnected
                _isModelSpeaking.value = false
                resolveConnect(false)
                onDisconnected?.invoke("Connection closed (code $code: $reason)")
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                Log.d(TAG, "WebSocket closed: $code $reason")
                _connectionState.value = ApisConnectionState.Disconnected
                _isModelSpeaking.value = false
            }
        })

        // No connection timeout — Apis is a cognitive system that needs time to think
    }

    fun disconnect() {
        timeoutTimer?.cancel()
        timeoutTimer = null
        webSocket?.close(1000, null)
        webSocket = null
        _connectionState.value = ApisConnectionState.Disconnected
        _isModelSpeaking.value = false
        isSpeaking = false
        resolveConnect(false)
    }

    /**
     * Send raw PCM audio from the microphone.
     * Also performs client-side VAD to detect end-of-speech.
     */
    fun sendAudio(data: ByteArray) {
        if (_connectionState.value != ApisConnectionState.Ready) return

        // Send raw PCM as binary WebSocket frame
        sendExecutor.execute {
            webSocket?.send(data.toByteString(0, data.size))
        }

        // Client-side VAD
        val amplitude = calculateRMSAmplitude(data)
        val now = System.currentTimeMillis()

        if (amplitude > SILENCE_THRESHOLD) {
            if (!isSpeaking) {
                isSpeaking = true
                speechStartTime = now
                Log.d(TAG, "Speech started")
            }
            lastSpeechTime = now
        } else if (isSpeaking) {
            val speechDuration = lastSpeechTime - speechStartTime
            val silenceDuration = now - lastSpeechTime

            if (silenceDuration >= SILENCE_DURATION_MS && speechDuration >= MIN_SPEECH_DURATION_MS) {
                // End of speech detected
                isSpeaking = false
                lastUserSpeechEnd = now
                responseLatencyLogged = false
                Log.d(TAG, "End of speech detected (${speechDuration}ms speech, ${silenceDuration}ms silence)")

                sendExecutor.execute {
                    val json = JSONObject().apply {
                        put("type", "end_of_speech")
                        if (wakeWordMode) {
                            put("mode", "wake_word")
                        }
                    }
                    webSocket?.send(json.toString())
                }
            }
        }
    }

    /**
     * Send a camera frame (JPEG) to Apis for visual context.
     */
    fun sendVideoFrame(bitmap: Bitmap) {
        if (_connectionState.value != ApisConnectionState.Ready) return
        sendExecutor.execute {
            val baos = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.JPEG, HiveConfig.VIDEO_JPEG_QUALITY, baos)
            val base64 = Base64.encodeToString(baos.toByteArray(), Base64.NO_WRAP)
            val json = JSONObject().apply {
                put("type", "frame")
                put("jpeg", base64)
            }
            webSocket?.send(json.toString())
        }
    }

    // Private helpers

    private fun resolveConnect(success: Boolean) {
        val cb = connectCallback
        connectCallback = null
        timeoutTimer?.cancel()
        timeoutTimer = null
        cb?.invoke(success)
    }

    private fun handleTextMessage(text: String) {
        try {
            val json = JSONObject(text)
            val type = json.optString("type", "")

            when (type) {
                "connected" -> {
                    val lCode = json.optString("link_code", null)
                    if (!lCode.isNullOrEmpty()) {
                        LinkManager.setLinkCode(lCode)
                    }
                    val dToken = json.optString("device_token", null)
                    val existingToken = SettingsManager.hiveDeviceToken
                    if (existingToken.isNullOrEmpty() && !dToken.isNullOrEmpty()) {
                        SettingsManager.hiveDeviceToken = dToken
                    } else if (!existingToken.isNullOrEmpty()) {
                        // Re-authenticate with saved token
                        sendExecutor.execute {
                            val authMsg = JSONObject().apply {
                                put("type", "authenticate")
                                put("device_token", existingToken)
                            }
                            webSocket?.send(authMsg.toString())
                        }
                    }
                    Log.d(TAG, "Apis connected: ${json.optString("message", "")}")
                    _connectionState.value = ApisConnectionState.Ready
                    resolveConnect(true)
                }

                "thinking" -> {
                    Log.d(TAG, "Apis is thinking...")
                    _isInferring.value = true
                    _isModelSpeaking.value = false
                }

                "text" -> {
                    // Apis's text response (displayed as caption, also spoken via TTS audio)
                    val content = json.optString("content", "")
                    if (content.isNotEmpty()) {
                        Log.d(TAG, "Apis: $content")
                        onOutputTranscription?.invoke(content)
                    }
                }

                "done" -> {
                    Log.d(TAG, "Turn complete")
                    _isModelSpeaking.value = false
                    _isInferring.value = false
                    responseLatencyLogged = false
                    onTurnComplete?.invoke()
                }

                "pong" -> {
                    // Keepalive response
                }

                "error" -> {
                    val message = json.optString("message", "Unknown error")
                    Log.e(TAG, "Apis error: $message")
                    // Don't disconnect on errors — let the session continue
                }

                "wake_word_detected" -> {
                    val remainingText = json.optString("remaining_text", "")
                    Log.d(TAG, "Wake word detected! Remaining: '$remainingText'")
                    onWakeWordDetected?.invoke(remainingText)
                }

                else -> {
                    Log.d(TAG, "Unknown message type: $type")
                }
            }
        } catch (e: Exception) {
            Log.e(TAG, "Error parsing message: ${e.message}")
        }
    }

    /**
     * Calculate RMS amplitude of PCM audio for simple VAD.
     */
    private fun calculateRMSAmplitude(data: ByteArray): Int {
        if (data.size < 2) return 0
        var sumSquares = 0L
        val samples = data.size / 2
        for (i in 0 until samples) {
            val sample = (data[i * 2 + 1].toInt() shl 8) or (data[i * 2].toInt() and 0xFF)
            sumSquares += sample.toLong() * sample.toLong()
        }
        return Math.sqrt(sumSquares.toDouble() / samples).toInt()
    }
}
