# Troubleshooting

**The other device doesn't appear**
1. Both devices on the same Wi-Fi (not a guest network: guest/hotel/office networks often block devices from seeing each other).
2. DropBridge is open on both. On Android keep it open the first time.
3. macOS: *System Settings → Privacy & Security → Local Network* → DropBridge on. Also allow it when the firewall asks.
4. Windows: allow DropBridge on **Private** networks when the firewall asks; make sure your Wi-Fi is set to *Private*.
5. VPN on either device? Turn it off or allow local network access.
6. Still nothing: use **Pair a device** on the computer and **Scan QR** on the phone, or *Connect by address* with the IP:port shown.

**"Couldn't connect" / "didn't answer"** — the address is wrong or a firewall blocks it. The address and port
are shown under the app title and in *Pair a device*; they change when you restart the app.

**The codes don't match** — decline. Someone may be intercepting the connection. Move to a network you trust.

**"This device's identity doesn't match"** — the device you paired with was reset/reinstalled, or someone is
impersonating it. If you reinstalled it: Settings → Trusted devices → Forget, then pair again.

**Transfer is slow**
- Use 5 GHz / Wi-Fi 6 rather than 2.4 GHz; get closer to the router.
- Phone → router → computer crosses the air twice. Plugging the computer into the router with Ethernet often doubles speed.
- No router? Turn on the phone's hotspot (5 GHz if offered), join it from the computer, then send.
- Disable VPNs; they route local traffic through a slower path.

**Transfer stops when the phone screen turns off** — tap *Allow* on the battery banner (or Settings →
Battery optimisation → DropBridge → Don't optimise). Some brands (Xiaomi, Huawei, Samsung "deep sleep") need
DropBridge added to their "never sleeping apps" list.

**"Not enough space"** — free space on the receiver or choose another download folder in Settings. Nothing
is written before this check.

**Connection lost mid-transfer** — DropBridge reconnects by itself for 2 minutes and continues from the last
verified byte. After that, press **Retry**: only missing files are sent.

**macOS: "Apple could not verify DropBridge"** — click Done, then System Settings → Privacy & Security →
**Open Anyway** (macOS 14 and older: right-click the app → Open). Or in Terminal:
`xattr -dr com.apple.quarantine /Applications/DropBridge.app`. Needed once, because builds aren't notarised yet.

**macOS asks for Keychain access after an update** — click *Always Allow*. Unsigned builds change signature
on every release; signed releases don't ask.

**Windows SmartScreen** — *More info → Run anyway*. Verify the SHA-256 against `SHA256SUMS.txt` on the release page first.

**Where are my files?** — Desktop: your Downloads folder (change in Settings). Android: `Download/DropBridge`.
Unfinished files are hidden as `.name.dbpart` and disappear when complete or cancelled.
