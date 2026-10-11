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

/**
 * This phone, as it tells each PC: a number it gave itself once (a PC's
 * key is this phone's alone from its first use) and what it is called.
 */
object Phone {
    var id = ""
        private set
    val name: String get() = listOf(android.os.Build.MANUFACTURER, android.os.Build.MODEL).filter { it.isNotBlank() }.joinToString(" ")

    /** Reads the number kept, or makes and keeps one. */
    fun know(context: Context) {
        if (id.isNotEmpty()) return
        val kept = context.getSharedPreferences("phone", Context.MODE_PRIVATE)
        id = kept.getString("id", null) ?: java.util.UUID.randomUUID().toString().also { kept.edit().putString("id", it).apply() }
    }
}

/** The PCs paired with, and the one being looked at: kept in the app's own storage. */
object Pcs {
    private fun kept(context: Context) = context.getSharedPreferences("pcs", Context.MODE_PRIVATE).also { Phone.know(context) }

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

/** One of the PC's monitors: its name, where it is on the desktop, and whether it is on and how bright (percent), each none where it cannot be asked nor changed by itself. */
data class Monitor(val name: String, val x: Int, val y: Int, val width: Int, val height: Int, val on: Boolean?, val brightness: Int?)

/** How the lights are when they are on: their colour (RRGGBB), what they do ("steady", "breathing", "rainbow", "heat"), and how bright they are (percent). */
data class Light(val color: String, val effect: String, val brightness: Int = 100)

/** One of the PC's power plans: what the PC knows it by, its name there, and whether it is the one in use. */
data class Plan(val id: String, val name: String, val on: Boolean)

/** What is playing on the PC ([title] empty where its program does not say: then only that it sounds). */
data class Media(val title: String, val artist: String, val app: String, val playing: Boolean)

/** One part of the PC: how warm it is (°C) and how busy (%), each where it is known. */
data class Read(val temp: Int?, val load: Int?)

/** The PC's parts as Glance reads them there, and how fast its fans turn. */
data class Reads(val cpu: Read, val gpu: Read, val ram: Read, val disk: Read, val cpuFan: Int?, val caseFan: Int?) {
    fun of(part: Part) = when (part) { Part.Cpu -> cpu; Part.Gpu -> gpu; Part.Ram -> ram; Part.Disk -> disk }
}

enum class Part { Cpu, Gpu, Ram, Disk }

private fun JSONObject.int(name: String) = if (isNull(name)) null else optInt(name)

private fun JSONObject.read(name: String) = optJSONObject(name)?.let { Read(it.int("temp"), it.int("load")) } ?: Read(null, null)

/** The PC as it says it is. */
data class PcState(
    val name: String, val lights: Boolean?, val night: Boolean, val screen: Boolean?, val volume: Int?, val muted: Boolean?, val monitors: List<Monitor> = emptyList(),
    val light: Light = Light("FFFFFF", "steady"), val media: Media? = null, val reads: Reads? = null, val power: List<Plan> = emptyList(),
) {
    companion object {
        fun from(json: JSONObject) = PcState(
            light = json.optJSONObject("light")?.let { Light(it.optString("color", "FFFFFF"), it.optString("effect", "steady"), it.optInt("brightness", 100)) } ?: Light("FFFFFF", "steady"),
            power = json.optJSONArray("power")?.let { list -> (0 until list.length()).map { at -> list.getJSONObject(at).let { Plan(it.optString("id"), it.optString("name"), it.optBoolean("on")) } } } ?: emptyList(),
            media = json.optJSONObject("media")?.let { Media(it.optString("title"), it.optString("artist"), it.optString("app"), it.optBoolean("playing")) },
            reads = json.optJSONObject("reads")?.let { Reads(it.read("cpu"), it.read("gpu"), it.read("ram"), it.read("disk"), it.int("cpu_fan"), it.int("case_fan")) },
            name = json.optString("name"),
            lights = if (json.isNull("lights")) null else json.optBoolean("lights"),
            night = json.optBoolean("night"),
            screen = if (json.isNull("screen")) null else json.optBoolean("screen"),
            volume = if (json.isNull("volume")) null else json.optInt("volume"),
            muted = if (json.isNull("muted")) null else json.optBoolean("muted"),
            monitors = json.optJSONArray("monitors")?.let { list ->
                (0 until list.length()).map { at ->
                    val one = list.getJSONObject(at)
                    Monitor(one.optString("name"), one.optInt("x"), one.optInt("y"), one.optInt("width"), one.optInt("height"), if (one.isNull("on")) null else one.optBoolean("on"), one.int("brightness"))
                }
            } ?: emptyList(),
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
            // Which phone this is: the PC keeps a key to the phone that first used it.
            setRequestProperty("X-Glance-Phone", Phone.id)
            setRequestProperty("X-Glance-Name", java.net.URLEncoder.encode(Phone.name, "UTF-8").replace("+", "%20"))
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

/** What the main page shows, and the colours to light the PC in: this phone's own, kept in the app's storage. */
object Look {
    /** The parts of the main page that may be left out, by the names they are kept under. */
    val PARTS = listOf("rig", "reads", "power", "lights", "monitors", "sound", "sleep")

    private fun kept(context: Context) = context.getSharedPreferences("look", Context.MODE_PRIVATE)

    /** Which parts are shown (all, till one is turned off). */
    fun shown(context: Context): Map<String, Boolean> = PARTS.associateWith { kept(context).getBoolean("show_$it", true) }

    fun show(context: Context, part: String, on: Boolean) = kept(context).edit().putBoolean("show_$part", on).apply()

    /** The colours a phone starts with. */
    private val GIVEN = listOf("FF7B4A", "FFFFFF", "70B3FF", "59C98A", "C68CFF", "FF5E7E")

    private fun list(text: String?) = text.orEmpty().split(',').filter { it.length == 6 }

    /**
     * The colours to choose from (RRGGBB), in their order: those given,
     * till any is taken away, and those made by hand after them. (Before
     * the given ones could be taken away, only those made by hand were
     * kept, as "colours".)
     */
    fun colours(context: Context): List<String> = kept(context).getString("palette", null)?.let(::list) ?: (GIVEN + list(kept(context).getString("colours", "")))

    fun keep(context: Context, colours: List<String>) = kept(context).edit().putString("palette", colours.joinToString(",")).apply()
}
