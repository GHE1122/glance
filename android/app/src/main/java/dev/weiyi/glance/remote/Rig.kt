package dev.weiyi.glance.remote

import android.graphics.Typeface
import android.provider.Settings
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.drawIntoCanvas
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.graphics.nativeCanvas
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import kotlin.math.cos
import kotlin.math.min
import kotlin.math.sin
import kotlin.math.sqrt

// The PC, drawn: every part a block of its own size in the case, all seen
// from the same corner (x to the right and down, y to the left and down, z
// up). The picture is 320 by 254, the case in its middle; what is said of
// its parts, either side of it.
private const val COS = 0.8660254f
private const val LEFT = -190f
private const val TOP = -148f
private const val WIDE = 320f
private const val TALL = 254f

private data class V(val x: Float, val y: Float, val z: Float) {
    operator fun plus(o: V) = V(x + o.x, y + o.y, z + o.z)
    operator fun minus(o: V) = V(x - o.x, y - o.y, z - o.z)
    operator fun times(k: Float) = V(x * k, y * k, z * k)
    val length get() = sqrt(x * x + y * y + z * z)
    /** Where it is in the picture. */
    val flat get() = Offset((x - y) * COS, (x + y) * 0.5f - z)
}

private fun v(x: Number, y: Number, z: Number) = V(x.toFloat(), y.toFloat(), z.toFloat())

/** A loop of corners in space as a path, each corner rounded by [r] in the plane of its two edges. */
private fun loop(points: List<V>, r: Float): Path {
    val path = Path()
    for (i in points.indices) {
        val corner = points[i]
        fun toward(other: V): Offset {
            val edge = other - corner
            return (corner + edge * (min(r, edge.length / 2) / edge.length)).flat
        }
        val from = toward(points[(i + points.size - 1) % points.size])
        val to = toward(points[(i + 1) % points.size])
        if (i == 0) path.moveTo(from.x, from.y) else path.lineTo(from.x, from.y)
        path.quadraticTo(corner.flat.x, corner.flat.y, to.x, to.y)
    }
    path.close()
    return path
}

/** A face of something: a flat drawing laid on it from its corner [o], across [u] and along [v]. */
private class Face(val o: V, val u: V, val v: V) {
    fun at(x: Float, y: Float) = (o + u * x + v * y).flat

    fun circle(r: Float): Path {
        val k = 0.5522848f * r
        val path = Path()
        fun curve(a: Offset, b: Offset, c: Offset) = path.cubicTo(a.x, a.y, b.x, b.y, c.x, c.y)
        at(r, 0f).let { path.moveTo(it.x, it.y) }
        curve(at(r, k), at(k, r), at(0f, r))
        curve(at(-k, r), at(-r, k), at(-r, 0f))
        curve(at(-r, -k), at(-k, -r), at(0f, -r))
        curve(at(k, -r), at(r, -k), at(r, 0f))
        path.close()
        return path
    }

    fun box(width: Float, height: Float, r: Float) = loop(listOf(o, o + u * width, o + u * width + v * height, o + v * height), r)

    /** The same laying, for what Android draws by itself (words). */
    fun matrix() = android.graphics.Matrix().apply {
        val (a, b) = (u.flat.x to u.flat.y)
        setValues(floatArrayOf(a, v.flat.x, o.flat.x, b, v.flat.y, o.flat.y, 0f, 0f, 1f))
    }
}

/** A block with soft corners: what it hides, then its side towards x, its side towards y and its top. */
private class Block(a: V, b: V, r: Float = 1.6f) {
    /** Its edge in the picture, all the way round. */
    val edge = listOf(v(a.x, a.y, b.z), v(b.x, a.y, b.z), v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(a.x, b.y, a.z), v(a.x, b.y, b.z)).map { it.flat }
    val hidden = Path().apply { edge.forEachIndexed { i, p -> if (i == 0) moveTo(p.x, p.y) else lineTo(p.x, p.y) }; close() }
    /** Each side, and how much of the block's colour it is filled with. */
    val sides = listOf(
        loop(listOf(v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(b.x, a.y, b.z)), r) to 0.10f,
        loop(listOf(v(a.x, b.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)), r) to 0.05f,
        loop(listOf(v(a.x, a.y, b.z), v(b.x, a.y, b.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)), r) to 0.17f,
    )

