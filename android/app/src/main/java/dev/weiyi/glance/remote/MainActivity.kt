package dev.weiyi.glance.remote

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import kotlin.math.min

/** From this width on (a tablet, a phone unfolded) the PC's picture has the right side to itself. */
private val WIDE = 600.dp

class MainActivity : ComponentActivity() {
    /** A pairing link this activity was opened with, till it is taken. */
    private var opened by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Dark behind the system's bars too, their icons light.
        enableEdgeToEdge(SystemBarStyle.dark(android.graphics.Color.TRANSPARENT), SystemBarStyle.dark(android.graphics.Color.TRANSPARENT))
        opened = intent?.dataString
        setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Accent, onPrimary = Color.Black, background = Ground, surface = Card, onBackground = Ink, onSurface = Ink)) {
                Surface(Modifier.fillMaxSize(), color = Ground) { App(opened) { opened = null } }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opened = intent.dataString
    }
}

/** What is over the main page: nothing, the settings, or the pairing of one more PC. */
private enum class Page { Home, Settings, Pairing }

@Composable
private fun App(opened: String?, taken: () -> Unit) {
    val context = LocalContext.current
    var pcs by remember { mutableStateOf(Pcs.all(context)) }
    var chosen by remember { mutableIntStateOf(Pcs.chosen(context)) }
    var page by remember { mutableStateOf(Page.Home) }
    var shown by remember { mutableStateOf(Look.shown(context)) }
    var colours by remember { mutableStateOf(Look.colours(context)) }
    // A code scanned that is no pairing code is said on the pairing page.
    var wrong by remember { mutableStateOf(false) }
    var unpairing by remember { mutableStateOf<Int?>(null) }
    val pair = { pc: Pc ->
        Pcs.add(context, pc)
        pcs = Pcs.all(context)
        chosen = Pcs.chosen(context)
        page = Page.Home
    }
    val take = { text: String -> Pc.fromLink(text)?.let(pair) ?: run { wrong = true; page = Page.Pairing } }
    // A link opened pairs with its PC.
    LaunchedEffect(opened) {
        opened?.let(Pc::fromLink)?.let(pair)
        taken()
    }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result -> result.contents?.let(take) }
    val prompt = stringResource(R.string.scan_prompt)
    val scan = { scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt(prompt).setBeepEnabled(false).setOrientationLocked(false)) }

    Box(Modifier.fillMaxSize().safeDrawingPadding()) {
        Column(Modifier.fillMaxSize()) {
            Computers(pcs, chosen, choose = { at -> Pcs.choose(context, at); chosen = at }, add = { wrong = false; page = Page.Pairing }, settings = { page = Page.Settings })
            val pc = pcs.getOrNull(chosen)
            if (pc == null) {
                Empty(scan) { wrong = false; page = Page.Pairing }
            } else {
                // Each PC with what was last heard of it, not the one before it.
                key(pc.key) { Control(pc, shown, colours) { kept -> Look.keep(context, kept); colours = kept } }
            }
        }
        // A page over the main one covers it, and takes what is pressed.
        if (page != Page.Home) {
            BackHandler { page = Page.Home }
            Box(Modifier.fillMaxSize().background(Ground).pointerInput(Unit) {}) {
                when (page) {
                    Page.Pairing -> Pairing(first = pcs.isEmpty(), wrong = wrong, back = { page = Page.Home }, scan = scan) { text -> wrong = false; take(text) }
                    else -> SettingsPage(
                        pcs, chosen, shown,
                        back = { page = Page.Home },
                        show = { part, on -> Look.show(context, part, on); shown = Look.shown(context) },
                        add = { wrong = false; page = Page.Pairing },
                        unpair = { unpairing = it },
                    )
                }
            }
        }
    }
    // Which PC is to be unpaired is asked by its name.
    unpairing?.let { at ->
        val which = pcs.getOrNull(at) ?: return@let
        Ask(stringResource(R.string.unpair_title, which.name), listOf(stringResource(R.string.unpair_what)), stringResource(R.string.unpair), stringResource(R.string.cancel), dismiss = { unpairing = null }) {
            val looked = pcs.getOrNull(chosen)
            Pcs.remove(context, at)
            pcs = Pcs.all(context)
            // The PC looked at stays the one looked at, unless it was the one unpaired.
            val still = pcs.indexOfFirst { it.key == looked?.key }
            if (still >= 0) Pcs.choose(context, still)
            chosen = Pcs.chosen(context)
        }
    }
}

