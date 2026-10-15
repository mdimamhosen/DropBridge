package dev.dropbridge.app

import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.MediaScannerConnection
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.app.NotificationCompat
import org.json.JSONArray
import org.json.JSONObject
import uniffi.dropbridge_ffi.DropNode
import uniffi.dropbridge_ffi.Listener
import java.io.File

data class Device(val id: String, val name: String, val kind: String, val host: String, val port: Int, val trusted: Boolean, val fingerprint: String? = id, val token: String? = null)

data class Transfer(
    val id: String, val outgoing: Boolean, val peer: String, val status: String,
    val sas: String = "", val done: Long = 0, val total: Long = 0, val speed: Long = 0, val avg: Long = 0, val eta: Long = 0,
    val filesDone: Int = 0, val filesTotal: Int = 0, val current: String = "", val paused: Boolean = false, val attempt: Int = 0,
    val ok: Boolean = false, val error: String? = null, val failed: List<String> = emptyList(),
    val bytes: Long = 0, val secs: Double = 0.0, val peak: Long = 0, val filesOk: Int = 0,
)

/** Process-wide holder of the Rust node and the UI state derived from its events. */
object Core : Listener {
    lateinit var app: Context
    var node: DropNode? = null
    var startError by mutableStateOf<String?>(null)
    val devices = mutableStateMapOf<String, Device>()
    val transfers = mutableStateMapOf<String, Transfer>()
    val order = mutableStateListOf<String>()
    val offers = mutableStateListOf<JSONObject>()
    val texts = mutableStateListOf<Pair<String, String>>()
    /** Outgoing transfers that are QR/code pairings; on success we trust the pinned peer too. */
    val pairing = mutableStateListOf<String>()
    var visible = false
    private val main = Handler(Looper.getMainLooper())
    private var multicast: WifiManager.MulticastLock? = null

