//! dropbridge CLI: receive, send, list peers, benchmark.
use anyhow::{bail, Context, Result};
use dropbridge::*;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const USAGE: &str = "dropbridge — fast encrypted local file transfer

USAGE
  dropbridge receive [--dir DIR] [--port N] [--yes]          wait for files (prints a pairing code)
  dropbridge send <TARGET> [PATH...] [--text TEXT] [--code C] send files/folders/text
  dropbridge peers                                            list nearby devices (3 s scan)
  dropbridge bench [--full] [--to TARGET]                     measure throughput

TARGET is a device name from `peers`, host:port, or a dropbridge://pair?... link.
--yes accepts every offer (only for trusted test setups).";

struct Chan(Mutex<Sender<Event>>);
impl Delegate for Chan {
    fn on_event(&self, e: Event) {
        let _ = self.0.lock().unwrap().send(e);
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default())
}

fn start(port: u16, dir: Option<String>, discovery: bool, auto: bool) -> Result<(Node, Receiver<Event>)> {
    start_as(".dropbridge-cli", port, dir, discovery, auto)
}

fn start_as(profile: &str, port: u16, dir: Option<String>, discovery: bool, auto: bool) -> Result<(Node, Receiver<Event>)> {
    let data = home().join(profile);
    std::fs::create_dir_all(&data)?;
    let idf = data.join("identity");
    let identity = std::fs::read_to_string(&idf).unwrap_or_default();
    let (tx, rx) = channel();
    let download = dir.unwrap_or_else(|| home().join("Downloads/DropBridge").to_string_lossy().into());
    let cfg = NodeConfig {
        name: hostname(),
        kind: "cli".into(),
        identity,
        data_dir: data.to_string_lossy().into(),
        download_dir: download,
        port,
        discovery,
        auto_accept_trusted: auto,
    };
    let node = Node::new(cfg, Arc::new(Chan(Mutex::new(tx))))?;
    if !idf.exists() {
        std::fs::write(&idf, node.identity_secret())?;
        #[cfg(unix)]
        std::fs::set_permissions(&idf, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    }
    Ok((node, rx))
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_end_matches(".local").to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "DropBridge CLI".into())
}