/** The PCs paired with, in a row: the one looked at lit; one more to add; and the settings. */
@Composable
private fun Computers(pcs: List<Pc>, chosen: Int, choose: (Int) -> Unit, add: () -> Unit, settings: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 6.dp, top = 8.dp).heightIn(min = 44.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            pcs.forEachIndexed { at, pc -> Chip(pc.name, at == chosen, ground = Card, lit = Accent) { choose(at) } }
            Chip(stringResource(R.string.add), false, ground = Card, press = add)
        }
        SignButton(Sign.GEAR, stringResource(R.string.settings), press = settings)
    }
}

/** A page's top: the way back, and what the page is. */
@Composable
private fun Top(title: String, back: (() -> Unit)?) {
    Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(start = 6.dp, end = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        if (back != null) SignButton(Sign.BACK, stringResource(R.string.back), tint = Ink, press = back) else Spacer(Modifier.width(6.dp))
        Text(title, fontSize = 20.sp, fontWeight = FontWeight.SemiBold, color = Ink)
    }
}

/** With no PC paired, only how to pair one. */
@Composable
private fun Empty(scan: () -> Unit, link: () -> Unit) {
    Column(Modifier.fillMaxSize().padding(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Panel(Modifier.widthIn(max = 480.dp)) {
            Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(stringResource(R.string.empty_title), fontSize = 17.sp, fontWeight = FontWeight.SemiBold, color = Ink)
                Text(stringResource(R.string.empty_how), color = Dim, fontSize = 14.sp, lineHeight = 21.sp, textAlign = TextAlign.Center)
                Box(Modifier.height(48.dp).clip(RoundedCornerShape(16.dp)).background(Accent).clickable(role = Role.Button, onClick = scan).padding(horizontal = 24.dp), contentAlignment = Alignment.Center) {
                    Text(stringResource(R.string.scan), color = Color.Black, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                }
                Words(stringResource(R.string.use_link), Dim, press = link)
            }
        }
    }
}

/** A PC's code is scanned, or its link pasted. */
@Composable
private fun Pairing(first: Boolean, wrong: Boolean, back: () -> Unit, scan: () -> Unit, take: (String) -> Unit) {
    var link by remember { mutableStateOf("") }
    var edited by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxSize()) {
        Top(stringResource(if (first) R.string.pair_title else R.string.pair_another), back)
        Column(Modifier.widthIn(max = 480.dp).verticalScroll(rememberScrollState()).padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.pair_how), color = Dim, fontSize = 14.sp, lineHeight = 21.sp)
            Box(Modifier.fillMaxWidth().height(52.dp).clip(RoundedCornerShape(16.dp)).background(Accent).clickable(role = Role.Button, onClick = scan), contentAlignment = Alignment.Center) {
                Text(stringResource(R.string.scan), color = Color.Black, fontSize = 16.sp, fontWeight = FontWeight.SemiBold)
            }
            OutlinedTextField(link, { link = it; edited = true }, Modifier.fillMaxWidth().padding(top = 12.dp), label = { Text(stringResource(R.string.paste_link)) }, singleLine = true, isError = wrong && !edited)
            if (wrong && !edited) Text(stringResource(R.string.not_a_code), color = Bad, fontSize = 13.sp)
            Words(stringResource(R.string.pair), Accent, size = 15f, weight = FontWeight.SemiBold, enabled = link.isNotBlank()) { edited = false; take(link) }
        }
    }
}

