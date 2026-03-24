package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.graphics.Bitmap
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import com.meta.wearable.dat.externalsampleapps.cameraaccess.MainActivity
import com.meta.wearable.dat.externalsampleapps.cameraaccess.R
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager
import com.meta.wearable.dat.externalsampleapps.cameraaccess.stream.StreamingMode
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.util.Timer
import java.util.TimerTask

/**
 * HiveBackgroundService — Keeps Apis running when the app is backgrounded.
 *
 * Owns the WebSocket connection, audio capture, and audio playback.
 * Shows a persistent notification so Android doesn't kill the process.
 *
 * Wake word "Hey Apis" support:
 * - Idle state: mic is captured, wake word detection runs locally
 * - Active state: audio is sent to Apis
 * - After Apis responds, stays active for 15s then goes idle
 * - Saying "Hey Apis" during playback interrupts and starts listening
 */
class HiveBackgroundService : Service() {
    companion object {
        private const val TAG = "ApisService"
        private const val CHANNEL_ID = "hive_foreground"
        private const val NOTIFICATION_ID = 1
        private const val IDLE_TIMEOUT_MS = 15000L
        const val ACTION_START = "com.hive.START"
        const val ACTION_STOP = "com.hive.STOP"
    }

    // Service binding
    inner class LocalBinder : Binder() {
        fun getService(): HiveBackgroundService = this@HiveBackgroundService
    }
    private val binder = LocalBinder()
    override fun onBind(intent: Intent?): IBinder = binder

    // Core services
    private var remoteService: HiveLiveService? = null
    private var localService: LocalInferenceService? = null
    
    val audioManager = AudioManager()
    private var wakeWordDetector: WakeWordDetector? = null
    private val serviceScope = CoroutineScope(Dispatchers.Main + SupervisorJob())
    private var stateObservationJob: Job? = null
    private var idleTimer: Timer? = null

    // State
    private val _sessionState = MutableStateFlow(SessionState())
    val sessionState: StateFlow<SessionState> = _sessionState.asStateFlow()

    var streamingMode: StreamingMode = StreamingMode.GLASSES