fn flag(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn has(args: &mut Vec<String>, name: &str) -> bool {
    args.iter().position(|a| a == name).map(|i| args.remove(i)).is_some()
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = if args.is_empty() { String::new() } else { args.remove(0) };
    let r = match cmd.as_str() {
        "receive" => receive(args),
        "send" => send(args),
        "peers" => peers().map(|_| ()),
        "bench" => bench(args),
        _ => {
            println!("{USAGE}");
            return;
        }
    };
    if let Err(e) = r {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn print_progress(e: &Event) {
    if let Event::Progress { bytes_done, bytes_total, speed_bps, avg_bps, eta_secs, files_done, files_total, paused, .. } = e {
        let pct = if *bytes_total > 0 { *bytes_done as f64 * 100.0 / *bytes_total as f64 } else { 100.0 };
        print!(
            "\r  {pct:5.1}%  {} / {}  {}/s (avg {}/s)  files {files_done}/{files_total}  ETA {eta_secs}s{}   ",
            human(*bytes_done),
            human(*bytes_total),
            human(*speed_bps),
            human(*avg_bps),
            if *paused { "  [paused]" } else { "" }
        );
        let _ = std::io::stdout().flush();
    }
}

fn print_finished(e: &Event) -> bool {
    let Event::Finished { ok, error, files_ok, failed, bytes, secs, avg_bps, peak_bps, .. } = e else {
        return false;
    };
    println!();
    if *ok {
        println!("✓ done: {files_ok} item(s), {} in {secs:.1}s — avg {}/s, peak {}/s", human(*bytes), human(*avg_bps), human(*peak_bps));
    } else {
        println!("✗ {}", error.clone().unwrap_or_else(|| "some files failed".into()));
        for f in failed {
            println!("  failed: {f}");
        }
    }
    *ok
}

fn receive(mut args: Vec<String>) -> Result<()> {
    let dir = flag(&mut args, "--dir");
    let port = flag(&mut args, "--port").map(|p| p.parse()).transpose()?.unwrap_or(0);
    let yes = has(&mut args, "--yes");
    let (node, rx) = start(port, dir, true, true)?;
    println!("Receiving as \"{}\" on port {} (addresses: {})", hostname(), node.port(), local_ips().join(", "));
    println!("Pairing link (valid 5 min): {}", node.pairing_uri());
    println!("Pairing code: {}", node.pairing_code());
    let stdin = std::io::stdin();
    for e in rx {
        match &e {
            Event::IncomingOffer { transfer_id, from_name, from_id, trusted, sas, files, bytes, texts, preview, .. } => {
                let extra = if *texts > 0 { format!(" + {texts} text") } else { String::new() };
                println!(
                    "\n{} \"{from_name}\" wants to send {files} file(s), {}{extra}",
                    if *trusted { "Trusted" } else { "UNKNOWN device" },
                    human(*bytes)
                );
                for p in preview {
                    println!("  · {p}");
                }
                println!("  Verification code: {sas} (must match the sender's screen)");
                let d = if yes {
                    Decision { accept: true, trust: false, conflict: Conflict::Rename }
                } else {
                    print!("  Accept? [y]es / [t]rust & accept / [N]o: ");
                    std::io::stdout().flush()?;
                    let mut l = String::new();
                    stdin.lock().read_line(&mut l)?;
                    let a = l.trim().to_lowercase();
                    Decision { accept: a == "y" || a == "t", trust: a == "t", conflict: Conflict::Rename }
                };
                if d.trust {
                    println!("  Trusted {from_name} ({}…)", &from_id[..12]);
                }
                node.respond(transfer_id, d);
            }
            Event::TextReceived { from_name, text, .. } => println!("\nText from {from_name}:\n{text}"),
            Event::ItemDone { path, ok: false, error, .. } => println!("\n  ✗ {path}: {}", error.clone().unwrap_or_default()),
            Event::Finished { .. } => {
                print_finished(&e);
            }
            _ => print_progress(&e),
        }
    }
    Ok(())
}

/// A device name from discovery, host:port, or a pairing link.
fn resolve_target(t: &str) -> Result<Target> {
    if t.starts_with("dropbridge://") {
        return parse_pairing_uri(t);
    }
    if let Some((h, p)) = t.rsplit_once(':') {
        if let Ok(port) = p.parse() {
            return Ok(Target { host: h.into(), port, fingerprint: None, token: None });
        }
    }
    let p = peers()?.into_iter().find(|p| p.1.eq_ignore_ascii_case(t)).with_context(|| format!("No nearby device named {t:?}"))?;
    Ok(Target { host: p.2.join(","), port: p.3, fingerprint: Some(p.0), token: None })
}

fn send(mut args: Vec<String>) -> Result<()> {
    let text = flag(&mut args, "--text");
    let code = flag(&mut args, "--code");
    if args.is_empty() {
        bail!("{USAGE}");
    }
    let mut target = resolve_target(&args.remove(0))?;
    if code.is_some() {
        target.token = code;
    }
    let mut items: Vec<SendItem> = args.into_iter().map(|p| SendItem::Path { path: p }).collect();
    items.extend(text.map(|text| SendItem::Text { text }));
    if items.is_empty() {
        bail!("nothing to send");
    }
    let (node, rx) = start(0, None, false, false)?;
    node.send(target, items);
    for e in rx {
        match &e {
            Event::AwaitingApproval { to_name, sas, .. } => {
                println!("Waiting for {to_name} to accept… verification code {sas}")
            }
            Event::Reconnecting { attempt, error, .. } => {
                println!("\n  reconnecting (attempt {attempt}): {error}")
            }
            Event::Finished { .. } => {
                if !print_finished(&e) {
                    std::process::exit(1);
                }
                return Ok(());
            }
            _ => print_progress(&e),
        }
    }
    Ok(())
}

/// (id, name, addrs, port)
type Found = (String, String, Vec<String>, u16);

fn peers() -> Result<Vec<Found>> {
    let (_node, rx) = start(0, Some(std::env::temp_dir().join("dropbridge-scan").to_string_lossy().into()), true, false)?;
    let end = Instant::now() + Duration::from_secs(3);
    let mut out = Vec::new();
    while let Ok(e) = rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
        if let Event::PeerFound { id, name, kind, addrs, port, trusted } = e {
            println!("{name:24} {kind:8} {}:{port} {}", addrs.first().cloned().unwrap_or_default(), if trusted { "trusted" } else { "" });
            out.push((id, name, addrs, port));
        }
    }
    if out.is_empty() {
        println!("No devices found. Make sure DropBridge is open on the other device and both are on the same Wi-Fi.");
    }
    Ok(out)
}

// ───────────────────────────── benchmark ─────────────────────────────

/// (cpu seconds, max RSS MB) for this process.
fn usage() -> (f64, f64) {
    #[cfg(unix)]
    unsafe {
        let mut u: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut u);
        let t = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1e6;
        // ru_maxrss is bytes on macOS, KiB on Linux.
        let rss = if cfg!(target_os = "macos") { u.ru_maxrss as f64 / 1048576.0 } else { u.ru_maxrss as f64 / 1024.0 };
        (t(u.ru_utime) + t(u.ru_stime), rss)
    }
    #[cfg(not(unix))]
    (0.0, 0.0)
}

fn fixture(dir: &Path, name: &str, size: u64) -> Result<PathBuf> {
    let p = dir.join(name);
    if std::fs::metadata(&p).map(|m| m.len()).ok() == Some(size) {
        return Ok(p);
    }
    // Incompressible pseudo-random data.
    let mut f = std::io::BufWriter::new(std::fs::File::create(&p)?);
    let mut x: u64 = 0x9E3779B97F4A7C15;
    let mut buf = vec![0u8; 1 << 20];
    let mut left = size;
    while left > 0 {
        for c in buf.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            c.copy_from_slice(&x.to_le_bytes()[..c.len()]);
        }
        let n = left.min(buf.len() as u64) as usize;
        f.write_all(&buf[..n])?;
        left -= n as u64;
    }
    Ok(p)
}

