package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.ApisConnectionState
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.GeminiUiState

@Composable
fun GeminiOverlay(
    uiState: GeminiUiState,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.fillMaxWidth().padding(horizontal = 16.dp),
    ) {
        ApisStatusBar(connectionState = uiState.connectionState)

        Spacer(modifier = Modifier.height(8.dp))

        if (uiState.userTranscript.isNotEmpty() || uiState.aiTranscript.isNotEmpty()) {
            TranscriptOverlay(
                userTranscript = uiState.userTranscript,
                aiTranscript = uiState.aiTranscript,
            )
        }

        if (uiState.isModelSpeaking) {
            Spacer(modifier = Modifier.height(4.dp))
            SpeakingIndicator()
        }
    }
}

@Composable
fun ApisStatusBar(
    connectionState: ApisConnectionState,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        StatusPill(
            label = "HIVE",
            color = when (connectionState) {
                is ApisConnectionState.Ready -> AppColor.StatusOnline
                is ApisConnectionState.Connecting -> AppColor.StatusWarning
                is ApisConnectionState.Error -> AppColor.StatusError
                is ApisConnectionState.Disconnected -> AppColor.StatusOffline
            },
        )
    }
}

@Composable
fun StatusPill(
    label: String,
    color: androidx.compose.ui.graphics.Color,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier
            .background(AppColor.BackgroundDark.copy(alpha = 0.7f), RoundedCornerShape(12.dp))
            .padding(horizontal = 10.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(
            modifier = Modifier
                .size(8.dp)
                .clip(CircleShape)
                .background(color),
        )
        Text(
            text = label,
            color = AppColor.TextPrimary,
            fontSize = 12.sp,
        )
    }
}

@Composable
fun TranscriptOverlay(
    userTranscript: String,
    aiTranscript: String,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier
            .background(AppColor.BackgroundDark.copy(alpha = 0.6f), RoundedCornerShape(8.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
    ) {
        if (userTranscript.isNotEmpty()) {
            Text(
                text = userTranscript,
                color = AppColor.TextSecondary,
                fontSize = 13.sp,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (aiTranscript.isNotEmpty()) {
            Text(
                text = aiTranscript,
                color = AppColor.SproutGreenLight,
                fontSize = 13.sp,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
fun SpeakingIndicator(modifier: Modifier = Modifier) {
    val infiniteTransition = rememberInfiniteTransition(label = "speaking")
    Row(
        modifier = modifier
            .background(AppColor.BackgroundDark.copy(alpha = 0.6f), RoundedCornerShape(8.dp))
            .padding(horizontal = 12.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        repeat(4) { index ->
            val height by infiniteTransition.animateFloat(
                initialValue = 4f,
                targetValue = 16f,
                animationSpec = infiniteRepeatable(
                    animation = tween(400, delayMillis = index * 100, easing = LinearEasing),
                    repeatMode = RepeatMode.Reverse,
                ),
                label = "bar$index",
            )
            Box(
                modifier = Modifier
                    .width(3.dp)
                    .height(height.dp)
                    .clip(RoundedCornerShape(1.5.dp))
                    .background(AppColor.SproutGreen),
            )
        }
        Spacer(modifier = Modifier.width(6.dp))
        Text(text = "HIVE", color = AppColor.SproutGreen.copy(alpha = 0.7f), fontSize = 11.sp)
    }
}
