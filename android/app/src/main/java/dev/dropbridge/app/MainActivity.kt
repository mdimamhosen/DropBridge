package dev.dropbridge.app

import android.Manifest
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeEncoder
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import org.json.JSONArray
import org.json.JSONObject
import uniffi.dropbridge_ffi.humanBytes
import uniffi.dropbridge_ffi.parseTarget
import kotlin.concurrent.thread

/** Something chosen to send: label for the chip + the core's SendItem JSON. */
data class Staged(val label: String, val item: JSONObject)

@OptIn(ExperimentalLayoutApi::class)
class MainActivity : ComponentActivity() {
    private val staged = mutableStateListOf<Staged>()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        Core.start(this)
        handleIntent(intent)
        setContent {
            val dark = isSystemInDarkTheme()
            val ctx = LocalContext.current
            val scheme = when {
                Build.VERSION.SDK_INT >= 31 && dark -> dynamicDarkColorScheme(ctx)
                Build.VERSION.SDK_INT >= 31 -> dynamicLightColorScheme(ctx)
                dark -> darkColorScheme()
                else -> lightColorScheme()
            }
            MaterialTheme(colorScheme = scheme) { Main() }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    override fun onStart() {
        super.onStart(); Core.visible = true
    }

    override fun onStop() {
        Core.visible = false; super.onStop()
    }

    private fun toast(s: String) = Toast.makeText(this, s, Toast.LENGTH_LONG).show()

    /** Share sheet and dropbridge:// links. */
    private fun handleIntent(i: Intent?) {
        when (i?.action) {
            Intent.ACTION_SEND -> {
                @Suppress("DEPRECATION")
                val u = i.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)
                if (u != null) stageUris(listOf(u))
                else i.getStringExtra(Intent.EXTRA_TEXT)?.let { stageText(it) }
            }
            Intent.ACTION_SEND_MULTIPLE -> {
                @Suppress("DEPRECATION")
                i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { stageUris(it) }
            }
            Intent.ACTION_VIEW -> i.dataString?.let { pairWith(it) }
        }
    }

    fun stageText(t: String) {
        if (t.isNotBlank()) staged.add(Staged("“${t.take(40)}${if (t.length > 40) "…" else ""}”", JSONObject().put("type", "text").put("text", t)))
    }

    fun stageUris(uris: List<Uri>) {
        for (u in uris) describe(u)?.let { staged.add(Staged(it.getString("rel_path"), it)) } ?: toast("Can't read one of the files")
    }

