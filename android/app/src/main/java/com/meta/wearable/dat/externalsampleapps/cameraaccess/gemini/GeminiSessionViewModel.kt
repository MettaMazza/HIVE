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
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager
import com.meta.wearable.dat.externalsampleapps.cameraaccess.stream.StreamingMode
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.io.File
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
    val isDownloading: Boolean = false,
    val downloadProgress: Float = 0f,
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
    private var currentInferenceMode: SettingsManager.InferenceMode = SettingsManager.inferenceMode

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
        val modeChanged = currentInferenceMode != SettingsManager.inferenceMode
        currentInferenceMode = SettingsManager.inferenceMode

        if (_uiState.value.isGeminiActive && !modeChanged) return

        if (modeChanged) {
            Log.d(TAG, "Inference mode changed! Restarting session...")
            stopSession()
        }

        if (!SettingsManager.isWorkerMode && !HiveConfig.isConfigured) {
            _uiState.value = _uiState.value.copy(
                errorMessage = "HIVE server not configured. Open Settings and add your server URL and auth token to use Queen mode."
            )
            return
        }

        _uiState.value = _uiState.value.copy(isGeminiActive = true, errorMessage = null)

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

    fun checkAndDownloadModel() {
        val context = getApplication<Application>()
        val modelFile = File(context.filesDir, "qwen35_08b.bin")
        if (modelFile.exists()) {
            Log.d(TAG, "Model file already exists.")
            return
        }

        Log.d(TAG, "Model file missing. Starting download...")
        _uiState.value = _uiState.value.copy(isDownloading = true, downloadProgress = 0f)

        // Standard Android DownloadManager
        val request = android.app.DownloadManager.Request(android.net.Uri.parse("https://huggingface.co/Qwen/Qwen3.5-0.8B-Instruct-TFLite/resolve/main/qwen3.5-0.8b-instruct-quantized.bin"))
            .setTitle("Apis Local Brain")
            .setDescription("Downloading Qwen 3.5 0.8B Model")
            .setNotificationVisibility(android.app.DownloadManager.Request.VISIBILITY_VISIBLE)
            .setDestinationUri(android.net.Uri.fromFile(modelFile))

        val dm = context.getSystemService(Context.DOWNLOAD_SERVICE) as android.app.DownloadManager
        val downloadId = dm.enqueue(request)

        // Progress polling
        viewModelScope.launch {
            var downloading = true
            while (downloading && isActive) {
                val query = android.app.DownloadManager.Query().setFilterById(downloadId)
                val cursor = dm.query(query)
                if (cursor.moveToFirst()) {
                    val status = cursor.getInt(cursor.getColumnIndexOrThrow(android.app.DownloadManager.COLUMN_STATUS))
                    val downloaded = cursor.getLong(cursor.getColumnIndexOrThrow(android.app.DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR))
                    val total = cursor.getLong(cursor.getColumnIndexOrThrow(android.app.DownloadManager.COLUMN_TOTAL_SIZE_BYTES))
                    
                    if (total > 0) {
                        val progress = downloaded.toFloat() / total.toFloat()
                        _uiState.value = _uiState.value.copy(downloadProgress = progress)
                    }

                    if (status == android.app.DownloadManager.STATUS_SUCCESSFUL) {
                        downloading = false
                        _uiState.update { current ->
                             current.copy(isDownloading = false, downloadProgress = 1f)
                        }
                        Log.d(TAG, "Model download successful.")
                        // If session was active, restart to initialize MediaPipe
                        startSession()
                    } else if (status == android.app.DownloadManager.STATUS_FAILED) {
                        downloading = false
                        _uiState.update { it.copy(isDownloading = false, errorMessage = "Model download failed. Please check network.") }
                    }
                }
                cursor.close()
                delay(1000)
            }
        }
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
        service?.sendVideoFrameIfThrottled(bitmap)
    }

    suspend fun sendTextMessage(content: String): String {
        _uiState.value = _uiState.value.copy(isInferring = true)
        
        // Wait for service binding if a bind is in progress
        var retryCount = 0
        while (service == null && bound && retryCount < 20) {
            delay(50)
            retryCount++
        }

        // This call routes to the service
        val response = service?.sendTextMessage(content) ?: run {
            _uiState.value = _uiState.value.copy(isInferring = false)
            "HIVE bridge not ready. Please try again in a moment."
        }
        
        return response
    }

    fun clearTranscripts() {
        service?.clearTranscripts()
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
