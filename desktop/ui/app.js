// DropBridge desktop UI. No framework; everything from the network is escaped via esc().
const T = window.__TAURI__;
const invoke = T.core.invoke;
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

let me = null;
const devices = new Map(); // id -> {id,name,kind,addrs,port,trusted,manual?,token?}
const transfers = new Map(); // id -> state
let staged = []; // absolute paths
const offers = []; // queued incoming offers
let dialogBusy = false;

function human(b) {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = b, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return i ? `${v.toFixed(1)} ${u[i]}` : `${b} B`;
}
function dur(s) {
  if (!isFinite(s) || s <= 0) return "";
  if (s < 60) return `${Math.ceil(s)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${Math.round((s % 3600) / 60)}m`;
}
const base = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p;
const kindIcon = (k) => ({ android: "📱", mac: "💻", windows: "🖥️", linux: "🐧", cli: "⌨️" })[k] || "📟";

function toast(msg) {
  const t = $("#toast");
  t.textContent = msg;
  t.classList.add("show");
  clearTimeout(toast.h);
  toast.h = setTimeout(() => t.classList.remove("show"), 3200);
}

// ───────── devices ─────────
function renderDevices() {
  const el = $("#devices");
  el.innerHTML = "";
  for (const d of devices.values()) {
    const b = document.createElement("button");
    b.className = "device";
    b.dataset.id = d.id;
    b.setAttribute("role", "listitem");
    const status = d.manual ? "Manual connection" : d.trusted ? "Trusted" : "Not paired yet";
    b.setAttribute("aria-label", `Send to ${d.name}, ${status}`);
    b.innerHTML = `<span class="av" aria-hidden="true">${kindIcon(d.kind)}</span><span class="nm">${esc(d.name)}</span><span class="st ${d.trusted ? "trusted" : ""}">${esc(status)}</span>`;
    b.onclick = () => sendTo(d);
    el.appendChild(b);
  }
  $("#empty").hidden = devices.size > 0;
}

function targetOf(d) {
  return { host: d.addrs.join(","), port: d.port, fingerprint: d.fingerprint ?? (d.manual ? null : d.id), token: d.token ?? null };
}

async function sendTo(d, paths) {
  let items = paths ?? staged;
  if (!items.length) {
    const picked = await T.dialog.open({ multiple: true, title: `Send to ${d.name}` });
    if (!picked) return;
    items = Array.isArray(picked) ? picked : [picked];
  }
  try {
    const id = await invoke("send", { target: targetOf(d), paths: items, text: null });
    transfers.set(id, { id, outgoing: true, peer: d.name, status: "connecting", files: items.map(base) });
    if (d.token) d.token = null; // one-time
    if (!paths) setStaged([]);
    renderTransfers();
  } catch (e) {
    toast(String(e));
  }
}

// ───────── staging & drag-drop ─────────
function setStaged(list) {
  staged = [...new Set(list)];
  const ul = $("#staged");
  ul.innerHTML = "";
  staged.forEach((p, i) => {
    const li = document.createElement("li");
    li.innerHTML = `<span title="${esc(p)}">${esc(base(p))}</span><button aria-label="Remove ${esc(base(p))}">✕</button>`;
    li.querySelector("button").onclick = () => setStaged(staged.filter((_, j) => j !== i));
    ul.appendChild(li);
  });
  $("#staged-hint").hidden = staged.length === 0;
}

function deviceAt(pos) {
  const r = window.devicePixelRatio || 1;
  const el = document.elementFromPoint(pos.x / r, pos.y / r);
  return el?.closest(".device");
}

T.event.listen("tauri://drag-over", (e) => {
  document.querySelectorAll(".device.over").forEach((x) => x.classList.remove("over"));
  const dev = deviceAt(e.payload.position);
  dev?.classList.add("over");
  $("#drop").classList.toggle("over", !dev);
});
T.event.listen("tauri://drag-leave", () => {
  document.querySelectorAll(".over").forEach((x) => x.classList.remove("over"));
});
T.event.listen("tauri://drag-drop", (e) => {
  document.querySelectorAll(".over").forEach((x) => x.classList.remove("over"));
  const dev = deviceAt(e.payload.position);
  if (dev) sendTo(devices.get(dev.dataset.id), e.payload.paths);
  else setStaged([...staged, ...e.payload.paths]);
});
T.event.listen("stage", (e) => setStaged([...staged, ...e.payload]));

