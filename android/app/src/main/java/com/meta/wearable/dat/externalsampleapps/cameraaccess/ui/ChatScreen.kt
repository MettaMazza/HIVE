package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.ApisConnectionState
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.GeminiSessionViewModel
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.GeminiUiState
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.HiveApiClient
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager
import kotlinx.coroutines.launch

/**
 * ChatScreen — HIVE chat interface with text + voice modes.
 *
 * Text mode: Message bubbles, text input, send button.
 * Voice mode: Status orb, waveform, mic button.
 * Toggle between modes via the mode selector.
 */

data class ChatMessage(
    val content: String,
    val isUser: Boolean,
    val timestamp: Long = System.currentTimeMillis(),
)

@Composable
fun ChatScreen(
    onNavigateToSettings: () -> Unit = {},
    modifier: Modifier = Modifier,
    geminiViewModel: GeminiSessionViewModel = viewModel(),
) {
    val uiState by geminiViewModel.uiState.collectAsStateWithLifecycle()
    var isVoiceMode by remember { mutableStateOf(false) }
    var messages by remember { mutableStateOf(listOf<ChatMessage>()) }
    var inputText by remember { mutableStateOf("") }
    var isLoading by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val listState = rememberLazyListState()


    // Auto-scroll on new messages
    LaunchedEffect(messages.size) {
        if (messages.isNotEmpty()) {
            listState.animateScrollToItem(messages.size - 1)
        }
    }

    // Sync local AI responses into message list
    LaunchedEffect(uiState.aiTranscript) {
        if (SettingsManager.isWorkerMode && uiState.aiTranscript.isNotEmpty()) {
            val response = uiState.aiTranscript
            messages = messages + ChatMessage(response, isUser = false)
            geminiViewModel.clearTranscripts()
        }
    }

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(AppColor.BackgroundDark),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize(),
        ) {
            // ── Top Bar ──
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = "HIVE",
                    style = MaterialTheme.typography.headlineMedium,
                    color = AppColor.SproutGreen,
                    fontWeight = FontWeight.Bold,
                )

                Row(verticalAlignment = Alignment.CenterVertically) {
                    // Voice / Text toggle
                    IconButton(onClick = { isVoiceMode = !isVoiceMode }) {
                        Icon(
                            imageVector = if (isVoiceMode) Icons.Outlined.ChatBubbleOutline
                                else Icons.Outlined.Mic,
                            contentDescription = if (isVoiceMode) "Text mode" else "Voice mode",
                            tint = AppColor.SproutGreen,
                        )
                    }
                    IconButton(onClick = onNavigateToSettings) {
                        Icon(Icons.Default.Settings, "Settings",
                            tint = AppColor.TextSecondary)
                    }
                }
            }

            if (isVoiceMode) {
                // ═══ VOICE MODE ═══
                VoiceModeContent(
                    uiState = uiState,
                    geminiViewModel = geminiViewModel,
                    modifier = Modifier.weight(1f),
                )
            } else {
                // ═══ TEXT MODE ═══

                // Message list
                LazyColumn(
                    state = listState,
                    modifier = Modifier.weight(1f).padding(horizontal = 16.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                    contentPadding = PaddingValues(vertical = 8.dp),
                ) {
                    if (messages.isEmpty() && !uiState.isDownloading) {
                        item {
                            EmptyState()
                        }
                    }
                    if (uiState.isDownloading) {
                        item {
                            Column(modifier = Modifier.fillMaxWidth().padding(16.dp)) {
                                Text("Downloading Local Brain: ${(uiState.downloadProgress * 100).toInt()}%", style = MaterialTheme.typography.bodySmall)
                                LinearProgressIndicator(
                                    progress = uiState.downloadProgress,
                                    modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                                    color = Color(0xFF4CAF50)
                                )
                            }
                        }
                    }
                    items(messages) { message ->
                        MessageBubble(message)
                    }
                    if (isLoading || uiState.isInferring) {
                        item {
                            TypingIndicator()
                        }
                    }
                }

                // Input bar
                TextInputBar(
                    text = inputText,
                    onTextChange = { inputText = it },
                    onSend = {
                        if (inputText.isNotBlank()) {
                            val userMsg = inputText.trim()
                            inputText = ""
                            messages = messages + ChatMessage(userMsg, isUser = true)
                            isLoading = true

                            scope.launch {
                                try {
                                    val response = geminiViewModel.sendTextMessage(userMsg)
                                    if (!SettingsManager.isWorkerMode) {
                                        messages = messages + ChatMessage(
                                            response.ifEmpty { "No response." },
                                            isUser = false,
                                        )
                                    }
                                } catch (e: Exception) {
                                    messages = messages + ChatMessage(
                                        "Connection error: ${e.message}",
                                        isUser = false,
                                    )
                                }
                                isLoading = false
                            }
                        }
                    },
                    isLoading = isLoading || uiState.isInferring,
                )
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Text Mode Components
// ═══════════════════════════════════════════════════════════════

@Composable
private fun EmptyState() {
    Column(
        modifier = Modifier.fillMaxWidth().padding(top = 80.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            modifier = Modifier
                .size(64.dp)
                .clip(CircleShape)
                .background(AppColor.SproutGreen.copy(alpha = 0.1f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(Icons.Outlined.Eco, null,
                tint = AppColor.SproutGreen.copy(alpha = 0.6f),
                modifier = Modifier.size(32.dp))
        }
        Spacer(modifier = Modifier.height(16.dp))
        Text("Start a conversation with HIVE",
            color = AppColor.TextSecondary, fontSize = 15.sp)
        Spacer(modifier = Modifier.height(4.dp))
        Text("Type a message below",
            color = AppColor.TextDisabled, fontSize = 13.sp)
    }
}

@Composable
private fun MessageBubble(message: ChatMessage) {
    val isUser = message.isUser
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = if (isUser) Arrangement.End else Arrangement.Start,
    ) {
        Box(
            modifier = Modifier
                .widthIn(max = 300.dp)
                .background(
                    if (isUser) AppColor.SproutGreen.copy(alpha = 0.15f)
                    else AppColor.BackgroundCard,
                    RoundedCornerShape(
                        topStart = 16.dp,
                        topEnd = 16.dp,
                        bottomStart = if (isUser) 16.dp else 4.dp,
                        bottomEnd = if (isUser) 4.dp else 16.dp,
                    ),
                )
                .padding(horizontal = 14.dp, vertical = 10.dp),
        ) {
            Text(
                text = message.content,
                color = if (isUser) AppColor.SproutGreenLight else AppColor.TextPrimary,
                fontSize = 14.sp,
                lineHeight = 20.sp,
            )
        }
    }
}

@Composable
private fun TypingIndicator() {
    val infiniteTransition = rememberInfiniteTransition(label = "typing")
    Row(
        modifier = Modifier.padding(start = 4.dp, top = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        repeat(3) { i ->
            val alpha by infiniteTransition.animateFloat(
                initialValue = 0.3f, targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    tween(500, delayMillis = i * 150),
                    RepeatMode.Reverse,
                ),
                label = "dot$i",
            )
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .clip(CircleShape)
                    .background(AppColor.SproutGreen.copy(alpha = alpha)),
            )
        }
        Spacer(modifier = Modifier.width(8.dp))
        Text("HIVE is thinking...",
            color = AppColor.TextDisabled, fontSize = 12.sp)
    }
}

@Composable
private fun TextInputBar(
    text: String,
    onTextChange: (String) -> Unit,
    onSend: () -> Unit,
    isLoading: Boolean,
) {
    Surface(
        color = AppColor.BackgroundElevated,
        tonalElevation = 0.dp,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = text,
                onValueChange = onTextChange,
                placeholder = { Text("Message HIVE...", color = AppColor.TextDisabled) },
                modifier = Modifier.weight(1f),
                shape = RoundedCornerShape(24.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = AppColor.SproutGreen,
                    unfocusedBorderColor = AppColor.Border,
                    cursorColor = AppColor.SproutGreen,
                    focusedTextColor = AppColor.TextPrimary,
                    unfocusedTextColor = AppColor.TextPrimary,
                ),
                singleLine = false,
                maxLines = 4,
            )
            Spacer(modifier = Modifier.width(8.dp))
            FloatingActionButton(
                onClick = onSend,
                modifier = Modifier.size(48.dp),
                shape = CircleShape,
                containerColor = if (text.isNotBlank() && !isLoading)
                    AppColor.SproutGreen else AppColor.BackgroundCard,
                contentColor = if (text.isNotBlank() && !isLoading)
                    AppColor.BackgroundDark else AppColor.TextDisabled,
                elevation = FloatingActionButtonDefaults.elevation(0.dp),
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, "Send", modifier = Modifier.size(22.dp))
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Voice Mode Components
// ═══════════════════════════════════════════════════════════════

@Composable
private fun VoiceModeContent(
    uiState: GeminiUiState,
    geminiViewModel: GeminiSessionViewModel,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(horizontal = 24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(modifier = Modifier.weight(1f))

        StatusOrb(uiState = uiState)

        Spacer(modifier = Modifier.height(24.dp))

        Text(
            text = when {
                uiState.isModelSpeaking -> "Apis is speaking..."
                uiState.isListening -> "Listening..."
                uiState.connectionState == ApisConnectionState.Ready -> "Say \"Hey Apis\" or tap the mic"
                uiState.connectionState is ApisConnectionState.Connecting -> "Connecting..."
                uiState.isGeminiActive -> "Connected"
                else -> "Tap to start"
            },
            color = AppColor.TextSecondary,
            style = MaterialTheme.typography.bodyLarge,
        )

        Spacer(modifier = Modifier.height(16.dp))

        if (uiState.userTranscript.isNotEmpty() || uiState.aiTranscript.isNotEmpty()) {
            TranscriptCard(
                userTranscript = uiState.userTranscript,
                aiTranscript = uiState.aiTranscript,
            )
        }

        Spacer(modifier = Modifier.weight(1f))

        MicButton(
            isActive = uiState.isGeminiActive,
            isListening = uiState.isListening,
            onClick = {
                if (uiState.isGeminiActive) geminiViewModel.stopSession()
                else {
                    // Trigger checkAndDownloadModel in startSession
                    geminiViewModel.startSession()
                    if (SettingsManager.isWorkerMode) {
                        geminiViewModel.checkAndDownloadModel()
                    }
                }
            },
        )

        Spacer(modifier = Modifier.height(32.dp))
    }
}

@Composable
private fun StatusOrb(uiState: GeminiUiState) {
    val infiniteTransition = rememberInfiniteTransition(label = "orb")
    val glowAlpha by infiniteTransition.animateFloat(
        initialValue = 0.3f, targetValue = 0.8f,
        animationSpec = infiniteRepeatable(
            tween(2000, easing = EaseInOutCubic),
            RepeatMode.Reverse,
        ), label = "glow",
    )
    val pulseScale by infiniteTransition.animateFloat(
        initialValue = 1f,
        targetValue = if (uiState.isModelSpeaking) 1.15f else 1.05f,
        animationSpec = infiniteRepeatable(
            tween(if (uiState.isModelSpeaking) 600 else 2000, easing = EaseInOutCubic),
            RepeatMode.Reverse,
        ), label = "pulse",
    )

    val orbColor = when {
        uiState.isModelSpeaking -> AppColor.SproutGreen
        uiState.isListening -> AppColor.SproutGreenLight
        uiState.connectionState == ApisConnectionState.Ready -> AppColor.SproutGreen
        uiState.connectionState is ApisConnectionState.Connecting -> AppColor.StatusWarning
        uiState.isGeminiActive -> AppColor.SproutGreenDark
        else -> AppColor.TextDisabled
    }

    Box(contentAlignment = Alignment.Center) {
        if (uiState.isGeminiActive) {
            Box(
                modifier = Modifier
                    .size((120 * pulseScale).dp)
                    .clip(CircleShape)
                    .background(Brush.radialGradient(
                        listOf(orbColor.copy(alpha = glowAlpha * 0.4f), orbColor.copy(alpha = 0f))
                    )),
            )
        }
        Box(
            modifier = Modifier
                .size((80 * pulseScale).dp)
                .shadow(
                    if (uiState.isGeminiActive) 16.dp else 4.dp,
                    CircleShape,
                    ambientColor = orbColor.copy(alpha = 0.3f),
                    spotColor = orbColor.copy(alpha = 0.5f),
                )
                .clip(CircleShape)
                .background(Brush.radialGradient(
                    listOf(orbColor.copy(alpha = 0.9f), orbColor.copy(alpha = 0.6f))
                )),
            contentAlignment = Alignment.Center,
        ) {
            if (uiState.isModelSpeaking || uiState.isListening) {
                WaveformBars(isSpeaking = uiState.isModelSpeaking)
            }
        }
    }
}

@Composable
private fun WaveformBars(isSpeaking: Boolean) {
    val infiniteTransition = rememberInfiniteTransition(label = "wave")
    Row(
        horizontalArrangement = Arrangement.spacedBy(3.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        repeat(5) { i ->
            val height by infiniteTransition.animateFloat(
                initialValue = 8f,
                targetValue = if (isSpeaking) 28f else 18f,
                animationSpec = infiniteRepeatable(
                    tween(if (isSpeaking) 300 else 500, delayMillis = i * 80, easing = EaseInOutCubic),
                    RepeatMode.Reverse,
                ), label = "bar$i",
            )
            Box(
                modifier = Modifier.width(4.dp).height(height.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(AppColor.BackgroundDark.copy(alpha = 0.7f)),
            )
        }
    }
}

@Composable
private fun TranscriptCard(userTranscript: String, aiTranscript: String) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
        shape = RoundedCornerShape(16.dp),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            if (userTranscript.isNotEmpty()) {
                Text(userTranscript, color = AppColor.TextSecondary, fontSize = 14.sp,
                    maxLines = 3, overflow = TextOverflow.Ellipsis)
            }
            if (aiTranscript.isNotEmpty()) {
                if (userTranscript.isNotEmpty()) Spacer(modifier = Modifier.height(8.dp))
                Text(aiTranscript, color = AppColor.SproutGreenLight, fontSize = 15.sp,
                    maxLines = 5, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

@Composable
private fun MicButton(isActive: Boolean, isListening: Boolean, onClick: () -> Unit) {
    FloatingActionButton(
        onClick = onClick,
        modifier = Modifier.size(72.dp),
        shape = CircleShape,
        containerColor = if (isActive) AppColor.SproutGreen else AppColor.BackgroundElevated,
        contentColor = if (isActive) AppColor.BackgroundDark else AppColor.TextSecondary,
        elevation = FloatingActionButtonDefaults.elevation(
            defaultElevation = if (isActive) 8.dp else 2.dp,
        ),
    ) {
        Icon(
            imageVector = if (isActive) Icons.Default.Mic else Icons.Default.MicOff,
            contentDescription = if (isActive) "Stop" else "Start",
            modifier = Modifier.size(32.dp),
        )
    }
}
