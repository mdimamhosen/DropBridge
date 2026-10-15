# Manual test checklist (needs real devices)

Automated coverage: `cargo test --workspace` (protocol, pairing, resume, security, conflicts, cancel, disk
space, concurrency, discovery on one host). The items below need hardware or OS UI. Record results with
date, device models and OS versions.

## Setup
- Android phone (Android 10+) with the APK installed; Mac with DropBridge.app; same Wi-Fi.
- Wireless debugging (optional, for logs): phone Settings → Developer options → Wireless debugging → `adb pair <ip:port>`.

## Discovery & pairing
- [ ] Open both apps: each lists the other within 2 s.
- [ ] Mac: *Pair a device*; phone: *Scan QR* → phone shows "Paired"; Mac toast "Paired with …"; both list the other as Trusted.
- [ ] Pairing by code: phone *More → Connect by address* with Mac IP:port + 6-digit code → paired.
- [ ] Wrong code 5× → codes invalidated; new code needed.
- [ ] Guest network that blocks multicast: QR/address still works.

## Transfers (both directions)
- [ ] Single photo from phone *Photos & videos*; verify it opens on the Mac.
- [ ] Folder with subfolders, an empty folder, hidden file, Unicode/emoji names → structure preserved.
- [ ] Share sheet: Gallery → Share → DropBridge → tap Mac.
- [ ] Share text from a browser (URL) → Mac shows text dialog, Copy works.
- [ ] Mac: drag files onto a device tile → sends immediately. Drag onto drop area → staged.
- [ ] Unknown device: approval shows the same code on both; Decline leaves nothing on disk.
- [ ] Conflict: send the same file twice → "Keep both" creates `name (1).ext`; Replace overwrites; Skip leaves original.
- [ ] Pause/Resume/Cancel from either side; cancelled partial disappears.
- [ ] Turn Wi-Fi off on the phone for 10 s during a 2 GB transfer → resumes, hash matches.
- [ ] Kill the app mid-transfer, reopen, Retry → continues from the partial.
- [ ] Phone screen off during a 5 GB transfer → completes (foreground notification visible).
- [ ] Receiver with less free space than the offer → clear "Not enough space" message, nothing written.
- [ ] > 10 GB file Mac → phone (if storage allows) → completes, memory of both apps stays flat.
- [ ] Deny notification permission → app still works; incoming requests shown when app is open.
- [ ] Deny camera → Scan QR explains, manual code still works.
- [ ] Hotspot: phone hotspot on, Mac joins it, transfer works.

## Accessibility
- [ ] Mac: Tab through every control; Enter/Space activate; Esc closes dialogs; VoiceOver reads device tiles ("Send to …").
- [ ] Android: TalkBack reads device cards ("Send to …") and progress; font size 200 % still usable.
- [ ] Light and dark mode on both.

## Performance
Use 1 GB, 100 MB and the 1,000-file set from `dropbridge bench` (fixtures in the temp folder), or any files.
1. Phone → Mac and Mac → phone with DropBridge; note avg/peak from the finished card.
2. Same files with AirDrop (Mac ⇄ iPhone) on the same network/room for reference.
3. Note Wi-Fi standard/band and whether the Mac is wired. Add a row to `BENCHMARKS.md` §3.
