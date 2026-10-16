# Benchmarks

Only measured numbers are listed here. Each row says where it was measured.
Run your own: `cargo run --release -p dropbridge-cli -- bench [--full] [--to <device>]`.

## 1. Engine ceiling (loopback, no radio)

MacBook Air, Apple M5, macOS 27, release build, sender and receiver in one process over 127.0.0.1 with
full TLS 1.3 + BLAKE3. Incompressible data. Measured 2026-10-16.

| Case | Time | Avg | Peak | CPU (both sides) | Max RSS | Failed |
|---|---|---|---|---|---|---|
| 100 MB file | 0.07 s | 1.5 GB/s | 1.5 GB/s | 288 % | 11 MB | 0 |
| 1 GB file | 0.57 s | 1.8 GB/s | 1.8 GB/s | 295 % | 36 MB | 0 |
| 10 GB file | 6.46 s | 1.5 GB/s | 2.0 GB/s | 253 % | 43 MB | 0 |
| 1,000 small files (2.7 MB) | 0.15–0.29 s | 12–21 MB/s | — | 100–200 % | 38 MB | 0 |

Memory stays flat (43 MB at 10 GB), confirming streaming with backpressure.
This is ~10× faster than any Wi-Fi link, so the network, not DropBridge, is the bottleneck in real use.

### Before / after: small-file batches

| Change | 1,000 files |
|---|---|
| Initial: serial finalize + `F_FULLFSYNC` per file | 4.23 s |
| Finalize off the receive loop (parallel) | 3.46 s |
| `fsync` instead of `F_FULLFSYNC` (ADR 008) | **0.15 s** (28× faster) |

## 2. Head-to-head with other tools (same machine)

`scripts/compare.sh` runs each tool between two processes on one Mac over loopback, so the radio is out of
the picture and only each tool's own overhead (encryption, framing, per-file cost) is measured. Apple M5,
macOS 27, 2026-10-16, single run, random data, outputs byte-compared with the source.

| Tool | Encryption | 1 GB file | 1,000 small files |
|---|---|---|---|
| `cp` (disk copy, the ceiling) | none | 0.19 s | 0.17 s |
| **DropBridge 0.1.0** | TLS 1.3 + BLAKE3 verify | **0.55 s** | **0.14 s** |
| HTTPS (`openssl s_server` + `curl`, keep-alive) | TLS 1.3 | 2.29 s | 1.28 s |
| croc 11.5.4 (local relay) | PAKE + AES-GCM | 7.38 s | 11.59 s |

What this does and doesn't show:
- DropBridge adds little over a raw disk copy, so on a real network the link is the bottleneck, not the app.
- The HTTPS row stands in for HTTP-based apps (LocalSend uses HTTPS). `openssl s_server` is a test server,
  not a tuned one, so treat it as a lower bound for HTTPS, not as LocalSend's speed.
- Apps that can't run headless on one machine (AirDrop, Quick Share, LocalSend, KDE Connect) are **not** in
  this table. They need the real-device runs in §4.

## 3. Android emulator ⇄ Mac (virtual network)

Pixel 6 emulator (Android 14, arm64) on the same Mac, through `adb forward` / the emulator NAT.
**This measures the emulator's virtual network, not Wi-Fi.**

| Direction | Size | Avg |
|---|---|---|
| Mac → Android | 50 MB folder + text | 71 MB/s (peak 100 MB/s) |
| Android → Mac | 200 MB file | 15 MB/s (emulator NAT limit) |

Both hashes matched the source.

## 4. Real Wi-Fi (phone ⇄ Mac)

**Not measured yet** — needs a physical phone. Procedure in [MANUAL_TESTS.md](MANUAL_TESTS.md#performance).
**Send yours:** open a [benchmark submission](https://github.com/mdimamhosen/DropBridge/issues/new?template=benchmark.yml);
accepted results are added here and on the website.
Until rows exist here, DropBridge makes no claim about speed relative to AirDrop.

Expected range, for planning only (not results): phone→router→Mac crosses the air twice, so roughly
25–60 MB/s on Wi-Fi 5 and 40–110 MB/s on Wi-Fi 6/6E; phone hotspot direct 30–80 MB/s. AirDrop (direct
AWDL radio) is commonly 30–80 MB/s on recent Apple hardware.

| Date | Phone | Mac | Network | 1 GB phone→Mac | 1 GB Mac→phone | 1,000 files | AirDrop same files |
|---|---|---|---|---|---|---|---|
| | | | | | | | |
