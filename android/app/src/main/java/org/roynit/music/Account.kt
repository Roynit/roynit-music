package org.roynit.music

import android.content.Context
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

/** The Roy account on roynit.dangz.org: the same one the PC app uses, so friends, profile and library carry over. */
object Account {
    const val BASE = "https://roynit.dangz.org/api/account"
    private var token: String? = null
    private var prefs: android.content.SharedPreferences? = null

    /** The signed-in account as the server describes it (name, bio, orbs, avatar_url...), empty when signed out. */
    var profile = JSONObject()
        private set

    val signedIn get() = token != null
    val name: String get() = profile.optString("display_name")
    val username: String get() = profile.optString("username")
    val orbs: Long get() = profile.optLong("orbs")

    /** Hidden from friends' online lists, like "Invisible" on the PC app. */
    var invisible = false
        private set

    fun load(context: Context) {
        if (prefs != null) return
        prefs = context.applicationContext.getSharedPreferences("roy_account", Context.MODE_PRIVATE).also {
            token = it.getString("token", null)
            profile = runCatching { JSONObject(it.getString("profile", "{}")!!) }.getOrDefault(JSONObject())
            invisible = it.getBoolean("invisible", false)
        }
    }

    private fun send(method: String, path: String, write: ((java.io.OutputStream) -> Unit)?, type: String?): JSONObject = try {
        val http = URL(BASE + path).openConnection() as HttpURLConnection
        http.requestMethod = method
        http.connectTimeout = 10_000
        http.readTimeout = 20_000
        token?.let { http.setRequestProperty("Authorization", "Bearer $it") }
        if (write != null) {
            http.doOutput = true
            http.setRequestProperty("Content-Type", type)
            http.outputStream.use(write)
        }
        val stream = if (http.responseCode in 200..299) http.inputStream else http.errorStream
        val text = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
        val json = runCatching { JSONObject(text) }.getOrNull() ?: JSONObject().put("error", "Unexpected reply from the server")
        if (http.responseCode == 401 && token != null && path != "/login") signOut()
        if (http.responseCode == 202) json.put("pending", true)
        json
    } catch (e: Exception) {
        JSONObject().put("error", "Could not reach the server")
    }

    /** Sends one request; returns the JSON reply or `{"error":"..."}`. Call it from a worker thread. */
    fun call(method: String, path: String, body: JSONObject? = null): JSONObject =
        send(method, path, body?.let { b -> { out: java.io.OutputStream -> out.write(b.toString().toByteArray()) } }, "application/json")

    /** Uploads a picture (profile picture or cover). */
    fun upload(path: String, bytes: ByteArray, type: String): JSONObject = send("PUT", path, { it.write(bytes) }, type)

    /** Takes an `{"account": ...}` reply into the profile. Returns an error text, or null. */
    fun accept(reply: JSONObject): String? {
        if (reply.has("error")) return reply.getString("error")
        val account = reply.optJSONObject("account") ?: return "Unexpected reply from the server"
        profile = account
        prefs?.edit()?.putString("profile", account.toString())?.apply()
        return null
    }

    /** Signs in with an email or username, or creates the account when [email] is given. Returns an error text or null. */
    fun enter(login: String, password: String, email: String? = null): String? {
        val reply = if (email == null) {
            call("POST", "/login", JSONObject().put("login", login).put("password", password))
        } else {
            call("POST", "/register", JSONObject().put("email", email).put("username", login).put("password", password))
        }
        return finish(reply)
    }

    private fun finish(reply: JSONObject): String? {
        if (reply.has("error")) return reply.getString("error")
        token = reply.optString("token").ifEmpty { return "Unexpected reply from the server" }
        prefs?.edit()?.putString("token", token)?.apply()
        return accept(reply)
    }

    /** Starts Google sign-in: the page to open in the browser and the code to wait on, or an error text. */
    fun googleStart(): Pair<String, String>? {
        val reply = call("POST", "/google/start")
        val url = reply.optString("url")
        val pickup = reply.optString("pickup")
        return if (url.startsWith("https://accounts.google.com/") && pickup.isNotEmpty()) url to pickup else null
    }

    /** Asks whether the browser sign-in is done: null while waiting, "" when signed in, otherwise an error text. */
    fun googlePickup(pickup: String): String? {
        val reply = call("POST", "/google/pickup", JSONObject().put("pickup", pickup))
        if (reply.optBoolean("pending")) return null
        return finish(reply) ?: ""
    }

    fun refresh(): String? = accept(call("GET", "/me"))

    fun setInvisible(value: Boolean) {
        invisible = value
        prefs?.edit()?.putBoolean("invisible", value)?.apply()
    }

    fun signOut() {
        token = null
        profile = JSONObject()
        prefs?.edit()?.remove("token")?.remove("profile")?.apply()
    }

    /** Server-side sign-out too, so the session cannot be reused. */
    fun logout() {
        call("POST", "/logout")
        signOut()
    }

    /** A person's picture address; [version] 0 means they have none. */
    fun avatarUrl(id: String, version: Int) = if (version > 0) "$BASE/avatar/$id?v=$version" else null

    fun bannerUrl(id: String, version: Int) = if (version > 0) "$BASE/banner/$id?v=$version" else null

    /** Tells friends what is playing, or that nothing is. Fire and forget. */
    fun presence(song: Song?, position: Long, playing: Boolean) {
        if (token == null) return
        val listening = song?.takeIf { !invisible }?.let {
            JSONObject().put("title", it.title).put("artist", it.artist).put("cover", it.cover ?: "")
                .put("duration", it.seconds).put("position", position / 1000).put("playing", playing)
        }
        call("PUT", "/presence", JSONObject().put("invisible", invisible).put("listening", listening ?: JSONObject.NULL))
    }
}
