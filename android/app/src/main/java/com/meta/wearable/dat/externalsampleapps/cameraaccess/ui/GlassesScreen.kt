package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import android.Manifest
import android.content.pm.PackageManager
import android.media.AudioManager
import android.media.ToneGenerator
import android.os.VibrationEffect
import android.os.Vibrator
import android.widget.Toast
import androidx.activity.compose.LocalActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.meta.wearable.dat.core.types.Permission
import com.meta.wearable.dat.core.types.PermissionStatus
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.ApisConnectionState
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.GeminiSessionViewModel
import com.meta.wearable.dat.externalsampleapps.cameraaccess.wearables.WearablesViewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive

/**
 * LiveCallScreen — Voice call with HIVE.
 *
 * - Vibrating vocal signature waveform
 * - Live transcript
 * - Toggle for glasses camera stream (disabled if not connected)
 * - Phone camera photo button with front/back flip
 */
@Composable
fun LiveCallScreen(
    viewModel: WearablesViewModel,
    onRequestWearablesPermission: suspend (Permission) -> PermissionStatus = { PermissionStatus.Denied },
    modifier: Modifier = Modifier,
    geminiViewModel: GeminiSessionViewModel = viewModel(),
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val geminiState by geminiViewModel.uiState.collectAsStateWithLifecycle()
    val activity = LocalActivity.current
    val context = LocalContext.current

    var glassesStreamOn by remember { mutableStateOf(false) }
    var showStreamView by remember { mutableStateOf(false) } // Show video feed on screen
    var showPhoneCamera by remember { mutableStateOf(false) }
    var useFrontCamera by remember { mutableStateOf(true) }

    val glassesConnected = uiState.isRegistered

    // Determine "thinking" state: server explicitly signals during inference
    val isThinking = geminiState.isInferring

    // Ambient thinking sound
    ThinkingSound(isThinking = isThinking)

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(AppColor.BackgroundDark),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.height(12.dp))

            // ── Header ──
            Text(
                text = "Live Call",
                style = MaterialTheme.typography.headlineMedium,
                color = AppColor.TextPrimary,
                fontWeight = FontWeight.Bold,
            )
            Spacer(modifier = Modifier.height(4.dp))
            Text(
                text = if (geminiState.isGeminiActive) "Connected to HIVE"
                    else "Tap the call button to start",
                color = AppColor.TextSecondary,
                fontSize = 13.sp,
            )

            Spacer(modifier = Modifier.height(24.dp))

            // ── Vocal Signature Waveform ──
            VocalSignature(
                isActive = geminiState.isGeminiActive,
                isSpeaking = geminiState.isModelSpeaking,
                isListening = geminiState.isListening,
            )

            Spacer(modifier = Modifier.height(16.dp))

            // Status
            Text(
                text = when {
                    geminiState.isModelSpeaking -> "HIVE is speaking..."
                    geminiState.isListening -> "Listening..."
                    isThinking -> "Thinking..."
                    geminiState.connectionState == ApisConnectionState.Ready -> "Ready"
                    geminiState.connectionState is ApisConnectionState.Connecting -> "Connecting..."
                    geminiState.isGeminiActive -> "Connected"
                    else -> "Offline"
                },
                color = when {
                    geminiState.isModelSpeaking -> AppColor.SproutGreen
                    geminiState.isListening -> AppColor.SproutGreenLight
                    isThinking -> Color(0xFFFFB74D) // amber for thinking
                    geminiState.isGeminiActive -> AppColor.StatusOnline
                    else -> AppColor.TextDisabled
                },
                fontWeight = FontWeight.SemiBold,
                fontSize = 14.sp,
            )

            Spacer(modifier = Modifier.height(20.dp))

            // ── Transcript ──
            if (geminiState.userTranscript.isNotEmpty() || geminiState.aiTranscript.isNotEmpty()) {
                Card(
                    modifier = Modifier.fillMaxWidth().weight(1f, fill = false),
                    colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
                    shape = RoundedCornerShape(16.dp),
                ) {
                    Column(
                        modifier = Modifier.padding(16.dp)
                            .verticalScroll(rememberScrollState()),
                    ) {
                        if (geminiState.userTranscript.isNotEmpty()) {
                            Text("You", color = AppColor.TextDisabled, fontSize = 11.sp,
                                fontWeight = FontWeight.SemiBold)
                            Text(geminiState.userTranscript, color = AppColor.TextSecondary,
                                fontSize = 14.sp, lineHeight = 20.sp)
                            Spacer(modifier = Modifier.height(12.dp))
                        }
                        if (geminiState.aiTranscript.isNotEmpty()) {
                            Text("HIVE", color = AppColor.SproutGreen.copy(alpha = 0.7f),
                                fontSize = 11.sp, fontWeight = FontWeight.SemiBold)
                            Text(geminiState.aiTranscript, color = AppColor.SproutGreenLight,
                                fontSize = 14.sp, lineHeight = 20.sp)
                        }
                    }
                }
                Spacer(modifier = Modifier.height(16.dp))
            } else {
                Spacer(modifier = Modifier.weight(1f))
            }

            // ── Controls Row ──
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceEvenly,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // Glasses — tap to stream in background, long press to show video
                LongPressGlassesButton(
                    isStreaming = glassesStreamOn,
                    isShowingVideo = showStreamView,
                    isEnabled = glassesConnected,
                    onTapped = {
                        // Tap toggles background streaming (camera data flows, no UI switch)
                        if (!glassesStreamOn) {
                            viewModel.startBackgroundStreaming(onRequestWearablesPermission)
                            glassesStreamOn = true
                        } else {
                            viewModel.stopBackgroundStreaming()
                            glassesStreamOn = false
                            showStreamView = false
                        }
                    },
                    onLongPressed = {
                        // Long press toggles showing the full-screen video feed
                        if (glassesStreamOn) {
                            if (!showStreamView) {
                                // Switch to full stream view
                                viewModel.navigateToStreaming(onRequestWearablesPermission)
                                showStreamView = true
                            } else {
                                // Back to background-only
                                viewModel.startBackgroundStreaming(onRequestWearablesPermission)
                                showStreamView = false
                            }
                        }
                    },
                )

                // Main call button
                FloatingActionButton(
                    onClick = {
                        if (geminiState.isGeminiActive) {
                            geminiViewModel.stopSession()
                        } else {
                            geminiViewModel.startSession()
                        }
                    },
                    modifier = Modifier.size(72.dp),
                    shape = CircleShape,
                    containerColor = if (geminiState.isGeminiActive)
                        AppColor.StatusError else AppColor.SproutGreen,
                    contentColor = Color.White,
                    elevation = FloatingActionButtonDefaults.elevation(
                        defaultElevation = if (geminiState.isGeminiActive) 4.dp else 8.dp,
                    ),
                ) {
                    Icon(
                        imageVector = if (geminiState.isGeminiActive)
                            Icons.Filled.CallEnd else Icons.Filled.Call,
                        contentDescription = if (geminiState.isGeminiActive) "End" else "Call",
                        modifier = Modifier.size(32.dp),
                    )
                }

                // Mute button — only shown during active call
                if (geminiState.isGeminiActive) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        IconButton(
                            onClick = { geminiViewModel.toggleMute() },
                            modifier = Modifier
                                .size(52.dp)
                                .clip(CircleShape)
                                .background(
                                    if (geminiState.isMuted) AppColor.StatusError.copy(alpha = 0.2f)
                                    else AppColor.BackgroundCard,
                                ),
                        ) {
                            Icon(
                                imageVector = if (geminiState.isMuted) Icons.Filled.MicOff
                                    else Icons.Filled.Mic,
                                contentDescription = if (geminiState.isMuted) "Unmute" else "Mute",
                                tint = if (geminiState.isMuted) AppColor.StatusError else AppColor.TextSecondary,
                                modifier = Modifier.size(24.dp),
                            )
                        }
                        Spacer(modifier = Modifier.height(4.dp))
                        Text(
                            if (geminiState.isMuted) "Muted" else "Mic",
                            color = if (geminiState.isMuted) AppColor.StatusError else AppColor.TextSecondary,
                            fontSize = 10.sp,
                        )
                    }
                }

                // Phone camera photo button
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    IconButton(
                        onClick = { showPhoneCamera = !showPhoneCamera },
                        modifier = Modifier
                            .size(52.dp)
                            .clip(CircleShape)
                            .background(
                                if (showPhoneCamera) AppColor.SproutGreen.copy(alpha = 0.2f)
                                else AppColor.BackgroundCard,
                            ),
                    ) {
                        Icon(
                            Icons.Outlined.CameraAlt, "Photo",
                            tint = if (showPhoneCamera) AppColor.SproutGreen else AppColor.TextSecondary,
                            modifier = Modifier.size(24.dp),
                        )
                    }
                    Spacer(modifier = Modifier.height(4.dp))
                    Text(
                        "Photo",
                        color = if (showPhoneCamera) AppColor.SproutGreen else AppColor.TextSecondary,
                        fontSize = 10.sp,
                    )
                }
            }

            // Camera flip (only when phone camera is showing)
            AnimatedVisibility(visible = showPhoneCamera) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
                    horizontalArrangement = Arrangement.Center,
                ) {
                    TextButton(onClick = { useFrontCamera = !useFrontCamera }) {
                        Icon(Icons.Outlined.FlipCameraAndroid, null,
                            tint = AppColor.SproutGreen, modifier = Modifier.size(20.dp))
                        Spacer(modifier = Modifier.width(8.dp))
                        Text(
                            if (useFrontCamera) "Switch to Back Camera"
                                else "Switch to Front Camera",
                            color = AppColor.SproutGreen, fontSize = 13.sp,
                        )
                    }
                }
            }

            Spacer(modifier = Modifier.height(16.dp))
        }

        // Phone camera preview overlay (bottom half)
        if (showPhoneCamera) {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .fillMaxHeight(0.35f)
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 80.dp) // above controls
                    .clip(RoundedCornerShape(topStart = 16.dp, topEnd = 16.dp))
                    .background(Color.Black),
            ) {
                PhoneCameraPreview(useFrontCamera = useFrontCamera)
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Vocal Signature Waveform
// ═══════════════════════════════════════════════════════════════

@Composable
private fun VocalSignature(
    isActive: Boolean,
    isSpeaking: Boolean,
    isListening: Boolean,
) {
    val infiniteTransition = rememberInfiniteTransition(label = "vocal")

    val baseColor = when {
        isSpeaking -> AppColor.SproutGreen
        isListening -> AppColor.SproutGreenLight
        isActive -> AppColor.SproutGreenDark
        else -> AppColor.TextDisabled
    }

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(80.dp)
            .clip(RoundedCornerShape(16.dp))
            .background(AppColor.BackgroundCard)
            .padding(horizontal = 16.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val barCount = 24
        repeat(barCount) { i ->
            val maxHeight = when {
                isSpeaking -> 56f
                isListening -> 40f
                isActive -> 20f
                else -> 8f
            }
            val speed = when {
                isSpeaking -> 200 + (i % 5) * 60
                isListening -> 400 + (i % 3) * 100
                else -> 1500
            }

            val height by infiniteTransition.animateFloat(
                initialValue = 4f,
                targetValue = maxHeight * (0.4f + 0.6f * ((i * 7 + 3) % barCount).toFloat() / barCount),
                animationSpec = infiniteRepeatable(
                    animation = tween(speed, delayMillis = i * 30, easing = EaseInOutCubic),
                    repeatMode = RepeatMode.Reverse,
                ),
                label = "bar$i",
            )

            Box(
                modifier = Modifier
                    .width(3.dp)
                    .height(if (isActive || isSpeaking || isListening) height.dp else 4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(
                        Brush.verticalGradient(
                            listOf(
                                baseColor.copy(alpha = 0.9f),
                                baseColor.copy(alpha = 0.4f),
                            ),
                        ),
                    ),
            )
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Thinking Sound — soft ambient pulse while waiting
// ═══════════════════════════════════════════════════════════════

@Composable
private fun ThinkingSound(isThinking: Boolean) {
    DisposableEffect(isThinking) {
        if (!isThinking) {
            onDispose { }
        } else {
            // Generate a soft 440Hz sine wave pulse using AudioTrack
            // (ToneGenerator is unreliable across devices)
            val sampleRate = 16000
            val durationMs = 200
            val numSamples = sampleRate * durationMs / 1000
            val samples = ShortArray(numSamples)
            val freq = 440.0

            for (i in samples.indices) {
                val t = i.toDouble() / sampleRate
                // Fade envelope: ramp up first 20%, sustain, ramp down last 20%
                val fadeIn = (i.toFloat() / (numSamples * 0.2f)).coerceAtMost(1f)
                val fadeOut = ((numSamples - i).toFloat() / (numSamples * 0.2f)).coerceAtMost(1f)
                val envelope = fadeIn * fadeOut
                // Low volume sine wave (amplitude ~2000 out of 32767)
                samples[i] = (Math.sin(2.0 * Math.PI * freq * t) * 2000 * envelope).toInt().toShort()
            }

            val bufSize = android.media.AudioTrack.getMinBufferSize(
                sampleRate,
                android.media.AudioFormat.CHANNEL_OUT_MONO,
                android.media.AudioFormat.ENCODING_PCM_16BIT,
            ).coerceAtLeast(samples.size * 2)

            var track: android.media.AudioTrack? = try {
                android.media.AudioTrack.Builder()
                    .setAudioAttributes(
                        android.media.AudioAttributes.Builder()
                            .setUsage(android.media.AudioAttributes.USAGE_MEDIA)
                            .setContentType(android.media.AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build()
                    )
                    .setAudioFormat(
                        android.media.AudioFormat.Builder()
                            .setSampleRate(sampleRate)
                            .setChannelMask(android.media.AudioFormat.CHANNEL_OUT_MONO)
                            .setEncoding(android.media.AudioFormat.ENCODING_PCM_16BIT)
                            .build()
                    )
                    .setBufferSizeInBytes(bufSize)
                    .setTransferMode(android.media.AudioTrack.MODE_STATIC)
                    .build()
            } catch (e: Exception) { null }

            val running = java.util.concurrent.atomic.AtomicBoolean(track != null)

            val thread = if (track != null) {
                track.write(samples, 0, samples.size)
                Thread {
                    while (running.get()) {
                        try {
                            track!!.stop()
                            track!!.reloadStaticData()
                            track!!.play()
                            Thread.sleep(2500)
                        } catch (e: Exception) { break }
                    }
                }.apply { isDaemon = true; start() }
            } else null

            onDispose {
                running.set(false)
                thread?.interrupt()
                try { track?.stop() } catch (_: Exception) { }
                try { track?.release() } catch (_: Exception) { }
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Long Press Glasses Button — 5s hold to toggle video stream
// ═══════════════════════════════════════════════════════════════

@Composable
private fun LongPressGlassesButton(
    isStreaming: Boolean,
    isShowingVideo: Boolean,
    isEnabled: Boolean,
    onTapped: () -> Unit,
    onLongPressed: () -> Unit,
) {
    val context = LocalContext.current
    var holdProgress by remember { mutableFloatStateOf(0f) }
    var isHolding by remember { mutableStateOf(false) }
    var wasLongPress by remember { mutableStateOf(false) }

    // Animate progress smoothly
    val animatedProgress by animateFloatAsState(
        targetValue = holdProgress,
        animationSpec = tween(100),
        label = "holdProgress",
    )

    // Long press timer
    LaunchedEffect(isHolding) {
        if (isHolding && isEnabled && isStreaming) {
            wasLongPress = false
            holdProgress = 0f
            val startTime = System.currentTimeMillis()
            val holdDuration = 5000L
            while (true) {
                val elapsed = System.currentTimeMillis() - startTime
                holdProgress = (elapsed.toFloat() / holdDuration).coerceIn(0f, 1f)
                if (holdProgress >= 1f) {
                    val vibrator = context.getSystemService(Vibrator::class.java)
                    vibrator?.vibrate(VibrationEffect.createOneShot(100, VibrationEffect.DEFAULT_AMPLITUDE))
                    wasLongPress = true
                    onLongPressed()
                    holdProgress = 0f
                    isHolding = false
                    break
                }
                delay(50)
            }
        } else {
            holdProgress = 0f
        }
    }

    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier
                .size(52.dp)
                .clip(CircleShape)
                .background(
                    when {
                        isShowingVideo -> AppColor.SproutGreen.copy(alpha = 0.3f)
                        isStreaming -> AppColor.SproutGreen.copy(alpha = 0.15f)
                        isEnabled -> AppColor.BackgroundCard
                        else -> AppColor.BackgroundCard.copy(alpha = 0.5f)
                    },
                )
                .then(
                    if (animatedProgress > 0f) {
                        Modifier.drawBehind {
                            val strokeWidth = 3.dp.toPx()
                            val radius = (size.minDimension - strokeWidth) / 2
                            drawArc(
                                color = AppColor.SproutGreen,
                                startAngle = -90f,
                                sweepAngle = animatedProgress * 360f,
                                useCenter = false,
                                topLeft = Offset(
                                    (size.width - radius * 2) / 2,
                                    (size.height - radius * 2) / 2,
                                ),
                                size = Size(radius * 2, radius * 2),
                                style = Stroke(width = strokeWidth, cap = StrokeCap.Round),
                            )
                        }
                    } else Modifier
                )
                .pointerInput(isEnabled, isStreaming) {
                    detectTapGestures(
                        onTap = {
                            if (!isEnabled) {
                                Toast.makeText(context, "Connect glasses first in Meta AI app",
                                    Toast.LENGTH_SHORT).show()
                            } else if (!wasLongPress) {
                                onTapped()
                            }
                            wasLongPress = false
                        },
                        onPress = {
                            if (isEnabled && isStreaming) {
                                isHolding = true
                                tryAwaitRelease()
                                isHolding = false
                            }
                        },
                    )
                },
        ) {
            Icon(
                imageVector = if (isShowingVideo) Icons.Filled.Visibility
                    else Icons.Outlined.Visibility,
                contentDescription = "Glasses",
                tint = when {
                    isStreaming -> AppColor.SproutGreen
                    isEnabled -> AppColor.TextSecondary
                    else -> AppColor.TextDisabled
                },
                modifier = Modifier.size(24.dp),
            )
        }
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = when {
                !isEnabled -> "Not connected"
                isHolding -> "Hold..."
                isShowingVideo -> "Viewing"
                isStreaming -> "Live"
                else -> "Stream"
            },
            color = when {
                isStreaming -> AppColor.SproutGreen
                isEnabled -> AppColor.TextSecondary
                else -> AppColor.TextDisabled
            },
            fontSize = 10.sp,
            textAlign = TextAlign.Center,
        )
    }
}

// ═══════════════════════════════════════════════════════════════
// Phone Camera Preview (CameraX)
// ═══════════════════════════════════════════════════════════════

@Composable
private fun PhoneCameraPreview(useFrontCamera: Boolean) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    var hasCameraPermission by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED
        )
    }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission()
    ) { granted -> hasCameraPermission = granted }

    LaunchedEffect(Unit) {
        if (!hasCameraPermission) {
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    if (!hasCameraPermission) {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.Center,
        ) {
            Text("Camera permission required", color = AppColor.TextSecondary)
        }
        return
    }

    val cameraSelector = if (useFrontCamera) CameraSelector.DEFAULT_FRONT_CAMERA
        else CameraSelector.DEFAULT_BACK_CAMERA

    AndroidView(
        factory = { ctx ->
            PreviewView(ctx).also { previewView ->
                val cameraProviderFuture = ProcessCameraProvider.getInstance(ctx)
                cameraProviderFuture.addListener({
                    val cameraProvider = cameraProviderFuture.get()
                    val preview = Preview.Builder().build().also {
                        it.surfaceProvider = previewView.surfaceProvider
                    }
                    try {
                        cameraProvider.unbindAll()
                        cameraProvider.bindToLifecycle(lifecycleOwner, cameraSelector, preview)
                    } catch (e: Exception) {
                        // Camera binding failed
                    }
                }, ContextCompat.getMainExecutor(ctx))
            }
        },
        update = { previewView ->
            // Rebind on camera flip
            val cameraProviderFuture = ProcessCameraProvider.getInstance(context)
            cameraProviderFuture.addListener({
                val cameraProvider = cameraProviderFuture.get()
                val preview = Preview.Builder().build().also {
                    it.surfaceProvider = previewView.surfaceProvider
                }
                try {
                    cameraProvider.unbindAll()
                    cameraProvider.bindToLifecycle(lifecycleOwner, cameraSelector, preview)
                } catch (e: Exception) {
                    // Camera binding failed
                }
            }, ContextCompat.getMainExecutor(context))
        },
        modifier = Modifier.fillMaxSize(),
    )
}