    val prefs get() = app.getSharedPreferences("settings", Context.MODE_PRIVATE)
    val defaultDir: String get() = File(Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS), "DropBridge").path

    fun deviceName(): String =
        prefs.getString("name", null) ?: Settings.Global.getString(app.contentResolver, Settings.Global.DEVICE_NAME) ?: Build.MODEL

    fun start(ctx: Context) {
        if (node != null) return
        app = ctx.applicationContext
        // mDNS replies are multicast; without this lock many phones drop them to save power.
        multicast = (app.getSystemService(Context.WIFI_SERVICE) as WifiManager).createMulticastLock("dropbridge-mdns").apply {
            setReferenceCounted(false); acquire()
        }
        try {
            val identity = SecureStore.load(app) ?: ""
            val dir = prefs.getString("dir", defaultDir)!!
            val n = DropNode(deviceName(), "android", identity, app.filesDir.path, dir, prefs.getBoolean("auto", false), this)
            if (identity.isEmpty()) SecureStore.save(app, n.identitySecret())
            node = n
        } catch (e: Exception) {
            startError = e.message
        }
    }

    fun trustedIds(): Set<String> {
        val a = JSONArray(node?.devicesJson() ?: "[]")
        return (0 until a.length()).map { a.getJSONObject(it) }.filter { !it.getBoolean("blocked") }.map { it.getString("id") }.toSet()
    }

    // Called on Rust threads.
    override fun onEvent(json: String) {
        val e = JSONObject(json)
        main.post { handle(e) }
    }

    override fun openFd(uri: String): Int = try {
        app.contentResolver.openFileDescriptor(Uri.parse(uri), "r")?.detachFd() ?: -1
    } catch (e: Exception) {
        -1
    }

    private fun upd(id: String, f: (Transfer) -> Transfer) {
        val cur = transfers[id] ?: Transfer(id, outgoing = false, peer = "", status = "running")
        if (id !in order) order.add(0, id)
        transfers[id] = f(cur)
        TransferService.sync(app)
    }

    fun addOutgoing(id: String, peer: String) = upd(id) { Transfer(id, outgoing = true, peer = peer, status = "connecting") }

    fun dismiss(id: String) {
        transfers.remove(id); order.remove(id)
    }

    private fun handle(e: JSONObject) {
        val id = e.optString("transfer_id")
        when (e.getString("type")) {
            "PeerFound" -> {
                val addrs = e.getJSONArray("addrs")
                val host = (0 until addrs.length()).joinToString(",") { addrs.getString(it) }
                val pid = e.getString("id")
                devices[pid] = Device(pid, e.getString("name"), e.getString("kind"), host, e.getInt("port"), e.getBoolean("trusted") || pid in trustedIds())
            }
            "PeerLost" -> devices.remove(e.getString("id"))
            "IncomingOffer" -> {
                offers.add(e)
                if (!visible) notifyIncoming(e)
            }
            "AwaitingApproval" -> upd(id) { it.copy(status = "waiting", peer = e.getString("to_name"), sas = e.getString("sas"), outgoing = true) }
            "Reconnecting" -> upd(id) { it.copy(status = "reconnecting", attempt = e.getInt("attempt")) }
            "Progress" -> upd(id) {
                it.copy(
                    status = if (e.getBoolean("paused")) "paused" else "running", paused = e.getBoolean("paused"), outgoing = e.getBoolean("outgoing"),
                    peer = e.getString("peer_name"), done = e.getLong("bytes_done"), total = e.getLong("bytes_total"), speed = e.getLong("speed_bps"),
                    avg = e.getLong("avg_bps"), eta = e.getLong("eta_secs"), filesDone = e.getInt("files_done"), filesTotal = e.getInt("files_total"),
                    current = e.getString("current"),
                )
            }
            "ItemDone" -> if (e.getBoolean("ok")) MediaScannerConnection.scanFile(app, arrayOf(e.getString("path")), null, null)
            "TextReceived" -> {
                texts.add(e.getString("from_name") to e.getString("text"))
                addHistory(JSONObject().put("kind", "text").put("time", System.currentTimeMillis() / 1000).put("peer_name", e.getString("from_name")).put("text", e.getString("text")))
            }
            "Finished" -> {
                val failed = e.getJSONArray("failed").let { a -> (0 until a.length()).map { a.getString(it) } }
                val err = if (e.isNull("error")) null else e.getString("error")
                if (!e.getBoolean("outgoing") && err == "Declined") { dismiss(id); TransferService.sync(app); return }
                upd(id) {
                    it.copy(
                        status = "done", ok = e.getBoolean("ok"), error = err, failed = failed, filesOk = e.getInt("files_ok"), bytes = e.getLong("bytes"),
                        secs = e.getDouble("secs"), avg = e.getLong("avg_bps"), peak = e.getLong("peak_bps"), outgoing = e.getBoolean("outgoing"),
                        peer = e.getString("peer_name").ifEmpty { it.peer },
                    )
                }
                if (id in pairing && e.getBoolean("ok")) {
                    node?.device(e.getString("peer_id"), e.getString("peer_name"), "trust")
                    devices.replaceAll { _, d -> if (d.id == e.getString("peer_id")) d.copy(trusted = true) else d }
                    return
                }
                addHistory(JSONObject(e.toString()).put("kind", "transfer").put("time", System.currentTimeMillis() / 1000))
            }
        }
    }

    private fun notifyIncoming(e: JSONObject) {
        val pi = PendingIntent.getActivity(app, 0, Intent(app, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE)
        val n = NotificationCompat.Builder(app, App.CH_INCOMING)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("${e.getString("from_name")} wants to send you something")
            .setContentText("Tap to review and accept")
            .setContentIntent(pi).setAutoCancel(true)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()
        try {
            app.getSystemService(NotificationManager::class.java).notify(2, n)
        } catch (_: SecurityException) {
        }
    }

    fun history(): JSONArray = JSONArray(prefs.getString("history", "[]"))

    private fun addHistory(o: JSONObject) {
        val h = history()
        val out = JSONArray().put(o)
        for (i in 0 until minOf(h.length(), 199)) out.put(h.get(i))
        prefs.edit().putString("history", out.toString()).apply()
    }

    fun clearHistory() = prefs.edit().remove("history").apply()
}