/** The page behind the gear: every PC paired with, each unpaired by itself, and what the main page shows. */
@Composable
private fun SettingsPage(pcs: List<Pc>, chosen: Int, shown: Map<String, Boolean>, back: () -> Unit, show: (String, Boolean) -> Unit, add: () -> Unit, unpair: (Int) -> Unit) {
    Column(Modifier.fillMaxSize()) {
        Top(stringResource(R.string.settings), back)
        Column(Modifier.widthIn(max = 560.dp).verticalScroll(rememberScrollState()).padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.paired_pcs), color = Dim, fontSize = 13.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(start = 16.dp, top = 8.dp))
            Panel {
                Column {
                    pcs.forEachIndexed { at, pc ->
                        Line(pc.name, pc.host, note = stringResource(R.string.looking_at).takeIf { at == chosen }) { Words(stringResource(R.string.unpair), Bad) { unpair(at) } }
                        Rule()
                    }
                    Box(Modifier.heightIn(min = 56.dp), contentAlignment = Alignment.CenterStart) { Words(stringResource(R.string.add_pc), Accent, size = 15f, press = add) }
                }
            }
            Text(stringResource(R.string.shows), color = Dim, fontSize = 13.sp, fontWeight = FontWeight.Medium, modifier = Modifier.padding(start = 16.dp, top = 8.dp))
            Panel {
                Column {
                    val parts = listOf(
                        Triple("rig", R.string.show_rig, R.string.show_rig_what), Triple("reads", R.string.show_reads, R.string.show_reads_what), Triple("lights", R.string.show_lights, R.string.show_lights_what),
                        Triple("monitors", R.string.show_monitors, R.string.show_monitors_what), Triple("sound", R.string.show_sound, R.string.show_sound_what), Triple("sleep", R.string.show_sleep, null),
                    )
                    parts.forEachIndexed { at, (part, name, what) ->
                        if (at > 0) Rule()
                        Line(stringResource(name), what?.let { stringResource(it) }) { Toggle(shown[part] != false, stringResource(name)) { show(part, it) } }
                    }
                }
            }
        }
    }
}

/** What is waiting to be asked of the PC, in turn. */
private class Asking(val what: String, val path: String, val body: JSONObject)

