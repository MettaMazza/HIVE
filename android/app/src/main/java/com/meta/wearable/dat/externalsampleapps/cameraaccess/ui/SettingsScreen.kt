package com.meta.wearable.dat.externalsampleapps.cameraaccess.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var hiveServerUrl by remember { mutableStateOf(SettingsManager.hiveServerUrl) }
    var hiveAuthToken by remember { mutableStateOf(SettingsManager.hiveAuthToken) }
    var webrtcSignalingURL by remember { mutableStateOf(SettingsManager.webrtcSignalingURL) }
    var inferenceMode by remember { mutableStateOf(SettingsManager.inferenceMode) }
    var showResetDialog by remember { mutableStateOf(false) }

    fun save() {
        SettingsManager.inferenceMode = inferenceMode
        SettingsManager.hiveServerUrl = hiveServerUrl.trim()
        SettingsManager.hiveAuthToken = hiveAuthToken.trim()
        SettingsManager.webrtcSignalingURL = webrtcSignalingURL.trim()
    }

    fun reload() {
        inferenceMode = SettingsManager.inferenceMode
        hiveServerUrl = SettingsManager.hiveServerUrl
        hiveAuthToken = SettingsManager.hiveAuthToken
        webrtcSignalingURL = SettingsManager.webrtcSignalingURL
    }

    Column(modifier = modifier.fillMaxSize()) {
        TopAppBar(
            title = { Text("ApisClaw Settings") },
            navigationIcon = {
                IconButton(onClick = {
                    save()
                    onBack()
                }) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                }
            },
        )

        Column(
            modifier = Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            // Inference Mode section
            SectionHeader("Inference Mode")

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp)
            ) {
                ModeButton(
                    text = "Worker (Local)",
                    isSelected = inferenceMode == SettingsManager.InferenceMode.WORKER,
                    onClick = { inferenceMode = SettingsManager.InferenceMode.WORKER },
                    modifier = Modifier.weight(1f)
                )
                ModeButton(
                    text = "Queen (Remote)",
                    isSelected = inferenceMode == SettingsManager.InferenceMode.QUEEN,
                    onClick = { inferenceMode = SettingsManager.InferenceMode.QUEEN },
                    modifier = Modifier.weight(1f)
                )
            }

            if (inferenceMode == SettingsManager.InferenceMode.WORKER) {
                Text(
                    text = "Uses local Qwen 3.5 0.8B model. All HIVE protocols remain active 1:1.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 4.dp)
                )
                Text(
                    text = "Model Status: Ready (Bundled)",
                    style = MaterialTheme.typography.labelSmall,
                    color = Color(0xFF4CAF50), // Green
                    modifier = Modifier.padding(top = 4.dp)
                )
            }

            // Apis section
            SectionHeader("Apis Server (Queen)")
            MonoTextField(
                value = hiveServerUrl,
                onValueChange = { hiveServerUrl = it },
                label = "Server URL",
                placeholder = "http://192.168.1.239:8420",
                keyboardType = KeyboardType.Uri,
                enabled = inferenceMode == SettingsManager.InferenceMode.QUEEN
            )
            MonoTextField(
                value = hiveAuthToken,
                onValueChange = { hiveAuthToken = it },
                label = "Auth Token (JWT)",
                placeholder = "eyJhbGci...",
                enabled = inferenceMode == SettingsManager.InferenceMode.QUEEN
            )

            // WebRTC section (optional)
            SectionHeader("WebRTC (Optional)")
            MonoTextField(
                value = webrtcSignalingURL,
                onValueChange = { webrtcSignalingURL = it },
                label = "Signaling URL",
                placeholder = "wss://your-server.example.com",
                keyboardType = KeyboardType.Uri,
            )

            // Reset
            TextButton(onClick = { showResetDialog = true }) {
                Text("Reset to Defaults", color = Color.Red)
            }

            Spacer(modifier = Modifier.height(32.dp))
        }
    }

    if (showResetDialog) {
        AlertDialog(
            onDismissRequest = { showResetDialog = false },
            title = { Text("Reset Settings") },
            text = { Text("This will reset all settings to the values built into the app.") },
            confirmButton = {
                TextButton(onClick = {
                    SettingsManager.resetAll()
                    reload()
                    showResetDialog = false
                }) {
                    Text("Reset", color = Color.Red)
                }
            },
            dismissButton = {
                TextButton(onClick = { showResetDialog = false }) {
                    Text("Cancel")
                }
            },
        )
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        text = title,
        style = MaterialTheme.typography.titleSmall,
        color = MaterialTheme.colorScheme.primary,
    )
}

@Composable
private fun ModeButton(
    text: String,
    isSelected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    androidx.compose.material3.Button(
        onClick = onClick,
        modifier = modifier,
        colors = androidx.compose.material3.ButtonDefaults.buttonColors(
            containerColor = if (isSelected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceVariant,
            contentColor = if (isSelected) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurfaceVariant
        ),
        shape = RoundedCornerShape(8.dp)
    ) {
        Text(text, fontSize = 12.sp)
    }
}

@Composable
private fun MonoTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    keyboardType: KeyboardType = KeyboardType.Text,
    enabled: Boolean = true,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label) },
        placeholder = { Text(placeholder) },
        modifier = Modifier.fillMaxWidth(),
        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
        enabled = enabled,
    )
}
