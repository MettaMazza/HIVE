package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import android.widget.Toast
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.HiveApiClient
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager
import com.meta.wearable.dat.externalsampleapps.cameraaccess.ui.AppColor
import com.meta.wearable.dat.externalsampleapps.cameraaccess.wearables.WearablesViewModel
import kotlinx.coroutines.launch
import com.meta.wearable.dat.core.types.Permission
import com.meta.wearable.dat.core.types.PermissionStatus
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * AccountScreen — Server connection + Discord linking via HIVE link codes.
 *
 * HIVE linking flow:
 * 1. App connects to HIVE WebSocket server → receives a 6-digit link code
 * 2. User types `/link <code>` in Discord
 * 3. HIVE binds the device to the Discord identity — stored persistently on disk
 * 4. On reconnect, device auto-authenticates via device token
 */
@Composable
fun AccountScreen(
    modifier: Modifier = Modifier,
    wearablesViewModel: WearablesViewModel? = null,
    onRequestWearablesPermission: (suspend (Permission) -> PermissionStatus)? = null,
) {
    val scrollState = rememberScrollState()
    val context = LocalContext.current

    // State
    var serverUrl by remember { mutableStateOf(SettingsManager.hiveServerUrl) }

    // Discord linking
    var discordId by remember { mutableStateOf("") }
    var verificationCode by remember { mutableStateOf("") }
    var codeSent by remember { mutableStateOf(false) }
    var discordLoading by remember { mutableStateOf(false) }
    var discordError by remember { mutableStateOf("") }

    val scope = rememberCoroutineScope()
    val apiClient = remember { HiveApiClient(serverUrl) }

    // Honey gold colors for HIVE branding
    val honeyGold = Color(0xFFF5A623)
    val honeyGoldDark = Color(0xFFD4891A)

    Box(
        modifier = modifier
            .fillMaxSize()
            .background(AppColor.BackgroundDark),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(scrollState)
                .padding(horizontal = 24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.height(16.dp))

            Text(
                text = "Account",
                style = MaterialTheme.typography.headlineMedium,
                color = AppColor.TextPrimary,
                fontWeight = FontWeight.Bold,
            )

            Spacer(modifier = Modifier.height(32.dp))

            // ═══ HIVE Connection ═══
            Box(
                modifier = Modifier
                    .size(80.dp)
                    .clip(CircleShape)
                    .background(honeyGold.copy(alpha = 0.15f)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    imageVector = Icons.Outlined.Hub,
                    contentDescription = null,
                    tint = honeyGold,
                    modifier = Modifier.size(40.dp),
                )
            }

            Spacer(modifier = Modifier.height(16.dp))

            Text("HIVE", color = AppColor.TextPrimary,
                fontWeight = FontWeight.Bold, fontSize = 22.sp)

            Spacer(modifier = Modifier.height(8.dp))

            Text(
                "Connect to your HIVE server, then link your Discord identity.",
                color = AppColor.TextSecondary,
                textAlign = TextAlign.Center,
                fontSize = 14.sp, lineHeight = 20.sp,
                modifier = Modifier.padding(horizontal = 16.dp),
            )

            Spacer(modifier = Modifier.height(32.dp))

            // Server Settings
            SectionHeader("Server Connection")

            Card(
                colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
                shape = RoundedCornerShape(16.dp),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        "Enter your HIVE server address. The glasses WebSocket runs on port 8422.",
                        color = AppColor.TextSecondary, fontSize = 13.sp,
                    )
                    Spacer(modifier = Modifier.height(12.dp))

                    AuthField("Server URL", serverUrl, { serverUrl = it }, "http://192.168.1.239:8422")

                    Spacer(modifier = Modifier.height(12.dp))

                    Button(
                        onClick = {
                            SettingsManager.hiveServerUrl = serverUrl
                            Toast.makeText(context, "Server URL saved", Toast.LENGTH_SHORT).show()
                        },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = honeyGold,
                            contentColor = AppColor.BackgroundDark,
                        ),
                        shape = RoundedCornerShape(12.dp),
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text("Save", fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(vertical = 4.dp))
                    }
                }
            }

            Spacer(modifier = Modifier.height(32.dp))

            // Discord Linking Instructions
            SectionHeader("Discord Integration")

            val currentDeviceToken = SettingsManager.hiveDeviceToken
            if (currentDeviceToken != null) {
                Card(
                    colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Row(
                        modifier = Modifier.padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(Icons.Outlined.CheckCircle, null,
                            tint = AppColor.StatusOnline, modifier = Modifier.size(24.dp))
                        Spacer(modifier = Modifier.width(12.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text("Discord Linked", color = AppColor.TextPrimary,
                                fontWeight = FontWeight.SemiBold)
                            Text("Your conversations sync to this app.",
                                color = AppColor.TextSecondary, fontSize = 13.sp)
                        }
                        TextButton(onClick = {
                            scope.launch {
                                // Wipe the local token
                                SettingsManager.hiveDeviceToken = null
                                discordId = ""
                                verificationCode = ""
                                codeSent = false
                                Toast.makeText(context, "Unlinked", Toast.LENGTH_SHORT).show()
                            }
                        }) {
                            Text("Unlink", color = AppColor.StatusError)
                        }
                    }
                }
            } else {
                Card(
                    colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(
                            "Link your Discord to sync your AI memory and identity.",
                            color = AppColor.TextSecondary, fontSize = 13.sp,
                        )
                        Spacer(modifier = Modifier.height(12.dp))

                        AuthField("Discord User ID", discordId, { discordId = it },
                            "Right-click name -> Copy User ID",
                            keyboardType = KeyboardType.Number)

                        if (codeSent) {
                            Spacer(modifier = Modifier.height(12.dp))
                            Text("Check your Discord DMs for a 6-digit code from Apis.",
                                color = honeyGold, fontSize = 13.sp)
                            Spacer(modifier = Modifier.height(8.dp))
                            AuthField("Verification Code", verificationCode,
                                { verificationCode = it }, "123456",
                                keyboardType = KeyboardType.Number)
                        }

                        if (discordError.isNotEmpty()) {
                            Spacer(modifier = Modifier.height(8.dp))
                            Text(discordError, color = AppColor.StatusError, fontSize = 12.sp)
                        }

                        Spacer(modifier = Modifier.height(12.dp))

                        Button(
                            onClick = {
                                scope.launch {
                                    discordLoading = true
                                    discordError = ""
                                    apiClient.updateBaseUrl(serverUrl)
                                    if (!codeSent) {
                                        val result = apiClient.requestDiscordCode(discordId)
                                        if (result.success) {
                                            codeSent = true
                                        } else {
                                            discordError = result.message
                                        }
                                    } else {
                                        val result = apiClient.verifyDiscordCode(discordId, verificationCode)
                                        if (result.success) {
                                            SettingsManager.hiveDeviceToken = result.newAccessToken
                                            Toast.makeText(context, "Discord linked!", Toast.LENGTH_SHORT).show()
                                        } else {
                                            discordError = result.message
                                        }
                                    }
                                    discordLoading = false
                                }
                            },
                            enabled = !discordLoading && discordId.isNotEmpty() &&
                                (!codeSent || verificationCode.length == 6),
                            colors = ButtonDefaults.buttonColors(
                                containerColor = Color(0xFF5865F2),
                                contentColor = Color.White,
                            ),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            if (discordLoading) {
                                CircularProgressIndicator(
                                    modifier = Modifier.size(16.dp),
                                    color = Color.White, strokeWidth = 2.dp,
                                )
                            } else {
                                Text(
                                    if (codeSent) "Verify Code" else "Send Verification Code",
                                    modifier = Modifier.padding(vertical = 4.dp),
                                )
                            }
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(32.dp))

            // Smart Glasses
            if (wearablesViewModel != null) {
                val wearableState by wearablesViewModel.uiState.collectAsStateWithLifecycle()
                SectionHeader("Smart Glasses")
                Card(
                    colors = CardDefaults.cardColors(containerColor = AppColor.BackgroundCard),
                    shape = RoundedCornerShape(16.dp),
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Icon(
                                Icons.Outlined.Visibility, null,
                                tint = if (wearableState.isRegistered) AppColor.StatusOnline else AppColor.TextDisabled,
                                modifier = Modifier.size(24.dp),
                            )
                            Spacer(modifier = Modifier.width(12.dp))
                            Column {
                                Text(
                                    if (wearableState.isRegistered) "Glasses Connected" else "No Glasses Paired",
                                    color = AppColor.TextPrimary,
                                    fontWeight = FontWeight.SemiBold,
                                )
                                Text(
                                    if (wearableState.isRegistered) "Meta Ray-Ban ready"
                                    else "Pair your Meta Ray-Ban smart glasses",
                                    color = AppColor.TextSecondary, fontSize = 13.sp,
                                )
                            }
                        }
                        Spacer(modifier = Modifier.height(12.dp))
                        val activity = LocalContext.current as? android.app.Activity
                        Button(
                            onClick = {
                                activity?.let {
                                    if (wearableState.isRegistered) {
                                        wearablesViewModel.startUnregistration(it)
                                    } else {
                                        wearablesViewModel.startRegistration(it)
                                    }
                                }
                            },
                            colors = ButtonDefaults.buttonColors(
                                containerColor = if (wearableState.isRegistered)
                                    AppColor.StatusError.copy(alpha = 0.8f) else honeyGold,
                                contentColor = if (wearableState.isRegistered) Color.White
                                    else AppColor.BackgroundDark,
                            ),
                            shape = RoundedCornerShape(12.dp),
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Text(
                                if (wearableState.isRegistered) "Disconnect Glasses" else "Connect Glasses",
                                fontWeight = FontWeight.SemiBold,
                                modifier = Modifier.padding(vertical = 4.dp),
                            )
                        }
                    }
                }
            }

            Spacer(modifier = Modifier.height(32.dp))

            // Reset settings
            TextButton(onClick = {
                SettingsManager.resetAll()
                serverUrl = SettingsManager.hiveServerUrl
                Toast.makeText(context, "Settings reset to defaults", Toast.LENGTH_SHORT).show()
            }) {
                Text("Reset All Settings", color = AppColor.StatusError)
            }

            Spacer(modifier = Modifier.height(32.dp))
        }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title.uppercase(),
        color = AppColor.TextSecondary,
        fontWeight = FontWeight.SemiBold,
        fontSize = 12.sp,
        letterSpacing = 1.sp,
        modifier = Modifier.fillMaxWidth().padding(bottom = 12.dp),
    )
}

@Composable
private fun AuthField(
    label: String,
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    isPassword: Boolean = false,
    keyboardType: KeyboardType = KeyboardType.Text,
) {
    val honeyGold = Color(0xFFF5A623)
    Column {
        Text(label, color = AppColor.TextSecondary, fontSize = 12.sp)
        Spacer(modifier = Modifier.height(4.dp))
        OutlinedTextField(
            value = value,
            onValueChange = onValueChange,
            placeholder = { Text(placeholder, color = AppColor.TextDisabled) },
            visualTransformation = if (isPassword) androidx.compose.ui.text.input.PasswordVisualTransformation()
                else androidx.compose.ui.text.input.VisualTransformation.None,
            keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
            colors = OutlinedTextFieldDefaults.colors(
                focusedBorderColor = honeyGold,
                unfocusedBorderColor = AppColor.Border,
                cursorColor = honeyGold,
                focusedTextColor = AppColor.TextPrimary,
                unfocusedTextColor = AppColor.TextPrimary,
            ),
            shape = RoundedCornerShape(8.dp),
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
    }
}


