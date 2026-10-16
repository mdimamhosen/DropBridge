<p align="center"><img src="assets/icon.png" width="96" alt="" /></p>
<h1 align="center">DropBridge</h1>
<p align="center">AirDrop-style file sharing between Android, macOS, Windows and Linux.<br/>
Direct, encrypted, no account, no cloud.</p>
<p align="center"><a href="https://github.com/mdimamhosen/DropBridge/releases/latest"><b>Download</b></a> · <a href="https://mdimamhosen.me/DropBridge/">Website</a> · <a href="docs/ARCHITECTURE.md">How it works</a></p>

---

## What it does
Open DropBridge on two devices on the same Wi-Fi. They find each other. Pick files, a folder, photos or
some text, tap the other device, accept on the other side. Done.

- **Files, folders, photos, videos, text and links**, any size (tested to 10 GB+), folder structure and timestamps kept.
- **Direct and private**: device-to-device over your local network. Nothing is uploaded anywhere. Works without internet.
- **Encrypted** with TLS 1.3; devices verify each other with a matching 6-digit code or a QR scan.
- **Verified**: every file is checked with BLAKE3 before it appears. Corrupted or partial files never show up as finished.
- **Resumes** after Wi-Fi drops, app restarts or reboots, from the last verified byte. Pause, cancel, retry only failed files.
- **Fast**: the engine moves 1.5–2 GB/s on loopback, so your Wi-Fi is the limit ([benchmarks](docs/BENCHMARKS.md)).

## Supported systems
| Platform | Minimum | Install |
|---|---|---|
| Android | 10 | `DropBridge-<version>.apk` |
| macOS | 11 Big Sur (Apple Silicon & Intel) | `.dmg` |
| Windows | 10 / 11 | `.msi` or `-setup.exe` |
| Linux | Ubuntu 22.04+ or similar (WebKitGTK 4.1) | `.AppImage` or `.deb` |
| Command line | macOS / Linux / Windows | `dropbridge-<target>` binaries |

iPhone/iPad: planned (needs its own app; the core is ready for it).

