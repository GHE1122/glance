package dev.weiyi.glance.remote

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
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

class MainActivity : ComponentActivity() {
    /** A pairing link this activity was opened with, till it is taken. */
    private var opened by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        opened = intent?.dataString
        setContent {
            MaterialTheme(colorScheme = darkColorScheme(primary = Accent, onPrimary = Color.Black, background = Ground, surface = Card, onBackground = Ink, onSurface = Ink)) {
                Surface(Modifier.fillMaxSize(), color = Ground) {
                    var pc by remember { mutableStateOf(Pc.load(this)) }
                    // A link opened pairs anew.
                    LaunchedEffect(opened) {
                        opened?.let(Pc::fromLink)?.let {
                            it.keep(this@MainActivity)
                            pc = it
                        }
                        opened = null
                    }
                    when (val paired = pc) {
                        null -> Pairing { pc = it.also { found -> found.keep(this) } }
                        else -> Control(paired) {
                            Pc.forget(this)
                            pc = null
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

/** Not paired yet: the PC's code is scanned, or its link pasted. */
@Composable
private fun Pairing(paired: (Pc) -> Unit) {
    var link by remember { mutableStateOf("") }
    var wrong by remember { mutableStateOf(false) }
    val take = { text: String -> Pc.fromLink(text)?.let(paired) ?: run { wrong = true } }
    val scanner = rememberLauncherForActivityResult(ScanContract()) { result -> result.contents?.let(take) }
    val prompt = stringResource(R.string.scan_prompt)
    Column(Modifier.fillMaxSize().safeDrawingPadding().padding(24.dp), verticalArrangement = Arrangement.Center) {
        Text(stringResource(R.string.pair_title), fontSize = 30.sp, fontWeight = FontWeight.SemiBold, lineHeight = 38.sp)
        Spacer(Modifier.height(12.dp))
        Text(stringResource(R.string.pair_how), color = Dim, fontSize = 15.sp, lineHeight = 22.sp)
        Spacer(Modifier.height(28.dp))
        Button(
            onClick = { scanner.launch(ScanOptions().setDesiredBarcodeFormats(ScanOptions.QR_CODE).setPrompt(prompt).setBeepEnabled(false).setOrientationLocked(true)) },
            modifier = Modifier.fillMaxWidth().height(56.dp),
            shape = RoundedCornerShape(16.dp),
        ) { Text(stringResource(R.string.scan), fontSize = 17.sp, fontWeight = FontWeight.SemiBold) }
        Spacer(Modifier.height(28.dp))
        OutlinedTextField(link, { link = it; wrong = false }, Modifier.fillMaxWidth(), label = { Text(stringResource(R.string.paste_link)) }, singleLine = true, isError = wrong)
        if (wrong) Text(stringResource(R.string.not_a_code), color = Bad, fontSize = 13.sp, modifier = Modifier.padding(top = 6.dp))
        TextButton(onClick = { take(link) }, enabled = link.isNotBlank()) { Text(stringResource(R.string.pair)) }
    }
}

/** Paired: the PC as it is, and what it may be asked. */
@Composable
private fun Control(pc: Pc, unpair: () -> Unit) {
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<PcState?>(null) }
    var failure by remember { mutableStateOf<Failure?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    var sleepArmed by remember { mutableStateOf(false) }
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

    fun act(what: String, path: String, body: JSONObject? = JSONObject()) {
        if (busy != null) return
        busy = what
        scope.launch {
            ask(path, body)
            busy = null
        }
    }

    // How it is, asked while this is on screen.
    LaunchedEffect(pc) {
        while (true) {
            if (busy == null) ask("/v1/state")
            delay(2500)
        }
    }
    // The sleep button asks twice: armed for a few seconds.
    LaunchedEffect(sleepArmed) {
        if (sleepArmed) {
            delay(4000)
            sleepArmed = false
        }
    }

    val reached = state != null && failure == null
    Column(Modifier.fillMaxSize().safeDrawingPadding().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
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
            TextButton(onClick = unpair) { Text(stringResource(R.string.unpair), color = Dim) }
        }

        val night = state?.night == true
        Tile(
            title = stringResource(R.string.night),
            detail = stringResource(if (night) R.string.night_on else R.string.night_how),
            lit = night, working = busy == "night", enabled = reached, tall = true,
        ) { act("night", "/v1/night", on(!night)) }

        Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
            Tile(stringResource(R.string.screen_off), stringResource(R.string.screen_off_how), lit = false, working = busy == "screen", enabled = reached, modifier = Modifier.weight(1f)) {
                act("screen", "/v1/screen-off")
            }
            val lights = state?.lights
            Tile(
                stringResource(R.string.lights),
                stringResource(when (lights) { true -> R.string.lights_on; false -> R.string.lights_off; null -> R.string.lights_unknown }),
                lit = lights == true, working = busy == "lights", enabled = reached, modifier = Modifier.weight(1f),
            ) { act("lights", "/v1/lights", on(lights != true)) }
        }

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
            onClick = { if (sleepArmed) { sleepArmed = false; act("sleep", "/v1/sleep") } else sleepArmed = true },
            enabled = reached && busy == null,
            modifier = Modifier.fillMaxWidth().height(56.dp), shape = RoundedCornerShape(18.dp),
            colors = ButtonDefaults.buttonColors(containerColor = if (sleepArmed) Bad else Card, contentColor = Ink),
        ) { Text(stringResource(if (sleepArmed) R.string.sleep_confirm else R.string.sleep), fontSize = 16.sp) }
    }
}

/** One thing to ask, as a large button: lit while it is on. */
@Composable
private fun Tile(title: String, detail: String, lit: Boolean, working: Boolean, enabled: Boolean, modifier: Modifier = Modifier, tall: Boolean = false, press: () -> Unit) {
    Column(
        modifier.fillMaxWidth().height(if (tall) 132.dp else 112.dp).clip(RoundedCornerShape(20.dp)).background(if (lit) Lit else Card)
            .clickable(enabled = enabled && !working, onClick = press).padding(18.dp),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Text(title, fontSize = if (tall) 22.sp else 18.sp, fontWeight = FontWeight.SemiBold, color = if (!enabled) Dim else if (lit) Accent else Ink)
        Text(if (working) stringResource(R.string.working) else detail, color = Dim, fontSize = 13.sp, lineHeight = 18.sp)
    }
}
