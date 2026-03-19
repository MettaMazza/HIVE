package com.meta.wearable.dat.externalsampleapps.cameraaccess.gemini

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Holds the ephemeral Discord link code received from the HIVE Server.
 * Populated by both HiveLiveService (glasses voice connection) and
 * HiveApiClient (text chat connection) so the AccountScreen can display it.
 */
object LinkManager {
    private val _linkCode = MutableStateFlow<String?>(null)
    val linkCode: StateFlow<String?> = _linkCode.asStateFlow()

    fun setLinkCode(code: String?) {
        _linkCode.value = code
    }
}