$("#btn-files").onclick = async () => {
  const p = await T.dialog.open({ multiple: true, title: "Choose files to send" });
  if (p) setStaged([...staged, ...(Array.isArray(p) ? p : [p])]);
};
$("#btn-folder").onclick = async () => {
  const p = await T.dialog.open({ directory: true, multiple: true, title: "Choose a folder to send" });
  if (p) setStaged([...staged, ...(Array.isArray(p) ? p : [p])]);
};

// ───────── transfers ─────────
function renderTransfers() {
  const el = $("#transfers");
  el.innerHTML = "";
  const list = [...transfers.values()].reverse();
  $("#no-transfers").hidden = list.length > 0;
  for (const t of list) {
    const card = document.createElement("div");
    card.className = "xfer";
    const dir = t.outgoing ? `To ${esc(t.peer)}` : `From ${esc(t.peer)}`;
    let meta = "", body = "", btns = "";
    if (t.status === "connecting") meta = "Connecting…";
    if (t.status === "waiting") meta = `Waiting for ${esc(t.peer)} to accept · verification code <b>${esc(t.sas)}</b>`;
    if (t.status === "reconnecting") meta = `Connection dropped — reconnecting (attempt ${t.attempt})…`;
    if (t.status === "running" || t.status === "paused" || t.status === "reconnecting") {
      const pct = t.total ? (t.done / t.total) * 100 : 0;
      if (t.status !== "reconnecting")
        meta = t.paused
          ? `Paused · ${human(t.done)} of ${human(t.total)}`
          : `${human(t.done)} of ${human(t.total)} · ${human(t.speed)}/s (avg ${human(t.avg)}/s) · ${dur(t.eta)} left · file ${Math.min(t.filesDone + 1, t.filesTotal)}/${t.filesTotal}`;
      body = `<progress max="100" value="${pct.toFixed(1)}" aria-label="Transfer progress ${pct.toFixed(0)} percent"></progress>`;
      btns = `<button data-a="${t.paused ? "resume" : "pause"}">${t.paused ? "Resume" : "Pause"}</button><button class="danger" data-a="cancel">Cancel</button>`;
    }
    if (t.status === "done") {
      meta = `${t.filesOk} item(s) · ${human(t.bytes)} in ${dur(t.secs) || "<1s"} · avg ${human(t.avgFinal)}/s · peak ${human(t.peak)}/s`;
      body = t.ok ? `<div class="okmsg">✓ Complete — every file verified</div>` : `<div class="err">${esc(t.error || "")}${t.failed?.length ? "\n" + t.failed.map(esc).join("\n") : ""}</div>`;
      btns = (!t.outgoing && t.lastPath ? `<button data-a="reveal">Show in folder</button>` : "") + (t.outgoing && !t.ok ? `<button data-a="retry">Retry</button>` : "") + `<button class="ghost" data-a="dismiss">Dismiss</button>`;
    }
    if (t.status === "connecting" || t.status === "waiting") btns = `<button class="danger" data-a="cancel">Cancel</button>`;
    card.innerHTML = `<div class="top"><span class="title">${t.outgoing ? "↑" : "↓"} ${dir}</span><span class="meta">${t.current ? esc(base(t.current)) : ""}</span></div><div class="meta">${meta}</div>${body}<div class="btns">${btns}</div>`;
    card.querySelectorAll("button[data-a]").forEach((b) => (b.onclick = () => act(t, b.dataset.a)));
    el.appendChild(card);
  }
}

async function act(t, a) {
  if (a === "dismiss") { transfers.delete(t.id); return renderTransfers(); }
  if (a === "reveal") return T.opener.revealItemInDir(t.lastPath);
  const nid = await invoke("control", { id: t.id, action: a });
  if (a === "retry") {
    if (!nid) return toast("Nothing left to retry.");
    transfers.delete(t.id);
    transfers.set(nid, { id: nid, outgoing: true, peer: t.peer, status: "connecting" });
    renderTransfers();
  }
}

