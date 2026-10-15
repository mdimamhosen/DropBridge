//! End-to-end tests: real nodes talking TLS over loopback.
use dropbridge::wire::{self, Item, ItemKind, Msg};
use dropbridge::*;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Rec(Mutex<Sender<Event>>);
impl Delegate for Rec {
    fn on_event(&self, e: Event) {
        let _ = self.0.lock().unwrap().send(e);
    }
}

struct Peer {
    node: Node,
    rx: Receiver<Event>,
    dir: tempfile::TempDir,
}

impl Peer {
    fn new(name: &str, auto: bool) -> Self {
        Self::with(name, auto, false)
    }
    fn with(name: &str, auto: bool, discovery: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = channel();
        let node = Node::new(
            NodeConfig {
                name: name.into(),
                kind: "cli".into(),
                identity: String::new(),
                data_dir: dir.path().join("data").to_string_lossy().into(),
                download_dir: dir.path().join("in").to_string_lossy().into(),
                port: 0,
                discovery,
                auto_accept_trusted: auto,
            },
            Arc::new(Rec(Mutex::new(tx))),
        )
        .unwrap();
        Self { node, rx, dir }
    }
    fn inbox(&self) -> PathBuf {
        self.dir.path().join("in")
    }
    fn target(&self) -> Target {
        Target {
            host: "127.0.0.1".into(),
            port: self.node.port(),
            fingerprint: Some(self.node.fingerprint()),
            token: None,
        }
    }
    fn wait(&self, secs: u64, f: impl Fn(&Event) -> bool) -> Event {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(e) if f(&e) => return e,
                Ok(_) => {}
                Err(_) => panic!("timed out waiting for event"),
            }
        }
    }
    fn finished(&self) -> Event {
        self.wait(60, |e| matches!(e, Event::Finished { .. }))
    }
}

/// Receiver that trusts and auto-accepts `sender`.
fn pair() -> (Peer, Peer) {
    let a = Peer::new("Alice", true);
    let b = Peer::new("Bob", true);
    a.node.trust(&b.node.fingerprint(), "Bob").unwrap();
    b.node.trust(&a.node.fingerprint(), "Alice").unwrap();
    (a, b)
}

fn hash(p: &Path) -> String {
    blake3::hash(&std::fs::read(p).unwrap())
        .to_hex()
        .to_string()
}

fn ok(e: &Event) -> bool {
    matches!(e, Event::Finished { ok: true, .. })
}

fn random_file(p: &Path, n: usize) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut v = vec![0u8; n];
    for (i, b) in v.iter_mut().enumerate() {
        *b = (i.wrapping_mul(2654435761) >> 13) as u8;
    }
    std::fs::write(p, v).unwrap();
}

#[test]
fn file_folder_and_text_both_directions() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("big.bin");
    random_file(&f, 5_000_123);
    let tree = src.path().join("Tree");
    random_file(&tree.join("sub/deep/x.txt"), 10);
    random_file(&tree.join(".hidden"), 3);
    random_file(&tree.join("ছবি 🎉 (1).txt"), 7);
    std::fs::create_dir_all(tree.join("empty")).unwrap();
    std::fs::write(tree.join("zero.txt"), b"").unwrap();
    let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    std::fs::File::options()
        .write(true)
        .open(&f)
        .unwrap()
        .set_modified(old)
        .unwrap();

    a.node.send(
        b.target(),
        vec![
            SendItem::Path {
                path: f.to_string_lossy().into(),
            },
            SendItem::Path {
                path: tree.to_string_lossy().into(),
            },
            SendItem::Text {
                text: "hello ✓".into(),
            },
        ],
    );
    let t = b.wait(10, |e| matches!(e, Event::TextReceived { .. }));
    assert!(matches!(t, Event::TextReceived { ref text, .. } if text == "hello ✓"));
    assert!(ok(&a.finished()));
    assert!(ok(&b.finished()));
    let got = b.inbox().join("big.bin");
    assert_eq!(hash(&got), hash(&f));
    assert_eq!(std::fs::metadata(&got).unwrap().modified().unwrap(), old);
    for rel in [
        "Tree/sub/deep/x.txt",
        "Tree/.hidden",
        "Tree/ছবি 🎉 (1).txt",
        "Tree/zero.txt",
    ] {
        assert_eq!(
            hash(&b.inbox().join(rel)),
            hash(&src.path().join(rel)),
            "{rel}"
        );
    }
    assert!(b.inbox().join("Tree/empty").is_dir());
    // no partial files left behind
    assert!(walk(&b.inbox())
        .iter()
        .all(|p| !p.to_string_lossy().ends_with(".dbpart")));

    // and back the other way
    b.node.send(
        a.target(),
        vec![SendItem::Path {
            path: got.to_string_lossy().into(),
        }],
    );
    assert!(ok(&b.finished()));
    assert_eq!(hash(&a.inbox().join("big.bin")), hash(&f));
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(p).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            v.extend(walk(&e.path()));
        }
        v.push(e.path());
    }
    v
}

