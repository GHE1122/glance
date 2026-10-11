package dev.weiyi.glance.remote

import android.content.Context
import android.net.Uri
import org.json.JSONArray
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
    }

    fun toJson(): JSONObject = JSONObject().put("host", host).put("port", port).put("key", key).put("name", name)
}

/** The PCs paired with, and the one being looked at: kept in the app's own storage. */
object Pcs {
    private fun kept(context: Context) = context.getSharedPreferences("pcs", Context.MODE_PRIVATE)

    fun all(context: Context): List<Pc> {
        val list = runCatching { JSONArray(kept(context).getString("list", "[]")) }.getOrElse { JSONArray() }
        return (0 until list.length()).mapNotNull { at ->
            val one = list.optJSONObject(at) ?: return@mapNotNull null
            Pc(one.optString("host"), one.optInt("port"), one.optString("key"), one.optString("name")).takeIf { it.host.isNotBlank() && it.key.length == 64 }
        }
    }

    private fun keep(context: Context, list: List<Pc>, chosen: Int) =
        kept(context).edit().putString("list", JSONArray(list.map(Pc::toJson)).toString()).putInt("chosen", chosen).apply()

    /** The one being looked at, as its place among them (0 when there is none). */
    fun chosen(context: Context): Int = kept(context).getInt("chosen", 0).coerceIn(0, (all(context).size - 1).coerceAtLeast(0))

    fun choose(context: Context, at: Int) = keep(context, all(context), at)

    /** Pairs with [pc] (a PC paired before, by its key, is paired anew in its place) and looks at it. */
    fun add(context: Context, pc: Pc) {
        val list = all(context).toMutableList()
        val at = list.indexOfFirst { it.key == pc.key }
        if (at >= 0) list[at] = pc else list.add(pc)
        keep(context, list, if (at >= 0) at else list.size - 1)
    }

    fun remove(context: Context, at: Int) {
        val list = all(context).toMutableList()
        if (at in list.indices) list.removeAt(at)
        keep(context, list, (at - 1).coerceAtLeast(0))
    }
}

/** The PC as it says it is. */
data class PcState(val name: String, val lights: Boolean?, val night: Boolean, val screen: Boolean?, val volume: Int?, val muted: Boolean?) {
    companion object {
        fun from(json: JSONObject) = PcState(
            name = json.optString("name"),
            lights = if (json.isNull("lights")) null else json.optBoolean("lights"),
            night = json.optBoolean("night"),
            screen = if (json.isNull("screen")) null else json.optBoolean("screen"),
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