function upd(id, patch) {
  const t = transfers.get(id) || { id, status: "running" };
  transfers.set(id, Object.assign(t, patch));
  renderTransfers();
}

// ───────── dialogs ─────────
function openDialog(html, onSubmit, onClose) {
  const dlg = $("#dlg");
  const form = $("#dlg-body");
  form.innerHTML = html;
  dialogBusy = true;
  form.onsubmit = (e) => {
    const v = e.submitter?.value;
    if (onSubmit && onSubmit(v, form) === false) e.preventDefault();
  };
  dlg.onclose = () => { dialogBusy = false; onClose && onClose(); nextOffer(); };
  dlg.showModal();
  (form.querySelector("[autofocus]") || form.querySelector("button.primary") || form.querySelector("button"))?.focus();
}

function nextOffer() {
  if (dialogBusy || !offers.length) return;
  const o = offers.shift();
  const what = [o.files ? `${o.files} file(s) · ${human(o.bytes)}` : "", o.texts ? `${o.texts} text message(s)` : ""].filter(Boolean).join(" + ");
  const warn = o.trusted ? "" : `<div class="warnbox">Unknown device. Make sure <b>${esc(o.from_name)}</b> shows the same code before accepting:<div class="code">${esc(o.sas)}</div></div>`;
  let answered = false;
  openDialog(
    `<h3>${esc(o.from_name)} wants to send you ${esc(what)}</h3>
     ${warn}
     <div class="list">${o.preview.map((p) => `<div class="it">${esc(p)}</div>`).join("")}${o.files > o.preview.length ? `<div class="hint">…and more</div>` : ""}</div>
     <label>If a file already exists
       <select name="conflict"><option value="Rename">Keep both (rename the new one)</option><option value="Overwrite">Replace it</option><option value="Skip">Skip it</option></select>
     </label>
     <div class="btns">
       <button value="no" class="danger">Decline</button>
       ${o.trusted ? "" : `<button value="trust">Accept &amp; trust</button>`}
       <button value="yes" class="primary" autofocus>Accept</button>
     </div>`,
    (v, form) => {
      answered = true;
      invoke("respond", { id: o.transfer_id, accept: v !== "no", trust: v === "trust", conflict: form.conflict.value });
      if (v !== "no") transfers.set(o.transfer_id, { id: o.transfer_id, outgoing: false, peer: o.from_name, status: "running", done: 0, total: o.bytes });
      if (v === "trust") refreshMe();
      renderTransfers();
    },
    () => { if (!answered) invoke("respond", { id: o.transfer_id, accept: false, trust: false, conflict: "Skip" }); }
  );
}

function showText(from, text) {
  openDialog(
    `<h3>Text from ${esc(from)}</h3><div class="textbox">${esc(text)}</div>
     <div class="btns"><button value="close">Close</button><button value="copy" class="primary" autofocus>Copy</button></div>`,
    (v) => { if (v === "copy") T.clipboardManager.writeText(text).then(() => toast("Copied")); }
  );
}

$("#btn-text").onclick = () => {
  if (!devices.size) return toast("No device to send to yet. Pair a device first.");
  const opts = [...devices.values()].map((d) => `<option value="${esc(d.id)}">${esc(d.name)}</option>`).join("");
  openDialog(
    `<h3>Send text</h3>
     <label>Message, link or note<textarea name="text" autofocus placeholder="Paste or type anything…"></textarea></label>
     <label>To<select name="to">${opts}</select></label>
     <div class="btns"><button value="cancel">Cancel</button><button value="send" class="primary">Send</button></div>`,
    (v, form) => {
      if (v !== "send") return;
      const d = devices.get(form.to.value);
      const text = form.text.value;
      if (!text.trim()) { toast("Type something to send."); return false; }
      invoke("send", { target: targetOf(d), paths: [], text })
        .then((id) => { transfers.set(id, { id, outgoing: true, peer: d.name, status: "connecting" }); renderTransfers(); })
        .catch((e) => toast(String(e)));
    }
  );
};

