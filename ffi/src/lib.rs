//! UniFFI bindings for Android (and later iOS). Events cross the boundary as JSON so the
//! Kotlin side doesn't need a mirrored type for every `Event` variant.
use dropbridge::*;
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{msg}")]
    Failed { msg: String },
}

fn err(e: impl std::fmt::Display) -> FfiError {
    FfiError::Failed { msg: e.to_string() }
}

#[uniffi::export(with_foreign)]
pub trait Listener: Send + Sync {
    fn on_event(&self, json: String);
    /// Open a content:// URI and return a detached fd, or -1.
    fn open_fd(&self, uri: String) -> i32;
}

struct Bridge(Arc<dyn Listener>);
impl Delegate for Bridge {
    fn on_event(&self, e: Event) {
        self.0.on_event(serde_json::to_string(&e).unwrap_or_default())
    }
    fn open_fd(&self, uri: String) -> i32 {
        self.0.open_fd(uri)
    }
}

#[derive(uniffi::Object)]
pub struct DropNode {
    node: Node,
}

#[uniffi::export]
impl DropNode {
    #[uniffi::constructor]
    pub fn new(
        name: String,
        kind: String,
        identity: String,
        data_dir: String,
        download_dir: String,
        auto_accept: bool,
        listener: Arc<dyn Listener>,
    ) -> Result<Arc<Self>, FfiError> {
        let cfg = NodeConfig { name, kind, identity, data_dir, download_dir, port: 0, discovery: true, auto_accept_trusted: auto_accept };
        Ok(Arc::new(Self { node: Node::new(cfg, Arc::new(Bridge(listener))).map_err(|e| err(format!("{e:#}")))? }))
    }

    pub fn identity_secret(&self) -> String {
        self.node.identity_secret()
    }
    pub fn fingerprint(&self) -> String {
        self.node.fingerprint()
    }
    pub fn port(&self) -> u16 {
        self.node.port()
    }
    pub fn local_ips(&self) -> Vec<String> {
        local_ips()
    }

    /// `items_json`: `[{"type":"uri","uri":..,"rel_path":..,"size":..,"mtime":..}, {"type":"text","text":..}, {"type":"dir","rel_path":..}]`
    pub fn send(
        &self,
        host: String,
        port: u16,
        fingerprint: Option<String>,
        token: Option<String>,
        items_json: String,
    ) -> Result<String, FfiError> {
        let items: Vec<SendItem> = serde_json::from_str(&items_json).map_err(err)?;
        Ok(self.node.send(Target { host, port, fingerprint, token }, items))
    }

    pub fn respond(&self, id: String, accept: bool, trust: bool, conflict: String) {
        let conflict = match conflict.as_str() {
            "Overwrite" => Conflict::Overwrite,
            "Skip" => Conflict::Skip,
            _ => Conflict::Rename,
        };
        self.node.respond(&id, Decision { accept, trust, conflict })
    }

    /// pause | resume | cancel | retry (returns the new id for retry)
    pub fn control(&self, id: String, action: String) -> Option<String> {
        match action.as_str() {
            "pause" => self.node.pause(&id),
            "resume" => self.node.resume(&id),
            "cancel" => self.node.cancel(&id),
            "retry" => return self.node.retry(&id),
            _ => {}
        }
        None
    }

    pub fn pairing_uri(&self) -> String {
        self.node.pairing_uri()
    }
    pub fn pairing_code(&self) -> String {
        self.node.pairing_code()
    }

    /// `{"id":..,"name":..,"blocked":..}` list
    pub fn devices_json(&self) -> String {
        let v: Vec<_> =
            self.node.devices().into_iter().map(|(id, d)| serde_json::json!({ "id": id, "name": d.name, "blocked": d.blocked })).collect();
        serde_json::to_string(&v).unwrap_or_default()
    }

    /// trust | block | forget
    pub fn device(&self, id: String, name: String, action: String) -> Result<(), FfiError> {
        match action.as_str() {
            "trust" => self.node.trust(&id, &name),
            "block" => self.node.block(&id, &name),
            _ => self.node.forget(&id),
        }
        .map_err(err)
    }

    pub fn set_download_dir(&self, dir: String) -> Result<(), FfiError> {
        self.node.set_download_dir(&dir).map_err(err)
    }
    pub fn set_auto_accept(&self, on: bool) {
        self.node.set_auto_accept(on)
    }
}

/// Parse a dropbridge:// link or host:port into `{"host","port","fingerprint","token"}`.
#[uniffi::export]
pub fn parse_target(input: String) -> Result<String, FfiError> {
    let s = input.trim();
    let t = if s.starts_with("dropbridge://") {
        parse_pairing_uri(s).map_err(err)?
    } else {
        let (h, p) = s.rsplit_once(':').ok_or_else(|| err("Enter the address shown on the other device, like 192.168.1.20:52345"))?;
        Target {
            host: h.trim_matches(['[', ']']).into(),
            port: p.parse().map_err(|_| err("The port must be a number"))?,
            fingerprint: None,
            token: None,
        }
    };
    Ok(serde_json::json!({ "host": t.host, "port": t.port, "fingerprint": t.fingerprint, "token": t.token }).to_string())
}

#[uniffi::export]
pub fn human_bytes(b: u64) -> String {
    human(b)
}
