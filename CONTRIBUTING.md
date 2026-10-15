# Contributing

Thanks for helping! DropBridge aims to stay small, fast and safe. Before a big change, open an issue so we can agree on the approach.

## Layout
`core/` holds every protocol, crypto and file-safety rule. UIs (`desktop/`, `android/`) only render events and
call the core API. If a change affects behaviour on more than one platform, it belongs in `core/`.

## Dev setup
See the README "Build from source" section. Quick loop:

```bash
cargo test --workspace          # all unit + end-to-end tests (~5 s)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cargo run -p dropbridge-cli -- receive        # terminal 1
cargo run -p dropbridge-cli -- send <host:port> some/file   # terminal 2
cd desktop/src-tauri && cargo tauri dev        # desktop app with live UI reload
scripts/build-android.sh                       # Android debug APK
```

## Rules
- Every behaviour change in `core/` comes with a test in `core/tests/e2e.rs` or a unit test.
- Never trust anything from the network: validate in `core` before touching the filesystem or UI.
- Desktop UI: render peer-supplied text only through `esc()` / `textContent`.
- Performance claims need a `dropbridge bench` run (before/after) in the PR description.
- No analytics, telemetry, accounts or cloud services.
- Keep dependencies minimal; explain any new one in the PR.

## Protocol changes
Bump `PROTOCOL_VERSION` in `core/src/wire.rs` and update `docs/PROTOCOL.md` for anything on the wire.

## Security issues
Please don't open a public issue. Email the maintainer (see GitHub profile) with details; we'll respond within a week.

## License
By contributing you agree your contribution is licensed under Apache-2.0.
