package dev.weiyi.glance.remote

import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import kotlin.concurrent.thread

/** Night mode from the quick settings: pressed, the PC is asked for the other of on and off. */
class NightTile : TileService() {
    private fun show(night: Boolean?) {
        val tile = qsTile ?: return
        tile.state = when (night) {
            true -> Tile.STATE_ACTIVE
            false -> Tile.STATE_INACTIVE
            null -> Tile.STATE_UNAVAILABLE
        }
        tile.updateTile()
    }

    /** Asks the PC ([body] for a change, none for how it is) and shows what it answers. */
    private fun ask(change: Boolean?) {
        val pc = Pcs.all(this).getOrNull(Pcs.chosen(this)) ?: return show(null)
        thread {
            val night = runCatching { if (change == null) pc.ask("/v1/state") else pc.ask("/v1/night", on(change)) }.getOrNull()?.night
            show(night)
        }
    }

    override fun onStartListening() = ask(null)

    override fun onClick() = ask(qsTile?.state != Tile.STATE_ACTIVE)
}