/** A PC paired with: how it is, and what it may be asked. */
@Composable
private fun Control(pc: Pc, shown: Map<String, Boolean>, colours: List<String>, keep: (List<String>) -> Unit) {
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<PcState?>(null) }
    var failure by remember { mutableStateOf<Failure?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    val waiting = remember { ArrayDeque<Asking>() }
    // How many things have been asked: what a look answers is stale if one was asked meanwhile.
    val asked = remember { intArrayOf(0) }
    var sleepAsked by remember { mutableStateOf(false) }
    var level by remember { mutableFloatStateOf(0f) }
    var sliding by remember { mutableStateOf(false) }
    var part by remember { mutableStateOf(Part.Cpu) }
    // A colour being made, shown before the PC has it.
    var making by remember { mutableStateOf<String?>(null) }

    suspend fun ask(path: String, body: JSONObject? = null) {
        val before = asked[0]
        val answer = try {
            withContext(Dispatchers.IO) { pc.ask(path, body) }
        } catch (error: Failure) {
            if (body != null || asked[0] == before) failure = error
            return
        }
        if (body == null && asked[0] != before) return
        state = answer
        failure = null
        if (!sliding) answer.volume?.let { level = it.toFloat() }
    }

    // What is asked shows at once as it will be; the PC's answer then says
    // how it is. Asked while another is on its way, it waits its turn (the
    // same thing asked again takes the place of the one waiting).
    fun act(what: String, path: String, body: JSONObject = JSONObject(), expect: (PcState) -> PcState = { it }) {
        asked[0]++
        state = state?.let(expect)
        waiting.removeAll { it.what == what }
        waiting.addLast(Asking(what, path, body))
        if (busy != null) return
        busy = what
        scope.launch {
            while (true) {
                val next = waiting.removeFirstOrNull() ?: break
                busy = next.what
                ask(next.path, next.body)
            }
            busy = null
        }
    }

    // How it is, asked while this is on screen.
    LaunchedEffect(pc) {
        while (true) {
            if (busy == null) ask("/v1/state")
            delay(1500)
        }
    }
    // Sleep says what it means first.
    if (sleepAsked) {
        Ask(stringResource(R.string.sleep_title), listOf(stringResource(R.string.sleep_what), stringResource(R.string.sleep_instead)), stringResource(R.string.sleep), stringResource(R.string.cancel), dismiss = { sleepAsked = false }) { act("sleep", "/v1/sleep") }
    }

    val reached = state != null && failure == null
    val now = state
    val night = now?.night == true
    val light = now?.light?.let { if (making != null) it.copy(color = making!!) else it }
    val working = stringResource(R.string.working)

    val rig: @Composable (Modifier, Boolean) -> Unit = { modifier, fill ->
        Column(modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(Card).padding(start = 8.dp, top = 8.dp, end = 8.dp, bottom = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val reads = now?.reads
            val names = mapOf(Part.Cpu to stringResource(R.string.cpu), Part.Gpu to stringResource(R.string.gpu), Part.Ram to stringResource(R.string.ram), Part.Disk to stringResource(R.string.disk))
            val notes = reads?.let {
                Notes(
                    Part.entries.associateWith { one ->
                        val read = reads.of(one)
                        Note(figure(read), if (read.temp != null && read.load != null) "${names[one]} · ${read.load}%" else names.getValue(one), if (one == Part.Cpu) reads.cpuFan?.let { stringResource(R.string.cpu_fan, it) }.orEmpty() else "")
                    },
                    reads.caseFan?.let { Note(it.toString(), stringResource(R.string.case_fans), stringResource(R.string.rpm)) },
                )
            }
            Rig(reads, notes, light?.takeIf { now?.lights == true }?.let { colour(it.color) }, night, part, if (fill) Modifier.weight(1f).fillMaxWidth() else Modifier.fillMaxWidth().aspectRatio(320f / 254f)) { part = it }
            if (now != null && reads == null) Text(stringResource(R.string.no_reads), color = Faint, fontSize = 12.sp, lineHeight = 17.sp, modifier = Modifier.padding(horizontal = 8.dp))
            if (reads != null && shown["reads"] != false) ReadsRow(reads, part, Modifier.padding(horizontal = 4.dp)) { part = it }
        }
    }

    val cards: @Composable () -> Unit = cards@{
        when (val why = failure) {
            null -> {}
            is Failure.Failed -> Panel { Text(why.why, color = Dim, fontSize = 14.sp, lineHeight = 21.sp) }
            else -> Panel { Text(stringResource(if (why == Failure.Refused) R.string.refused else R.string.unreachable), color = Dim, fontSize = 14.sp, lineHeight = 21.sp) }
        }
        if (now == null) return@cards
        // With the picture turned off, the readings alone.
        val reads = now.reads
        if (shown["rig"] == false && shown["reads"] != false && reads != null) {
            Panel {
                ReadsRow(reads, null) {}
                val fans = listOfNotNull(reads.cpuFan?.let { stringResource(R.string.cpu_fan_line, it) }, reads.caseFan?.let { stringResource(R.string.case_fan_line, it) })
                if (fans.isNotEmpty()) Text(fans.joinToString("　"), color = Dim, fontSize = 12.5.sp)
            }
        }
        Tile(
            Sign.NIGHT, stringResource(R.string.night), if (busy == "night") working else stringResource(if (night) R.string.night_on else R.string.night_how),
            lit = night, enabled = reached, big = true, modifier = Modifier.fillMaxWidth(),
        ) { act("night", "/v1/night", on(!night)) { it.copy(night = !night, screen = night) } }
        Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            // Lit while the screens are on: pressed, they go off, and on again.
            val screen = now.screen
            Tile(
                Sign.SCREEN, stringResource(R.string.screen), if (busy == "screen") working else stringResource(if (screen == false) R.string.screen_is_off else R.string.screen_is_on),
                lit = screen != false, enabled = reached, modifier = Modifier.weight(1f).fillMaxHeight(),
            ) { act("screen", "/v1/screen", on(screen == false)) { it.copy(screen = screen == false) } }
            val lights = now.lights
            Tile(
                Sign.LIGHTS, stringResource(R.string.lights),
                if (busy == "lights") working else when (lights) { true -> stringResource(R.string.lights_on, effectName(now.light.effect)); false -> stringResource(R.string.lights_off); null -> stringResource(R.string.lights_unknown) },
                lit = lights != false, enabled = reached, modifier = Modifier.weight(1f).fillMaxHeight(),
            ) { act("lights", "/v1/lights", on(lights == false)) { it.copy(lights = lights == false) } }
        }
        if (shown["lights"] != false && now.lights == true && !night && light != null) {
            LightsCard(
                light, heat = reads != null, own = colours, enabled = reached, keep = keep, preview = { making = it },
                choose = { color, effect, brightness ->
                    making = null
                    val body = JSONObject().put("on", true)
                    color?.let { body.put("color", it) }
                    effect?.let { body.put("effect", it) }
                    brightness?.let { body.put("brightness", it) }
                    act("light", "/v1/lights", body) { it.copy(light = Light(color ?: it.light.color, effect ?: it.light.effect, brightness ?: it.light.brightness)) }
                },
            )
        }
        // Each monitor by itself, where there are several.
        if (shown["monitors"] != false && now.monitors.size > 1) {
            Panel {
                Heading(stringResource(R.string.monitors)) { Places(now.monitors, now.screen != false) }
                Column {
                    now.monitors.forEachIndexed { at, monitor ->
                        if (at > 0) Rule()
                        MonitorRow(
                            at, monitor, enabled = reached && now.screen != false, lit = now.screen != false,
                            turn = { wanted -> act("monitor$at", "/v1/monitor", JSONObject().put("index", at).put("on", wanted)) { was -> was.copy(monitors = was.monitors.mapIndexed { i, one -> if (i == at) one.copy(on = wanted) else one }) } },
                            dim = { wanted -> act("bright$at", "/v1/monitor", JSONObject().put("index", at).put("brightness", wanted)) { was -> was.copy(monitors = was.monitors.mapIndexed { i, one -> if (i == at) one.copy(brightness = wanted) else one }) } },
                        )
                    }
                }
            }
        }
        // The sound: how loud, and what is playing, paused and played from here.
        if (shown["sound"] != false && now.volume != null) {
            Panel {
                val muted = now.muted == true
                Heading(stringResource(R.string.sound)) {
                    Words(stringResource(if (muted) R.string.unmute else R.string.mute), if (muted) Accent else Dim, size = 13f, enabled = reached) { act("mute", "/v1/volume", JSONObject().put("muted", !muted)) { it.copy(muted = !muted) } }
                }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    Bar(
                        level, 0f..100f, stringResource(R.string.volume), Modifier.weight(1f), enabled = reached, track = filled(if (muted) Faint else Accent),
                        change = { sliding = true; level = it },
                        done = { sliding = false; act("volume", "/v1/volume", JSONObject().put("level", level.toInt())) { it.copy(volume = level.toInt(), muted = false) } },
                    )
                    Text(if (muted) stringResource(R.string.muted) else "${level.toInt()}%", color = Ink, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.End, modifier = Modifier.width(44.dp))
                }
                now.media?.let { media ->
                    Rule()
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(Modifier.weight(1f)) {
                            Text(media.title, color = Ink, fontSize = 15.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            val about = listOf(stringResource(if (media.playing) R.string.playing else R.string.paused), media.artist, media.app).filter { it.isNotBlank() }
                            Text(about.joinToString(" · "), color = Dim, fontSize = 12.5.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                        Row {
                            val tell = { what: String, expect: (PcState) -> PcState -> act("media", "/v1/media", JSONObject().put("do", what), expect) }
                            SignButton(Sign.PREVIOUS, stringResource(R.string.previous), tint = Ink, enabled = reached) { tell("previous") { it } }
                            SignButton(if (media.playing) Sign.PAUSE else Sign.PLAY, stringResource(if (media.playing) R.string.pause else R.string.play), tint = Ink, ground = Card2, enabled = reached) {
                                tell("toggle") { it.copy(media = it.media?.copy(playing = !media.playing)) }
                            }
                            SignButton(Sign.NEXT, stringResource(R.string.next), tint = Ink, enabled = reached) { tell("next") { it } }
                        }
                    }
                }
            }
        }
        // Sleep says what it is where it is pressed, and again before it is done.
        if (shown["sleep"] != false) {
            Tile(Sign.SLEEP, stringResource(R.string.sleep), stringResource(R.string.sleep_how), lit = false, enabled = reached, big = true, modifier = Modifier.fillMaxWidth()) { sleepAsked = true }
        }
    }

    Column(Modifier.fillMaxSize()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(now?.name?.ifBlank { pc.name } ?: pc.name, fontSize = 22.sp, fontWeight = FontWeight.SemiBold, color = Ink, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Box(Modifier.size(8.dp).clip(CircleShape).background(if (reached) Good else if (failure == null) Faint else Bad))
                Text(
                    stringResource(when (failure) { null -> if (now == null) R.string.connecting else R.string.connected; Failure.Unreachable -> R.string.unreachable_short; Failure.Refused -> R.string.refused_short; else -> R.string.failed_short }),
                    color = Dim, fontSize = 13.sp, maxLines = 1,
                )
            }
        }
        // Folded, the PC's picture leads the one column; unfolded, it has the right side to itself.
        BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            val pictured = shown["rig"] != false && now != null
            val gap = Arrangement.spacedBy(12.dp)
            if (maxWidth >= WIDE && pictured) {
                Row(Modifier.fillMaxSize()) {
                    Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = gap) { cards() }
                    Box(Modifier.weight(1f).fillMaxHeight().padding(top = 4.dp, end = 16.dp, bottom = 24.dp)) { rig(Modifier.fillMaxHeight(), true) }
                }
            } else {
                Column(Modifier.widthIn(max = 560.dp).verticalScroll(rememberScrollState()).padding(start = 16.dp, top = 4.dp, end = 16.dp, bottom = 24.dp), verticalArrangement = gap) {
                    if (pictured) rig(Modifier, false)
                    cards()
                }
            }
        }
    }
}