fn small_files(dir: &Path, n: usize) -> Result<PathBuf> {
    let p = dir.join(format!("small-{n}"));
    if !p.exists() {
        std::fs::create_dir_all(&p)?;
        for i in 0..n {
            std::fs::write(p.join(format!("f{i:05}.txt")), format!("small file {i} ").repeat(1 + i % 400))?;
        }
    }
    Ok(p)
}

fn bench(mut args: Vec<String>) -> Result<()> {
    let full = has(&mut args, "--full");
    let to = flag(&mut args, "--to");
    let fx = std::env::temp_dir().join("dropbridge-bench");
    std::fs::create_dir_all(&fx)?;
    let mut cases = vec![
        ("100 MB file", fixture(&fx, "100MB.bin", 100_000_000)?),
        ("1 GB file", fixture(&fx, "1GB.bin", 1_000_000_000)?),
        ("1,000 small files", small_files(&fx, 1000)?),
    ];
    if full {
        cases.push(("10 GB file", fixture(&fx, "10GB.bin", 10_000_000_000)?));
    }
    // Loopback by default (engine ceiling, no radio involved); --to measures a real link.
    let inbox = std::env::temp_dir().join("dropbridge-bench-in");
    let (recv, target) = match &to {
        Some(t) => (None, resolve_target(t)?),
        None => {
            let (n, rx) = start_as(".dropbridge-cli-bench", 0, Some(inbox.to_string_lossy().into()), false, true)?;
            let t = Target { host: "127.0.0.1".into(), port: n.port(), fingerprint: Some(n.fingerprint()), token: None };
            (Some((n, rx)), t)
        }
    };
    let (sender, srx) = start(0, None, false, false)?;
    if let Some((n, _)) = &recv {
        n.trust(&sender.fingerprint(), "bench")?;
    }
    println!("| case | time | avg | peak | CPU (this process) | max RSS | failed |");
    println!("|---|---|---|---|---|---|---|");
    for (name, path) in cases {
        let _ = std::fs::remove_dir_all(&inbox); // keep disk use flat across cases
        let (c0, _) = usage();
        let t0 = Instant::now();
        sender.send(target.clone(), vec![SendItem::Path { path: path.to_string_lossy().into() }]);
        let fin = loop {
            if let e @ Event::Finished { .. } = srx.recv()? {
                break e;
            }
        };
        let wall = t0.elapsed().as_secs_f64();
        let (c1, rss) = usage();
        let Event::Finished { ok, error, avg_bps, peak_bps, failed, .. } = fin else { unreachable!() };
        if !ok {
            bail!("{name}: {}", error.unwrap_or_default());
        }
        println!(
            "| {name} | {wall:.2} s | {}/s | {}/s | {:.0}% | {rss:.0} MB | {} |",
            human(avg_bps),
            human(peak_bps),
            (c1 - c0) / wall * 100.0,
            failed.len()
        );
    }
    let _ = std::fs::remove_dir_all(&inbox);
    Ok(())
}
