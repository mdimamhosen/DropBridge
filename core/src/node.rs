//! A DropBridge node: listens for offers, sends files/text, emits events to the app.
use crate::identity::Identity;
use crate::trust::{Device, TrustStore};
use crate::wire::*;
use crate::{discovery, paths, tls};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::timeout;
use tokio_rustls::TlsStream;

const CHUNK: usize = 1 << 20;
const PIPE_DEPTH: usize = 8; // chunks in flight per file: bounds memory at ~8 MiB per transfer
const MAX_ITEMS: usize = 200_000;
const MAX_TEXT: usize = 8 << 20;
const RETRY_WINDOW: Duration = Duration::from_secs(120);
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);

pub struct NodeConfig {
    pub name: String,
    /// "mac", "android", "windows", "linux", "cli"
    pub kind: String,
    /// `Identity::to_secret()` from secure storage; empty = generate a new one.
    pub identity: String,
    pub data_dir: String,
    pub download_dir: String,
    /// 0 = any free port.
    pub port: u16,
    pub discovery: bool,
    pub auto_accept_trusted: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum Event {
    PeerFound {
        id: String,
        name: String,
        kind: String,
        addrs: Vec<String>,
        port: u16,
        trusted: bool,
    },
    PeerLost {
        id: String,
    },
    /// Receiver: call `Node::respond` with a decision.
    IncomingOffer {
        transfer_id: String,
        from_id: String,
        from_name: String,
        from_kind: String,
        trusted: bool,
        sas: String,
        files: u32,
        bytes: u64,
        texts: u32,
        preview: Vec<String>,
    },
    /// Sender: waiting for the other side; show `sas` so users can compare codes.
    AwaitingApproval {
        transfer_id: String,
        to_name: String,
        sas: String,
    },
    Progress {
        transfer_id: String,
        outgoing: bool,
        peer_name: String,
        bytes_done: u64,
        bytes_total: u64,
        files_done: u32,
        files_total: u32,
        speed_bps: u64,
        avg_bps: u64,
        eta_secs: u64,
        current: String,
        paused: bool,
    },
    Reconnecting {
        transfer_id: String,
        attempt: u32,
        error: String,
    },
    ItemDone {
        transfer_id: String,
        path: String,
        ok: bool,
        error: Option<String>,
    },
    TextReceived {
        transfer_id: String,
        from_name: String,
        text: String,
    },
    Finished {
        transfer_id: String,
        outgoing: bool,
        peer_id: String,
        peer_name: String,
        ok: bool,
        error: Option<String>,
        files_ok: u32,
        failed: Vec<String>,
        bytes: u64,
        secs: f64,
        avg_bps: u64,
        peak_bps: u64,
    },
}

pub trait Delegate: Send + Sync {
    fn on_event(&self, e: Event);
    /// Android: open a content:// URI and return a detached fd (or -1).
    fn open_fd(&self, _uri: String) -> i32 {
        -1
    }
}

#[derive(Clone, Debug)]
pub enum SendItem {
    Path {
        path: String,
    },
    Text {
        text: String,
    },
    /// A file the platform opens for us (Android SAF). `rel_path` keeps folder structure.
    Uri {
        uri: String,
        rel_path: String,
        size: u64,
        mtime: Option<i64>,
    },
    /// An (empty) folder from a platform picker.
    Dir {
        rel_path: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Conflict {
    Rename,
    Overwrite,
    Skip,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decision {
    pub accept: bool,
    pub trust: bool,
    pub conflict: Conflict,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    /// One or more comma-separated addresses; tried in order.
    pub host: String,
    pub port: u16,
    /// Expected fingerprint (trusted device or QR); connection aborts on mismatch.
    pub fingerprint: Option<String>,
    /// One-time pairing token/code from the receiver's screen.
    pub token: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Run,
    Pause,
    Cancel,
}

/// Errors that must not trigger an automatic reconnect.
#[derive(Debug)]
struct Fatal(String);
impl std::fmt::Display for Fatal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Fatal {}
fn fatal(s: impl Into<String>) -> anyhow::Error {
    Fatal(s.into()).into()
}

struct Accepted {
    decision: Decision,
    done: HashSet<u32>,
}

struct Inner {
    name: String,
    kind: String,
    fp: String,
    server: Arc<rustls::ServerConfig>,
    client: Arc<rustls::ClientConfig>,
    delegate: Arc<dyn Delegate>,
    trust: Mutex<TrustStore>,
    download_dir: Mutex<PathBuf>,
    auto_accept: AtomicBool,
    pending: Mutex<HashMap<String, (String, oneshot::Sender<Decision>)>>,
    ctl: Mutex<HashMap<String, Arc<watch::Sender<Ctl>>>>,
    tokens: Mutex<HashMap<String, Instant>>,
    bad_tokens: AtomicU32,
    accepted: Mutex<HashMap<String, Accepted>>,
}

pub struct Node {
    inner: Arc<Inner>,
    rt: tokio::runtime::Runtime,
    port: u16,
    secret: String,
    _mdns: Option<mdns_sd::ServiceDaemon>,
}

impl Node {
    pub fn new(cfg: NodeConfig, delegate: Arc<dyn Delegate>) -> Result<Self> {
        std::fs::create_dir_all(&cfg.data_dir).context("Can't create the app data folder")?;
        std::fs::create_dir_all(&cfg.download_dir).context("Can't create the download folder")?;
        let id = if cfg.identity.is_empty() {
            Identity::generate()?
        } else {
            Identity::from_secret(&cfg.identity)?
        };
        let inner = Arc::new(Inner {
            name: cfg.name.clone(),
            kind: cfg.kind.clone(),
            fp: id.fingerprint(),
            server: tls::server_config(&id)?,
            client: tls::client_config(&id)?,
            delegate,
            trust: Mutex::new(TrustStore::load(
                Path::new(&cfg.data_dir).join("trusted.json"),
            )),
            download_dir: Mutex::new(cfg.download_dir.into()),
            auto_accept: AtomicBool::new(cfg.auto_accept_trusted),
            pending: Default::default(),
            ctl: Default::default(),
            tokens: Default::default(),
            bad_tokens: AtomicU32::new(0),
            accepted: Default::default(),
        });
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let listener = rt
            .block_on(TcpListener::bind(("0.0.0.0", cfg.port)))
            .with_context(|| {
                format!(
                    "Can't listen on port {} — is another copy running?",
                    cfg.port
                )
            })?;
        let port = listener.local_addr()?.port();
        rt.spawn(accept_loop(inner.clone(), listener));
        let mdns = if cfg.discovery {
            let i = inner.clone();
            Some(discovery::start(
                &inner.fp,
                &cfg.name,
                &cfg.kind,
                port,
                move |f| {
                    let e = match f {
                        discovery::Found::Up {
                            id,
                            name,
                            kind,
                            addrs,
                            port,
                        } => {
                            let trusted = i.trust.lock().unwrap().is_trusted(&id);
                            Event::PeerFound {
                                id,
                                name,
                                kind,
                                addrs,
                                port,
                                trusted,
                            }
                        }
                        discovery::Found::Down { id } => Event::PeerLost { id },
                    };
                    i.delegate.on_event(e);
                },
            )?)
        } else {
            None
        };
        Ok(Self {
            inner,
            rt,
            port,
            secret: id.to_secret(),
            _mdns: mdns,
        })
    }

    /// Persist this in OS secure storage and pass it back as `NodeConfig::identity`.
    pub fn identity_secret(&self) -> String {
        self.secret.clone()
    }
    pub fn fingerprint(&self) -> String {
        self.inner.fp.clone()
    }
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Start sending; progress and the result arrive as events. Returns the transfer id.
    pub fn send(&self, target: Target, items: Vec<SendItem>) -> String {
        let tid = hex::encode(rand::random::<[u8; 8]>());
        let ctl = self.inner.new_ctl(&tid);
        self.rt.spawn(run_send(
            self.inner.clone(),
            tid.clone(),
            target,
            items,
            ctl,
        ));
        tid
    }

    pub fn respond(&self, transfer_id: &str, d: Decision) {
        if let Some((_, tx)) = self.inner.pending.lock().unwrap().remove(transfer_id) {
            let _ = tx.send(d);
        }
    }

    pub fn pause(&self, transfer_id: &str) {
        self.inner.set_ctl(transfer_id, Ctl::Pause)
    }
    pub fn resume(&self, transfer_id: &str) {
        self.inner.set_ctl(transfer_id, Ctl::Run)
    }
    pub fn cancel(&self, transfer_id: &str) {
        self.inner.set_ctl(transfer_id, Ctl::Cancel);
        self.respond(
            transfer_id,
            Decision {
                accept: false,
                trust: false,
                conflict: Conflict::Skip,
            },
        );
    }

    pub fn trust(&self, fp: &str, name: &str) -> Result<()> {
        self.inner.trust.lock().unwrap().set(fp, name, false)
    }
    pub fn block(&self, fp: &str, name: &str) -> Result<()> {
        self.inner.trust.lock().unwrap().set(fp, name, true)
    }
    pub fn forget(&self, fp: &str) -> Result<()> {
        self.inner.trust.lock().unwrap().forget(fp)
    }
    pub fn devices(&self) -> Vec<(String, Device)> {
        self.inner
            .trust
            .lock()
            .unwrap()
            .devices
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn set_download_dir(&self, dir: &str) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        *self.inner.download_dir.lock().unwrap() = dir.into();
        Ok(())
    }
    pub fn set_auto_accept(&self, on: bool) {
        self.inner.auto_accept.store(on, Ordering::Relaxed)
    }

    /// QR payload: `dropbridge://pair?h=ip1,ip2&p=port&fp=<fingerprint>&t=<one-time token>` (valid 5 min).
    pub fn pairing_uri(&self) -> String {
        let t = self
            .inner
            .new_token(hex::encode(rand::random::<[u8; 16]>()));
        format!(
            "dropbridge://pair?h={}&p={}&fp={}&t={}",
            local_ips().join(","),
            self.port,
            self.inner.fp,
            t
        )
    }

    /// Short code for typing by hand (valid 5 min, 5 wrong guesses invalidate all codes).
    pub fn pairing_code(&self) -> String {
        self.inner
            .new_token(format!("{:06}", rand::random::<u32>() % 1_000_000))
    }
}

pub fn parse_pairing_uri(s: &str) -> Result<Target> {
    let q = s
        .strip_prefix("dropbridge://pair?")
        .context("Not a DropBridge pairing code")?;
    let kv: HashMap<&str, &str> = q.split('&').filter_map(|p| p.split_once('=')).collect();
    let fp = kv
        .get("fp")
        .filter(|f| f.len() == 64 && f.chars().all(|c| c.is_ascii_hexdigit()));
    Ok(Target {
        host: kv
            .get("h")
            .context("pairing code has no address")?
            .to_string(),
        port: kv
            .get("p")
            .and_then(|p| p.parse().ok())
            .context("pairing code has no port")?,
        fingerprint: Some(
            fp.context("pairing code has no valid fingerprint")?
                .to_string(),
        ),
        token: kv.get("t").map(|t| t.to_string()),
    })
}

pub fn local_ips() -> Vec<String> {
    let mut v: Vec<_> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && i.ip().is_ipv4())
        .map(|i| i.ip().to_string())
        .collect();
    v.dedup();
    v
}

impl Inner {
    fn emit(&self, e: Event) {
        self.delegate.on_event(e)
    }
    fn new_ctl(&self, tid: &str) -> Arc<watch::Sender<Ctl>> {
        let c = Arc::new(watch::channel(Ctl::Run).0);
        self.ctl.lock().unwrap().insert(tid.to_string(), c.clone());
        c
    }
    fn set_ctl(&self, tid: &str, v: Ctl) {
        if let Some(c) = self.ctl.lock().unwrap().get(tid) {
            c.send_replace(v);
        }
    }
    fn new_token(&self, t: String) -> String {
        let mut m = self.tokens.lock().unwrap();
        m.retain(|_, exp| *exp > Instant::now());
        m.insert(t.clone(), Instant::now() + Duration::from_secs(300));
        self.bad_tokens.store(0, Ordering::Relaxed);
        t
    }
    /// One-time use. Too many wrong guesses burn every outstanding code.
    fn take_token(&self, t: &str) -> bool {
        let mut m = self.tokens.lock().unwrap();
        match m.remove(t) {
            Some(exp) if exp > Instant::now() => true,
            _ => {
                if self.bad_tokens.fetch_add(1, Ordering::Relaxed) + 1 >= 5 {
                    m.clear();
                }
                false
            }
        }
    }
}

fn tune(s: &TcpStream) {
    let _ = s.set_nodelay(true);
    let sock = socket2::SockRef::from(s);
    let _ = sock.set_send_buffer_size(4 << 20);
    let _ = sock.set_recv_buffer_size(4 << 20);
    // Detect silently dropped Wi-Fi within ~30 s instead of hanging forever.
    let _ = sock.set_tcp_keepalive(
        &socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(10))
            .with_interval(Duration::from_secs(5)),
    );
}

fn now_mtime(m: &std::fs::Metadata) -> Option<i64> {
    m.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

fn ago(t: Instant) -> f64 {
    t.elapsed().as_secs_f64().max(1e-6)
}

struct Meter {
    tid: String,
    outgoing: bool,
    peer_name: String,
    total: u64,
    done: u64,
    files_total: u32,
    files_done: u32,
    start: Instant,
    base: u64,
    last: Instant,
    last_done: u64,
    speed: u64,
    peak: u64,
    current: String,
    paused: bool,
}

impl Meter {
    fn new(tid: &str, outgoing: bool) -> Self {
        let n = Instant::now();
        Self {
            tid: tid.into(),
            outgoing,
            peer_name: String::new(),
            total: 0,
            done: 0,
            files_total: 0,
            files_done: 0,
            start: n,
            base: 0,
            last: n,
            last_done: 0,
            speed: 0,
            peak: 0,
            current: String::new(),
            paused: false,
        }
    }
    /// Bytes already present (resume) count as done but not toward speed.
    fn reset(&mut self, total: u64, done: u64, files: u32) {
        (
            self.total,
            self.done,
            self.base,
            self.last_done,
            self.files_total,
        ) = (total, done, done, done, files);
        (self.start, self.last) = (Instant::now(), Instant::now());
    }
    fn avg(&self) -> u64 {
        ((self.done - self.base.min(self.done)) as f64 / ago(self.start)) as u64
    }
    fn tick(&mut self, d: &Inner, force: bool) {
        let dt = ago(self.last);
        if !force && dt < 0.25 {
            return;
        }
        if dt >= 0.25 {
            self.speed = ((self.done.saturating_sub(self.last_done)) as f64 / dt) as u64;
            self.peak = self.peak.max(self.speed);
            (self.last, self.last_done) = (Instant::now(), self.done);
        }
        let avg = self.avg().max(1);
        d.emit(Event::Progress {
            transfer_id: self.tid.clone(),
            outgoing: self.outgoing,
            peer_name: self.peer_name.clone(),
            bytes_done: self.done,
            bytes_total: self.total,
            files_done: self.files_done,
            files_total: self.files_total,
            speed_bps: self.speed,
            avg_bps: self.avg(),
            eta_secs: self.total.saturating_sub(self.done) / avg,
            current: self.current.clone(),
            paused: self.paused,
        });
    }
    fn finish(
        &self,
        d: &Inner,
        peer_id: &str,
        files_ok: u32,
        failed: Vec<String>,
        err: Option<String>,
    ) {
        d.emit(Event::Finished {
            transfer_id: self.tid.clone(),
            outgoing: self.outgoing,
            peer_id: peer_id.into(),
            peer_name: self.peer_name.clone(),
            ok: err.is_none() && failed.is_empty(),
            error: err,
            files_ok,
            failed,
            bytes: self.done - self.base.min(self.done),
            secs: ago(self.start),
            avg_bps: self.avg(),
            peak_bps: self.peak.max(self.avg()),
        });
    }
}

// ───────────────────────────── sending ─────────────────────────────

#[derive(Clone)]
enum Src {
    None,
    Path(PathBuf),
    Uri(String),
}

struct Entry {
    item: Item,
    src: Src,
}

fn build_manifest(items: Vec<SendItem>) -> Result<Vec<Entry>> {
    let mut out: Vec<Entry> = Vec::new();
    for it in items {
        let mut push = |kind, path: String, size, mtime, text, src| {
            let id = out.len() as u32;
            out.push(Entry {
                item: Item {
                    id,
                    kind,
                    path,
                    size,
                    mtime,
                    text,
                },
                src,
            });
        };
        match it {
            SendItem::Text { text } => {
                if text.len() > MAX_TEXT {
                    bail!("Text is too long to send as a message; save it to a file and send the file.");
                }
                push(
                    ItemKind::Text,
                    "Text".into(),
                    text.len() as u64,
                    None,
                    Some(text),
                    Src::None,
                )
            }
            SendItem::Dir { rel_path } => push(ItemKind::Dir, rel_path, 0, None, None, Src::None),
            SendItem::Uri {
                uri,
                rel_path,
                size,
                mtime,
            } => push(ItemKind::File, rel_path, size, mtime, None, Src::Uri(uri)),
            SendItem::Path { path } => {
                let p = PathBuf::from(&path);
                let md = std::fs::metadata(&p).with_context(|| format!("Can't read {path}"))?;
                let base = p.parent().unwrap_or(Path::new("")).to_path_buf();
                let rel = |q: &Path| {
                    q.strip_prefix(&base)
                        .unwrap_or(q)
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/")
                };
                if md.is_dir() {
                    for e in walkdir::WalkDir::new(&p)
                        .follow_links(false)
                        .sort_by_file_name()
                    {
                        let e = e.with_context(|| format!("Can't read folder {path}"))?;
                        let ft = e.file_type();
                        if ft.is_dir() {
                            push(ItemKind::Dir, rel(e.path()), 0, None, None, Src::None);
                        } else if ft.is_file() {
                            let m = e.metadata()?;
                            push(
                                ItemKind::File,
                                rel(e.path()),
                                m.len(),
                                now_mtime(&m),
                                None,
                                Src::Path(e.path().into()),
                            );
                        } // symlinks and special files are skipped on purpose
                    }
                } else {
                    push(
                        ItemKind::File,
                        rel(&p),
                        md.len(),
                        now_mtime(&md),
                        None,
                        Src::Path(p),
                    );
                }
            }
        }
        if out.len() > MAX_ITEMS {
            bail!("Too many files in one transfer (limit {MAX_ITEMS}); send them in parts or zip them.");
        }
    }
    Ok(out)
}

async fn run_send(
    inner: Arc<Inner>,
    tid: String,
    target: Target,
    items: Vec<SendItem>,
    ctl: Arc<watch::Sender<Ctl>>,
) {
    let mut m = Meter::new(&tid, true);
    let entries = match tokio::task::spawn_blocking(move || build_manifest(items))
        .await
        .unwrap()
    {
        Ok(e) => e,
        Err(e) => return m.finish(&inner, "", 0, vec![], Some(e.to_string())),
    };
    let mut verified: HashMap<u32, Option<String>> = HashMap::new(); // id -> error
    let mut peer_id = String::new();
    let deadline = Instant::now() + RETRY_WINDOW;
    let mut attempt = 0;
    let res = loop {
        let r = send_once(
            &inner,
            &tid,
            &target,
            &entries,
            &mut verified,
            &mut m,
            &ctl,
            &mut peer_id,
        )
        .await;
        match r {
            Err(e)
                if e.downcast_ref::<Fatal>().is_none()
                    && Instant::now() < deadline
                    && *ctl.borrow() != Ctl::Cancel =>
            {
                attempt += 1;
                inner.emit(Event::Reconnecting {
                    transfer_id: tid.clone(),
                    attempt,
                    error: format!("{e:#}"),
                });
                tokio::time::sleep(Duration::from_secs((1u64 << attempt.min(3)).min(8))).await;
            }
            r => break r,
        }
    };
    inner.ctl.lock().unwrap().remove(&tid);
    let path = |id: &u32| entries[*id as usize].item.path.clone();
    let failed: Vec<String> = verified
        .iter()
        .filter(|(_, e)| e.is_some())
        .map(|(id, e)| format!("{}: {}", path(id), e.as_ref().unwrap()))
        .collect();
    let ok = verified.values().filter(|e| e.is_none()).count() as u32;
    m.finish(
        &inner,
        &peer_id,
        ok,
        failed,
        res.err().map(|e| format!("{e:#}")),
    );
}

async fn connect(inner: &Inner, t: &Target) -> Result<TlsStream<TcpStream>> {
    let mut last = anyhow!("no address to connect to");
    for h in t.host.split(',').map(str::trim).filter(|h| !h.is_empty()) {
        match timeout(Duration::from_secs(4), TcpStream::connect((h, t.port))).await {
            Ok(Ok(tcp)) => {
                tune(&tcp);
                let name = rustls::pki_types::ServerName::try_from("dropbridge")?;
                let c = tokio_rustls::TlsConnector::from(inner.client.clone()).connect(name, tcp).await?;
                return Ok(TlsStream::from(c));
            }
            Ok(Err(e)) => last = anyhow!("Couldn't connect to {h}:{} ({e}). Check both devices are on the same Wi-Fi and DropBridge is open.", t.port),
            Err(_) => last = anyhow!("{h}:{} didn't answer. Check both devices are on the same Wi-Fi, and that a firewall or VPN isn't blocking DropBridge.", t.port),
        }
    }
    Err(last)
}

enum Chunk {
    Start(u64),
    Data(Vec<u8>),
    End(String),
    Err(String),
}

fn open_src(src: &Src, d: &dyn Delegate) -> Result<File> {
    match src {
        Src::Path(p) => File::open(p).with_context(|| format!("Can't open {}", p.display())),
        #[cfg(unix)]
        Src::Uri(u) => {
            let fd = d.open_fd(u.clone());
            if fd < 0 {
                bail!("Can't open {u}; it may have been moved or permission was revoked");
            }
            // SAFETY: the platform handed us ownership of a fresh, detached fd.
            Ok(unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) })
        }
        _ => {
            let _ = d;
            bail!("unsupported source")
        }
    }
}

fn read_full(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// Disk read + BLAKE3 on a blocking thread, so hashing never stalls the network task.
fn read_source(
    src: Src,
    offset: u64,
    prefix: Option<String>,
    tx: mpsc::Sender<Chunk>,
    d: Arc<dyn Delegate>,
) {
    let r = (|| -> Result<()> {
        let mut f = open_src(&src, &*d)?;
        let mut h = blake3::Hasher::new();
        let mut start = 0;
        if offset > 0 {
            std::io::copy(&mut (&mut f).take(offset), &mut h)?;
            if prefix.as_deref() == Some(h.finalize().to_hex().as_str()) {
                start = offset;
            } else {
                h = blake3::Hasher::new();
                f.seek(SeekFrom::Start(0))?;
            }
        }
        tx.blocking_send(Chunk::Start(start))?;
        loop {
            let mut buf = vec![0u8; CHUNK];
            let n = read_full(&mut f, &mut buf)?;
            if n == 0 {
                break;
            }
            buf.truncate(n);
            h.update(&buf);
            tx.blocking_send(Chunk::Data(buf))?;
        }
        tx.blocking_send(Chunk::End(h.finalize().to_hex().to_string()))?;
        Ok(())
    })();
    if let Err(e) = r {
        let _ = tx.blocking_send(Chunk::Err(format!("{e:#}")));
    }
}

/// Reader side of an outgoing transfer. `Err` = connection problem (retryable).
type Incoming = std::result::Result<Msg, String>;

fn handle_incoming(
    m: Incoming,
    verified: &mut HashMap<u32, Option<String>>,
    ctl: &watch::Sender<Ctl>,
) -> Result<bool> {
    match m {
        Ok(Msg::Verified { item, ok, error }) => {
            verified.insert(
                item,
                if ok {
                    None
                } else {
                    Some(error.unwrap_or_else(|| "verification failed".into()))
                },
            );
        }
        Ok(Msg::Pause) => {
            ctl.send_replace(Ctl::Pause);
        }
        Ok(Msg::Resume) => {
            ctl.send_replace(Ctl::Run);
        }
        Ok(Msg::Done) => return Ok(true),
        Ok(Msg::Cancel { reason }) => return Err(fatal(reason)),
        Ok(other) => return Err(fatal(format!("protocol error: unexpected {other:?}"))),
        Err(e) => bail!("Connection lost: {e}"),
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
async fn send_once(
    inner: &Arc<Inner>,
    tid: &str,
    target: &Target,
    entries: &[Entry],
    verified: &mut HashMap<u32, Option<String>>,
    m: &mut Meter,
    ctl: &Arc<watch::Sender<Ctl>>,
    peer_id: &mut String,
) -> Result<()> {
    let conn = connect(inner, target).await?;
    let p = tls::peer(&conn)?;
    *peer_id = p.fingerprint.clone();
    if p.fingerprint == inner.fp {
        return Err(fatal("That's this device. Pick another device to send to."));
    }
    if let Some(fp) = &target.fingerprint {
        if !fp.eq_ignore_ascii_case(&p.fingerprint) {
            return Err(fatal("This device's identity doesn't match the one you paired with. Someone may be impersonating it, so nothing was sent."));
        }
    }
    if inner.trust.lock().unwrap().is_blocked(&p.fingerprint) {
        return Err(fatal(
            "You blocked this device. Unblock it in Settings to send to it.",
        ));
    }
    let (mut r, mut w) = tokio::io::split(conn);
    write_msg(
        &mut w,
        &Msg::Hello {
            name: inner.name.clone(),
            kind: inner.kind.clone(),
            version: PROTOCOL_VERSION,
        },
    )
    .await?;
    let name = match timeout(Duration::from_secs(10), read_msg(&mut r)).await?? {
        Msg::Hello { name, .. } => name,
        Msg::Cancel { reason } => return Err(fatal(reason)),
        _ => bail!("protocol error: expected Hello"),
    };
    m.peer_name = name.clone();
    inner.emit(Event::AwaitingApproval {
        transfer_id: tid.into(),
        to_name: name,
        sas: p.sas,
    });

    // Items already verified in an earlier attempt are not offered again.
    let todo: Vec<&Entry> = entries
        .iter()
        .filter(|e| !verified.contains_key(&e.item.id))
        .collect();
    write_msg(
        &mut w,
        &Msg::Offer {
            transfer_id: tid.into(),
            items: todo.iter().map(|e| e.item.clone()).collect(),
            token: target.token.clone(),
        },
    )
    .await?;
    let ans = tokio::select! {
        a = timeout(APPROVAL_TIMEOUT + Duration::from_secs(10), read_msg(&mut r)) => a.map_err(|_| fatal("The other device didn't respond in time."))??,
        _ = wait_cancel(ctl) => { let _ = write_msg(&mut w, &Msg::Cancel { reason: "Sender cancelled".into() }).await; return Err(fatal("Cancelled")); }
    };
    let Msg::Answer {
        accepted,
        items: answers,
        reason,
    } = ans
    else {
        bail!("protocol error: expected Answer")
    };
    if !accepted {
        return Err(fatal(
            reason.unwrap_or_else(|| "The other device declined.".into()),
        ));
    }
    let answers: HashMap<u32, ItemAnswer> = answers.into_iter().map(|a| (a.id, a)).collect();
    let files: Vec<(&Entry, ItemAnswer)> = todo
        .iter()
        .filter(|e| e.item.kind == ItemKind::File)
        .filter_map(|e| {
            answers
                .get(&e.item.id)
                .filter(|a| a.accept)
                .map(|a| (*e, a.clone()))
        })
        .collect();
    for e in &todo {
        if e.item.kind != ItemKind::File {
            verified.insert(e.item.id, None);
        } else if !answers.get(&e.item.id).is_some_and(|a| a.accept) {
            verified.insert(e.item.id, Some("skipped by receiver".into()));
        }
    }
    let total: u64 = files.iter().map(|(e, _)| e.item.size).sum();
    let resumed: u64 = files.iter().map(|(_, a)| a.offset).sum();
    m.reset(total, resumed, files.len() as u32);

    let (itx, mut irx) = mpsc::unbounded_channel::<Incoming>();
    let reader = tokio::spawn(async move {
        loop {
            let r = read_msg(&mut r).await.map_err(|e| e.to_string());
            let stop = matches!(r, Err(_) | Ok(Msg::Done) | Ok(Msg::Cancel { .. }));
            if itx.send(r).is_err() || stop {
                break;
            }
        }
    });
    let _abort = AbortOnDrop(reader);
    let res = stream_files(inner, &mut w, files, verified, m, ctl, &mut irx).await;
    if let Err(e) = res {
        if e.downcast_ref::<Fatal>().is_none() {
            while let Ok(Some(msg)) = timeout(Duration::from_millis(500), irx.recv()).await {
                if let Ok(Msg::Cancel { reason }) = msg {
                    return Err(fatal(reason));
                }
            }
        }
        return Err(e);
    }
    Ok(())
}

async fn stream_files(
    inner: &Arc<Inner>,
    w: &mut WriteHalf<TlsStream<TcpStream>>,
    files: Vec<(&Entry, ItemAnswer)>,
    verified: &mut HashMap<u32, Option<String>>,
    m: &mut Meter,
    ctl: &Arc<watch::Sender<Ctl>>,
    irx: &mut mpsc::UnboundedReceiver<Incoming>,
) -> Result<()> {
    let mut ctl_rx = ctl.subscribe();
    let mut remote_done = false;

    for (e, a) in files {
        let (ctx, mut crx) = mpsc::channel(PIPE_DEPTH);
        let d = inner.delegate.clone();
        let (src, prefix) = (e.src.clone(), a.prefix_hash.clone());
        tokio::task::spawn_blocking(move || read_source(src, a.offset, prefix, ctx, d));
        m.current = e.item.path.clone();
        let id = e.item.id;
        let mut started = false;
        while let Some(c) = crx.recv().await {
            match c {
                Chunk::Start(off) => {
                    m.done -= a.offset - off;
                    m.base -= a.offset - off;
                    write_msg(
                        w,
                        &Msg::Start {
                            item: id,
                            offset: off,
                        },
                    )
                    .await?;
                    started = true;
                }
                Chunk::Data(b) => {
                    while let Ok(msg) = irx.try_recv() {
                        remote_done |= handle_incoming(msg, verified, ctl)?;
                    }
                    loop {
                        let c = *ctl_rx.borrow_and_update();
                        match c {
                            Ctl::Run => break,
                            Ctl::Cancel => {
                                let _ = write_msg(
                                    w,
                                    &Msg::Cancel {
                                        reason: "Sender cancelled".into(),
                                    },
                                )
                                .await;
                                return Err(fatal("Cancelled"));
                            }
                            Ctl::Pause => {
                                if !m.paused {
                                    m.paused = true;
                                    m.tick(inner, true);
                                }
                                tokio::select! {
                                    _ = ctl_rx.changed() => {}
                                    msg = irx.recv() => { if let Some(msg) = msg { remote_done |= handle_incoming(msg, verified, ctl)?; } }
                                }
                            }
                        }
                    }
                    if m.paused {
                        m.paused = false;
                        m.reset(m.total, m.done, m.files_total); // don't let the pause drag the average down
                    }
                    write_data(w, id, &b).await?;
                    m.done += b.len() as u64;
                    m.tick(inner, false);
                }
                Chunk::End(hash) => {
                    write_msg(w, &Msg::FileEnd { item: id, hash }).await?;
                    m.files_done += 1;
                }
                Chunk::Err(err) => {
                    if started {
                        write_msg(
                            w,
                            &Msg::FileEnd {
                                item: id,
                                hash: String::new(),
                            },
                        )
                        .await?;
                    }
                    verified.insert(id, Some(err));
                }
            }
        }
    }
    write_msg(w, &Msg::Done).await?;
    m.tick(inner, true);
    while !remote_done {
        match timeout(Duration::from_secs(120), irx.recv()).await {
            Ok(Some(msg)) => remote_done = handle_incoming(msg, verified, ctl)?,
            Ok(None) => bail!("Connection lost while waiting for confirmation"),
            Err(_) => bail!("The other device stopped responding"),
        }
    }
    Ok(())
}

async fn wait_cancel(ctl: &watch::Sender<Ctl>) {
    let mut rx = ctl.subscribe();
    let _ = rx.wait_for(|c| *c == Ctl::Cancel).await;
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);
impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort()
    }
}

// ───────────────────────────── receiving ─────────────────────────────

async fn accept_loop(inner: Arc<Inner>, l: TcpListener) {
    loop {
        if let Ok((tcp, _)) = l.accept().await {
            let i = inner.clone();
            tokio::spawn(async move {
                let _ = handle_conn(i, tcp).await;
            });
        }
    }
}

async fn writer_task(mut w: WriteHalf<TlsStream<TcpStream>>, mut rx: mpsc::UnboundedReceiver<Msg>) {
    while let Some(m) = rx.recv().await {
        if write_msg(&mut w, &m).await.is_err() {
            break;
        }
    }
}

struct Planned {
    item: Item,
    target: PathBuf,
}

struct Current {
    id: u32,
    tx: Option<mpsc::Sender<Vec<u8>>>,
    job: tokio::task::JoinHandle<Result<String>>,
}

fn hash_prefix(p: &Path) -> Result<(u64, String)> {
    let mut f = File::open(p)?;
    let mut h = blake3::Hasher::new();
    let n = std::io::copy(&mut f, &mut h)?;
    Ok((n, h.finalize().to_hex().to_string()))
}

fn open_part(p: &Path) -> std::io::Result<File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true).write(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
    o.open(p)
}

/// Flush to the OS before rename. macOS `sync_all` is F_FULLFSYNC (drive-cache flush, ~3 ms and
/// serialised device-wide), which made 1,000-file batches 4x slower; plain fsync survives app crashes,
/// and the BLAKE3 prefix check on resume catches anything a power cut leaves behind.
fn durable(f: &File) -> std::io::Result<()> {
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        if unsafe { libc::fsync(f.as_raw_fd()) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_vendor = "apple"))]
    f.sync_data()
}