    fun holds(p: Offset): Boolean {
        var inside = false
        for (i in edge.indices) {
            val (a, b) = edge[i] to edge[(i + 1) % edge.size]
            if ((a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x) inside = !inside
        }
        return inside
    }
}

private const val W = 56f
private const val D = 124f
private const val H = 128f
private const val R = 9f

private object Case {
    /** The inside of the case: its floor, the wall the board is on, the back wall. */
    val walls = listOf(
        loop(listOf(v(0, 0, 0), v(W, 0, 0), v(W, D, 0), v(0, D, 0)), R),
        loop(listOf(v(0, 0, 0), v(0, D, 0), v(0, D, H), v(0, 0, H)), R),
        loop(listOf(v(0, 0, 0), v(W, 0, 0), v(W, 0, H), v(0, 0, H)), R),
    )
    val boardOn = Face(v(0.5, 10, 32), v(0, 1, 0), v(0, 0, 1))
    val board = boardOn.box(86f, 86f, 3f)
    val rear = Face(v(W / 2, 0.6, 98), v(1, 0, 0), v(0, 0, 1))
    /** The power supply's shroud along the bottom, its vents towards the glass. */
    val shroud = Block(v(2, 2, 2), v(W - 2, D - 2, 24), 3f)
    val vents = Face(v(W - 2, 18, 8), v(0, 1, 0), v(0, 0, 1))
    /** A drive lying on the shroud, towards the front. */
    val disk = Block(v(16, 86, 24), v(46, 114, 28))
    val diskLid = Face(v(20, 90, 28), v(1, 0, 0), v(0, 1, 0)).box(22f, 20f, 2f)
    /** The card: long, flat, out from the board; its light along the edge towards the glass. */
    val gpu = Block(v(0.5, 10, 58), v(42, 102, 72), 2.4f)
    val gpuTop = Face(v(6, 18, 72), v(1, 0, 0), v(0, 1, 0)).box(30f, 76f, 3f)
    val gpuLight = v(42, 20, 65).flat to v(42, 92, 65).flat
    /** The processor's cooler: its fan on top, its little screen saying how warm it is. */
    val cpu = Block(v(0.5, 28, 86), v(22, 52, 110), 2.4f)
    val cpuFan = Face(v(11.5, 40, 110.3), v(1, 0, 0), v(0, 1, 0))
    // (Across towards the back of the case: that way it reads from the left.)
    val screenOn = Face(v(22, 49, 107), v(0, -1, 0), v(0, 0, -1))
    val screen = screenOn.box(18f, 18f, 3.5f)
    val ram = listOf(60f, 66f, 72f, 78f).map { Block(v(0.5, it, 84), v(15, it + 2.4f, 116), 0.8f) }
    /** The front: three fans behind it. */
    val front = listOf(38f, 70f, 102f).map { Face(v(W / 2, D - 0.6, it), v(1, 0, 0), v(0, 0, 1)) }
    /** The case itself, over all of it: three panels with soft corners. */
    val shell = listOf(
        loop(listOf(v(0, 0, H), v(W, 0, H), v(W, D, H), v(0, D, H)), R),
        loop(listOf(v(W, 0, 0), v(W, D, 0), v(W, D, H), v(W, 0, H)), R),
        loop(listOf(v(0, D, 0), v(W, D, 0), v(W, D, H), v(0, D, H)), R),
    )

