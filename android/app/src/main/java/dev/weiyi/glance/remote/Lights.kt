package dev.weiyi.glance.remote

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.drag
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.roundToInt
import kotlin.math.sin

/** The colour a PC names as RRGGBB. */
fun colour(hex: String) = Color(0xFF000000 or (hex.toLongOrNull(16) ?: 0xFFFFFF))

/** A colour by its hue (degrees), how much colour it has and how bright it is (each 0 to 1), as RRGGBB. */
private fun hex(hue: Float, depth: Float, bright: Float) = "%06X".format(Color.hsv(hue, depth, bright).toArgb() and 0xFFFFFF)

/** The colours given to choose from. */
private val GIVEN = listOf("FF7B4A", "FFFFFF", "70B3FF", "59C98A", "C68CFF", "FF5E7E")

/** What the lights may do, as the PC names each, with its word here. */
private val EFFECTS = listOf("steady" to R.string.effect_steady, "breathing" to R.string.effect_breathing, "rainbow" to R.string.effect_rainbow, "heat" to R.string.effect_heat)

@Composable
fun effectName(effect: String) = stringResource(EFFECTS.firstOrNull { it.first == effect }?.second ?: R.string.effect_steady)

/** Every hue in turn, round a circle from three o'clock. */
private val HUES = Brush.sweepGradient((0..6).map { Color.hsv(it * 60f % 360f, 1f, 1f) })

/**
 * The lights' effect and colour. [light] is how they are; [heat], whether
 * the PC can light them by how warm it is; [own], the colours made here
 * before. A colour being made is [preview]ed as it changes and [choose]n
 * when let go.
 */
@Composable
fun LightsCard(light: Light, heat: Boolean, own: List<String>, enabled: Boolean, keep: (List<String>) -> Unit, preview: (String?) -> Unit, choose: (color: String?, effect: String?) -> Unit) {
    // The colour the lights had as the ring was opened, to go back to; none: the ring is shut.
    var was by remember { mutableStateOf<String?>(null) }
    var deleting by remember { mutableStateOf<String?>(null) }
    Panel {
        Heading(stringResource(R.string.show_lights))
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for ((effect, name) in EFFECTS) {
                if (effect == "heat" && !heat) continue
                Chip(stringResource(name), light.effect == effect) { if (enabled) choose(null, effect) }
            }
        }
        // The colours given, then one's own, then the one that opens the ring.
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            for (one in GIVEN + own) {
                val chosen = was == null && light.color.equals(one, ignoreCase = true)
                val named = stringResource(R.string.colour, "#$one")
                val mine = one in own
                Canvas(
                    Modifier.size(36.dp).semantics { contentDescription = named; role = Role.Button }.pointerInput(one, mine, enabled) {
                        if (!enabled) return@pointerInput
                        detectTapGestures(
                            // One's own colour, held: asked whether to delete it.
                            onLongPress = { if (mine) deleting = one },
                            onTap = { was = null; preview(null); choose(one, null) },
                        )
                    },
                ) {
                    drawCircle(colour(one))
                    // The one chosen, ringed inside itself.
                    if (chosen) drawCircle(Card, size.minDimension / 2 - 4.5.dp.toPx(), style = Stroke(3.dp.toPx()))
                }
            }
            val making = stringResource(R.string.pick_colour)
            Canvas(Modifier.size(36.dp).clip(RoundedCornerShape(50)).clickable(enabled = enabled, role = Role.Button) { was = if (was == null) light.color else null }.semantics { contentDescription = making }) {
                drawCircle(HUES)
                drawCircle(Card, size.minDimension / 2 - 10.dp.toPx())
                if (was != null) drawCircle(Ink, size.minDimension / 2 - 1.25.dp.toPx(), style = Stroke(2.5.dp.toPx()))
            }
        }
        if (was != null || own.isNotEmpty()) Text(stringResource(R.string.colours_hint), color = Faint, fontSize = 12.sp, lineHeight = 17.sp)
        was?.let { before ->
            Picker(
                from = before,
                changed = { preview(it) },
                settled = { choose(it, null) },
                cancel = { was = null; preview(null); if (!light.color.equals(before, ignoreCase = true)) choose(before, null) },
                use = { made -> if ((GIVEN + own).none { it.equals(made, ignoreCase = true) }) keep(own + made); was = null },
            )
        }
    }
    deleting?.let { one ->
        Ask(stringResource(R.string.delete_colour_title), listOf(stringResource(R.string.delete_colour_what)), stringResource(R.string.delete), stringResource(R.string.cancel), dismiss = { deleting = null }) { keep(own - one) }
    }
}

