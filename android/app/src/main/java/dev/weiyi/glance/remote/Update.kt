package dev.weiyi.glance.remote

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.net.Uri
import android.os.Build
import android.provider.Settings
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * The newer app (see `Newer`), taken in the app: downloaded from Glance's site and handed to the system to install.
 * The system asks the user first, as it does for every app not from a store, and installs it only if it is signed
 * as this one is: what is downloaded cannot be another app passing for this one.
 */
object Update {
    /** Where a newer app may be downloaded from: Glance's site's own downloads, by the name a release gives it. */
    private val FROM = Regex("""https://glancepc\.com/dl/Glance_[0-9]+(\.[0-9]+){1,3}\.apk""")

    /** The most an app may be (bytes): ours is a few megabytes. */
    private const val MOST = 64L shl 20

    /** How the taking of a newer app stands, as the banner says it. */
    sealed interface State {
        /** Not begun (or begun again after a failure). */
        data object Offered : State

        /** This app may not install apps yet: the system's page for that was opened, to be come back from. */
        data object Allow : State

        /** Downloading: how much of it, in hundredths (none: its size not known). */
        data class Downloading(val percent: Int?) : State

        /** Handed to the system, which asks the user. */
        data object Installing : State

        /** It could not be downloaded, or handed over: to be tried again. */
        data object Failed : State
    }

    /** Whether [url] is one a newer app may be downloaded from. */
    fun allowed(url: String) = FROM.matches(url)

    /** Whether this app may hand apps to the system to install (the user lets it, once, in the system's settings). */
    fun mayInstall(context: Context) = context.packageManager.canRequestPackageInstalls()

    /** The system's page where the user lets this app install apps. */
    fun allowing(context: Context) = Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))

    /**
     * Downloads the app at [url] (see [allowed]) and hands it to the system to install, telling [told] how it goes:
     * not from the main thread.
     */
    fun take(context: Context, url: String, told: (State) -> Unit) {
        if (!allowed(url)) return told(State.Failed)
        val file = File(context.cacheDir, "update.apk")
        try {
            download(url, file) { percent -> told(State.Downloading(percent)) }
            install(context, file)
            told(State.Installing)
        } catch (_: Exception) {
            told(State.Failed)
        } finally {
            file.delete()
        }
    }

    private fun download(url: String, into: File, progress: (Int?) -> Unit) {
        val asking = (URL(url).openConnection() as HttpURLConnection).apply {
            connectTimeout = 10_000
            readTimeout = 20_000
            instanceFollowRedirects = false
            setRequestProperty("User-Agent", "GlanceApp/${BuildConfig.VERSION_NAME}")
        }
        try {
            if (asking.responseCode != 200) throw java.io.IOException("answered ${asking.responseCode}")
            val size = asking.contentLengthLong.takeIf { it in 1..MOST }
            asking.inputStream.use { input ->
                into.outputStream().use { output ->
                    val buffer = ByteArray(64 shl 10)
                    var done = 0L
                    // (Said once for each hundredth; once only, its size not known.)
                    var said: Int? = -1
                    while (true) {
                        val read = input.read(buffer)
                        if (read < 0) break
                        done += read
                        if (done > MOST) throw java.io.IOException("larger than an app is")
                        output.write(buffer, 0, read)
                        val percent = size?.let { (done * 100 / it).toInt() }
                        if (percent != said) {
                            said = percent
                            progress(percent)
                        }
                    }
                }
            }
        } finally {
            asking.disconnect()
        }
    }

    /** Hands [file] to the system's installer, as an update of this app (see [Installed] for what it answers). */
    private fun install(context: Context, file: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(context.packageName)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
        }
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            file.inputStream().use { input ->
                session.openWrite("app.apk", 0, file.length()).use { output ->
                    input.copyTo(output)
                    session.fsync(output)
                }
            }
            val answered = PendingIntent.getBroadcast(
                context, id, Intent(context, Installed::class.java).setPackage(context.packageName),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
            )
            session.commit(answered.intentSender)
        }
    }
}

/** What the system's installer answers: asking the user first, it hands back the page that asks, shown here. */
class Installed : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE) != PackageInstaller.STATUS_PENDING_USER_ACTION) return
        val asking = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
        } else {
            @Suppress("DEPRECATION") intent.getParcelableExtra(Intent.EXTRA_INTENT)
        }
        asking?.let { context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
    }
}