#[test]
fn unknown_sender_needs_approval_and_codes_match() {
    let a = Peer::new("Alice", false);
    let b = Peer::new("Bob", false);
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("a.txt");
    std::fs::write(&f, "x").unwrap();

    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    let Event::AwaitingApproval { sas: s1, .. } =
        a.wait(10, |e| matches!(e, Event::AwaitingApproval { .. }))
    else {
        unreachable!()
    };
    let Event::IncomingOffer {
        transfer_id,
        sas: s2,
        trusted,
        files,
        ..
    } = b.wait(10, |e| matches!(e, Event::IncomingOffer { .. }))
    else {
        unreachable!()
    };
    assert_eq!(s1, s2, "both screens show the same verification code");
    assert!(!trusted);
    assert_eq!(files, 1);
    b.node.respond(
        &transfer_id,
        Decision {
            accept: false,
            trust: false,
            conflict: Conflict::Rename,
        },
    );
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("declined"))
    );
    assert!(!b.inbox().join("a.txt").exists());

    // accept + trust → next transfer is auto-accepted
    b.node.set_auto_accept(true);
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    let Event::IncomingOffer { transfer_id, .. } =
        b.wait(10, |e| matches!(e, Event::IncomingOffer { .. }))
    else {
        unreachable!()
    };
    b.node.respond(
        &transfer_id,
        Decision {
            accept: true,
            trust: true,
            conflict: Conflict::Rename,
        },
    );
    assert!(ok(&a.finished()));
    a.node.send(
        b.target(),
        vec![SendItem::Text {
            text: "auto".into(),
        }],
    );
    assert!(ok(&a.finished()));
}

#[test]
fn impersonation_is_refused() {
    let (a, b) = pair();
    let mut t = b.target();
    t.fingerprint = Some("ab".repeat(32));
    a.node.send(
        t,
        vec![SendItem::Text {
            text: "secret".into(),
        }],
    );
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("impersonat"))
    );
    assert!(
        b.rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "receiver saw nothing"
    );
}

#[test]
fn pairing_code_trusts_and_wrong_code_is_rejected() {
    let a = Peer::new("Alice", false);
    let b = Peer::new("Bob", true);
    let mut t = b.target();
    t.token = Some("000000x".into());
    a.node
        .send(t.clone(), vec![SendItem::Text { text: "hi".into() }]);
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("pairing code"))
    );

    let uri = b.node.pairing_uri();
    let mut t = parse_pairing_uri(&uri).unwrap();
    t.host = "127.0.0.1".into();
    assert_eq!(
        t.fingerprint.as_deref(),
        Some(b.node.fingerprint().as_str())
    );
    a.node
        .send(t.clone(), vec![SendItem::Text { text: "hi".into() }]);
    assert!(ok(&a.finished()), "token = trusted, auto-accepted");
    assert!(b
        .node
        .devices()
        .iter()
        .any(|(fp, _)| *fp == a.node.fingerprint()));
    // tokens are one-time
    a.node.send(
        t,
        vec![SendItem::Text {
            text: "again".into(),
        }],
    );
    assert!(matches!(a.finished(), Event::Finished { ok: false, .. }));

    let code = b.node.pairing_code();
    assert_eq!(code.len(), 6);
}

#[test]
fn resumes_from_partial_and_restarts_on_mismatch() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("r.bin");
    random_file(&f, 3_000_000);
    let data = std::fs::read(&f).unwrap();
    std::fs::create_dir_all(b.inbox()).unwrap();
    std::fs::write(b.inbox().join(".r.bin.dbpart"), &data[..1_000_000]).unwrap();
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    let Event::Finished { ok, bytes, .. } = a.finished() else {
        unreachable!()
    };
    assert!(ok);
    assert_eq!(bytes, 2_000_000, "only the missing part was sent");
    assert_eq!(hash(&b.inbox().join("r.bin")), hash(&f));

    // stale partial from a different file: restart from 0, still correct
    std::fs::write(b.inbox().join(".r.bin.dbpart"), vec![7u8; 500_000]).unwrap();
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    let Event::Finished { ok, bytes, .. } = a.finished() else {
        unreachable!()
    };
    assert!(ok);
    assert_eq!(bytes, 3_000_000);
    assert_eq!(hash(&b.inbox().join("r (1).bin")), hash(&f));
}

