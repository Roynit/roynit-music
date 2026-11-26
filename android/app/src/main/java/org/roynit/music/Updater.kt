package org.roynit.music

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import org.json.JSONArray
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Updates inside the app, with the same safety as the PC app: the new APK must come from this project's own GitHub
 * releases and must match the SHA-256 published next to it before Android is asked to install it. Android itself
 * then checks that it is signed with the same key as the app already installed.
 */
object Updater {
    private const val RELEASES = "https://api.github.com/repos/Roynit/roynit-music/releases?per_page=15"
    private const val DOWNLOADS = "https://github.com/Roynit/roynit-music/releases/download/"
    private const val ASSET = "roy-music-android-arm64.apk"

    class Release(val version: String, val apk: String, val checksum: String, val notes: String)

    private fun get(address: String): ByteArray {
        val http = URL(address).openConnection() as HttpURLConnection
        http.connectTimeout = 10_000
        http.readTimeout = 30_000
        http.setRequestProperty("Accept", "application/vnd.github+json")
        return http.inputStream.use { it.readBytes() }
    }

    private fun numbers(version: String) = version.removePrefix("android-v").removePrefix("v").split('.').map { it.toIntOrNull() ?: 0 }

    private fun newer(candidate: String, current: String): Boolean {
        val a = numbers(candidate)
        val b = numbers(current)
        for (i in 0 until maxOf(a.size, b.size)) {
            val x = a.getOrElse(i) { 0 }
            val y = b.getOrElse(i) { 0 }
            if (x != y) return x > y
        }
        return false
    }

    /** The newest Android release when it is newer than [current], or null. Call from a worker thread. */
    fun check(current: String): Release? = runCatching {
        val releases = JSONArray(String(get(RELEASES)))
        for (i in 0 until releases.length()) {
            val r = releases.getJSONObject(i)
            val tag = r.optString("tag_name")
            if (!tag.startsWith("android-v") || r.optBoolean("draft") || r.optBoolean("prerelease")) continue
            if (!newer(tag, current)) return null
            val assets = r.getJSONArray("assets")
            fun find(name: String): String? = (0 until assets.length()).map { assets.getJSONObject(it) }.firstOrNull { it.optString("name") == name }?.optString("browser_download_url")
            val apk = find(ASSET)?.takeIf { it.startsWith(DOWNLOADS) } ?: return null
            val sum = find("$ASSET.sha256")?.takeIf { it.startsWith(DOWNLOADS) } ?: return null
            return Release(tag.removePrefix("android-v"), apk, sum, r.optString("body"))
        }
        null
    }.getOrNull()

    /** The release notes cut down to the main points: a few short lines, no markdown. */
    fun summary(notes: String): String {
        val clean = notes.lines().map { it.trim().replace(Regex("[*`#>]"), "").trim() }.filter { it.isNotEmpty() }
        val points = clean.filter { it.startsWith("-") || it.startsWith("•") }.map { it.trimStart('-', '•', ' ') }
        return (points.ifEmpty { clean.take(1) }).take(4).joinToString("\n") { "• " + it.take(70) }
    }

    /** Downloads the APK and checks its SHA-256. Returns the file, or null when anything is wrong. Worker thread. */
    fun download(activity: Activity, release: Release): File? = runCatching {
        val expected = String(get(release.checksum)).trim().split(Regex("\\s+")).first().lowercase()
        if (expected.length != 64 || !expected.all { it in "0123456789abcdef" }) return null
        val dir = File(activity.cacheDir, "updates").apply { mkdirs() }
        val file = File(dir, "$ASSET.${release.version}")
        if (file.isFile && file.length() > 0 && sha(file) == expected) return file
        dir.listFiles()?.forEach(File::delete)
        val http = URL(release.apk).openConnection() as HttpURLConnection
        http.connectTimeout = 10_000
        http.readTimeout = 30_000
        http.inputStream.use { input -> file.outputStream().use { out -> input.copyTo(out) } }
        if (sha(file) != expected) {
            file.delete()
            return null
        }
        file
    }.getOrNull()

    private fun sha(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buffer = ByteArray(64 * 1024)
            while (true) {
                val n = input.read(buffer)
                if (n < 0) break
                digest.update(buffer, 0, n)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }

    /** Hands a checked APK to Android's installer (main thread). */
    fun launch(activity: Activity, file: File) {
        if (!activity.packageManager.canRequestPackageInstalls()) {
            // Android asks the user once to let this app install updates.
            activity.say("Allow Roy Music to install updates, then tap Update again.")
            activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}")))
            return
        }
        val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.files", file)
        activity.startActivity(
            Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }

    /** Download, check, install. Returns an error text, or null. Worker thread. */
    fun install(activity: Activity, release: Release): String? {
        val file = download(activity, release) ?: return "The update could not be downloaded or did not match its checksum"
        activity.runOnUiThread { launch(activity, file) }
        return null
    }
}