    /** The part under a point of the picture, the nearest the eye first. */
    fun part(p: Offset): Part? = when {
        ram.any { it.holds(p) } -> Part.Ram
        cpu.holds(p) -> Part.Cpu
        gpu.holds(p) -> Part.Gpu
        disk.holds(p) -> Part.Disk
        else -> null
    }
}

/** What is said beside the picture of one thing in it: a figure, and one or two lines under it. */
class Note(val figure: String, val first: String, val second: String = "")

/** What is said of each part, and of the case's fans. */
class Notes(val parts: Map<Part, Note>, val fans: Note?)

/** Where each note is: the point of the case its line starts at, where its words are, and whether they end there. */
private class Place(val from: V, val x: Float, val y: Float, val ends: Boolean)

private val PLACES = mapOf(
    Part.Cpu to Place(v(22, 36, 100), 66f, -100f, false),
    Part.Gpu to Place(v(42, 36, 68), 74f, -30f, false),
    Part.Disk to Place(v(40, 98, 28), 74f, 62f, false),
    Part.Ram to Place(v(15, 70, 116), -122f, -124f, true),
)
private val FANS = Place(v(W / 2, D, 38), -134f, 64f, true)

private val STEEL = Color(0xFF8EA2C0)

/** A part is the case's own steel (tinted by the lights), till it runs warm: then amber, then red. */
private fun heat(temp: Int?, tint: Color) = when {
    temp == null -> tint
    temp >= 75 -> Hot
    temp >= 62 -> Warm
    else -> tint
}

private fun shade(color: Color, part: Float) = lerp(Card, color, part)

private fun DrawScope.block(block: Block, color: Color, width: Float) {
    drawPath(block.hidden, shade(color, 0.08f))
    for ((side, part) in block.sides) {
        drawPath(side, shade(color, part))
        drawPath(side, color, style = Stroke(width, cap = StrokeCap.Round, join = StrokeJoin.Round))
    }
}

/** A fan lying in a face: its ring, its blades (turned by [turned] degrees) and its hub. */
private fun DrawScope.fan(on: Face, r: Float, turned: Float, color: Color, width: Float) {
    val stroke = Stroke(width, cap = StrokeCap.Round, join = StrokeJoin.Round)
    val ring = on.circle(r)
    drawPath(ring, shade(color, 0.06f))
    drawPath(ring, color, style = stroke)
    val blades = Path()
    for (i in 0 until 7) {
        val angle = Math.toRadians((turned + i * 360f / 7).toDouble())
        val (c, s) = cos(angle).toFloat() to sin(angle).toFloat()
        fun at(x: Float, y: Float) = on.at(x * c - y * s, x * s + y * c)
        val (from, bend, to) = Triple(at(r * 0.2f, 0f), at(r * 0.55f, -r * 0.42f), at(r * 0.86f, -r * 0.1f))
        blades.moveTo(from.x, from.y)
        blades.quadraticTo(bend.x, bend.y, to.x, to.y)
    }
    drawPath(blades, color, style = stroke)
    val hub = on.circle(r * 0.2f)
    drawPath(hub, Card)
    drawPath(hub, color, style = stroke)
}

/** Draws at less than its full strength, as one picture (not each line faded over the others). */
private inline fun DrawScope.faded(alpha: Float, draw: DrawScope.() -> Unit) {
    if (alpha >= 1f) return draw()
    drawIntoCanvas { it.saveLayer(Rect(LEFT, TOP, LEFT + WIDE, TOP + TALL), Paint().apply { this.alpha = alpha }) }
    draw()
    drawIntoCanvas { it.restore() }
}

/**
 * The PC drawn. [reads] colour its parts by how warm they are and turn its
 * fans; [notes] are what is said beside it (none: only the picture);
 * [light] is the lights' colour while they are on, which the whole inside
 * of the case takes; at [night] it is dim and still. A part pressed is
 * [choose]n.
 */
@Composable
fun Rig(reads: Reads?, notes: Notes?, light: Color?, night: Boolean, chosen: Part, modifier: Modifier = Modifier, choose: (Part) -> Unit) {
    val picked by rememberUpdatedState(choose)
    // How far each kind of fan has turned (the case's, the processor's), in degrees.
    val turned = remember { FloatArray(2) }
    var frame by remember { mutableLongStateOf(0L) }
    val speeds by rememberUpdatedState((reads?.caseFan ?: 0) to (reads?.cpuFan ?: 0))
    val context = LocalContext.current
    // Still where the phone is set to show no motion.
    val moving = remember { Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) > 0f }
    val turning = moving && !night && reads != null && ((reads.caseFan ?: 0) > 0 || (reads.cpuFan ?: 0) > 0)
    LaunchedEffect(turning) {
        if (!turning) return@LaunchedEffect
        var last = withFrameNanos { it }
        while (true) {
            val now = withFrameNanos { it }
            val passed = (now - last) / 1e9f
            last = now
            // One turn in the picture for sixteen of the fan's: slow enough to be seen turning.
            turned[0] = (turned[0] + passed * speeds.first * 6f / 16f) % 360f
            turned[1] = (turned[1] + passed * speeds.second * 6f / 16f) % 360f
            frame = now
        }
    }
    val words = remember {
        android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG)
    }
    Canvas(
        modifier.pointerInput(Unit) {
            detectTapGestures { tap ->
                val k = min(size.width / WIDE, size.height / TALL)
                val origin = Offset((size.width - WIDE * k) / 2 - LEFT * k, (size.height - TALL * k) / 2 - TOP * k)
                Case.part((tap - origin) / k)?.let { picked(it) }
            }
        },
    ) {
        frame
        val k = min(size.width / WIDE, size.height / TALL)
        val thin = 1.dp.toPx() / k
        val lit = light.takeUnless { night }
        val tint = if (lit != null) lerp(STEEL, lit, 0.42f) else STEEL
        fun colour(part: Part) = heat(reads?.of(part)?.temp, tint)
        fun width(part: Part) = if (notes != null && chosen == part) thin * 1.8f else thin
        withTransform({
            translate((size.width - WIDE * k) / 2 - LEFT * k, (size.height - TALL * k) / 2 - TOP * k)
            scale(k, k, Offset.Zero)
        }) {
            faded(if (night) 0.4f else 1f) {
                val stroke = Stroke(thin, cap = StrokeCap.Round, join = StrokeJoin.Round)
                // The lights' colour on the walls and the floor.
                val wall = if (lit != null) lerp(Color(0xFF12161B), lit, 0.09f) else Color(0xFF12161B)
                for (one in Case.walls) {
                    drawPath(one, wall)
                    drawPath(one, Color(0xFF3A424D), style = stroke)
                }
                // The board, flat on its wall, a few of its lines.
                drawPath(Case.board, Color(0xFF161B21))
                drawPath(Case.board, Color(0xFF46505C), style = stroke)
                for (y in listOf(12f, 20f)) drawLine(Color(0xFF46505C).copy(alpha = 0.55f), Case.boardOn.at(8f, y), Case.boardOn.at(78f, y), thin, StrokeCap.Round)
                // The fans are lit as the lights are; out, they are the case's grey.
                val fans = lit ?: Color(0xFF59636F)
                fan(Case.rear, 16f, turned[0], fans, thin)
                block(Case.shroud, Color(0xFF4A5461), thin)
                for (i in 0 until 9) drawLine(Color(0xFF4A5461).copy(alpha = 0.7f), Case.vents.at(i * 6f, 0f), Case.vents.at(i * 6f, 10f), thin, StrokeCap.Round)

                block(Case.disk, colour(Part.Disk), width(Part.Disk))
                drawPath(Case.diskLid, colour(Part.Disk).copy(alpha = 0.5f), style = stroke)

                block(Case.gpu, colour(Part.Gpu), width(Part.Gpu))
                drawPath(Case.gpuTop, colour(Part.Gpu).copy(alpha = 0.5f), style = stroke)
                if (lit != null) drawLine(lit, Case.gpuLight.first, Case.gpuLight.second, thin * 2.5f, StrokeCap.Round)

                block(Case.cpu, colour(Part.Cpu), width(Part.Cpu))
                fan(Case.cpuFan, 8.5f, turned[1], colour(Part.Cpu), thin)
                drawPath(Case.screen, Color(0xFF0B0E12))
                drawPath(Case.screen, colour(Part.Cpu), style = stroke)
                reads?.cpu?.temp?.let { temp ->
                    drawIntoCanvas {
                        val canvas = it.nativeCanvas
                        canvas.save()
                        canvas.concat(Case.screenOn.matrix())
                        words.apply { textSize = 8.5f; typeface = Typeface.DEFAULT_BOLD; textAlign = android.graphics.Paint.Align.CENTER; color = colour(Part.Cpu).toArgb() }
                        canvas.drawText(temp.toString(), 9f, 12.4f, words)
                        canvas.restore()
                    }
                }

                for (stick in Case.ram) block(stick, colour(Part.Ram), width(Part.Ram))
                for (one in Case.front) fan(one, 15f, turned[0], fans, thin)
                val shell = if (lit != null) lerp(Color(0xFF8F99A6), lit, 0.22f) else Color(0xFF8F99A6)
                for (panel in Case.shell) drawPath(panel, shell, style = Stroke(thin * 1.4f, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
            if (notes != null) {
                faded(if (night) 0.55f else 1f) {
                    fun say(place: Place, note: Note, here: Boolean) {
                        val from = place.from.flat
                        val elbow = place.x + if (place.ends) 12f else -12f
                        val path = Path().apply { moveTo(from.x, from.y); lineTo(elbow, place.y); lineTo(place.x, place.y) }
                        drawPath(path, Faint, style = Stroke(thin * 0.8f, cap = StrokeCap.Round, join = StrokeJoin.Round))
                        drawCircle(Ink, 2f, from)
                        val x = place.x + if (place.ends) -5f else 5f
                        drawIntoCanvas {
                            val canvas = it.nativeCanvas
                            words.textAlign = if (place.ends) android.graphics.Paint.Align.RIGHT else android.graphics.Paint.Align.LEFT
                            words.apply { textSize = 15f; typeface = Typeface.DEFAULT_BOLD; color = (if (here) Accent else Ink).toArgb() }
                            canvas.drawText(note.figure, x, place.y + 1f, words)
                            words.apply { textSize = 9.5f; typeface = Typeface.DEFAULT; color = Dim.toArgb() }
                            canvas.drawText(note.first, x, place.y + 13f, words)
                            if (note.second.isNotEmpty()) canvas.drawText(note.second, x, place.y + 25f, words)
                        }
                    }
                    for ((part, place) in PLACES) notes.parts[part]?.let { say(place, it, chosen == part) }
                    notes.fans?.let { say(FANS, it, false) }
                }
            }
        }
    }
}
