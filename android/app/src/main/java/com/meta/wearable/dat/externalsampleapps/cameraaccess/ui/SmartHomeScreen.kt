package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.animation.core.*
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * SmartHomeScreen — Coming Soon stub.
 */
@Composable
fun SmartHomeScreen(
    modifier: Modifier = Modifier,
) {
    val infiniteTransition = rememberInfiniteTransition(label = "sprout")
    val breathe by infiniteTransition.animateFloat(
        initialValue = 0.4f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = tween(3000, easing = EaseInOutCubic),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "breathe",
    )

    val floatOffset by infiniteTransition.animateFloat(
        initialValue = 0f,
        targetValue = 10f,
        animationSpec = infiniteRepeatable(
            animation = tween(4000, easing = EaseInOutCubic),
            repeatMode = RepeatMode.Reverse,
        ),
        label = "float",
    )

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(AppColor.BackgroundDark),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier
                .padding(horizontal = 48.dp),
        ) {
            // Animated sprout icon
            Box(
                modifier = Modifier.offset(y = (-floatOffset).dp),
                contentAlignment = Alignment.Center,
            ) {
                // Glow
                Box(
                    modifier = Modifier
                        .size(120.dp)
                        .alpha(breathe * 0.3f)
                        .clip(CircleShape)
                        .background(
                            Brush.radialGradient(
                                colors = listOf(
                                    AppColor.SproutGreen.copy(alpha = 0.4f),
                                    AppColor.SproutGreen.copy(alpha = 0f),
                                ),
                            ),
                        ),
                )
                // Icon
                Icon(
                    imageVector = Icons.Outlined.Home,
                    contentDescription = null,
                    tint = AppColor.SproutGreen.copy(alpha = breathe),
                    modifier = Modifier.size(64.dp),
                )
            }

            Spacer(modifier = Modifier.height(32.dp))

            Text(
                text = "Smart Home",
                style = MaterialTheme.typography.headlineMedium,
                color = AppColor.TextPrimary,
                fontWeight = FontWeight.Bold,
            )

            Spacer(modifier = Modifier.height(12.dp))

            Text(
                text = "Control your smart home devices with Apis voice commands",
                color = AppColor.TextSecondary,
                style = MaterialTheme.typography.bodyMedium,
                textAlign = TextAlign.Center,
            )

            Spacer(modifier = Modifier.height(32.dp))

            // Coming soon badge
            Card(
                colors = CardDefaults.cardColors(
                    containerColor = AppColor.SproutGreen.copy(alpha = 0.1f),
                ),
                shape = RoundedCornerShape(24.dp),
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 20.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Icon(
                        imageVector = Icons.Outlined.Schedule,
                        contentDescription = null,
                        tint = AppColor.SproutGreen,
                        modifier = Modifier.size(18.dp),
                    )
                    Text(
                        text = "Coming Soon",
                        color = AppColor.SproutGreen,
                        fontWeight = FontWeight.SemiBold,
                        fontSize = 14.sp,
                    )
                }
            }

            Spacer(modifier = Modifier.height(40.dp))

            // Feature preview cards
            FeaturePreviewCard(
                icon = Icons.Outlined.LightMode,
                title = "Lights",
                description = "\"Hey Apis, dim the living room lights\"",
            )
            Spacer(modifier = Modifier.height(12.dp))
            FeaturePreviewCard(
                icon = Icons.Outlined.Thermostat,
                title = "Climate",
                description = "\"Hey Apis, set temperature to 72°\"",
            )
            Spacer(modifier = Modifier.height(12.dp))
            FeaturePreviewCard(
                icon = Icons.Outlined.Lock,
                title = "Security",
                description = "\"Hey Apis, lock the front door\"",
            )
        }
    }
}

@Composable
private fun FeaturePreviewCard(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    description: String,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
        shape = RoundedCornerShape(12.dp),
        modifier = Modifier
            .fillMaxWidth()
            .alpha(0.5f),
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = icon,
                contentDescription = null,
                tint = AppColor.TextDisabled,
                modifier = Modifier.size(24.dp),
            )
            Spacer(modifier = Modifier.width(12.dp))
            Column {
                Text(title, color = AppColor.TextSecondary, fontWeight = FontWeight.Medium, fontSize = 14.sp)
                Text(description, color = AppColor.TextDisabled, fontSize = 12.sp)
            }
        }
    }
}