/// Disk write + BLAKE3 on a blocking thread. Returns the hash of the whole file.
fn write_part(
    part: PathBuf,
    offset: u64,
    mtime: Option<i64>,
    mut rx: mpsc::Receiver<Vec<u8>>,
) -> Result<String> {
    let mut f = open_part(&part).with_context(|| format!("Can't write {}", part.display()))?;
    let mut h = blake3::Hasher::new();
    if offset > 0 {
        let n = std::io::copy(&mut (&mut f).take(offset), &mut h)?;
        if n != offset {
            bail!("partial file is shorter than the resume point");
        }
    }
    f.set_len(offset)?;
    f.seek(SeekFrom::Start(offset))?;
    while let Some(b) = rx.blocking_recv() {
        f.write_all(&b).map_err(|e| {
            if e.raw_os_error() == Some(28) {
                anyhow!("The disk is full")
            } else {
                anyhow!("Can't write {}: {e}", part.display())
            }
        })?;
        h.update(&b);
    }
    durable(&f)?;
    if let Some(t) = mtime.filter(|t| *t >= 0) {
        let _ = f.set_modified(UNIX_EPOCH + Duration::from_secs(t as u64));
    }
    Ok(h.finalize().to_hex().to_string())
}

async fn handle_conn(inner: Arc<Inner>, tcp: TcpStream) -> Result<()> {
    tune(&tcp);
    let conn = timeout(
        Duration::from_secs(10),
        tokio_rustls::TlsAcceptor::from(inner.server.clone()).accept(tcp),
    )
    .await??;
    let conn = TlsStream::from(conn);
    let p = tls::peer(&conn)?;
    let fp = p.fingerprint.clone();
    let (mut r, mut w) = tokio::io::split(conn);
    if inner.trust.lock().unwrap().is_blocked(&fp) {
        return write_msg(
            &mut w,
            &Msg::Cancel {
                reason: "The other device declined.".into(),
            },
        )
        .await;
    }
    let Msg::Hello { name, kind, .. } =
        timeout(Duration::from_secs(10), read_msg(&mut r)).await??
    else {
        bail!("expected Hello")
    };
    let (otx, orx) = mpsc::unbounded_channel();
    let writer = tokio::spawn(writer_task(w, orx));
    let _ = otx.send(Msg::Hello {
        name: inner.name.clone(),
        kind: inner.kind.clone(),
        version: PROTOCOL_VERSION,
    });
    let Msg::Offer {
        transfer_id: tid,
        items,
        token,
    } = timeout(Duration::from_secs(30), read_msg(&mut r)).await??
    else {
        bail!("expected Offer")
    };
    let res = receive(
        &inner, &mut r, &otx, &tid, items, token, &fp, &name, &kind, &p.sas,
    )
    .await;
    if let Err(e) = &res {
        let _ = otx.send(Msg::Cancel {
            reason: format!("{e:#}"),
        });
        // Keep reading briefly: closing with unread data sends a TCP reset that can destroy the Cancel.
        let _ = timeout(Duration::from_secs(2), async {
            while read_frame(&mut r).await.is_ok() {}
        })
        .await;
    }
    drop(otx);
    let _ = timeout(Duration::from_secs(5), writer).await;
    res
}

