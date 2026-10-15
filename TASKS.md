# Task list

Legend: [x] done and verified · [~] done, needs real-device verification · [ ] open

## Phase 1 — Architecture
- [x] Stack, protocol, security model, ADRs → docs/ARCHITECTURE.md, PROTOCOL.md, THREAT_MODEL.md

## Phase 2 — Prototype
- [x] Rust core: identity, mutual TLS 1.3, SAS codes, mDNS, send/receive, BLAKE3 verify
- [x] CLI (send/receive/peers/bench)
- [x] Desktop app (Tauri) receives from CLI over TLS (file + text, history, Keychain identity)
- [x] Android app on emulator: Mac→Android (folder + Unicode + text) and Android→Mac (200 MB), hashes match
- [~] Discovery between phone and Mac over real Wi-Fi (works CLI↔CLI on one host, ~1 s)

## Phase 3 — Core product
- [x] Multi-file, folders (empty dirs, hidden, Unicode), text/links
- [x] Desktop drag-and-drop (onto device = send, onto area = stage), tray/menu bar, notifications
- [x] Android share sheet (files + text), file/photo/folder pickers, QR scan, pairing code display
- [x] History, pause/resume/cancel (both sides), retry only failed, conflict rename/overwrite/skip
- [x] Trusted devices, forget/block, QR + 6-digit code + manual IP
- [x] First-run guide (Android), battery-optimisation warning
- [ ] Finder Services menu / Windows "Send to" shortcut (command-line paths already accepted)
- [ ] In-app Android LocalOnlyHotspot (system hotspot works today)
- [ ] Desktop: change device name in Settings (uses the OS computer name)

## Phase 4 — Performance
- [x] Benchmark tool; loopback numbers; small-file fix (4.23 s → 0.15 s)
- [~] Real Wi-Fi measurements vs AirDrop (needs phone) → BENCHMARKS.md §3
- [ ] Parallel streams / compression: only if Wi-Fi measurements show a need (ADR 006/007)

## Phase 5 — Hardening
- [x] Tests: traversal/malicious offer, impersonation, wrong/expired codes, blocked device, resume (partial,
      stale partial, connection cut by proxy), cancel both sides, low disk, 1,000 files + concurrent senders
- [x] 10 GB file benchmark with flat memory
- [~] Permission denial, Doze/OEM killers, flaky Wi-Fi → docs/MANUAL_TESTS.md

## Phase 6 — Packaging & deploy
- [x] README, CONTRIBUTING, LICENSE, troubleshooting, manual tests
- [x] CI, release (dmg/msi/exe/deb/AppImage/APK/CLI + SHA256SUMS), GitHub Pages site
- [ ] Publish repo + first release (needs owner go-ahead)
- [ ] Android release keystore (owner), macOS notarisation / Windows signing (paid certificates, optional)