$("#btn-pair").onclick = async () => {
  try {
    const p = await invoke("pairing");
    openDialog(
      `<h3>Pair a device</h3>
       <p>On your phone, open DropBridge → <b>Scan QR</b>. Or type this code and address on the other device:</p>
       <div class="qr" aria-label="Pairing QR code">${p.svg}</div>
       <div class="code" aria-label="Pairing code">${esc(p.code)}</div>
       <p class="hint">Address: ${p.ips.map((ip) => esc(`${ip}:${p.port}`)).join(" or ") || "no network connection"} · code works once, for 5 minutes.</p>
       <div class="btns"><button value="close" class="primary">Done</button></div>`
    );
  } catch (e) { toast(String(e)); }
};

$("#btn-manual").onclick = () => {
  openDialog(
    `<h3>Connect manually</h3>
     <p class="hint">Use this when the device doesn't appear automatically (guest Wi-Fi, VPN, or a network that blocks discovery). On the other device, open <b>Pair a device</b> to see its address and code.</p>
     <label>Address or pairing link<input type="text" name="addr" autofocus placeholder="192.168.1.20:52345  or  dropbridge://pair?..." /></label>
     <label>Pairing code (optional, pairs the devices)<input type="text" name="code" inputmode="numeric" maxlength="6" placeholder="123456" /></label>
     <div class="btns"><button value="cancel">Cancel</button><button value="add" class="primary">Add device</button></div>`,
    (v, form) => {
      if (v !== "add") return;
      invoke("parse_target", { input: form.addr.value })
        .then((t) => {
          const id = `manual:${t.host}:${t.port}`;
          devices.set(id, { id, name: t.host, kind: "", addrs: [t.host], port: t.port, manual: true, fingerprint: t.fingerprint, token: form.code.value.trim() || t.token });
          renderDevices();
          toast("Added. Click it to send.");
        })
        .catch((e) => toast(String(e)));
    }
  );
};

$("#btn-history").onclick = async () => {
  const h = await invoke("history");
  const rows = h.map((x) => {
    const when = new Date(x.time * 1000).toLocaleString();
    if (x.kind === "text") return `<div class="it"><b>Text from ${esc(x.peer_name)}</b> · ${esc(when)}<div class="textbox">${esc(x.text)}</div></div>`;
    const dir = x.outgoing ? `Sent to ${esc(x.peer_name)}` : `Received from ${esc(x.peer_name)}`;
    const res = x.ok ? `✓ ${x.files_ok} item(s), ${human(x.bytes)} · avg ${human(x.avg_bps)}/s` : `✗ ${esc(x.error || `${x.failed.length} failed`)}`;
    return `<div class="it"><b>${dir}</b> · ${esc(when)}<br>${res}</div>`;
  });
  openDialog(
    `<h3>History</h3><div class="list">${rows.join("") || `<p class="hint">No transfers yet.</p>`}</div>
     <div class="btns"><button value="clear" class="danger">Clear history</button><button value="close" class="primary">Close</button></div>`,
    (v) => { if (v === "clear") invoke("clear_history"); }
  );
};

