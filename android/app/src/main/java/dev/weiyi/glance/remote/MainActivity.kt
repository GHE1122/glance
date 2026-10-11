package dev.weiyi.glance.remote

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

private val Accent = Color(0xFFFF7B4A)
private val Ground = Color(0xFF0E1116)
private val Card = Color(0xFF181C23)
private val Lit = Color(0xFF2A1D17)
private val Ink = Color(0xFFF2F4F7)
private val Dim = Color(0xFF98A2AE)
private val Good = Color(0xFF58C98B)
private val Bad = Color(0xFFE5645A)

/** From this width on (a tablet, a phone unfolded) the controls are in two columns. */
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
                Surface(Modifier.fillMaxSize(), color = Ground) {
                    var pcs by remember { mutableStateOf(Pcs.all(this)) }
                    var chosen by remember { mutableIntStateOf(Pcs.chosen(this)) }
                    var adding by remember { mutableStateOf(false) }
                    val pair = { pc: Pc ->
                        Pcs.add(this, pc)
                        pcs = Pcs.all(this)
                        chosen = Pcs.chosen(this)
                        adding = false
                    }
                    // A link opened pairs with its PC.
                    LaunchedEffect(opened) {
                        opened?.let(Pc::fromLink)?.let(pair)
                        opened = null
                    }
                    val pc = pcs.getOrNull(chosen)
                    if (pc == null || adding) {
                        BackHandler(enabled = adding) { adding = false }
                        Pairing(first = pcs.isEmpty(), back = { adding = false }.takeIf { adding }, paired = pair)
                    } else {
                        Column(Modifier.fillMaxSize().safeDrawingPadding()) {
                            Computers(pcs, chosen, choose = { at -> Pcs.choose(this@MainActivity, at); chosen = at }, add = { adding = true })
                            // Each PC with what was last heard of it, not the one before it.
                            key(pc.key) {
                                Control(pc) {
                                    Pcs.remove(this@MainActivity, chosen)
                                    pcs = Pcs.all(this@MainActivity)
                                    chosen = Pcs.chosen(this@MainActivity)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opened = intent.dataString
    }
}

/** The PCs paired with, in a row: the one looked at lit; and one more to add. */
@Composable
private fun Computers(pcs: List<Pc>, chosen: Int, choose: (Int) -> Unit, add: () -> Unit) {
    Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 10.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        pcs.forEachIndexed { at, pc ->
            val here = at == chosen
            Text(
                pc.name, fontSize = 14.sp, fontWeight = if (here) FontWeight.SemiBold else FontWeight.Normal, color = if (here) Color.Black else Dim,
                modifier = Modifier.clip(RoundedCornerShape(50)).background(if (here) Accent else Card).clickable { choose(at) }.padding(horizontal = 14.dp, vertical = 8.dp),
            )
        }
        Text(
            stringResource(R.string.add_pc), fontSize = 14.sp, color = Dim,
            modifier = Modifier.clip(RoundedCornerShape(50)).background(Card).clickable(onClick = add).padding(horizontal = 14.dp, vertical = 8.dp),
        )
    }
}

/** A PC's code is scanned, or its link pasted. */
@Composable
private fun Pairing(first: Boolean, back: (() -> Unit)?, paired: (Pc) -> Unit) {
    var link by remember { mutableStateOf("") }
    var wrong by remember { mutableStateOf(false) }
    val take = { text: String -> Pc.fromLink(text)?.let(paired) ?: run { wrong = true } }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result -> result.contents?.let(take) }
    val prompt = stringResource(R.string.scan_prompt)
    Box(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.Center) {
        // Back to the PCs paired with, where there are some.
        if (back != null) TextButton(onClick = back, modifier = Modifier.align(Alignment.TopStart).padding(8.dp)) { Text(stringResource(R.string.back), color = Dim, fontSize = 16.sp) }
        Column(Modifier.widthIn(max = 480.dp).verticalScroll(rememberScrollState()).padding(24.dp)) {
            Text(stringResource(if (first) R.string.pair_title else R.string.pair_another), fontSize = 30.sp, fontWeight = FontWeight.SemiBold, lineHeight = 38.sp)
            Spacer(Modifier.height(12.dp))
            Text(stringResource(R.string.pair_how), color = Dim, fontSize = 15.sp, lineHeight = 22.sp)
            Spacer(Modifier.height(28.dp))
            Button(
                onClick = { scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt(prompt).setBeepEnabled(false).setOrientationLocked(false)) },
                modifier = Modifier.fillMaxWidth().height(56.dp),
                shape = RoundedCornerShape(16.dp),
            ) { Text(stringResource(R.string.scan), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) }
            Spacer(Modifier.height(28.dp))
            OutlinedTextField(link, { link = it; wrong = false }, Modifier.fillMaxWidth(), label = { Text(stringResource(R.string.paste_link)) }, singleLine = true, isError = wrong)
            if (wrong) Text(stringResource(R.string.not_a_code), color = Bad, fontSize = 13.sp, modifier = Modifier.padding(top = 6.dp))
            TextButton(onClick = { take(link) }, enabled = link.isNotBlank()) { Text(stringResource(R.string.pair)) }
        }
    }
}