fn decline(otx: &mpsc::UnboundedSender<Msg>, reason: &str) {
    let _ = otx.send(Msg::Answer {
        accepted: false,
        items: vec![],
        reason: Some(reason.into()),
    });
}

#[allow(clippy::too_many_arguments)]
async fn receive(
    inner: &Arc<Inner>,
    r: &mut ReadHalf<TlsStream<TcpStream>>,
    otx: &mpsc::UnboundedSender<Msg>,
    tid: &str,
    items: Vec<Item>,
    token: Option<String>,
    fp: &str,
    name: &str,
    kind: &str,
    sas: &str,
) -> Result<()> {
    let mut m = Meter::new(tid, false);
    m.peer_name = name.into();
    // Validate everything the peer told us before asking the user anything.
    if tid.is_empty() || tid.len() > 64 || items.len() > MAX_ITEMS {
        decline(otx, "Invalid transfer request");
        bail!("invalid offer");
    }
    let mut rels = Vec::with_capacity(items.len());
    let mut bytes: u64 = 0;
    for (i, it) in items.iter().enumerate() {
        if i > 0 && items[i - 1].id >= it.id {
            decline(otx, "Invalid transfer request");
            bail!("offer item ids out of order");
        }
        if it.kind == ItemKind::Text {
            rels.push(PathBuf::new());
            continue;
        }
        match paths::sanitize(&it.path) {
            Ok(p) => rels.push(p),
            Err(e) => {
                decline(otx, &format!("Refused an unsafe file name: {e}"));
                bail!("unsafe path in offer: {e}");
            }
        }
        bytes = bytes.checked_add(it.size).context("invalid size")?;
    }

    if let Some(t) = token {
        if inner.take_token(&t) {
            inner.trust.lock().unwrap().set(fp, name, false)?;
        } else {
            decline(
                otx,
                "The pairing code is wrong or expired. Show a new code and try again.",
            );
            bail!("bad pairing token");
        }
    }
    let trusted = inner.trust.lock().unwrap().is_trusted(fp);
    let key = format!("{fp}:{tid}");
    let prior = inner.accepted.lock().unwrap().get(&key).map(|a| a.decision);
    let decision = if let Some(d) = prior {
        d // reconnect of a transfer the user already approved
    } else if trusted && inner.auto_accept.load(Ordering::Relaxed) {
        Decision {
            accept: true,
            trust: false,
            conflict: Conflict::Rename,
        }
    } else {
        let rx = {
            let mut pend = inner.pending.lock().unwrap();
            if pend.contains_key(tid) || pend.values().any(|(f, _)| f == fp) || pend.len() >= 8 {
                drop(pend);
                decline(
                    otx,
                    "Busy: another request from this device is waiting for approval.",
                );
                return Ok(());
            }
            let (tx, rx) = oneshot::channel();
            pend.insert(tid.to_string(), (fp.to_string(), tx));
            rx
        };
        inner.new_ctl(tid);
        inner.emit(Event::IncomingOffer {
            transfer_id: tid.into(),
            from_id: fp.into(),
            from_name: name.into(),
            from_kind: kind.into(),
            trusted,
            sas: sas.into(),
            files: items.iter().filter(|i| i.kind == ItemKind::File).count() as u32,
            bytes,
            texts: items.iter().filter(|i| i.kind == ItemKind::Text).count() as u32,
            preview: items
                .iter()
                .filter(|i| i.kind != ItemKind::Dir)
                .take(5)
                .map(|i| {
                    if i.kind == ItemKind::Text {
                        i.text
                            .clone()
                            .unwrap_or_default()
                            .chars()
                            .take(80)
                            .collect()
                    } else {
                        i.path.clone()
                    }
                })
                .collect(),
        });
        let d = timeout(APPROVAL_TIMEOUT, rx)
            .await
            .ok()
            .and_then(|r| r.ok());
        inner.pending.lock().unwrap().remove(tid);
        match d {
            Some(d) if d.accept => d,
            _ => {
                decline(
                    otx,
                    if d.is_none() {
                        "Nobody accepted on the other device in time."
                    } else {
                        "The other device declined."
                    },
                );
                inner.ctl.lock().unwrap().remove(tid);
                m.finish(inner, fp, 0, vec![], Some("Declined".into()));
                return Ok(());
            }
        }
    };
    if decision.trust {
        inner.trust.lock().unwrap().set(fp, name, false)?;
    }
    inner
        .accepted
        .lock()
        .unwrap()
        .entry(key.clone())
        .or_insert(Accepted {
            decision,
            done: HashSet::new(),
        });
    let existing = inner.ctl.lock().unwrap().get(tid).cloned();
    let ctl = existing.unwrap_or_else(|| inner.new_ctl(tid));

    // Plan targets, find resumable partials, check space.
    let dest = inner.download_dir.lock().unwrap().clone();
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("Can't create download folder {}", dest.display()))?;
    let mut answers = Vec::new();
    let mut plan: HashMap<u32, Planned> = HashMap::new();
    let mut need: u64 = 0;
    let (mut total, mut have, mut nfiles) = (0u64, 0u64, 0u32);
    for (it, rel) in items.into_iter().zip(rels) {
        match it.kind {
            ItemKind::Text => {
                inner.emit(Event::TextReceived {
                    transfer_id: tid.into(),
                    from_name: name.into(),
                    text: it.text.clone().unwrap_or_default(),
                });
                answers.push(ItemAnswer {
                    id: it.id,
                    accept: true,
                    offset: 0,
                    prefix_hash: None,
                });
            }
            ItemKind::Dir => {
                let ok = paths::resolve(&dest, &rel)
                    .and_then(|p| Ok(std::fs::create_dir_all(p)?))
                    .is_ok();
                answers.push(ItemAnswer {
                    id: it.id,
                    accept: ok,
                    offset: 0,
                    prefix_hash: None,
                });
            }
            ItemKind::File => {
                let target = match paths::resolve(&dest, &rel) {
                    Ok(t) => t,
                    Err(_) => {
                        answers.push(ItemAnswer {
                            id: it.id,
                            accept: false,
                            offset: 0,
                            prefix_hash: None,
                        });
                        continue;
                    }
                };
                if decision.conflict == Conflict::Skip && target.exists() {
                    answers.push(ItemAnswer {
                        id: it.id,
                        accept: false,
                        offset: 0,
                        prefix_hash: None,
                    });
                    continue;
                }
                let part = paths::part_path(&target);
                let (offset, prefix_hash) =
                    match tokio::task::spawn_blocking(move || hash_prefix(&part)).await? {
                        Ok((n, h)) if n > 0 && n <= it.size => (n, Some(h)),
                        _ => (0, None),
                    };
                need += it.size - offset;
                (total, have, nfiles) = (total + it.size, have + offset, nfiles + 1);
                answers.push(ItemAnswer {
                    id: it.id,
                    accept: true,
                    offset,
                    prefix_hash,
                });
                plan.insert(it.id, Planned { item: it, target });
            }
        }
    }
    let free = fs4::available_space(&dest).unwrap_or(u64::MAX);
    if need.saturating_add(need / 100).saturating_add(16 << 20) > free && need > 0 {
        let msg = format!("Not enough space on {}: needs {}, only {} free. Free up space or choose another download folder.", inner.name, human(need), human(free));
        decline(otx, &msg);
        m.finish(inner, fp, 0, vec![], Some(msg));
        return Ok(());
    }
    let _ = otx.send(Msg::Answer {
        accepted: true,
        items: answers,
        reason: None,
    });
    m.reset(total, have, nfiles);

    // Forward local pause/cancel to the sender.
    let mut ctl_rx = ctl.subscribe();
    let otx2 = otx.clone();
    let fwd = tokio::spawn(async move {
        while ctl_rx.changed().await.is_ok() {
            let c = *ctl_rx.borrow_and_update();
            let _ = otx2.send(match c {
                Ctl::Run => Msg::Resume,
                Ctl::Pause => Msg::Pause,
                Ctl::Cancel => Msg::Cancel {
                    reason: "The receiver cancelled".into(),
                },
            });
        }
    });
    let _fwd = AbortOnDrop(fwd);

    let mut cur: Option<Current> = None;
    let mut failed = Vec::new();
    let (dtx, mut drx) =
        mpsc::unbounded_channel::<(u32, std::result::Result<(), (bool, String)>)>();
    let mut outstanding = 0usize;
    let writers = Arc::new(tokio::sync::Semaphore::new(64));
    let result: Result<()> = async {
        // Rename into place (serial, so conflict names can't race) and report.
        let mut finalize =
            |item: u32, r: std::result::Result<(), (bool, String)>| -> Result<bool> {
                let pl = &plan[&item];
                let part = paths::part_path(&pl.target);
                let res = match r {
                    Ok(()) => {
                        let fin = if decision.conflict == Conflict::Overwrite {
                            pl.target.clone()
                        } else {
                            paths::unique(&pl.target)
                        };
                        std::fs::rename(&part, &fin)
                            .map(|_| fin)
                            .map_err(|e| format!("Can't save file: {e}"))
                    }
                    Err((disk, e)) => {
                        let _ = std::fs::remove_file(&part);
                        if disk {
                            return Err(fatal(e));
                        }
                        Err(e)
                    }
                };
                let ok = res.is_ok();
                let _ = otx.send(Msg::Verified {
                    item,
                    ok,
                    error: res.as_ref().err().cloned(),
                });
                let shown = res
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| pl.item.path.clone());
                inner.emit(Event::ItemDone {
                    transfer_id: tid.into(),
                    path: shown,
                    ok,
                    error: res.as_ref().err().cloned(),
                });
                if ok {
                    if let Some(a) = inner.accepted.lock().unwrap().get_mut(&key) {
                        a.done.insert(item);
                    }
                } else {
                    failed.push(format!("{}: {}", pl.item.path, res.unwrap_err()));
                }
                Ok(ok)
            };
        loop {
            let frame = tokio::select! {
                f = read_frame(r) => f?,
                _ = wait_cancel(&ctl) => return Err(fatal("Cancelled")),
            };
            while let Ok((item, r)) = drx.try_recv() {
                outstanding -= 1;
                m.files_done += finalize(item, r)? as u32;
            }
            match frame {
                Frame::Control(Msg::Start { item, offset }) => {
                    let Some(pl) = plan.get(&item) else {
                        bail!("protocol error: Start for unknown item")
                    };
                    if cur.is_some() || offset > pl.item.size {
                        bail!("protocol error: bad Start");
                    }
                    let part = paths::part_path(&pl.target);
                    let resumed = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                    if offset > resumed {
                        bail!("protocol error: resume point beyond what we have");
                    }
                    m.done -= resumed.min(m.done) - offset.min(resumed); // sender restarted from earlier
                    m.base = m.base.min(m.done);
                    let (tx, rx) = mpsc::channel(PIPE_DEPTH);
                    let mtime = pl.item.mtime;
                    m.current = pl.item.path.clone();
                    let permit = writers.clone().acquire_owned().await?;
                    let job = tokio::task::spawn_blocking(move || {
                        let _permit = permit;
                        write_part(part, offset, mtime, rx)
                    });
                    cur = Some(Current {
                        id: item,
                        tx: Some(tx),
                        job,
                    });
                }
                Frame::Data { item, bytes } => {
                    let c = cur
                        .as_mut()
                        .filter(|c| c.id == item)
                        .context("protocol error: data for wrong item")?;
                    let n = bytes.len() as u64;
                    if c.tx.as_ref().unwrap().send(bytes).await.is_err() {
                        let c = cur.take().unwrap();
                        let err = c
                            .job
                            .await?
                            .err()
                            .map(|e| format!("{e:#}"))
                            .unwrap_or_default();
                        let _ = std::fs::remove_file(paths::part_path(&plan[&item].target));
                        return Err(fatal(err));
                    }
                    m.done += n;
                    m.tick(inner, false);
                }
                Frame::Control(Msg::FileEnd { item, hash }) => {
                    let mut c = cur
                        .take()
                        .filter(|c| c.id == item)
                        .context("protocol error: FileEnd for wrong item")?;
                    drop(c.tx.take());
                    // Flush+verify runs off the receive loop so small files pipeline; renames stay serial below.
                    let dtx = dtx.clone();
                    outstanding += 1;
                    tokio::spawn(async move {
                        let r = match c.job.await {
                            Ok(Ok(h)) if h == hash => Ok(()),
                            Ok(Ok(_)) => Err((
                                false,
                                "Corrupted in transit (hash mismatch); retry this file".to_string(),
                            )),
                            Ok(Err(e)) => Err((true, format!("{e:#}"))),
                            Err(e) => Err((true, e.to_string())),
                        };
                        let _ = dtx.send((item, r));
                    });
                }
                Frame::Control(Msg::Pause) => {
                    m.paused = true;
                    m.tick(inner, true);
                }
                Frame::Control(Msg::Resume) => m.paused = false,
                Frame::Control(Msg::Done) => {
                    while outstanding > 0 {
                        let (item, r) = drx.recv().await.context("internal error")?;
                        outstanding -= 1;
                        m.files_done += finalize(item, r)? as u32;
                    }
                    let _ = otx.send(Msg::Done);
                    return Ok(());
                }
                Frame::Control(Msg::Cancel { reason }) => return Err(fatal(reason)),
                Frame::Control(other) => bail!("protocol error: unexpected {other:?}"),
            }
        }
    }
    .await;
    if let Some(c) = cur.take() {
        drop(c.tx);
        let _ = c.job.await; // partial stays on disk for resume unless cancelled
        if result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<Fatal>())
            .is_some()
        {
            let _ = std::fs::remove_file(paths::part_path(&plan[&c.id].target));
        }
    }
    let fin = result.as_ref().is_ok()
        || result
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<Fatal>())
            .is_some();
    if fin {
        inner.ctl.lock().unwrap().remove(tid);
        inner.accepted.lock().unwrap().remove(&key);
    }
    m.tick(inner, true);
    let err = result.as_ref().err().map(|e| {
        if fin {
            format!("{e:#}")
        } else {
            format!("Connection lost ({e:#}). The transfer will resume if the sender reconnects.")
        }
    });
    m.finish(inner, fp, m.files_done, failed, err);
    result.or_else(|e| if fin { Ok(()) } else { Err(e) })
}

pub fn human(b: u64) -> String {
    let u = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < u.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", u[i])
    }
}

/// Wall clock helper for apps building history entries.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
