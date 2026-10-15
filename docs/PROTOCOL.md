# DropBridge protocol v1

Transport: TCP, then TLS 1.3 with **mutual** authentication. Each device has a self-signed ECDSA P-256
certificate. Certificates are not validated against a CA; instead:

- **Device id** = lowercase hex SHA-256 of the peer's DER certificate.
- **Verification code (SAS)** = `u32_be(SHA-256(EKM)[0..4]) mod 1_000_000`, zero-padded to 6 digits, where
  `EKM = TLS exporter("EXPORTER-dropbridge-sas", no context, 32 bytes)`.

No TLS 0-RTT is used, so nothing can be replayed.

## Frames

```
[u8 kind][u32 big-endian length][payload]
kind 1 = control: payload is a JSON message (below)
kind 2 = data:    payload is [u32 big-endian item id][file bytes]
```
Maximum frame size 64 MiB; larger frames close the connection.

## Messages (`"t"` is the tag)

| Message | Direction | Fields |
|---|---|---|
| `Hello` | both, first | `name`, `kind`, `version` (=1) |
| `Offer` | sender | `transfer_id` (≤64 chars), `items[]`, `token` (optional one-time pairing token/code) |
| `Answer` | receiver | `accepted`, `items[]` of `{id, accept, offset, prefix_hash}`, `reason` |
| `Start` | sender | `item`, `offset` — data for this item follows from `offset` |
| `FileEnd` | sender | `item`, `hash` (BLAKE3 hex of the whole file) |
| `Verified` | receiver | `item`, `ok`, `error` |
| `Pause` / `Resume` | either | — |
| `Done` | sender, then receiver echoes | — |
| `Cancel` | either | `reason` (shown to the user) |

`Item = {id, kind: File|Dir|Text, path, size, mtime, text}`. Paths are relative, `/`-separated; ids are
strictly increasing. `Text` items carry their content inline (≤ 8 MiB).

## Sequence

```
S → R  Hello                     R → S  Hello
S → R  Offer
                                 (validate, ask user, check space, find partials)
                                 R → S  Answer
S → R  Start(i, off) Data… FileEnd(i, h)   (repeat per accepted file, no waiting)
                                 R → S  Verified(i)   (asynchronously, as files finish)
S → R  Done                      R → S  Done
```

## Resume

The receiver keeps `.<name>.dbpart` next to the destination. On a new `Offer` it answers with the part's
length and BLAKE3. The sender hashes the same prefix of its file: if equal it sends `Start(offset)`,
otherwise `Start(0)` and the receiver truncates. Reconnects within a transfer reuse the same `transfer_id`;
the receiver remembers the user's approval for that `(peer, transfer_id)` and does not ask again.

## Pairing

`dropbridge://pair?h=<ip>[,<ip>]&p=<port>&fp=<fingerprint>&t=<token>` (QR), or a 6-digit code typed by hand.
Tokens are single-use, expire after 5 minutes, and 5 wrong attempts invalidate all outstanding codes.
An `Offer` carrying a valid token makes the receiver trust the sender; an empty `Offer` with a valid token
is a pure pairing handshake. The sender pins `fp` from the QR, so a QR pairing cannot be intercepted.
