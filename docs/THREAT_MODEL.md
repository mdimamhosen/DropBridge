# Threat model and security decisions

## Assets
Files and text in transit; the files already on the receiving device; each device's identity key;
the user's decision about who may send to them.

## Attacker
Someone on the same Wi-Fi or LAN (café, office, hotel): can see and modify traffic, run their own
DropBridge-like client, advertise fake devices, and connect to any open port. Out of scope: an attacker
with code execution on either device, or one who can read the screen of both devices.

## Threats and mitigations

| Threat | Mitigation | Test |
|---|---|---|
| Eavesdropping | TLS 1.3 (AES-GCM / ChaCha20-Poly1305) on every connection; no plaintext mode exists | all e2e tests run over TLS |
| Man-in-the-middle on first contact | 6-digit code from the TLS exporter is shown on both screens; differs under MITM. QR pairing pins the receiver's fingerprint | `unknown_sender_needs_approval_and_codes_match` |
| Impersonating a paired device | Sender pins the fingerprint of trusted/QR devices and aborts on mismatch; receivers recognise trusted devices only by fingerprint, never by name | `impersonation_is_refused` |
| Unsolicited files | Unknown senders always need approval; trusted devices only auto-accept when the user enabled it; one pending prompt per device, 8 max | `unknown_sender_needs_approval…` |
| Path traversal / malicious names | Every path is NFC-normalised and rejected if absolute, contains `..`, `.`, empty parts, NUL/control characters, or components > 255 bytes; Windows reserved names/characters rewritten; writes refuse to pass through existing symlinks; partial files opened with `O_NOFOLLOW` | `malicious_offer_is_refused`, `paths::tests` |
| Replay | TLS 1.3 without 0-RTT; pairing tokens single-use with 5-minute expiry | `pairing_code_trusts_and_wrong_code_is_rejected` |
| Pairing-code guessing | 10⁶ codes, 5 wrong attempts burn all codes, 5-minute lifetime | same |
| Resource exhaustion | 64 MiB frame cap, 200,000 items per offer, 8 MiB text, free-space check before accepting, bounded buffers | `wire::tests`, `refuses_when_disk_too_small` |
| Corrupted / truncated data | Whole-file BLAKE3 verified before rename; resume validates the prefix hash | `resumes_from_partial_and_restarts_on_mismatch` |
| Blocked device | Refused right after the TLS handshake | `blocked_device_is_refused` |
| Identity theft from disk | Private key stored in macOS Keychain, Windows Credential Manager, Linux Secret Service (0600 file fallback), Android: AES-GCM wrapped with a non-exportable Android Keystore key | manual |
| Exposure to the internet | No relay or cloud; the listener only accepts direct connections. Do not port-forward it | — |
| UI injection (desktop webview) | All peer-supplied strings rendered through an HTML-escaping helper; strict CSP | review |
| Privacy | No analytics, no telemetry, no network calls except to the peer | review |

## Known limitations
- The verification code protects only users who compare it. Unknown-device prompts say so explicitly.
- Anyone on the LAN can see a device's name and that it runs DropBridge (mDNS is public by design).
- Trusted-device fingerprints are stored in plain JSON (they are public values; integrity relies on OS file permissions).
- Unsigned desktop builds: users must bypass Gatekeeper/SmartScreen once; verify downloads with the published SHA-256 sums.