    data class SessionState(
        val isActive: Boolean = false,
        val isListening: Boolean = false,
        val connectionState: ApisConnectionState = ApisConnectionState.Disconnected,
        val isModelSpeaking: Boolean = false,
        val isInferring: Boolean = false,
        val isMuted: Boolean = false,
        val errorMessage: String? = null,
        val userTranscript: String = "",
        val aiTranscript: String = "",
    )

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopSession()
                stopSelf()
                return START_NOT_STICKY
            }
        }
        return START_STICKY
    }

    fun startSession() {
        if (_sessionState.value.isActive) return

        val isWorker = SettingsManager.isWorkerMode
        
        if (!isWorker && !HiveConfig.isConfigured) {
            _sessionState.value = _sessionState.value.copy(
                errorMessage = "HIVE server not configured. Open Settings and add your server URL and auth token."
            )
            return
        }

        // Start as foreground service
        val statusMsg = if (isWorker) "Apis (Local Worker) is ready" else "Connecting to Apis (Queen)..."
        startForeground(NOTIFICATION_ID, buildNotification(statusMsg))
        _sessionState.value = _sessionState.value.copy(isActive = true)

        if (isWorker) {
            localService = LocalInferenceService(this)
            wireService(localService!!, null)
            localService?.connect { connected ->
               if (connected) onServiceReady()
            }
        } else {
            remoteService = HiveLiveService()
            wireService(null, remoteService!!)
            remoteService?.connect { connected ->
               if (connected) onServiceReady()
            }
        }

        // Start observing state
        stateObservationJob = serviceScope.launch {
            while (isActive) {
                delay(100)
                val currentLocal = localService
                val currentRemote = remoteService
                
                if (currentLocal != null) {
                    _sessionState.update { current ->
                        current.copy(
                            connectionState = currentLocal.connectionState.value,
                            isModelSpeaking = currentLocal.isModelSpeaking.value,
                            isInferring = currentLocal.isInferring.value
                        )
                    }
                } else if (currentRemote != null) {
                    _sessionState.update { current ->
                        current.copy(
                            connectionState = currentRemote.connectionState.value,
                            isModelSpeaking = currentRemote.isModelSpeaking.value,
                            isInferring = currentRemote.isInferring.value
                        )
                    }
                }
            }
        }
    }

    private fun wireService(local: LocalInferenceService?, remote: HiveLiveService?) {
        // Shared audio input
        audioManager.onAudioCaptured = lambda@{ data ->
            if (isModelSpeaking()) return@lambda
            local?.sendAudio(data)
            remote?.sendAudio(data)
        }

        // Shared callbacks
        val onAudio = { data: ByteArray -> audioManager.playAudio(data) }
        val onTurn = {
            _sessionState.value = _sessionState.value.copy(userTranscript = "")
            startIdleTimer()
        }
        val onInterrupted = { audioManager.stopPlayback() }
        val onInput = { text: String ->
            _sessionState.update { current ->
                current.copy(
                    userTranscript = current.userTranscript + text,
                    aiTranscript = ""
                )
            }
        }
        val onOutput = { text: String ->
            _sessionState.update { it.copy(aiTranscript = text) }
        }

        local?.apply {
            onAudioReceived = onAudio
            onTurnComplete = onTurn
            this.onInterrupted = onInterrupted
            onInputTranscription = onInput
            onOutputTranscription = onOutput
        }
        remote?.apply {
            onAudioReceived = onAudio
            onTurnComplete = onTurn
            this.onInterrupted = onInterrupted
            onInputTranscription = onInput
            onOutputTranscription = onOutput
            onWakeWordDetected = { onWakeWordDetected() }
            onDisconnected = { reason -> 
                if (_sessionState.value.isActive) {
                    stopSession()
                    _sessionState.value = _sessionState.value.copy(
                        errorMessage = "Connection lost: $reason"
                    )
                }
            }
        }
    }

    private fun onServiceReady() {
        try {
            audioManager.startCapture()
            _sessionState.value = _sessionState.value.copy(isListening = true)
            remoteService?.wakeWordMode = false
            updateNotification("Apis is listening 🎧")
        } catch (e: Exception) {
            _sessionState.value = _sessionState.value.copy(errorMessage = "Mic failed: ${e.message}")
            stopSession()
        }
    }

    private fun isModelSpeaking(): Boolean {
        return localService?.isModelSpeaking?.value == true || remoteService?.isModelSpeaking?.value == true
    }

    fun stopSession() {
        cancelIdleTimer()
        audioManager.stopCapture()
        
        remoteService?.disconnect()
        remoteService = null
        
        localService?.release()
        localService = null
        
        wakeWordDetector?.release()
        wakeWordDetector = null
        stateObservationJob?.cancel()
        stateObservationJob = null
        _sessionState.value = SessionState()
        stopForeground(STOP_FOREGROUND_REMOVE)
    }

    fun sendVideoFrameIfThrottled(bitmap: Bitmap) {
        if (!_sessionState.value.isActive) return
        if (_sessionState.value.connectionState != ApisConnectionState.Ready) return
        remoteService?.sendVideoFrame(bitmap)
        localService?.sendVideoFrame(bitmap)
    }

    fun clearError() {
        _sessionState.value = _sessionState.value.copy(errorMessage = null)
    }

    fun toggleMute() {
        val newMuted = !audioManager.isMuted
        audioManager.isMuted = newMuted
        _sessionState.value = _sessionState.value.copy(isMuted = newMuted)
    }

    // ─── Wake Word ──────────────────────────────────────────────

    suspend fun sendTextMessage(content: String): String = withContext(Dispatchers.IO) {
        if (SettingsManager.isWorkerMode) {
            val local = localService ?: return@withContext "Local Worker not initialized"
            // For Worker mode, we return a deferred response
            local.processText(content)
            "Processing locally..." 
        } else {
            val client = HiveApiClient(SettingsManager.hiveServerUrl).apply {
                accessToken = SettingsManager.hiveAuthToken
            }
            client.sendMessage(content)
        }
    }

    fun clearTranscripts() {
        _sessionState.value = _sessionState.value.copy(
            userTranscript = "",
            aiTranscript = ""
        )
    }

    private fun onWakeWordDetected() {
        cancelIdleTimer()

        // If Apis is speaking, interrupt
        if (_sessionState.value.isModelSpeaking) {
            audioManager.stopPlayback()
        }

        // Activate listening
        _sessionState.value = _sessionState.value.copy(
            isListening = true,
            userTranscript = "",
            aiTranscript = ""
        )
        remoteService?.wakeWordMode = false
        updateNotification("Apis is listening 🎧")
        Log.d(TAG, "Wake word activated — listening")
    }

    private fun startIdleTimer() {
        cancelIdleTimer()
        // Wake word disabled for now — session stays in listening mode.
        // To re-enable, uncomment the timer schedule below.
        /*
        idleTimer = Timer().apply {
            schedule(object : TimerTask() {
                override fun run() {
                    Log.d(TAG, "Idle timeout — waiting for wake word")
                    _sessionState.value = _sessionState.value.copy(isListening = false)
                    remoteService?.wakeWordMode = true
                    updateNotification("Say \"Apis\" to talk")
                }
            }, IDLE_TIMEOUT_MS)
        }
        */
    }

    private fun cancelIdleTimer() {
        idleTimer?.cancel()
        idleTimer = null
    }

    // ─── Notification ───────────────────────────────────────────

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID, "Apis Assistant",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Keeps Apis listening in the background"
                setShowBadge(false)
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(text: String): Notification {
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP
        }
        val pendingIntent = PendingIntent.getActivity(
            this, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        val stopIntent = Intent(this, HiveBackgroundService::class.java).apply {
            action = ACTION_STOP
        }
        val stopPendingIntent = PendingIntent.getService(
            this, 1, stopIntent,
            PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("Apis")
            .setContentText(text)
            .setSmallIcon(R.drawable.camera_access_icon)
            .setContentIntent(pendingIntent)
            .addAction(0, "Stop", stopPendingIntent)
            .setOngoing(true)
            .setSilent(true)
            .build()
    }

    private fun updateNotification(text: String) {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification(text))
    }

    override fun onDestroy() {
        stopSession()
        serviceScope.cancel()
        super.onDestroy()
    }
}
