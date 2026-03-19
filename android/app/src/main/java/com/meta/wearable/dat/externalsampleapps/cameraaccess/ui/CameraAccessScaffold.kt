package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.*
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.meta.wearable.dat.core.types.Permission
import com.meta.wearable.dat.core.types.PermissionStatus
import com.meta.wearable.dat.externalsampleapps.cameraaccess.BuildConfig
import com.meta.wearable.dat.externalsampleapps.cameraaccess.wearables.WearablesViewModel

/**
 * CameraAccessScaffold — HIVE tab-based navigation.
 *
 * 4 tabs:  Chat | Glasses | Smart Home | Account
 *
 * When glasses are streaming, full-screen StreamScreen takes over.
 */

enum class HIVETab(
    val label: String,
    val selectedIcon: ImageVector,
    val unselectedIcon: ImageVector,
) {
    CHAT("Chat", Icons.Filled.Mic, Icons.Outlined.Mic),
    LIVE_CALL("Live Call", Icons.Filled.Call, Icons.Outlined.Call),
    SMART_HOME("Home", Icons.Filled.Home, Icons.Outlined.Home),
    ACCOUNT("Account", Icons.Filled.Person, Icons.Outlined.Person),
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun CameraAccessScaffold(
    viewModel: WearablesViewModel,
    onRequestWearablesPermission: suspend (Permission) -> PermissionStatus,
    modifier: Modifier = Modifier,
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    var selectedTab by remember { mutableStateOf(HIVETab.CHAT) }

    // Auto-stream video frames in the background whenever Apis is active
    val geminiViewModel: com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.GeminiSessionViewModel = androidx.lifecycle.viewmodel.compose.viewModel()
    val geminiState by geminiViewModel.uiState.collectAsStateWithLifecycle()

    LaunchedEffect(geminiState.isGeminiActive, uiState.hasActiveDevice) {
        if (geminiState.isGeminiActive && uiState.hasActiveDevice && !uiState.isBackgroundStreaming) {
            viewModel.startBackgroundStreaming(onRequestWearablesPermission)
        } else if (!geminiState.isGeminiActive && uiState.isBackgroundStreaming) {
            viewModel.stopBackgroundStreaming()
        }
    }

    HiveTheme {
        // Full-screen streaming mode overrides everything (but NOT for background streaming)
        if (uiState.isStreaming && !uiState.isBackgroundStreaming) {
            StreamScreen(
                wearablesViewModel = viewModel,
                isPhoneMode = uiState.isPhoneMode,
            )
            return@HiveTheme
        }

        // Settings screen (full overlay)
        if (uiState.isSettingsVisible) {
            SettingsScreen(onBack = { viewModel.hideSettings() })
            return@HiveTheme
        }

        Scaffold(
            modifier = modifier.fillMaxSize(),
            containerColor = AppColor.BackgroundDark,
            bottomBar = {
                HIVEBottomBar(
                    selectedTab = selectedTab,
                    onTabSelected = { selectedTab = it },
                )
            },
        ) { innerPadding ->
            Box(modifier = Modifier.fillMaxSize().padding(innerPadding)) {
                // Hidden StreamScreen for background camera streaming
                // The DAT SDK camera pipeline only runs when StreamScreen is composed
                if (uiState.isBackgroundStreaming) {
                    Box(modifier = Modifier.size(0.dp)) {
                        StreamScreen(
                            wearablesViewModel = viewModel,
                            isPhoneMode = false,
                        )
                    }
                }

                when (selectedTab) {
                    HIVETab.CHAT -> ChatScreen(
                        onNavigateToSettings = { viewModel.showSettings() },
                    )
                    HIVETab.LIVE_CALL -> LiveCallScreen(
                        viewModel = viewModel,
                        onRequestWearablesPermission = onRequestWearablesPermission,
                    )
                    HIVETab.SMART_HOME -> SmartHomeScreen()
                    HIVETab.ACCOUNT -> AccountScreen(
                        wearablesViewModel = viewModel,
                        onRequestWearablesPermission = onRequestWearablesPermission,
                    )
                }
            }
        }
    }
}

@Composable
private fun HIVEBottomBar(
    selectedTab: HIVETab,
    onTabSelected: (HIVETab) -> Unit,
) {
    NavigationBar(
        containerColor = AppColor.BackgroundElevated,
        contentColor = AppColor.TextSecondary,
        tonalElevation = 0.dp,
    ) {
        HIVETab.entries.forEach { tab ->
            val isSelected = selectedTab == tab
            NavigationBarItem(
                selected = isSelected,
                onClick = { onTabSelected(tab) },
                icon = {
                    Icon(
                        imageVector = if (isSelected) tab.selectedIcon else tab.unselectedIcon,
                        contentDescription = tab.label,
                        modifier = Modifier.size(24.dp),
                    )
                },
                label = {
                    Text(
                        text = tab.label,
                        fontSize = 11.sp,
                    )
                },
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = AppColor.SproutGreen,
                    selectedTextColor = AppColor.SproutGreen,
                    unselectedIconColor = AppColor.TextDisabled,
                    unselectedTextColor = AppColor.TextDisabled,
                    indicatorColor = AppColor.SproutGreen.copy(alpha = 0.12f),
                ),
            )
        }
    }
}