/**
 * One monitor: its number and name, its size and how bright it is, its
 * switch; and under them, where it says how bright it is, that as a line
 * to slide (the figure beside its size follows the finger).
 */
@Composable
private fun MonitorRow(at: Int, monitor: Monitor, enabled: Boolean, lit: Boolean, turn: (Boolean) -> Unit, dim: (Int) -> Unit) {
    var level by remember { mutableFloatStateOf((monitor.brightness ?: 0).toFloat()) }
    var sliding by remember { mutableStateOf(false) }
    // As the monitor says, but for while it is being slid.
    LaunchedEffect(monitor.brightness) { if (!sliding) monitor.brightness?.let { level = it.toFloat() } }
    val size = "${monitor.width} × ${monitor.height}"
    val detail = when {
        monitor.on == null -> "$size · ${stringResource(R.string.monitor_fixed)}"
        monitor.brightness != null -> "$size · ${stringResource(R.string.brightness_is, level.toInt())}"
        else -> size
    }
    Line(
        monitor.name, detail,
        lead = { Box(Modifier.size(24.dp).clip(RoundedCornerShape(7.dp)).background(Card2), contentAlignment = Alignment.Center) { Text("${at + 1}", color = Dim, fontSize = 12.sp, fontWeight = FontWeight.SemiBold) } },
    ) { Toggle(monitor.on != false && lit, monitor.name, enabled = enabled && monitor.on != null, change = turn) }
    if (monitor.brightness != null) {
        val can = enabled && monitor.on != false
        Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.width(24.dp), contentAlignment = Alignment.Center) { Sign(Sign.BRIGHT, if (can) Dim else Faint, size = 18.dp) }
            Bar(level, 0f..100f, stringResource(R.string.brightness), Modifier.weight(1f), enabled = can, slim = true, track = filled(Accent), change = { sliding = true; level = it }, done = { sliding = false; dim(level.toInt()) })
        }
    }
}

