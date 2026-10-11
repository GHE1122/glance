package dev.weiyi.glance.remote

import android.content.Context
import android.net.Uri
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.security.SecureRandom
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/** The PC paired with: where it is, the key shared with it, and its name. */
data class Pc(val host: String, val port: Int, val key: String, val name: String) {
    companion object {
        /** A pairing link, as the PC's QR code holds it: `glance://pair?h=…&p=…&k=…&n=…`. */
        fun fromLink(link: String): Pc? {
            val uri = runCatching { Uri.parse(link.trim()) }.getOrNull() ?: return null
            if (uri.scheme != "glance" || uri.host != "pair") return null
            val host = uri.getQueryParameter("h")?.takeIf { it.isNotBlank() } ?: return null
            val port = uri.getQueryParameter("p")?.toIntOrNull()?.takeIf { it in 1..65535 } ?: return null
            val key = uri.getQueryParameter("k")?.takeIf { it.length == 64 && it.all { c -> c.isDigit() || c in 'a'..'f' } } ?: return null
            return Pc(host, port, key, uri.getQueryParameter("n") ?: host)
        }

        fun load(context: Context): Pc? {
            val kept = context.getSharedPreferences("pc", Context.MODE_PRIVATE)
            return Pc(kept.getString("host", null) ?: return null, kept.getInt("port", 0), kept.getString("key", null) ?: return null, kept.getString("name", "") ?: "")
        }

        fun forget(context: Context) = context.getSharedPreferences("pc", Context.MODE_PRIVATE).edit().clear().apply()
    }

    fun keep(context: Context) =
        context.getSharedPreferences("pc", Context.MODE_PRIVATE).edit().putString("host", host).putInt("port", port).putString("key", key).putString("name", name).apply()
}

/** The PC as it says it is. */
data class PcState(val name: String, val lights: Boolean?, val night: Boolean, val volume: Int?, val muted: Boolean?) {
    companion object {
        fun from(json: JSONObject) = PcState(
            name = json.optString("name"),
            lights = if (json.isNull("lights")) null else json.optBoolean("lights"),
            night = json.optBoolean("night"),
            volume = if (json.isNull("volume")) null else json.optInt("volume"),
            muted = if (json.isNull("muted")) null else json.optBoolean("muted"),
        )
    }
}

/** What went wrong asking the PC. */
sealed class Failure : Exception() {
    /** No answer: the PC is off, asleep, or on another network. */
    object Unreachable : Failure()
    /** The PC does not take this phone's key (paired anew since, perhaps). */
    object Refused : Failure()
    /** The PC could not do it, in its words. */
    class Failed(val why: String) : Failure()
}

private fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it) }

private fun unhex(text: String) = ByteArray(text.length / 2) { text.substring(2 * it, 2 * it + 2).toInt(16).toByte() }

/**
 * Asks the PC for [path] (with [body], a POST; without, a GET), signed with
 * the shared key over what is asked, when, and a number used once. Blocks:
 * call it off the main thread. A long wait is allowed: the PC may be
 * starting its lighting program.
 */
fun Pc.ask(path: String, body: JSONObject? = null): PcState {
    val method = if (body == null) "GET" else "POST"
    val sent = body?.toString()?.toByteArray() ?: ByteArray(0)
    val time = (System.currentTimeMillis() / 1000).toString()
    val nonce = hex(ByteArray(8).also { SecureRandom().nextBytes(it) })
    val mac = Mac.getInstance("HmacSHA256").apply { init(SecretKeySpec(unhex(key), "HmacSHA256")) }
    for (part in listOf(method, path, time, nonce)) mac.update("$part\n".toByteArray())
    val sign = hex(mac.doFinal(sent))
    val connection = try {
        (URL("http://$host:$port$path").openConnection() as HttpURLConnection).apply {
            requestMethod = method
            connectTimeout = 2500
            readTimeout = 60000
            setRequestProperty("X-Glance-Time", time)
            setRequestProperty("X-Glance-Nonce", nonce)
            setRequestProperty("X-Glance-Sign", sign)
            if (body != null) {
                doOutput = true
                setRequestProperty("Content-Type", "application/json")
                outputStream.use { it.write(sent) }
            }
        }
    } catch (error: Exception) {
        throw Failure.Unreachable
    }
    try {
        val status = try { connection.responseCode } catch (error: Exception) { throw Failure.Unreachable }
        val text = (if (status == 200) connection.inputStream else connection.errorStream)?.use { it.readBytes().decodeToString() } ?: ""
        val json = runCatching { JSONObject(text) }.getOrElse { JSONObject() }
        return when (status) {
            200 -> PcState.from(json)
            401 -> throw Failure.Refused
            else -> throw Failure.Failed(json.optString("error", status.toString()))
        }
    } finally {
        connection.disconnect()
    }
}

fun on(value: Boolean) = JSONObject().put("on", value)