$("#btn-settings").onclick = async () => {
  await refreshMe();
  const s = me.settings;
  const devs = me.devices.map((d) => `<div class="it"><b>${esc(d.name)}</b> ${d.blocked ? "(blocked)" : "(trusted)"}<br><span class="hint">${esc(d.id.slice(0, 16))}…</span>
      <div class="btns"><button value="forget:${esc(d.id)}">Forget</button>${d.blocked ? "" : `<button value="block:${esc(d.id)}" class="danger">Block</button>`}</div></div>`).join("");
  openDialog(
    `<h3>Settings</h3>
     <label>Save received files to<input type="text" name="dir" value="${esc(s.download_dir)}" /></label>
     <button value="browse" type="submit">Choose folder…</button>
     <label class="check"><input type="checkbox" name="auto" ${s.auto_accept ? "checked" : ""}/> Accept files from trusted devices automatically</label>
     <h3>Trusted devices</h3><div class="list">${devs || `<p class="hint">None yet. Devices become trusted when you pair or choose “Accept &amp; trust”.</p>`}</div>
     <p class="hint">This device: ${esc(me.name)} · ID ${esc(me.fingerprint.slice(0, 16))}… · v${esc(me.version)}</p>
     <div class="btns"><button value="cancel">Cancel</button><button value="save" class="primary">Save</button></div>`,
    (v, form) => {
      if (v === "browse") {
        T.dialog.open({ directory: true, title: "Save received files to" }).then((p) => { if (p) form.dir.value = p; });
        return false;
      }
      if (v?.includes(":")) {
        const [action, id] = v.split(/:(.*)/s);
        const d = me.devices.find((x) => x.id === id);
        invoke("device", { id, name: d?.name || "", action }).then(refreshMe);
        return;
      }
      if (v === "save")
        invoke("set_settings", { settings: { name: s.name, download_dir: form.dir.value, auto_accept: form.auto.checked } })
          .then(() => toast("Saved"))
          .catch((e) => toast(String(e)));
    }
  );
};

// ───────── core events ─────────
T.event.listen("db", ({ payload: e }) => {
  switch (e.type) {
    case "PeerFound":
      devices.set(e.id, { id: e.id, name: e.name, kind: e.kind, addrs: e.addrs, port: e.port, trusted: e.trusted || me?.devices.some((d) => d.id === e.id && !d.blocked) });
      return renderDevices();
    case "PeerLost":
      devices.delete(e.id);
      return renderDevices();
    case "IncomingOffer":
      offers.push(e);
      return nextOffer();
    case "AwaitingApproval":
      return upd(e.transfer_id, { status: "waiting", peer: e.to_name, sas: e.sas, outgoing: true });
    case "Reconnecting":
      return upd(e.transfer_id, { status: "reconnecting", attempt: e.attempt });
    case "Progress":
      return upd(e.transfer_id, {
        status: e.paused ? "paused" : "running", paused: e.paused, outgoing: e.outgoing, peer: e.peer_name, done: e.bytes_done, total: e.bytes_total,
        speed: e.speed_bps, avg: e.avg_bps, eta: e.eta_secs, filesDone: e.files_done, filesTotal: e.files_total, current: e.current,
      });
    case "ItemDone":
      if (e.ok) upd(e.transfer_id, { lastPath: e.path });
      return;
    case "TextReceived":
      return showText(e.from_name, e.text);
    case "Finished":
      if (e.error === "Declined" && !e.outgoing) { transfers.delete(e.transfer_id); return renderTransfers(); }
      if (e.ok && e.files_ok === 0 && e.bytes === 0 && !e.failed.length) {
        // pairing handshake (QR / code) with nothing to send
        transfers.delete(e.transfer_id);
        toast(`Paired with ${e.peer_name}`);
        refreshMe();
        return renderTransfers();
      }
      return upd(e.transfer_id, {
        status: "done", ok: e.ok, error: e.error, failed: e.failed, filesOk: e.files_ok, bytes: e.bytes, secs: e.secs,
        avgFinal: e.avg_bps, peak: e.peak_bps, outgoing: e.outgoing, peer: e.peer_name || transfers.get(e.transfer_id)?.peer || "device",
      });
  }
});

async function refreshMe() {
  me = await invoke("info");
  $("#me").textContent = `Visible as “${me.name}” · ${me.ips[0] ? `${me.ips[0]}:${me.port}` : "no network"}`;
  for (const d of devices.values()) d.trusted = me.devices.some((x) => x.id === d.id && !x.blocked);
  renderDevices();
}

document.addEventListener("keydown", (e) => {
  if (dialogBusy || e.target.matches("input,textarea")) return;
  if (e.key === "p") $("#btn-pair").click();
  if (e.key === "h") $("#btn-history").click();
  if (e.key === "o" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); $("#btn-files").click(); }
});
document.addEventListener("dragover", (e) => e.preventDefault());

refreshMe();
renderTransfers();
