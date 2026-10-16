<p align="center"><img src="assets/icon.png" width="96" alt="" /></p>
<h1 align="center">DropBridge</h1>
<p align="center"><b>AirDrop for Android and every computer.</b><br/>
Send files, folders, photos, videos and text between Android, macOS, Windows and Linux.<br/>
Direct over your Wi-Fi · end-to-end encrypted · no account · no cloud.</p>

<p align="center">
  <a href="https://mdimamhosen.me/DropBridge/"><b>Website</b></a> ·
  <a href="https://github.com/mdimamhosen/DropBridge/releases/latest"><b>Download</b></a> ·
  <a href="#install">Install</a> ·
  <a href="#how-to-use">How to use</a> ·
  <a href="docs/BENCHMARKS.md">Benchmarks</a> ·
  <a href="CONTRIBUTING.md">Contribute</a>
</p>

<p align="center">
  <a href="https://github.com/mdimamhosen/DropBridge/actions/workflows/ci.yml"><img src="https://github.com/mdimamhosen/DropBridge/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/mdimamhosen/DropBridge/releases/latest"><img src="https://img.shields.io/github/v/release/mdimamhosen/DropBridge" alt="Latest release" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License" /></a>
</p>

---

## Download

| Platform | Download | Requirements |
|---|---|---|
| **Android** | [DropBridge-Android.apk](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-Android.apk) | Android 10+ |
| **macOS** (M1–M5) | [DropBridge-macOS-AppleSilicon.dmg](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-macOS-AppleSilicon.dmg) | macOS 11+ |
| **macOS** (Intel) | [DropBridge-macOS-Intel.dmg](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-macOS-Intel.dmg) | macOS 11+ |
| **Windows** | [Installer (.msi)](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-Windows-x64.msi) · [Setup (.exe)](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-Windows-x64-setup.exe) | Windows 10 / 11 |
| **Linux** | [.deb](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-Linux-amd64.deb) · [AppImage](https://github.com/mdimamhosen/DropBridge/releases/latest/download/DropBridge-Linux-x86_64.AppImage) | Ubuntu 22.04+ or similar |
| **Command line** | [all builds](https://github.com/mdimamhosen/DropBridge/releases/latest) | macOS · Linux · Windows |

Every file is listed with its SHA-256 in `SHA256SUMS.txt` on the [release page](https://github.com/mdimamhosen/DropBridge/releases/latest).

## Install

<details open>
<summary><b>Android</b></summary>

1. Download the APK on your phone and open it.
2. If asked, allow your browser or file manager to **install unknown apps** (one time), then tap **Install**.
3. Open DropBridge, tap **Continue** and allow notifications.
</details>

<details open>
<summary><b>macOS</b></summary>

1. Download the `.dmg` for your Mac (Apple menu → About This Mac: "Apple M…" = Apple Silicon, "Intel" = Intel), open it and drag **DropBridge** into **Applications**.
2. Open DropBridge. macOS says it can't verify the developer — click **Done**.
3. Go to **System Settings → Privacy & Security**, scroll down, click **Open Anyway** next to DropBridge and confirm.
   On macOS 14 or older: right-click the app → **Open** → **Open**.
4. Allow **Local Network** access when asked, so it can find your phone.

Or do steps 2–3 in Terminal:
```bash
xattr -dr com.apple.quarantine /Applications/DropBridge.app
```
This one-time step is needed because builds aren't notarised by Apple yet.
</details>

<details open>
<summary><b>Windows</b></summary>

1. Run the `.msi` (or `-setup.exe`).
2. If SmartScreen appears: **More info → Run anyway** (the installer isn't code-signed yet).
3. Open DropBridge and click **Allow** when Windows Firewall asks, with **Private networks** ticked.
</details>

<details open>
<summary><b>Linux</b></summary>

```bash
# Ubuntu / Debian
sudo apt install ./DropBridge-Linux-amd64.deb

# Any distribution
chmod +x DropBridge-Linux-x86_64.AppImage && ./DropBridge-Linux-x86_64.AppImage
```
If you use a firewall (e.g. `ufw`), allow DropBridge on your local network.
</details>

<details>
<summary><b>Command line</b> (any OS, also good for servers)</summary>

```bash
# macOS / Linux: download the binary for your system from the release page, then
chmod +x dropbridge-* && sudo mv dropbridge-* /usr/local/bin/dropbridge
dropbridge receive                                  # wait for files, prints a pairing code
dropbridge send "MacBook Air" photo.jpg Videos/     # by device name
dropbridge send 192.168.1.20:52345 notes.pdf --text "here you go"
dropbridge peers                                    # list nearby devices
```
</details>

## How to use

1. **Open DropBridge on both devices.** They must be on the same Wi-Fi — or connect the computer to your phone's hotspot. Devices appear automatically in about a second.
2. **Send.**
   - *Computer:* drag files or folders onto a device (sends right away), or drop them in the window and then click a device. **Send text** sends a link or note.
   - *Android:* tap **Files**, **Photos & videos**, **Folder** or **Text**, then tap a device. From any app: **Share → DropBridge**.
3. **Accept on the other device.** A new device shows a 6-digit code on both screens — check they match, then **Accept** (or **Accept & trust** to skip the check next time).

Device not showing up? On the computer click **Pair a device** to show a QR code, then tap **Scan QR** on the phone. Or use **Connect manually / Connect by address** with the address and code shown. See [Troubleshooting](docs/TROUBLESHOOTING.md).

Received files go to your **Downloads** folder (desktop, changeable in Settings) or **Download/DropBridge** (Android).

## Features

- **Any size**: streams files instead of loading them into memory (a 10 GB file used a flat ~43 MB).
- **Folders kept intact**: structure, empty folders, hidden files, Unicode names and timestamps.
- **Every file verified** with BLAKE3 before it appears; partial data stays hidden.
- **Resumes** after Wi-Fi drops, app restarts or reboots, from the last verified byte. **Retry** sends only what failed.
- **Pause, resume, cancel** from either side; keep-both / replace / skip for duplicates; transfer history.
- **Text and links** with one-click copy.
- **Native feel**: Android share sheet and photo picker; desktop drag-and-drop, menu bar/tray, notifications, dark mode, keyboard and screen-reader support.

## Security and privacy

- Every connection uses **TLS 1.3**, with a unique key per device stored in the OS keychain (Keychain, Credential Manager, Secret Service, Android Keystore).
- **No man-in-the-middle**: both screens show a 6-digit code derived from the encrypted session; QR pairing pins the device identity.
- **Nothing without your approval**: unknown devices always ask; block or forget devices any time.
- Path-traversal and malicious-filename protection, free-space checks, and replay protection.
- **No accounts, no servers, no analytics.** Details: [Threat model](docs/THREAT_MODEL.md).

## Performance

Same Mac, each tool sending between two processes (this measures each tool's own overhead, with the radio taken out):

| Tool | 1 GB file | 1,000 small files |
|---|---|---|
| **DropBridge** | **0.55 s** | **0.14 s** |
| HTTPS (`openssl` + `curl`) | 2.29 s | 1.28 s |
| croc 11.5.4 | 7.38 s | 11.59 s |
| Disk copy (`cp`, the ceiling) | 0.19 s | 0.17 s |

Over Wi-Fi your network sets the speed. Phone ⇄ computer comparisons against AirDrop, Quick Share and LocalSend need real devices — **[submit your results](https://github.com/mdimamhosen/DropBridge/issues/new?template=benchmark.yml)**. Method and all numbers: [docs/BENCHMARKS.md](docs/BENCHMARKS.md). Reproduce with `scripts/compare.sh`.

## Permissions

| Permission | Where | Why |
|---|---|---|
| Local Network | macOS | find and connect to your devices on Wi-Fi |
| Firewall (incoming) | macOS, Windows | let the other device connect to send you files |
| Notifications | Android 13+, desktop | incoming requests and progress |
| Camera | Android, only for **Scan QR** | read the pairing QR code |
| Foreground service | Android (no prompt) | keep a transfer going with the screen off |
| Battery optimisation exemption | Android (optional) | some phones pause background transfers otherwise |

## Known limitations

- macOS and Windows builds aren't notarised / code-signed yet, hence the one-time step during install.
- Android receives while the app is open or a transfer is running (Android limits background listeners).
- macOS has no Wi-Fi Direct and AirDrop's radio is private to Apple, so without a shared network use the phone's hotspot.
- No iPhone app yet; no internet relay (local transfers only, by design).

## How it works

A Rust core (`core/`) handles discovery (mDNS), mutual TLS 1.3, the transfer protocol, BLAKE3 verification, resume and path safety. The desktop app is Tauri 2 (one codebase for macOS, Windows, Linux); the Android app is Kotlin + Jetpack Compose using the same core through UniFFI.
More: [Architecture](docs/ARCHITECTURE.md) · [Protocol](docs/PROTOCOL.md) · [Threat model](docs/THREAT_MODEL.md).

```
core/      Rust engine: protocol, TLS, transfers, resume, discovery
cli/       dropbridge command line (send / receive / peers / bench)
desktop/   Tauri app for macOS, Windows, Linux
android/   Android app (Kotlin, Compose)
ffi/       UniFFI bindings used by Android
site/      website
docs/      architecture, protocol, threat model, benchmarks, troubleshooting
```

## Build from source

**Prerequisites**
- Rust (stable): `curl https://sh.rustup.rs -sSf | sh`
- Desktop: `cargo install tauri-cli --version 2.12.3 --locked`
  - macOS: Xcode command-line tools
  - Linux: `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev pkg-config build-essential`
  - Windows: MSVC build tools + WebView2 (built into Windows 11)
- Android: JDK 17, Android SDK (platform 35) + NDK 27, `cargo install cargo-ndk --locked`,
  `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`, and `ANDROID_HOME` set.

**Commands**
```bash
git clone https://github.com/mdimamhosen/DropBridge && cd DropBridge
cargo test --workspace                      # unit + end-to-end tests over real TLS
cargo run -p dropbridge-cli -- receive      # CLI receiver
cd desktop/src-tauri && cargo tauri dev     # desktop app with live reload
cd desktop/src-tauri && cargo tauri build   # installers → target/release/bundle/
scripts/build-android.sh                    # Android debug APK
cargo run --release -p dropbridge-cli -- bench   # benchmark (add --full for 10 GB)
```

**Releases**: pushing a tag `v*` builds every installer, the APK and CLI binaries in GitHub Actions and publishes them with `SHA256SUMS.txt`. Android signing uses the repo secrets `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. For notarised macOS builds, add `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` as `env:` on the *Build* step of `release.yml`.

## Contributing

Bug reports, device test results and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).
Found a security issue? Please report it privately (details in CONTRIBUTING.md).

## License

[Apache-2.0](LICENSE) © Md Imam Hosen
