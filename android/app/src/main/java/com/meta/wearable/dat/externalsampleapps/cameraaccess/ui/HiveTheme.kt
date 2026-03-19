package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.Typography
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/**
 * HIVE Material3 Theme — Dark mode with soft pastel green.
 */

private val HiveColorScheme = darkColorScheme(
    primary = AppColor.SproutGreen,
    onPrimary = AppColor.BackgroundDark,
    primaryContainer = AppColor.SproutGreenDark,
    onPrimaryContainer = AppColor.TextPrimary,
    secondary = AppColor.SproutGreenLight,
    onSecondary = AppColor.BackgroundDark,
    secondaryContainer = AppColor.BackgroundElevated,
    onSecondaryContainer = AppColor.TextPrimary,
    tertiary = AppColor.AccentDim,
    background = AppColor.BackgroundDark,
    onBackground = AppColor.TextPrimary,
    surface = AppColor.BackgroundCard,
    onSurface = AppColor.TextPrimary,
    surfaceVariant = AppColor.BackgroundElevated,
    onSurfaceVariant = AppColor.TextSecondary,
    outline = AppColor.Border,
    error = AppColor.StatusError,
    onError = Color.White,
    errorContainer = AppColor.DestructiveBackground,
    onErrorContainer = AppColor.StatusError,
)

private val HiveTypography = Typography(
    headlineLarge = TextStyle(
        fontWeight = FontWeight.Bold,
        fontSize = 28.sp,
        letterSpacing = (-0.5).sp,
        color = AppColor.TextPrimary,
    ),
    headlineMedium = TextStyle(
        fontWeight = FontWeight.SemiBold,
        fontSize = 22.sp,
        letterSpacing = (-0.3).sp,
        color = AppColor.TextPrimary,
    ),
    headlineSmall = TextStyle(
        fontWeight = FontWeight.SemiBold,
        fontSize = 18.sp,
        color = AppColor.TextPrimary,
    ),
    titleLarge = TextStyle(
        fontWeight = FontWeight.SemiBold,
        fontSize = 20.sp,
        letterSpacing = 0.sp,
    ),
    titleMedium = TextStyle(
        fontWeight = FontWeight.Medium,
        fontSize = 16.sp,
        letterSpacing = 0.15.sp,
    ),
    bodyLarge = TextStyle(
        fontWeight = FontWeight.Normal,
        fontSize = 16.sp,
        lineHeight = 24.sp,
        letterSpacing = 0.5.sp,
    ),
    bodyMedium = TextStyle(
        fontWeight = FontWeight.Normal,
        fontSize = 14.sp,
        lineHeight = 20.sp,
        letterSpacing = 0.25.sp,
    ),
    bodySmall = TextStyle(
        fontWeight = FontWeight.Normal,
        fontSize = 12.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.4.sp,
    ),
    labelLarge = TextStyle(
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
        letterSpacing = 0.1.sp,
    ),
    labelMedium = TextStyle(
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        letterSpacing = 0.5.sp,
    ),
)

@Composable
fun HiveTheme(
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = HiveColorScheme,
        typography = HiveTypography,
        content = content,
    )
}
