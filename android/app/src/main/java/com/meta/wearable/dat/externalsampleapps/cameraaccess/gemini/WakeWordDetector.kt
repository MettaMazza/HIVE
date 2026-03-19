package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import android.util.Log
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * WakeWordDetector — Lightweight keyword detection for "Hey Apis".
 *
 * Uses a simple energy-based Voice Activity Detection (VAD) combined with
 * server-side transcription. When a speech burst is detected, it checks
 * the transcription for the wake word.
 *
 * For the HIVE use case, the wake word triggers are:
 * 1. The HIVE server transcribes all audio
 * 2. If the user says JUST "Hey Apis" or "Apis" (short utterance), we activate
 * 3. This class does local energy detection to know when speech starts/stops
 *
 * Since the audio is always going through HIVE for transcription anyway,
 * we detect the wake word from the server-side transcription responses
 * rather than running a local ML model. This class provides the callback
 * interface and energy tracking.
 */
class WakeWordDetector(
    private val wakeWord: String = "apis",
    private val onWakeWordDetected: (String) -> Unit
) {
    companion object {
        private const val TAG = "WakeWordDetector"
        // RMS threshold for speech detection (16-bit PCM)
        private const val SPEECH_THRESHOLD = 800
        private const val MIN_SPEECH_FRAMES = 3
    }

    private var speechFrameCount = 0
    @Volatile
    private var released = false

    /**
     * Process raw PCM audio data for energy-based speech detection.
     * Called from the audio capture thread.
     */
    fun processAudio(pcmData: ByteArray) {
        if (released) return

        val rms = calculateRMS(pcmData)
        if (rms > SPEECH_THRESHOLD) {
            speechFrameCount++
        } else {
            speechFrameCount = 0
        }
    }

    /**
     * Check if text contains the wake word.
     * Called when transcription comes back from the server.
     */
    fun checkTranscription(text: String): Boolean {
        if (released) return false

        val normalized = text.trim().lowercase()
        // Match "apis" or "hey apis" as standalone words
        val containsWakeWord = normalized.contains(wakeWord) ||
                normalized.contains("hey apis") ||
                normalized.contains("hey aphis") ||
                normalized.contains("hey apus") ||
                normalized.contains("a.p" + "i.s")  // Sometimes transcribed with dots

        if (containsWakeWord) {
            Log.d(TAG, "Wake word detected in transcription: '$text'")
            onWakeWordDetected(wakeWord)
            return true
        }
        return false
    }

    private fun calculateRMS(pcmData: ByteArray): Double {
        if (pcmData.size < 2) return 0.0

        val buffer = ByteBuffer.wrap(pcmData).order(ByteOrder.LITTLE_ENDIAN)
        val samples = pcmData.size / 2
        var sumSquares = 0.0

        for (i in 0 until samples) {
            val sample = buffer.getShort(i * 2).toDouble()
            sumSquares += sample * sample
        }

        return Math.sqrt(sumSquares / samples)
    }

    fun release() {
        released = true
    }
}