    private fun describe(uri: Uri): JSONObject? = try {
        var name = uri.lastPathSegment ?: "file"
        var size = -1L
        var mtime: Long? = null
        contentResolver.query(uri, null, null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                c.getColumnIndex(OpenableColumns.DISPLAY_NAME).takeIf { it >= 0 }?.let { c.getString(it) }?.let { name = it }
                c.getColumnIndex(OpenableColumns.SIZE).takeIf { it >= 0 && !c.isNull(it) }?.let { size = c.getLong(it) }
                c.getColumnIndex(DocumentsContract.Document.COLUMN_LAST_MODIFIED).takeIf { it >= 0 && !c.isNull(it) }?.let { mtime = c.getLong(it) / 1000 }
            }
        }
        if (size < 0) size = contentResolver.openAssetFileDescriptor(uri, "r")?.use { it.length } ?: 0
        JSONObject().put("type", "uri").put("uri", uri.toString()).put("rel_path", name.replace('/', '_')).put("size", size).put("mtime", mtime ?: JSONObject.NULL)
    } catch (e: Exception) {
        null
    }

    /** Walk a picked folder keeping its structure, including empty subfolders. */
    fun stageTree(tree: Uri) = thread {
        val out = mutableListOf<JSONObject>()
        val root = DocumentsContract.getTreeDocumentId(tree)
        fun query(doc: String, f: (id: String, name: String, mime: String, size: Long, mtime: Long) -> Unit) {
            val cols = arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME, DocumentsContract.Document.COLUMN_MIME_TYPE,
                DocumentsContract.Document.COLUMN_SIZE, DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            )
            contentResolver.query(DocumentsContract.buildChildDocumentsUriUsingTree(tree, doc), cols, null, null, null)?.use { c ->
                while (c.moveToNext()) f(c.getString(0), c.getString(1), c.getString(2) ?: "", if (c.isNull(3)) 0 else c.getLong(3), if (c.isNull(4)) 0 else c.getLong(4))
            }
        }
        var rootName = "Folder"
        contentResolver.query(DocumentsContract.buildDocumentUriUsingTree(tree, root), arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) rootName = c.getString(0)
        }
        fun walk(doc: String, rel: String) {
            out.add(JSONObject().put("type", "dir").put("rel_path", rel))
            query(doc) { id, name, mime, size, mtime ->
                if (mime == DocumentsContract.Document.MIME_TYPE_DIR) walk(id, "$rel/$name")
                else out.add(
                    JSONObject().put("type", "uri").put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString())
                        .put("rel_path", "$rel/$name").put("size", size).put("mtime", if (mtime > 0) mtime / 1000 else JSONObject.NULL)
                )
            }
        }
        try {
            walk(root, rootName)
            val files = out.count { it.getString("type") == "uri" }
            runOnUiThread { out.forEach { staged.add(Staged(if (it.getString("type") == "dir") "" else it.getString("rel_path"), it)) }; compactFolder(rootName, files) }
        } catch (e: Exception) {
            runOnUiThread { toast("Can't read that folder: ${e.message}") }
        }
    }

    /** Show a picked folder as one chip instead of hundreds. */
    private fun compactFolder(name: String, files: Int) {
        val items = staged.filter { it.item.optString("rel_path").startsWith("$name/") || it.item.optString("rel_path") == name }
        staged.removeAll(items)
        staged.add(Staged("📁 $name ($files files)", JSONObject().put("type", "group").put("items", JSONArray(items.map { it.item }))))
    }

    private fun itemsJson(): String {
        val a = JSONArray()
        for (s in staged) {
            if (s.item.optString("type") == "group") {
                val g = s.item.getJSONArray("items"); for (i in 0 until g.length()) a.put(g.get(i))
            } else a.put(s.item)
        }
        return a.toString()
    }

    fun sendTo(d: Device) {
        val n = Core.node ?: return
        try {
            val id = n.send(d.host, d.port.toUShort(), d.fingerprint, d.token, itemsJson())
            Core.addOutgoing(id, d.name)
            staged.clear()
            if (d.token != null) Core.devices[d.id] = d.copy(token = null) // codes are one-time
        } catch (e: Exception) {
            toast(e.message ?: "Couldn't start the transfer")
        }
    }

    /** Scanned QR / opened link: pair now (empty offer carrying the one-time token). */
    fun pairWith(link: String) {
        try {
            val t = JSONObject(parseTarget(link))
            val d = Device("manual:${t.getString("host")}:${t.getInt("port")}", t.getString("host").split(",")[0], "", t.getString("host"), t.getInt("port"), true,
                t.optString("fingerprint").ifEmpty { null }, t.optString("token").ifEmpty { null })
            val id = Core.node!!.send(d.host, d.port.toUShort(), d.fingerprint, d.token, if (staged.isEmpty()) "[]" else itemsJson())
            staged.clear()
            Core.addOutgoing(id, "new device")
            Core.pairing.add(id)
        } catch (e: Exception) {
            toast(e.message ?: "Not a DropBridge code")
        }
    }

    @OptIn(ExperimentalMaterial3Api::class)
    @Composable
    fun Main() {
        val ctx = LocalContext.current
        var pendingTarget by remember { mutableStateOf<Device?>(null) }
        var dialog by remember { mutableStateOf(if (Core.prefs.getBoolean("onboarded", false)) "" else "welcome") }
        var menu by remember { mutableStateOf(false) }
        var batteryOk by remember { mutableStateOf(true) }
        LaunchedEffect(dialog) { batteryOk = (getSystemService(POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(packageName) }

        val afterPick = { pendingTarget?.let { if (staged.isNotEmpty()) sendTo(it) }; pendingTarget = null }
        val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { stageUris(it); afterPick() }
        val pickMedia = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia()) { stageUris(it); afterPick() }
        val pickTree = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { it?.let { stageTree(it) } }
        val scan = rememberLauncherForActivityResult(ScanContract()) { r -> r.contents?.let { pairWith(it) } }
        val notifPerm = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}

        Scaffold(topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text("DropBridge", fontWeight = FontWeight.SemiBold)
                        val ip = Core.node?.localIps()?.firstOrNull()
                        Text("Visible as ${Core.deviceName()}${ip?.let { " · $it" } ?: " · no Wi-Fi"}", style = MaterialTheme.typography.bodySmall)
                    }
                },
                actions = {
                    TextButton(onClick = { scan.launch(ScanOptions().setPrompt("Scan the code shown on your computer").setBeepEnabled(false).setOrientationLocked(false)) }) { Text("Scan QR") }
                    Box {
                        TextButton(onClick = { menu = true }) { Text("More") }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            listOf("pair" to "Show my pairing code", "manual" to "Connect by address", "history" to "History", "settings" to "Settings").forEach { (k, v) ->
                                DropdownMenuItem(text = { Text(v) }, onClick = { dialog = k; menu = false })
                            }
                        }
                    }
                },
            )
        }) { pad ->
            LazyColumn(Modifier.padding(pad).fillMaxSize(), contentPadding = PaddingValues(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Core.startError?.let { err -> item { Banner("DropBridge couldn't start: $err") } }
                if (!batteryOk) item {
                    Banner("Battery optimisation may pause transfers when the screen is off.", "Allow") {
                        startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                    }
                }
                item {
                    Card(Modifier.fillMaxWidth()) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Text("Send", style = MaterialTheme.typography.titleMedium)
                            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                FilledTonalButton(onClick = { pickFiles.launch(arrayOf("*/*")) }) { Text("Files") }
                                FilledTonalButton(onClick = { pickMedia.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageAndVideo)) }) { Text("Photos & videos") }
                                FilledTonalButton(onClick = { pickTree.launch(null) }) { Text("Folder") }
                                FilledTonalButton(onClick = { dialog = "text" }) { Text("Text") }
                            }
                            if (staged.isNotEmpty()) {
                                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                    staged.filter { it.label.isNotEmpty() }.forEach { s ->
                                        InputChip(selected = false, onClick = { staged.remove(s) }, label = { Text(s.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                                            trailingIcon = { Text("✕") }, modifier = Modifier.widthIn(max = 260.dp))
                                    }
                                }
                                Text("Now tap a device below.", color = MaterialTheme.colorScheme.primary, fontWeight = FontWeight.SemiBold)
                            }
                        }
                    }
                }
                item { Text("Nearby devices", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                if (Core.devices.isEmpty()) item {
                    Text("Looking for devices… Open DropBridge on your computer, on the same Wi-Fi. Not showing up? Use Scan QR.", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                items(Core.devices.values.sortedBy { it.name }, key = { it.id }) { d ->
                    ElevatedCard(Modifier.fillMaxWidth().clickable(onClickLabel = "Send to ${d.name}", role = Role.Button) {
                        if (staged.isEmpty()) { pendingTarget = d; pickFiles.launch(arrayOf("*/*")) } else sendTo(d)
                    }) {
                        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                            Text(mapOf("mac" to "💻", "windows" to "🖥️", "linux" to "🐧", "android" to "📱").getOrDefault(d.kind, "📟"), fontSize = 28.sp)
                            Spacer(Modifier.width(14.dp))
                            Column(Modifier.weight(1f)) {
                                Text(d.name, fontWeight = FontWeight.SemiBold)
                                Text(if (d.trusted) "Trusted" else "Not paired yet", style = MaterialTheme.typography.bodySmall,
                                    color = if (d.trusted) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            Text(if (staged.isEmpty()) "Choose…" else "Send", color = MaterialTheme.colorScheme.primary)
                        }
                    }
                }
                if (Core.order.isNotEmpty()) item { Text("Transfers", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                items(Core.order.toList(), key = { it }) { id -> Core.transfers[id]?.let { TransferCard(it) } }
            }
        }

        // Dialogs: incoming offers and texts first, then whichever the user opened.
        Core.offers.firstOrNull()?.let { OfferDialog(it) } ?: Core.texts.firstOrNull()?.let { (from, text) ->
            AlertDialog(onDismissRequest = { Core.texts.removeAt(0) }, title = { Text("Text from $from") },
                text = { SelectionContainer { Text(text, Modifier.verticalScroll(rememberScrollState())) } },
                confirmButton = {
                    TextButton(onClick = {
                        (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText("DropBridge", text))
                        Core.texts.removeAt(0)
                    }) { Text("Copy") }
                },
                dismissButton = { TextButton(onClick = { Core.texts.removeAt(0) }) { Text("Close") } })
        }
        when (dialog) {
            "welcome" -> AlertDialog(onDismissRequest = {}, title = { Text("Welcome to DropBridge") },
                text = {
                    Text(
                        "Send files, folders and text straight to your computer — no account, no cloud, encrypted end to end.\n\n" +
                            "• Notifications: so you see incoming requests and transfer progress.\n" +
                            "• Camera: only when you tap Scan QR to pair.\n" +
                            "• Your phone and computer need to be on the same Wi-Fi (or connect to your phone's hotspot)."
                    )
                },
                confirmButton = {
                    TextButton(onClick = {
                        Core.prefs.edit().putBoolean("onboarded", true).apply()
                        if (Build.VERSION.SDK_INT >= 33) notifPerm.launch(Manifest.permission.POST_NOTIFICATIONS)
                        dialog = ""
                    }) { Text("Continue") }
                })
            "text" -> {
                var t by remember { mutableStateOf("") }
                AlertDialog(onDismissRequest = { dialog = "" }, title = { Text("Send text") },
                    text = { OutlinedTextField(t, { t = it }, Modifier.fillMaxWidth().heightIn(min = 140.dp), label = { Text("Message, link or note") }) },
                    confirmButton = { TextButton(onClick = { stageText(t); dialog = "" }) { Text("Add") } },
                    dismissButton = { TextButton(onClick = { dialog = "" }) { Text("Cancel") } })
            }
            "pair" -> {
                val uri = remember { Core.node?.pairingUri() ?: "" }
                val code = remember { Core.node?.pairingCode() ?: "" }
                val bmp = remember(uri) { BarcodeEncoder().encodeBitmap(uri, BarcodeFormat.QR_CODE, 640, 640) }
                AlertDialog(onDismissRequest = { dialog = "" }, title = { Text("Pair a device") },
                    text = {
                        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
                            Image(bmp.asImageBitmap(), contentDescription = "Pairing QR code", Modifier.size(220.dp))
                            Text(code, fontFamily = FontFamily.Monospace, fontSize = 30.sp, fontWeight = FontWeight.Bold, letterSpacing = 4.sp)
                            Text("On the other device choose “Connect manually” and enter ${Core.node?.localIps()?.firstOrNull() ?: "?"}:${Core.node?.port()} and this code. Works once, for 5 minutes.",
                                style = MaterialTheme.typography.bodySmall)
                        }
                    },
                    confirmButton = { TextButton(onClick = { dialog = "" }) { Text("Done") } })
            }
            "manual" -> {
                var addr by remember { mutableStateOf("") }
                var code by remember { mutableStateOf("") }
                AlertDialog(onDismissRequest = { dialog = "" }, title = { Text("Connect by address") },
                    text = {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Text("On your computer open “Pair a device” to see its address and code.", style = MaterialTheme.typography.bodySmall)
                            OutlinedTextField(addr, { addr = it }, label = { Text("Address, e.g. 192.168.1.20:52345") }, singleLine = true)
                            OutlinedTextField(code, { code = it.filter(Char::isDigit).take(6) }, label = { Text("Pairing code (optional)") }, singleLine = true)
                        }
                    },
                    confirmButton = {
                        TextButton(onClick = {
                            try {
                                val t = JSONObject(parseTarget(addr))
                                val id = "manual:${t.getString("host")}:${t.getInt("port")}"
                                Core.devices[id] = Device(id, t.getString("host"), "", t.getString("host"), t.getInt("port"), false, null, code.ifEmpty { null })
                                if (code.isNotEmpty() && staged.isEmpty()) {
                                    val tid = Core.node!!.send(t.getString("host"), t.getInt("port").toUShort(), null, code, "[]")
                                    Core.addOutgoing(tid, t.getString("host")); Core.pairing.add(tid)
                                    Core.devices[id] = Core.devices[id]!!.copy(token = null)
                                }
                                dialog = ""
                            } catch (e: Exception) {
                                toast(e.message ?: "Invalid address")
                            }
                        }) { Text("Add") }
                    },
                    dismissButton = { TextButton(onClick = { dialog = "" }) { Text("Cancel") } })
            }
            "history" -> {
                val h = remember { Core.history() }
                AlertDialog(onDismissRequest = { dialog = "" }, title = { Text("History") },
                    text = {
                        Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            if (h.length() == 0) Text("No transfers yet.")
                            for (i in 0 until h.length()) {
                                val x = h.getJSONObject(i)
                                val when_ = java.text.DateFormat.getDateTimeInstance().format(java.util.Date(x.getLong("time") * 1000))
                                if (x.getString("kind") == "text") Text("Text from ${x.getString("peer_name")} · $when_\n${x.getString("text").take(200)}")
                                else Text(
                                    (if (x.getBoolean("outgoing")) "Sent to " else "Received from ") + x.optString("peer_name") + " · $when_\n" +
                                        if (x.getBoolean("ok")) "✓ ${x.getInt("files_ok")} item(s), ${humanBytes(x.getLong("bytes").toULong())}" else "✗ ${x.optString("error", "some files failed")}"
                                )
                            }
                        }
                    },
                    confirmButton = { TextButton(onClick = { dialog = "" }) { Text("Close") } },
                    dismissButton = { TextButton(onClick = { Core.clearHistory(); dialog = "" }) { Text("Clear") } })
            }
            "settings" -> SettingsDialog { dialog = "" }
        }
    }

    @Composable
    fun Banner(msg: String, action: String? = null, onClick: () -> Unit = {}) {
        Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.tertiaryContainer), modifier = Modifier.fillMaxWidth()) {
            Row(Modifier.padding(14.dp), verticalAlignment = Alignment.CenterVertically) {
                Text(msg, Modifier.weight(1f))
                action?.let { TextButton(onClick = onClick) { Text(it) } }
            }
        }
    }

    @Composable
    fun TransferCard(t: Transfer) {
        Card(Modifier.fillMaxWidth()) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                val pairingOnly = t.id in Core.pairing
                Text((if (t.outgoing) "↑ To " else "↓ From ") + t.peer, fontWeight = FontWeight.SemiBold)
                val h = { b: Long -> humanBytes(b.coerceAtLeast(0).toULong()) }
                when (t.status) {
                    "connecting" -> Text("Connecting…")
                    "waiting" -> Text("Waiting for ${t.peer} to accept · code ${t.sas}")
                    "reconnecting" -> Text("Wi-Fi dropped — reconnecting (attempt ${t.attempt})…")
                    "running", "paused" -> {
                        LinearProgressIndicator(progress = { if (t.total > 0) t.done.toFloat() / t.total else 0f }, Modifier.fillMaxWidth())
                        Text(
                            if (t.paused) "Paused · ${h(t.done)} of ${h(t.total)}"
                            else "${h(t.done)} of ${h(t.total)} · ${h(t.speed)}/s · ${t.eta}s left · file ${minOf(t.filesDone + 1, t.filesTotal)}/${t.filesTotal}",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                    "done" -> when {
                        pairingOnly && t.ok -> Text("✓ Paired. You can now send without codes.", color = MaterialTheme.colorScheme.primary)
                        t.ok -> Text("✓ ${t.filesOk} item(s) · ${h(t.bytes)} in ${"%.1f".format(t.secs)}s · avg ${h(t.avg)}/s · verified", color = MaterialTheme.colorScheme.primary)
                        else -> Text("✗ ${t.error ?: "Some files failed"}${t.failed.joinToString("") { "\n• $it" }}", color = MaterialTheme.colorScheme.error)
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val n = Core.node
                    when (t.status) {
                        "running", "paused" -> {
                            OutlinedButton(onClick = { n?.control(t.id, if (t.paused) "resume" else "pause") }) { Text(if (t.paused) "Resume" else "Pause") }
                            OutlinedButton(onClick = { n?.control(t.id, "cancel") }) { Text("Cancel") }
                        }
                        "connecting", "waiting", "reconnecting" -> OutlinedButton(onClick = { n?.control(t.id, "cancel") }) { Text("Cancel") }
                        "done" -> {
                            if (t.outgoing && !t.ok && !pairingOnly) OutlinedButton(onClick = {
                                n?.control(t.id, "retry")?.let { nid -> Core.dismiss(t.id); Core.addOutgoing(nid, t.peer) } ?: toast("Nothing left to retry")
                            }) { Text("Retry") }
                            TextButton(onClick = { Core.dismiss(t.id) }) { Text("Dismiss") }
                        }
                    }
                }
            }
        }
    }

    @Composable
    fun OfferDialog(o: JSONObject) {
        var conflict by remember(o) { mutableStateOf("Rename") }
        val trusted = o.getBoolean("trusted")
        val from = o.getString("from_name")
        val answer = { accept: Boolean, trust: Boolean ->
            Core.node?.respond(o.getString("transfer_id"), accept, trust, conflict)
            Core.offers.remove(o)
        }
        val files = o.getInt("files")
        val what = listOfNotNull(
            if (files > 0) "$files file(s) · ${humanBytes(o.getLong("bytes").toULong())}" else null,
            if (o.getInt("texts") > 0) "${o.getInt("texts")} text" else null,
        ).joinToString(" + ")
        AlertDialog(onDismissRequest = {}, title = { Text("$from wants to send $what") },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (!trusted) {
                        Text("Unknown device. Check that $from shows the same code:")
                        Text(o.getString("sas"), fontFamily = FontFamily.Monospace, fontSize = 30.sp, fontWeight = FontWeight.Bold, letterSpacing = 4.sp)
                    }
                    val p = o.getJSONArray("preview")
                    for (i in 0 until p.length()) Text("• ${p.getString(i)}", maxLines = 2, overflow = TextOverflow.Ellipsis)
                    if (files > 0) {
                        Text("If a file already exists:", style = MaterialTheme.typography.labelMedium)
                        listOf("Rename" to "Keep both", "Overwrite" to "Replace", "Skip" to "Skip").forEach { (k, v) ->
                            Row(Modifier.selectable(conflict == k, role = Role.RadioButton) { conflict = k }, verticalAlignment = Alignment.CenterVertically) {
                                RadioButton(conflict == k, null); Text(v)
                            }
                        }
                    }
                }
            },
            confirmButton = {
                Row {
                    if (!trusted) TextButton(onClick = { answer(true, true) }) { Text("Accept & trust") }
                    TextButton(onClick = { answer(true, false) }) { Text("Accept") }
                }
            },
            dismissButton = { TextButton(onClick = { answer(false, false) }) { Text("Decline") } })
    }

    @Composable
    fun SettingsDialog(close: () -> Unit) {
        var auto by remember { mutableStateOf(Core.prefs.getBoolean("auto", false)) }
        var devs by remember { mutableStateOf(JSONArray(Core.node?.devicesJson() ?: "[]")) }
        AlertDialog(onDismissRequest = close, title = { Text("Settings") },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("Accept automatically from trusted devices", Modifier.weight(1f))
                        Switch(auto, { auto = it; Core.prefs.edit().putBoolean("auto", it).apply(); Core.node?.setAutoAccept(it) })
                    }
                    Text("Received files are saved to: ${Core.prefs.getString("dir", Core.defaultDir)}", style = MaterialTheme.typography.bodySmall)
                    Text("Trusted devices", style = MaterialTheme.typography.titleSmall)
                    if (devs.length() == 0) Text("None yet. Pair with Scan QR or choose “Accept & trust”.", style = MaterialTheme.typography.bodySmall)
                    for (i in 0 until devs.length()) {
                        val d = devs.getJSONObject(i)
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(d.getString("name") + if (d.getBoolean("blocked")) " (blocked)" else "", Modifier.weight(1f))
                            TextButton(onClick = { Core.node?.device(d.getString("id"), d.getString("name"), "forget"); devs = JSONArray(Core.node?.devicesJson()) }) { Text("Forget") }
                            if (!d.getBoolean("blocked")) TextButton(onClick = { Core.node?.device(d.getString("id"), d.getString("name"), "block"); devs = JSONArray(Core.node?.devicesJson()) }) { Text("Block") }
                        }
                    }
                    TextButton(onClick = { startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) }) { Text("Battery optimisation settings") }
                    Text("Device ID ${Core.node?.fingerprint()?.take(16)}…", style = MaterialTheme.typography.bodySmall)
                }
            },
            confirmButton = { TextButton(onClick = close) { Text("Done") } })
    }
}