/** A PC paired with: how it is, and what it may be asked. */
@Composable
private fun Control(pc: Pc, unpair: () -> Unit) {
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<PcState?>(null) }
    var failure by remember { mutableStateOf<Failure?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    var sleepAsked by remember { mutableStateOf(false) }
    var unpairArmed by remember { mutableStateOf(false) }
    var level by remember { mutableFloatStateOf(0f) }
    var sliding by remember { mutableStateOf(false) }

    suspend fun ask(path: String, body: JSONObject? = null) {
        try {
            val answer = withContext(Dispatchers.IO) { pc.ask(path, body) }
            state = answer
            failure = null
            if (!sliding) answer.volume?.let { level = it.toFloat() }
        } catch (error: Failure) {
            failure = error
        }
    }

    // What is asked shows at once as it will be; the PC's answer then says
    // how it is.
    fun act(what: String, path: String, body: JSONObject = JSONObject(), expect: (PcState) -> PcState = { it }) {
        if (busy != null) return
        busy = what
        state = state?.let(expect)
        scope.launch {
            ask(path, body)
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
    // Unpairing asks twice: armed for a few seconds.
    LaunchedEffect(unpairArmed) {
        if (unpairArmed) {
            delay(4000)
            unpairArmed = false
        }
    }
    // Sleep says what it means first.
    if (sleepAsked) {
        AlertDialog(
            onDismissRequest = { sleepAsked = false },
            title = { Text(stringResource(R.string.sleep_title)) },
            text = { Text(stringResource(R.string.sleep_what), color = Dim, lineHeight = 21.sp) },
            confirmButton = { TextButton(onClick = { sleepAsked = false; act("sleep", "/v1/sleep") }) { Text(stringResource(R.string.sleep), color = Bad) } },
            dismissButton = { TextButton(onClick = { sleepAsked = false }) { Text(stringResource(R.string.cancel), color = Dim) } },
            containerColor = Card,
        )
    }

    val reached = state != null && failure == null
    val head: @Composable () -> Unit = {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(state?.name?.ifBlank { pc.name } ?: pc.name, fontSize = 26.sp, fontWeight = FontWeight.SemiBold)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(if (reached) Good else Bad))
                    Spacer(Modifier.width(8.dp))
                    Text(
                        when (val why = failure) {
                            null -> stringResource(if (state == null) R.string.connecting else R.string.connected)
                            Failure.Unreachable -> stringResource(R.string.unreachable)
                            Failure.Refused -> stringResource(R.string.refused)
                            is Failure.Failed -> why.why
                        },
                        color = Dim, fontSize = 14.sp,
                    )
                }
            }
            TextButton(onClick = { if (unpairArmed) unpair() else unpairArmed = true }) {
                Text(stringResource(if (unpairArmed) R.string.unpair_confirm else R.string.unpair), color = if (unpairArmed) Bad else Dim)
            }
        }
    }
    val switches: @Composable () -> Unit = {
        val night = state?.night == true
        Tile(
            title = stringResource(R.string.night),
            detail = stringResource(if (night) R.string.night_on else R.string.night_how),
            lit = night, working = busy == "night", enabled = reached, tall = true,
        ) { act("night", "/v1/night", on(!night)) { it.copy(night = !night) } }
        Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            // Lit while the screens are on: pressed, they go off, and on again.
            val screen = state?.screen
            Tile(
                stringResource(R.string.screen),
                stringResource(when (screen) { false -> R.string.screen_is_off; else -> R.string.screen_is_on }),
                lit = screen != false, working = busy == "screen", enabled = reached, modifier = Modifier.weight(1f),
            ) { act("screen", "/v1/screen", on(screen == false)) { it.copy(screen = screen == false) } }
            val lights = state?.lights
            Tile(
                stringResource(R.string.lights),
                stringResource(when (lights) { true -> R.string.lights_on; false -> R.string.lights_off; null -> R.string.lights_unknown }),
                lit = lights != false, working = busy == "lights", enabled = reached, modifier = Modifier.weight(1f),
            ) { act("lights", "/v1/lights", on(lights == false)) { it.copy(lights = lights == false) } }
        }
        // Each monitor by itself, where there are several.
        val monitors = state?.monitors.orEmpty()
        if (monitors.size > 1) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(Card).padding(horizontal = 18.dp, vertical = 10.dp)) {
                monitors.forEachIndexed { at, monitor ->
                    Row(Modifier.fillMaxWidth().height(52.dp), verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            // A monitor Windows has no name for is known by its size.
                            val named = monitor.name.substringBefore('(').trim().takeUnless { it.startsWith("Generic") || it.isBlank() }
                            Text(named ?: "${monitor.width} × ${monitor.height}", fontSize = 16.sp, color = if (monitor.on == null) Dim else Ink)
                            Text(if (monitor.on == null) stringResource(R.string.monitor_fixed) else "${monitor.width} × ${monitor.height}", fontSize = 12.sp, color = Dim)
                        }
                        if (monitor.on != null) {
                            Switch(
                                checked = monitor.on, enabled = reached && busy == null,
                                onCheckedChange = { wanted ->
                                    act("monitor", "/v1/monitor", JSONObject().put("index", at).put("on", wanted)) { now ->
                                        now.copy(monitors = now.monitors.mapIndexed { i, one -> if (i == at) one.copy(on = wanted) else one })
                                    }
                                },
                                colors = SwitchDefaults.colors(checkedTrackColor = Accent, checkedThumbColor = Color.Black),
                            )
                        }
                    }
                }
            }
        }
    }
    val rest: @Composable () -> Unit = {
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(20.dp)).background(Card).padding(horizontal = 18.dp, vertical = 14.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.volume), fontSize = 17.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
                val muted = state?.muted == true
                TextButton(onClick = { act("mute", "/v1/volume", JSONObject().put("muted", !muted)) }, enabled = reached) {
                    Text(stringResource(if (muted) R.string.unmute else R.string.mute), color = if (muted) Accent else Dim)
                }
            }
            Slider(
                value = level, valueRange = 0f..100f, enabled = reached,
                onValueChange = { sliding = true; level = it },
                onValueChangeFinished = {
                    sliding = false
                    scope.launch { ask("/v1/volume", JSONObject().put("level", level.toInt())) }
                },
                colors = SliderDefaults.colors(thumbColor = Accent, activeTrackColor = Accent),
            )
        }
        Button(
            onClick = { sleepAsked = true },
            enabled = reached && busy == null,
            modifier = Modifier.fillMaxWidth().height(56.dp), shape = RoundedCornerShape(18.dp),
            colors = ButtonDefaults.buttonColors(containerColor = Card, contentColor = Ink),
        ) { Text(stringResource(R.string.sleep), fontSize = 16.sp) }
    }

    // Narrow (a phone, a fold's cover): one column. Wide (unfolded, a
    // tablet): the switches beside the rest, no wider than reads well.
    BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        val gap = Arrangement.spacedBy(14.dp)
        if (maxWidth < WIDE) {
            Column(Modifier.verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = gap) {
                head()
                switches()
                rest()
            }
        } else {
            Column(Modifier.widthIn(max = 920.dp).verticalScroll(rememberScrollState()).padding(horizontal = 28.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(18.dp)) {
                head()
                Row(horizontalArrangement = Arrangement.spacedBy(18.dp)) {
                    Column(Modifier.weight(1f), verticalArrangement = gap) { switches() }
                    Column(Modifier.weight(1f), verticalArrangement = gap) { rest() }
                }
            }
        }
    }
}

/** One thing to ask, as a large button: lit while it is on. */
@Composable
private fun Tile(title: String, detail: String, lit: Boolean, working: Boolean, enabled: Boolean, modifier: Modifier = Modifier, tall: Boolean = false, press: () -> Unit) {
    Column(
        modifier.fillMaxWidth().height(if (tall) 132.dp else 112.dp).clip(RoundedCornerShape(20.dp)).background(if (lit && enabled) Lit else Card)
            .clickable(enabled = enabled && !working, onClick = press).padding(18.dp),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(title, fontSize = if (tall) 22.sp else 18.sp, fontWeight = FontWeight.SemiBold, color = if (!enabled) Dim else if (lit) Accent else Ink)
        Text(if (working) stringResource(R.string.working) else detail, color = Dim, fontSize = 13.sp, lineHeight = 18.sp)
    }
}
