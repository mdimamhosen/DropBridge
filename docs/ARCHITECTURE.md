# Architecture

DropBridge is one Rust core with two thin user interfaces.

```
            ┌──────────── dropbridge-core (Rust) ────────────┐
            │ discovery (mDNS)  · identity + TLS 1.3 (rustls) │
            │ wire protocol     · transfer engine + resume    │
            │ path safety       · trust store                 │
            └───────┬───────────────────┬───────────────────┬─┘
                    │ direct            │ UniFFI (JNI)      │ direct
        desktop/ (Tauri 2)        android/ (Compose)       cli/
        macOS · Windows · Linux   Android 10+              tests, bench, headless
```

All protocol, crypto, hashing and file-safety logic lives in `core/`, so it is written once and tested once
(`core/tests/e2e.rs` runs real nodes against each other over TLS). The UIs only render events and call a
small API (`send`, `respond`, `pause/resume/cancel/retry`, `pairing_uri`, `trust/block/forget`).

## Transfer lifecycle

1. **Discovery** — every device advertises `_dropbridge._tcp` over mDNS with TXT `id` (certificate
   fingerprint), `n` (name), `k` (kind). Fallbacks: QR link, 6-digit code + address, manual `host:port`.
2. **Connect** — TCP, then mutual TLS 1.3 with each device's self-signed P-256 certificate.
3. **Identify** — each side computes the peer fingerprint (SHA-256 of its certificate) and a 6-digit
   verification code from the TLS exporter. Both screens show the same code only if nobody is in the middle.
4. **Offer** — sender lists items (files, folders, text) with relative paths, sizes, mtimes.
5. **Approve** — receiver validates every path, checks free space, then asks the user (unless the sender is
   trusted and auto-accept is on). The answer includes, per file, how many bytes it already has from an
   interrupted attempt plus their BLAKE3 hash.
6. **Stream** — files go one after another on the same connection (no per-file round trips). Each file is
   read and hashed on a blocking thread, encrypted and sent by the network task, written and hashed on a
   blocking thread at the receiver. Bounded channels (8 × 1 MiB) give backpressure, so memory stays flat.
7. **Verify** — sender sends the whole-file BLAKE3; receiver compares, flushes, and only then renames
   `.name.dbpart` to the final name. Incomplete data never looks like a finished file.
8. **Recover** — on connection loss the sender reconnects for up to 2 minutes and re-offers only what is
   not yet verified; the receiver reports its partial bytes and the transfer continues from there.

## Decisions (ADRs)

Each row is an architecture decision record (context → decision → reason). Change one only with new evidence and update this table.

| # | Decision | Why |
|---|---|---|
| 001 | Rust core + Tauri desktop + native Android | One implementation of security-critical code; native share sheet / foreground service on Android; one desktop codebase for 3 OSes |
| 002 | TCP + TLS 1.3 (rustls, ring) rather than QUIC | Hardware AES on ARMv8/Apple Silicon, kernel TCP is cheaper on phones than userspace UDP crypto; LAN has no head-of-line problem worth QUIC |
| 003 | Self-signed identities, trust by fingerprint, SAS codes | No accounts or CA; MITM visible via matching codes; QR pins the fingerprint |
| 004 | BLAKE3 whole-file hash + prefix hash for resume | 1–3 GB/s hashing, no per-chunk metadata; resume validates the exact bytes already on disk |
| 005 | mDNS via `mdns-sd` on all platforms (Android too, with a MulticastLock) | One discovery implementation; Bonjour-compatible. Fallback if a phone filters multicast: QR/code/IP |
| 006 | Single stream per transfer, parallel streams not used | Loopback benchmark shows the single stream at 1.5–2 GB/s, far above any Wi-Fi link; revisit only if real Wi-Fi 6E/7 measurements show a ceiling |
| 007 | No compression in v1 | Phone media is already compressed; zstd only helps on links slower than its own throughput, which modern Wi-Fi is not. Revisit with measurements |
| 008 | `fsync` instead of `F_FULLFSYNC` on Apple | Full drive-cache flush per file made 1,000-file batches 28× slower; the resume prefix-hash check catches power-loss damage |
| 009 | Events cross the FFI as JSON | Avoids mirroring every event type in UniFFI; Kotlin parses with `org.json` |

## Directory map

| Path | What |
|---|---|
| `core/src/node.rs` | Node API, send/receive state machines, resume, events |
| `core/src/wire.rs` | Frame format and protocol messages |
| `core/src/tls.rs` | rustls configs, fingerprint + SAS |
| `core/src/paths.rs` | Filename/path sanitising, symlink checks, conflict names |
| `core/src/discovery.rs` | mDNS advertise/browse |
| `core/src/trust.rs` | Trusted/blocked devices |
| `ffi/` | UniFFI bindings used by Android |
| `desktop/` | Tauri app: `src-tauri/src/main.rs` + `ui/` (plain HTML/JS) |
| `android/` | Kotlin/Compose app |
| `cli/` | `dropbridge` command line: send/receive/peers/bench |
| `site/` | Public web page (GitHub Pages) |