/// Forwards to `to`; the first connection is cut after `cut` bytes.
fn flaky_proxy(to: u16, cut: usize) -> u16 {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for (n, c) in l.incoming().enumerate() {
            let mut c = c.unwrap();
            let mut s = std::net::TcpStream::connect(("127.0.0.1", to)).unwrap();
            let (mut c2, mut s2) = (c.try_clone().unwrap(), s.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut s2, &mut c2);
            });
            std::thread::spawn(move || {
                let limit = if n == 0 { cut } else { usize::MAX };
                let mut buf = [0u8; 65536];
                let mut sent = 0;
                while let Ok(k) = c.read(&mut buf) {
                    if k == 0 || s.write_all(&buf[..k]).is_err() {
                        break;
                    }
                    sent += k;
                    if sent > limit {
                        let _ = s.shutdown(std::net::Shutdown::Both);
                        let _ = c.shutdown(std::net::Shutdown::Both);
                        break;
                    }
                }
            });
        }
    });
    port
}

#[test]
fn reconnects_and_resumes_after_connection_loss() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("drop.bin");
    random_file(&f, 20_000_000);
    let mut t = b.target();
    t.port = flaky_proxy(b.node.port(), 6_000_000);
    a.node.send(
        t,
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    a.wait(30, |e| matches!(e, Event::Reconnecting { .. }));
    let Event::Finished { ok, bytes, .. } = a.finished() else {
        unreachable!()
    };
    assert!(ok);
    assert!(
        bytes < 20_000_000,
        "second attempt resumed instead of restarting (sent {bytes})"
    );
    assert_eq!(hash(&b.inbox().join("drop.bin")), hash(&f));
}

#[test]
fn conflict_policies() {
    let a = Peer::new("Alice", false);
    let b = Peer::new("Bob", false);
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("c.txt");
    std::fs::write(&f, "new").unwrap();
    std::fs::create_dir_all(b.inbox()).unwrap();
    std::fs::write(b.inbox().join("c.txt"), "old").unwrap();
    for (policy, expect_old, extra) in [
        (Conflict::Skip, "old", false),
        (Conflict::Rename, "old", true),
        (Conflict::Overwrite, "new", true),
    ] {
        let _ = std::fs::remove_file(b.inbox().join("c (1).txt"));
        a.node.send(
            b.target(),
            vec![SendItem::Path {
                path: f.to_string_lossy().into(),
            }],
        );
        let Event::IncomingOffer { transfer_id, .. } =
            b.wait(10, |e| matches!(e, Event::IncomingOffer { .. }))
        else {
            unreachable!()
        };
        b.node.respond(
            &transfer_id,
            Decision {
                accept: true,
                trust: false,
                conflict: policy,
            },
        );
        a.finished();
        b.finished();
        assert_eq!(
            std::fs::read_to_string(b.inbox().join("c.txt")).unwrap(),
            expect_old,
            "{policy:?}"
        );
        assert_eq!(
            b.inbox().join("c (1).txt").exists(),
            extra && policy == Conflict::Rename,
            "{policy:?}"
        );
        std::fs::write(b.inbox().join("c.txt"), "old").unwrap();
    }
}

#[test]
fn pause_resume_and_cancel() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("p.bin");
    random_file(&f, 200_000_000);
    let tid = a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    a.wait(
        10,
        |e| matches!(e, Event::Progress { bytes_done, .. } if *bytes_done > 0),
    );
    a.node.pause(&tid);
    a.wait(10, |e| matches!(e, Event::Progress { paused: true, .. }));
    std::thread::sleep(Duration::from_millis(300));
    a.node.resume(&tid);
    a.wait(10, |e| matches!(e, Event::Progress { paused: false, .. }));
    a.node.cancel(&tid);
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("Cancelled"))
    );
    assert!(matches!(b.finished(), Event::Finished { ok: false, .. }));
    std::thread::sleep(Duration::from_millis(200));
    assert!(!b.inbox().join("p.bin").exists());
    assert!(
        !b.inbox().join(".p.bin.dbpart").exists(),
        "cancel removes partial data"
    );
}

#[test]
fn receiver_cancel_stops_sender() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("q.bin");
    random_file(&f, 300_000_000);
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    let Event::Progress { transfer_id, .. } = b.wait(
        10,
        |e| matches!(e, Event::Progress { bytes_done, .. } if *bytes_done > 0),
    ) else {
        unreachable!()
    };
    b.node.cancel(&transfer_id);
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("receiver cancelled"))
    );
}

