package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import android.app.Application
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.graphics.Bitmap
import android.os.IBinder
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.meta.wearable.dat.externalsampleapps.cameraaccess.stream.StreamingMode
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

/**
 * ApisSessionViewModel — Binds to HiveBackgroundService for persistent sessions.
 *
 * The service owns the WebSocket, audio, and wake word detection.
 * This ViewModel just observes state and relays UI actions.
 */

data class GeminiUiState(
    val isGeminiActive: Boolean = false,
    val connectionState: ApisConnectionState = ApisConnectionState.Disconnected,
    val isModelSpeaking: Boolean = false,
    val isInferring: Boolean = false,
    val isMuted: Boolean = false,
    val errorMessage: String? = null,
    val userTranscript: String = "",
    val aiTranscript: String = "",
    val isListening: Boolean = false,
)

class GeminiSessionViewModel(application: Application) : AndroidViewModel(application) {
    companion object {
        private const val TAG = "ApisSessionVM"
    }

    private val _uiState = MutableStateFlow(GeminiUiState())
    val uiState: StateFlow<GeminiUiState> = _uiState.asStateFlow()

    private var service: HiveBackgroundService? = null
    private var bound = false
    private var stateObservationJob: Job? = null
    private var lastVideoFrameTime: Long = 0

    var streamingMode: StreamingMode = StreamingMode.GLASSES

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            val localBinder = binder as HiveBackgroundService.LocalBinder
            service = localBinder.getService()
            bound = true
            Log.d(TAG, "Bound to HiveBackgroundService")

            // Start observing service state
            stateObservationJob = viewModelScope.launch {
                while (isActive) {
                    delay(100)
                    service?.sessionState?.value?.let { state ->
                        _uiState.value = GeminiUiState(
                            isGeminiActive = state.isActive,
                            connectionState = state.connectionState,
                            isModelSpeaking = state.isModelSpeaking,
                            isInferring = state.isInferring,
                            isMuted = state.isMuted,
                            errorMessage = state.errorMessage,
                            userTranscript = state.userTranscript,
                            aiTranscript = state.aiTranscript,
                            isListening = state.isListening,
                        )
                    }
                }
            }

            // Start the session
            service?.streamingMode = streamingMode
            service?.startSession()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            service = null
            bound = false
            stateObservationJob?.cancel()
            Log.d(TAG, "Unbound from HiveBackgroundService")
        }
    }

    fun startSession() {
        if (_uiState.value.isGeminiActive) return

        if (!HiveConfig.isConfigured) {
            _uiState.value = _uiState.value.copy(
                errorMessage = "HIVE server not configured. Open Settings and add your server URL and auth token."
            )
            return
        }

        _uiState.value = _uiState.value.copy(isGeminiActive = true)

        // Start and bind to the foreground service
        val context = getApplication<Application>()
        val intent = Intent(context, HiveBackgroundService::class.java)
        ContextCompat.startForegroundService(context, intent)
        context.bindService(intent, connection, Context.BIND_AUTO_CREATE)
    }

    fun stopSession() {
        stateObservationJob?.cancel()
        stateObservationJob = null

        service?.stopSession()

        if (bound) {
            try {
                getApplication<Application>().unbindService(connection)
            } catch (e: Exception) {
                Log.w(TAG, "Unbind failed: ${e.message}")
            }
            bound = false
        }

        service = null
        _uiState.value = GeminiUiState()
    }

    fun toggleMute() {
        service?.toggleMute()
    }

    fun sendVideoFrameIfThrottled(bitmap: Bitmap) {
        if (!_uiState.value.isGeminiActive) return
        if (_uiState.value.connectionState != ApisConnectionState.Ready) return
        val now = System.currentTimeMillis()
        if (now - lastVideoFrameTime < HiveConfig.VIDEO_FRAME_INTERVAL_MS) return
        lastVideoFrameTime = now
        service?.hiveService?.sendVideoFrame(bitmap)
    }

    fun clearError() {
        _uiState.value = _uiState.value.copy(errorMessage = null)
        service?.clearError()
    }

    override fun onCleared() {
        super.onCleared()
        // Don't stop the service — let it keep running in the background
        if (bound) {
            try {
                getApplication<Application>().unbindService(connection)
            } catch (e: Exception) {
                Log.w(TAG, "Unbind on clear failed: ${e.message}")
            }
        }
    }
}