/**
 * The colour, set by hand: a ring of hues, the colour itself in its middle,
 * then how much colour and how bright; kept, or given up. It starts as
 * [from]; each change is [changed], and [settled] when the finger lifts.
 */
@Composable
private fun Picker(from: String, changed: (String) -> Unit, settled: (String) -> Unit, cancel: () -> Unit, use: (String) -> Unit) {
    var hsv by remember {
        mutableStateOf(FloatArray(3).also { android.graphics.Color.colorToHSV(colour(from).toArgb(), it) })
    }
    val now = hex(hsv[0], hsv[1], hsv[2])
    val set by rememberUpdatedState { next: FloatArray -> hsv = next; changed(hex(next[0], next[1], next[2])) }
    val done by rememberUpdatedState { settled(hex(hsv[0], hsv[1], hsv[2])) }
    Column(Modifier.fillMaxWidth().padding(top = 4.dp), verticalArrangement = Arrangement.spacedBy(16.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        val hue = stringResource(R.string.hue)
        Box(Modifier.size(196.dp), contentAlignment = Alignment.Center) {
            Canvas(
                Modifier.size(196.dp).semantics { contentDescription = hue }.pointerInput(Unit) {
                    fun turn(at: Offset) = set(floatArrayOf((Math.toDegrees(atan2(at.y - size.height / 2f, at.x - size.width / 2f).toDouble()).toFloat() + 360f) % 360f, hsv[1], hsv[2]))
                    awaitEachGesture {
                        val down = awaitFirstDown()
                        down.consume()
                        turn(down.position)
                        drag(down.id) { it.consume(); turn(it.position) }
                        done()
                    }
                },
            ) {
                val band = 26.dp.toPx()
                val middle = size.minDimension / 2 - band / 2
                drawCircle(HUES, middle, style = Stroke(band))
                drawCircle(colour(now), size.minDimension / 2 - 42.dp.toPx())
                val angle = Math.toRadians(hsv[0].toDouble())
                val knob = center + Offset(cos(angle).toFloat(), sin(angle).toFloat()) * middle
                drawCircle(Color(0x99000000), 17.dp.toPx(), knob + Offset(0f, 1.dp.toPx()))
                drawCircle(Color.White, 16.dp.toPx(), knob)
                drawCircle(Color.hsv(hsv[0], 1f, 1f), 13.dp.toPx(), knob)
            }
            val ink = if (hsv[2] > 0.6f && hsv[1] < 0.75f) Color.Black else Color.White
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text("#$now", color = ink, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.6.sp)
                Text(stringResource(R.string.this_colour), color = ink.copy(alpha = 0.75f), fontSize = 11.sp)
            }
        }
        @Composable
        fun slide(label: String, at: Int, from: Float, track: Brush) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(label, color = Dim, fontSize = 13.sp, modifier = Modifier.width(36.dp))
                Bar(hsv[at], from..1f, label, Modifier.weight(1f), track = { track }, change = { value -> set(hsv.copyOf().also { it[at] = value }) }, done = { done() })
                Text("${(hsv[at] * 100).roundToInt()}%", color = Ink, fontSize = 13.sp, textAlign = TextAlign.End, modifier = Modifier.width(40.dp))
            }
        }
        slide(stringResource(R.string.saturation), 1, 0f, Brush.horizontalGradient(listOf(Color.hsv(hsv[0], 0f, hsv[2]), Color.hsv(hsv[0], 1f, hsv[2]))))
        // (Never quite dark: that is the lights off.)
        slide(stringResource(R.string.brightness), 2, 0.05f, Brush.horizontalGradient(listOf(Color.Black, Color.hsv(hsv[0], hsv[1], 1f))))
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.weight(1f).height(44.dp).clip(RoundedCornerShape(14.dp)).background(Card2).clickable(role = Role.Button, onClick = cancel), contentAlignment = Alignment.Center) {
                Text(stringResource(R.string.cancel), color = Ink, fontSize = 15.sp)
            }
            Box(Modifier.weight(1f).height(44.dp).clip(RoundedCornerShape(14.dp)).background(Ink).clickable(role = Role.Button) { use(now) }, contentAlignment = Alignment.Center) {
                Text(stringResource(R.string.use_colour), color = Color.Black, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
            }
        }
    }
}
