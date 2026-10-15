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

## 2. Android emulator ⇄ Mac (virtual network)

Pixel 6 emulator (Android 14, arm64) on the same Mac, through `adb forward` / the emulator NAT.
**This measures the emulator's virtual network, not Wi-Fi.**

| Direction | Size | Avg |
|---|---|---|
| Mac → Android | 50 MB folder + text | 71 MB/s (peak 100 MB/s) |
| Android → Mac | 200 MB file | 15 MB/s (emulator NAT limit) |

Both hashes matched the source.

## 3. Real Wi-Fi (phone ⇄ Mac)

**Not measured yet** — needs a physical phone. Procedure in [MANUAL_TESTS.md](MANUAL_TESTS.md#performance).
Until rows exist here, DropBridge makes no claim about speed relative to AirDrop.

Expected range, for planning only (not results): phone→router→Mac crosses the air twice, so roughly
25–60 MB/s on Wi-Fi 5 and 40–110 MB/s on Wi-Fi 6/6E; phone hotspot direct 30–80 MB/s. AirDrop (direct
AWDL radio) is commonly 30–80 MB/s on recent Apple hardware.

| Date | Phone | Mac | Network | 1 GB phone→Mac | 1 GB Mac→phone | 1,000 files | AirDrop same files |
|---|---|---|---|---|---|---|---|
| | | | | | | | |