/** A part's one figure: how warm it is; where that is not known, how busy. */
private fun figure(read: Read) = read.temp?.let { "$it°" } ?: read.load?.let { "$it%" } ?: "—"

/** The four parts, each a figure and its name; one pressed is chosen in the picture ([chosen] none: there is no picture). */
@Composable
private fun ReadsRow(reads: Reads, chosen: Part?, modifier: Modifier = Modifier, choose: (Part) -> Unit) {
    val names = mapOf(Part.Cpu to R.string.cpu, Part.Gpu to R.string.gpu, Part.Ram to R.string.ram, Part.Disk to R.string.disk)
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (part in Part.entries) {
            val read = reads.of(part)
            val name = stringResource(names.getValue(part))
            Column(
                Modifier.weight(1f).heightIn(min = 52.dp).clip(RoundedCornerShape(12.dp)).background(Card2)
                    .border(1.dp, if (chosen == part) Accent else Color.Transparent, RoundedCornerShape(12.dp))
                    .clickable(enabled = chosen != null, role = Role.Button) { choose(part) }.padding(horizontal = 8.dp, vertical = 7.dp),
            ) {
                Text(figure(read), color = Ink, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                Text(if (read.temp != null && read.load != null) "$name ${read.load}%" else name, color = Dim, fontSize = 11.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** Where each monitor is, small, to tell them apart: not for pressing. */
@Composable
private fun Places(monitors: List<Monitor>, screen: Boolean) {
    val paint = remember { android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG).apply { textAlign = android.graphics.Paint.Align.CENTER; typeface = android.graphics.Typeface.DEFAULT_BOLD } }
    Canvas(Modifier.size(74.dp, 30.dp)) {
        val left = monitors.minOf { it.x }
        val top = monitors.minOf { it.y }
        val wide = monitors.maxOf { it.x + it.width } - left
        val tall = monitors.maxOf { it.y + it.height } - top
        val line = 1.dp.toPx()
        val k = min((size.width - line) / wide, (size.height - line) / tall)
        // At the card's right edge, in the middle of its own height.
        val origin = Offset(size.width - line / 2 - wide * k, (size.height - tall * k) / 2)
        monitors.forEachIndexed { at, monitor ->
            val on = monitor.on != false && screen
            val corner = Offset(origin.x + (monitor.x - left) * k, origin.y + (monitor.y - top) * k)
            val box = Size(monitor.width * k, monitor.height * k)
            // (A hair apart, where two touch.)
            val inset = Offset(line, line)
            val within = Size(box.width - 2 * line, box.height - 2 * line)
            if (on) drawRoundRect(Lit, corner + inset, within, CornerRadius(2.dp.toPx()))
            drawRoundRect(if (on) Accent else Color(0xFF3A414C), corner + inset, within, CornerRadius(2.dp.toPx()), style = Stroke(line))
            paint.textSize = min(box.height * 0.5f, 8.dp.toPx())
            paint.color = (if (on) Ink else Faint).toArgb()
            drawIntoCanvas { it.nativeCanvas.drawText("${at + 1}", corner.x + box.width / 2, corner.y + box.height / 2 + paint.textSize * 0.36f, paint) }
        }
    }
}
