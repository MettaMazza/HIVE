package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.ui.graphics.Color

/**
 * HIVE Color Palette — Dark mode with honey gold and royal gold accents.
 */
object AppColor {
    // Primary palette — Honey Gold + Royal Gold
    val HiveGold = Color(0xFFF5A623)               // Main accent — warm honey gold
    val HiveGoldDark = Color(0xFFD4891A)            // Pressed/darker variant
    val HiveGoldLight = Color(0xFFFFE0A3)           // Light variant for text highlights
    val HiveGlow = Color(0x40F5A623)                // 25% alpha glow

    // Secondary accent
    val HiveAmber = Color(0xFFFFB347)               // Warm amber secondary
    val HiveRoyal = Color(0xFFFFD700)               // Royal gold for emphasis

    // Background layers
    val BackgroundDark = Color(0xFF0D0D0D)           // Deepest background
    val BackgroundCard = Color(0xFF1A1A1A)            // Card/surface background
    val BackgroundElevated = Color(0xFF242424)        // Elevated surfaces (bottom bar, dialogs)
    val BackgroundInput = Color(0xFF2A2A2A)           // Input fields, search bars

    // Text
    val TextPrimary = Color(0xFFF0F0F0)              // Primary text
    val TextSecondary = Color(0xFFAAAAAA)             // Secondary/muted text
    val TextDisabled = Color(0xFF666666)              // Disabled text

    // Status colors
    val StatusOnline = Color(0xFF66BB6A)              // Connected/online
    val StatusWarning = Color(0xFFFFB74D)             // Connecting/warning
    val StatusError = Color(0xFFEF5350)               // Error/disconnected
    val StatusOffline = Color(0xFF757575)              // Offline/inactive

    // Legacy compatibility
    val Green = StatusOnline
    val Red = StatusError
    val Yellow = StatusWarning
    val DeepBlue = HiveGold   // Replace Meta blue with HIVE gold
    val DestructiveBackground = Color(0xFF2D1215)
    val DestructiveForeground = StatusError

    // Accent
    val Accent = HiveGold
    val AccentDim = Color(0x80F5A623)                // 50% alpha
    val Border = Color(0xFF333333)

    // Compatibility aliases (used in existing UI code)
    val SproutGreen = HiveGold
    val SproutGreenDark = HiveGoldDark
    val SproutGreenLight = HiveGoldLight
    val SproutGlow = HiveGlow
}
