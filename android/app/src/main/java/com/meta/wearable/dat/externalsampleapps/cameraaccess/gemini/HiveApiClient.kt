package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import android.util.Log
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlin.coroutines.resume
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.util.concurrent.TimeUnit
import com.meta.wearable.dat.externalsampleapps.cameraaccess.settings.SettingsManager
import com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini.LinkManager
/**
 * Handles register, login, token refresh, and Discord verification.
 */
class HiveApiClient(
    private var baseUrl: String,
) {
    companion object {
        private const val TAG = "HiveApiClient"
        private val JSON_MEDIA = "application/json".toMediaType()
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.SECONDS)  // No read timeout — Apis thinks as long as it needs
        .pingInterval(0, TimeUnit.SECONDS)  // No ping timeout
        .build()

    var accessToken: String = ""
    var refreshToken: String = ""

    fun updateBaseUrl(url: String) {
        baseUrl = url.trimEnd('/')
    }

    // ═══════════════════════════════════════════════════════
    // Auth Endpoints
    // ═══════════════════════════════════════════════════════

    data class AuthResult(
        val success: Boolean,
        val message: String,
        val accessToken: String = "",
        val refreshToken: String = "",
        val userId: String = "",
    )

    suspend fun register(email: String, password: String, username: String = ""): AuthResult =
        withContext(Dispatchers.IO) {
            try {
                val body = JSONObject().apply {
                    put("email", email)
                    put("password", password)
                    if (username.isNotEmpty()) put("username", username)
                }
                val response = post("/api/auth/register", body)
                if (response.has("access_token")) {
                    accessToken = response.getString("access_token")
                    refreshToken = response.optString("refresh_token", "")
                    AuthResult(
                        success = true,
                        message = response.optString("message", "Account created!"),
                        accessToken = accessToken,
                        refreshToken = refreshToken,
                        userId = response.optString("user_id", ""),
                    )
                } else {
                    AuthResult(false, response.optString("error", "Registration failed"))
                }
            } catch (e: Exception) {
                Log.e(TAG, "Register error", e)
                AuthResult(false, "Connection error: ${e.message}")
            }
        }

    suspend fun login(email: String, password: String): AuthResult =
        withContext(Dispatchers.IO) {
            try {
                val body = JSONObject().apply {
                    put("email", email)
                    put("password", password)
                }
                val response = post("/api/auth/login", body)
                if (response.has("access_token")) {
                    accessToken = response.getString("access_token")
                    refreshToken = response.optString("refresh_token", "")
                    AuthResult(
                        success = true,
                        message = response.optString("message", "Login successful!"),
                        accessToken = accessToken,
                        refreshToken = refreshToken,
                    )
                } else {
                    AuthResult(false, response.optString("error", "Login failed"))
                }
            } catch (e: Exception) {
                Log.e(TAG, "Login error", e)
                AuthResult(false, "Connection error: ${e.message}")
            }
        }

    // ═══════════════════════════════════════════════════════
    // Discord Verification
    // ═══════════════════════════════════════════════════════

    data class SimpleResult(
        val success: Boolean,
        val message: String,
        val newAccessToken: String = "",
    )

    suspend fun requestDiscordCode(discordId: String): SimpleResult = suspendCancellableCoroutine { cont ->
        val wsUrl = baseUrl.replace("http://", "ws://").replace("https://", "wss://")
        val request = okhttp3.Request.Builder().url(wsUrl).build()
        val ws = client.newWebSocket(request, object : okhttp3.WebSocketListener() {
            override fun onMessage(webSocket: okhttp3.WebSocket, text: String) {
                try {
                    val json = JSONObject(text)
                    when (json.optString("type")) {
                        "connected" -> {
                            webSocket.send(JSONObject().apply {
                                put("type", "link_request")
                                put("discord_id", discordId)
                            }.toString())
                        }
                        "link_requested" -> {
                            if (cont.isActive) cont.resume(SimpleResult(true, "Code sent!"))
                            webSocket.close(1000, null)
                        }
                        "link_error" -> {
                            if (cont.isActive) cont.resume(SimpleResult(false, json.optString("message", "Error")))
                            webSocket.close(1000, null)
                        }
                    }
                } catch (e: Exception) {
                    if (cont.isActive) cont.resume(SimpleResult(false, "Parse error"))
                }
            }
            override fun onFailure(webSocket: okhttp3.WebSocket, t: Throwable, response: okhttp3.Response?) {
                if (cont.isActive) cont.resume(SimpleResult(false, t.message ?: "Connection failed"))
            }
        })
        cont.invokeOnCancellation { ws.cancel() }
    }

    suspend fun verifyDiscordCode(discordId: String, code: String): SimpleResult = suspendCancellableCoroutine { cont ->
        val wsUrl = baseUrl.replace("http://", "ws://").replace("https://", "wss://")
        val request = okhttp3.Request.Builder().url(wsUrl).build()
        val ws = client.newWebSocket(request, object : okhttp3.WebSocketListener() {
            override fun onMessage(webSocket: okhttp3.WebSocket, text: String) {
                try {
                    val json = JSONObject(text)
                    when (json.optString("type")) {
                        "connected" -> {
                            webSocket.send(JSONObject().apply {
                                put("type", "link_verify")
                                put("code", code)
                            }.toString())
                        }
                        "link_success" -> {
                            val token = json.optString("device_token", "")
                            if (cont.isActive) cont.resume(SimpleResult(true, "Linked successfully", token))
                            webSocket.close(1000, null)
                        }
                        "link_error" -> {
                            if (cont.isActive) cont.resume(SimpleResult(false, json.optString("message", "Error")))
                            webSocket.close(1000, null)
                        }
                    }
                } catch (e: Exception) {
                    if (cont.isActive) cont.resume(SimpleResult(false, "Parse error"))
                }
            }
            override fun onFailure(webSocket: okhttp3.WebSocket, t: Throwable, response: okhttp3.Response?) {
                if (cont.isActive) cont.resume(SimpleResult(false, t.message ?: "Connection failed"))
            }
        })
        cont.invokeOnCancellation { ws.cancel() }
    }    // ═══════════════════════════════════════════════════════
    // Text Chat (via WebSocket)
    // ═══════════════════════════════════════════════════════

    suspend fun sendMessage(content: String): String =
        withContext(Dispatchers.IO) {
            try {
                // Use the same WebSocket URL as the live service (bare, no path)
                val url = HiveConfig.websocketURL() ?: return@withContext "Server not configured"

                val request = Request.Builder()
                    .url(url)
                    .build()

                var response = ""
                val latch = java.util.concurrent.CountDownLatch(1)

                val ws = client.newWebSocket(request, object : okhttp3.WebSocketListener() {
                    override fun onMessage(webSocket: okhttp3.WebSocket, text: String) {
                        try {
                            val json = JSONObject(text)
                            when (json.optString("type")) {
                                "connected" -> {
                                    val lCode = json.optString("link_code", null)
                                    if (!lCode.isNullOrEmpty()) {
                                        LinkManager.setLinkCode(lCode)
                                    }
                                    val dToken = json.optString("device_token", null)
                                    val existingToken = SettingsManager.hiveDeviceToken
                                    if (existingToken.isNullOrEmpty() && !dToken.isNullOrEmpty()) {
                                        SettingsManager.hiveDeviceToken = dToken
                                    } else if (!existingToken.isNullOrEmpty()) {
                                        val authMsg = JSONObject().apply {
                                            put("type", "authenticate")
                                            put("device_token", existingToken)
                                        }
                                        webSocket.send(authMsg.toString())
                                    }
                                    // Send our message
                                    val msg = JSONObject().apply {
                                        put("type", "message")
                                        put("content", content)
                                    }
                                    webSocket.send(msg.toString())
                                }
                                "response", "text" -> {
                                    response = json.optString("content", "")
                                }
                                "done" -> {
                                    // Turn complete — close and return
                                    webSocket.close(1000, "done")
                                    latch.countDown()
                                }
                                "error" -> {
                                    response = json.optString("message",
                                        json.optString("detail", "Error"))
                                    webSocket.close(1000, "error")
                                    latch.countDown()
                                }
                                "thinking" -> { /* ignore */ }
                            }
                        } catch (e: Exception) {
                            Log.e(TAG, "WS message parse error", e)
                        }
                    }

                    override fun onFailure(
                        webSocket: okhttp3.WebSocket,
                        t: Throwable,
                        r: okhttp3.Response?,
                    ) {
                        response = "Connection failed: ${t.message}"
                        latch.countDown()
                    }
                })

                // Wait indefinitely — Apis thinks as long as it needs
                latch.await()
                response
            } catch (e: Exception) {
                Log.e(TAG, "sendMessage error", e)
                "Error: ${e.message}"
            }
        }

    // ═══════════════════════════════════════════════════════
    // HTTP Helpers
    // ═══════════════════════════════════════════════════════

    private fun post(path: String, body: JSONObject): JSONObject {
        val request = Request.Builder()
            .url("$baseUrl$path")
            .post(body.toString().toRequestBody(JSON_MEDIA))
            .build()
        val response = client.newCall(request).execute()
        val responseBody = response.body?.string() ?: "{}"
        return JSONObject(responseBody)
    }

    private fun postAuth(path: String, body: JSONObject): JSONObject {
        val request = Request.Builder()
            .url("$baseUrl$path")
            .addHeader("Authorization", "Bearer $accessToken")
            .post(body.toString().toRequestBody(JSON_MEDIA))
            .build()
        val response = client.newCall(request).execute()
        val responseBody = response.body?.string() ?: "{}"
        return JSONObject(responseBody)
    }
}