#[test]
fn refuses_when_disk_too_small() {
    let (a, b) = pair();
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("huge.img");
    std::fs::File::create(&f).unwrap().set_len(1 << 52).unwrap(); // sparse 4 PB
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: f.to_string_lossy().into(),
        }],
    );
    assert!(
        matches!(a.finished(), Event::Finished { ok: false, error: Some(ref e), .. } if e.contains("Not enough space"))
    );
}

#[test]
fn many_small_files_and_concurrent_senders() {
    let (a, b) = pair();
    let c = Peer::new("Carol", true);
    b.node.trust(&c.node.fingerprint(), "Carol").unwrap();
    let src = tempfile::tempdir().unwrap();
    for i in 0..1000 {
        std::fs::write(
            src.path().join(format!("small/{i:04}.txt")).tap_mkdir(),
            format!("file {i}"),
        )
        .unwrap();
    }
    let big = src.path().join("c.bin");
    random_file(&big, 30_000_000);
    a.node.send(
        b.target(),
        vec![SendItem::Path {
            path: src.path().join("small").to_string_lossy().into(),
        }],
    );
    c.node.send(
        b.target(),
        vec![SendItem::Path {
            path: big.to_string_lossy().into(),
        }],
    );
    let Event::Finished { ok, files_ok, .. } = a.finished() else {
        unreachable!()
    };
    assert!(ok && files_ok == 1001, "1000 files + folder ({files_ok})");
    assert!(ok_(c.finished()));
    assert_eq!(
        std::fs::read_dir(b.inbox().join("small")).unwrap().count(),
        1000
    );
    assert_eq!(hash(&b.inbox().join("c.bin")), hash(&big));
}

fn ok_(e: Event) -> bool {
    ok(&e)
}

trait TapMkdir {
    fn tap_mkdir(self) -> Self;
}
impl TapMkdir for PathBuf {
    fn tap_mkdir(self) -> Self {
        std::fs::create_dir_all(self.parent().unwrap()).unwrap();
        self
    }
}

#[test]
fn blocked_device_is_refused() {
    let (a, b) = pair();
    b.node.block(&a.node.fingerprint(), "Alice").unwrap();
    a.node
        .send(b.target(), vec![SendItem::Text { text: "x".into() }]);
    assert!(matches!(a.finished(), Event::Finished { ok: false, .. }));
}

/// A hand-written hostile client: path traversal must be refused before anything is written.
#[test]
fn malicious_offer_is_refused() {
    let b = Peer::new("Bob", true);
    let id = Identity::generate().unwrap();
    b.node.trust(&id.fingerprint(), "Mallory").unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    for evil in [
        "../escape.txt",
        "/etc/evil",
        "a/../../escape.txt",
        "x\u{0}y",
    ] {
        let reply = rt.block_on(async {
            let tcp = tokio::net::TcpStream::connect(("127.0.0.1", b.node.port()))
                .await
                .unwrap();
            let cfg = dropbridge::tls::client_config(&id).unwrap();
            let name = rustls::pki_types::ServerName::try_from("dropbridge").unwrap();
            let mut s = tokio_rustls::TlsConnector::from(cfg)
                .connect(name, tcp)
                .await
                .unwrap();
            wire::write_msg(
                &mut s,
                &Msg::Hello {
                    name: "Mallory".into(),
                    kind: "cli".into(),
                    version: 1,
                },
            )
            .await
            .unwrap();
            wire::read_msg(&mut s).await.unwrap();
            let item = Item {
                id: 0,
                kind: ItemKind::File,
                path: evil.into(),
                size: 4,
                mtime: None,
                text: None,
            };
            wire::write_msg(
                &mut s,
                &Msg::Offer {
                    transfer_id: "t".into(),
                    items: vec![item],
                    token: None,
                },
            )
            .await
            .unwrap();
            wire::read_msg(&mut s).await.unwrap()
        });
        assert!(
            matches!(
                reply,
                Msg::Answer {
                    accepted: false,
                    ..
                }
            ),
            "{evil:?} → {reply:?}"
        );
    }
    assert!(!b.dir.path().join("escape.txt").exists());
}

#[test]
fn discovery_finds_peer() {
    let a = Peer::with("Alice-disc", false, true);
    let b = Peer::with("Bob-disc", false, true);
    let id = b.node.fingerprint();
    let start = Instant::now();
    a.wait(
        10,
        |e| matches!(e, Event::PeerFound { id: i, .. } if *i == id),
    );
    eprintln!("discovered in {:?}", start.elapsed());
}
