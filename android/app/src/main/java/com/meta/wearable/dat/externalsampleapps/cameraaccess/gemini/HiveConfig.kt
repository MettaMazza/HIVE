package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager

/**
 * HiveConfig — Configuration for connecting to the HIVE WebSocket server.
 *
 * Replaces GeminiConfig. Instead of connecting to Google's BidiGenerateContent API,
 * we connect directly to Apis's /ws/glasses endpoint.
 *
 * The HIVE server handles STT (Whisper) and TTS (OpenAI) server-side,
 * so this app just streams raw PCM audio and JPEG frames.
 */
object HiveConfig {
    // Audio format (matches HIVE server expectations)
    const val INPUT_AUDIO_SAMPLE_RATE = 16000
    const val OUTPUT_AUDIO_SAMPLE_RATE = 24000
    const val AUDIO_CHANNELS = 1
    const val AUDIO_BITS_PER_SAMPLE = 16

    // Video frame throttling
    const val VIDEO_FRAME_INTERVAL_MS = 1000L
    const val VIDEO_JPEG_QUALITY = 50

    // Server URL and auth from settings
    val serverUrl: String
        get() = SettingsManager.hiveServerUrl

    val authToken: String
        get() = SettingsManager.hiveAuthToken

    fun websocketURL(): String? {
        if (serverUrl.isEmpty()) return null
        val wsUrl = serverUrl
            .replace("https://", "wss://")
            .replace("http://", "ws://")
            .trimEnd('/')
        // Connect to the bare WebSocket server — no path needed.
        // Append token query param only if authToken is set.
        return if (authToken.isNotEmpty()) {
            "$wsUrl?token=$authToken"
        } else {
            wsUrl // Dev mode — no token required
        }
    }

    val isConfigured: Boolean
        get() = serverUrl.isNotEmpty()
                && serverUrl != "https://YOUR_HIVE_SERVER"
}
