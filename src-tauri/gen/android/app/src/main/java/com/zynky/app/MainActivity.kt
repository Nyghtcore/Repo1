package com.zynky.app

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import androidx.activity.enableEdgeToEdge
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import android.content.pm.PackageManager

/**
 * Zynky keeps its vault in shared storage (e.g. /storage/emulated/0/Documents/ZynkyVault)
 * so that Syncthing can sync the same folder. Reading and writing arbitrary files there
 * requires "All files access" on Android 11+ and the legacy storage permissions below that.
 * We ask once when the app starts; the user can always grant it later from system settings.
 */
class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    requestStorageAccess()
  }

  private fun requestStorageAccess() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      if (!Environment.isExternalStorageManager()) {
        val intent = Intent(
          Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
          Uri.parse("package:$packageName")
        )
        try {
          startActivity(intent)
        } catch (_: Exception) {
          // Some devices lack the per-app screen; fall back to the global list.
          startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
        }
      }
    } else {
      val needed = arrayOf(
        Manifest.permission.READ_EXTERNAL_STORAGE,
        Manifest.permission.WRITE_EXTERNAL_STORAGE
      ).filter {
        ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
      }
      if (needed.isNotEmpty()) {
        ActivityCompat.requestPermissions(this, needed.toTypedArray(), STORAGE_REQUEST)
      }
    }
  }

  companion object {
    private const val STORAGE_REQUEST = 4021
  }
}