## Install
Download from the [latest release](https://github.com/mdimamhosen/DropBridge/releases/latest) and check the
file against `SHA256SUMS.txt`.

- **Android**: open the APK, allow "Install unknown apps" for your browser/file manager once.
- **macOS**: open the `.dmg`, drag DropBridge to Applications. First launch: right-click → **Open** → **Open**
  (builds aren't notarised yet). Allow **Local Network** access when asked.
- **Windows**: run the installer; if SmartScreen appears choose *More info → Run anyway*. Allow on **Private** networks.
- **Linux**: `chmod +x DropBridge_*.AppImage && ./DropBridge_*.AppImage`, or `sudo apt install ./dropbridge_*.deb`.

## How to use
1. Open DropBridge on both devices (same Wi-Fi, or join the phone's hotspot).
2. **Send**: drop files on a device (desktop), or choose *Files / Photos / Folder / Text* and tap a device (Android).
   From any Android app: **Share → DropBridge**.
3. **Receive**: a request appears with a 6-digit code. If the device is new, check the sender shows the same code,
   then **Accept** (or **Accept & trust** to skip the code next time).

Device not showing? Computer: **Pair a device** shows a QR code. Phone: **Scan QR**. Or use
*Connect by address* with the IP:port and code shown. More in [Troubleshooting](docs/TROUBLESHOOTING.md).

## Permissions, and why
| Permission | Platform | Why |
|---|---|---|
| Local Network | macOS, iOS | find devices and connect to them on your Wi-Fi |
| Firewall (incoming) | macOS, Windows | let the other device connect to receive files |
| Notifications | Android 13+, desktop | incoming requests and progress |
| Camera | Android, only on *Scan QR* | read the pairing QR code |
| Multicast, Wi-Fi state | Android (no prompt) | device discovery |
| Foreground service | Android (no prompt) | keep a transfer going with the screen off |
| Battery optimisation exemption | Android (optional) | some phones stop background transfers otherwise |

DropBridge collects no analytics, has no servers, and never reads anything except the files you choose.

## Architecture in one paragraph
A Rust core (`core/`) does discovery (mDNS), mutual TLS 1.3 with per-device keys, the transfer protocol,
BLAKE3 verification, resume and all path-safety checks. The desktop app is Tauri 2 (one codebase for macOS,
Windows, Linux) calling the core directly; the Android app is Kotlin/Jetpack Compose calling it through
UniFFI. See [Architecture](docs/ARCHITECTURE.md), [Protocol](docs/PROTOCOL.md), [Threat model](docs/THREAT_MODEL.md).

## Build from source
**Prerequisites**
- Rust 1.80+ (`curl https://sh.rustup.rs -sSf | sh`)
- Desktop: `cargo install tauri-cli --locked` (v2). macOS: Xcode command-line tools. Linux:
  `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev pkg-config build-essential`.
  Windows: MSVC build tools + WebView2 (preinstalled on Windows 11).
- Android: JDK 17, Android SDK (platform 35) + NDK 27, `cargo install cargo-ndk --locked`,
  `rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`, `ANDROID_HOME` set.

**Commands**
```bash
git clone https://github.com/mdimamhosen/DropBridge && cd DropBridge
cargo test --workspace                         # tests (unit + end-to-end over TLS)
cargo run -p dropbridge-cli -- receive         # CLI receiver
cargo run -p dropbridge-cli -- send <host:port|name|dropbridge://...> FILES... [--text "hi"]
cd desktop/src-tauri && cargo tauri dev        # desktop app (dev)
cd desktop/src-tauri && cargo tauri build      # desktop installers → target/release/bundle/
scripts/build-android.sh                       # Android debug APK
scripts/build-android.sh release               # Android release APK
```

## Benchmarks
```bash
cargo run --release -p dropbridge-cli -- bench          # loopback: 100 MB, 1 GB, 1,000 files
cargo run --release -p dropbridge-cli -- bench --full   # + 10 GB
dropbridge receive --yes   # on device B (trusted test setup only)
dropbridge bench --to <B's ip:port>                     # real network
```
Results and method: [docs/BENCHMARKS.md](docs/BENCHMARKS.md). We only publish measured numbers.

## Release builds and signing
Pushing a tag `v*` runs `.github/workflows/release.yml`, which builds every installer, the APK and CLI binaries,
and publishes a GitHub Release with `SHA256SUMS.txt`.

- **Android**: create a keystore once
  `keytool -genkeypair -v -keystore dropbridge.jks -alias dropbridge -keyalg RSA -keysize 4096 -validity 10000`,
  then add repo secrets `ANDROID_KEYSTORE_BASE64` (`base64 -i dropbridge.jks`), `ANDROID_KEYSTORE_PASSWORD`,
  `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Without them the APK is signed with a throwaway debug key
  (installable, but future signed versions won't update over it). Keep the keystore safe: losing it means
  users must reinstall.
- **macOS** (optional, Apple Developer Program): secrets `APPLE_CERTIFICATE` (base64 .p12),
  `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (app-specific),
  `APPLE_TEAM_ID`, and pass them as `env:` on the *Build* step of `release.yml` (marked there). Tauri then
  signs and notarises automatically.
- **Windows** (optional): an Authenticode certificate; see Tauri's
  [Windows signing guide](https://v2.tauri.app/distribute/sign/windows/).

## Known limitations
- Real-Wi-Fi speed numbers versus AirDrop are not measured yet; no speed claims until they are.
- Android only receives while the app is open or a transfer is running (Android limits background listeners).
- macOS has no Wi-Fi Direct and AWDL (AirDrop's radio) is private to Apple, so without a shared network use the phone's hotspot.
- Desktop builds aren't code-signed yet (one-time Gatekeeper/SmartScreen step; macOS may ask for Keychain access after updates).
- Android 11+ can't see files other apps put in `Download/DropBridge`, so a name clash with such a file fails that one file ("Can't save file"); retry with "Keep both" after renaming.
- No iOS app, no internet relay (by design for v1).

## Contributing
See [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports with OS versions and steps are very welcome.

## License
[Apache-2.0](LICENSE)
