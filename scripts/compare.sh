#!/usr/bin/env bash
# Head-to-head on ONE machine over loopback: measures each tool's own overhead (crypto, framing,
# per-file cost) with the radio taken out. It does NOT predict Wi-Fi speed between devices.
# Usage: scripts/compare.sh            (needs: cargo build --release -p dropbridge-cli, croc, openssl, curl)
set -euo pipefail
cd "$(dirname "$0")/.."
DB=$PWD/target/release/dropbridge
W=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$W"' EXIT
FX=$W/fx; mkdir -p "$FX/small"
head -c 1000000000 /dev/urandom > "$FX/1GB.bin"
for i in $(seq -w 0 999); do head -c $((RANDOM % 8000 + 100)) /dev/urandom > "$FX/small/f$i.txt"; done

now() { python3 -c 'import time;print(time.time())'; }
run() { local s=$(now); "$@" >/dev/null 2>&1; local e=$(now); python3 -c "print(f'{$e-$s:.2f}')"; }

echo "| Tool | Encryption | 1 GB file | 1,000 small files |"
echo "|---|---|---|---|"

# Disk copy: the ceiling for any tool on this machine.
c1=$(run cp "$FX/1GB.bin" "$W/cp.bin"); c2=$(run cp -R "$FX/small" "$W/cp-small")
echo "| cp (disk copy, reference) | none | ${c1}s | ${c2}s |"

# DropBridge: receiver and sender as separate processes.
HOME=$W/h1 "$DB" receive --yes --port 47311 --dir "$W/db-in" >/dev/null 2>&1 &
sleep 1
d1=$(run "$DB" send 127.0.0.1:47311 "$FX/1GB.bin"); d2=$(run "$DB" send 127.0.0.1:47311 "$FX/small")
cmp -s "$FX/1GB.bin" "$W/db-in/1GB.bin" || { echo "DropBridge output mismatch" >&2; exit 1; }
echo "| DropBridge | TLS 1.3 + BLAKE3 verify | ${d1}s | ${d2}s |"

# HTTPS (the transport LocalSend and many apps use): openssl s_server -WWW + curl, one request per file.
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj /CN=x -keyout "$W/k.pem" -out "$W/c.pem" -days 1 >/dev/null 2>&1
(cd "$FX" && exec openssl s_server -quiet -accept 47312 -WWW -cert "$W/c.pem" -key "$W/k.pem" >/dev/null 2>&1) &
sleep 1
h1=$(run curl -sk -o "$W/h.bin" https://127.0.0.1:47312/1GB.bin)
cmp -s "$FX/1GB.bin" "$W/h.bin" || { echo "HTTPS output mismatch" >&2; exit 1; }
mkdir -p "$W/h-small"
urls=(); for f in "$FX"/small/*; do n=$(basename "$f"); urls+=(-o "$W/h-small/$n" "https://127.0.0.1:47312/small/$n"); done
h2=$(run curl -sk "${urls[@]}")
echo "| HTTPS (openssl + curl, keep-alive) | TLS 1.3 | ${h1}s | ${h2}s |"

# croc with a local relay (PAKE + AES-GCM).
if command -v croc >/dev/null; then
  croc relay --ports 47320,47321,47322,47323,47324 >/dev/null 2>&1 &
  sleep 1
  cr() { # $1 = path
    local code="db-bench-$RANDOM-$RANDOM"
    (cd "$W" && mkdir -p croc-in && cd croc-in && CROC_SECRET=$code croc --yes --overwrite --relay 127.0.0.1:47320 >/dev/null 2>&1) &
    local rp=$!
    local s=$(now)
    CROC_SECRET=$code croc --relay 127.0.0.1:47320 send --code "$code" "$1" >/dev/null 2>&1 || true
    wait $rp || true
    local e=$(now); python3 -c "print(f'{$e-$s:.2f}')"
  }
  r1=$(cr "$FX/1GB.bin"); r2=$(cr "$FX/small")
  cmp -s "$FX/1GB.bin" "$W/croc-in/1GB.bin" || r1="failed"
  echo "| croc $(croc --version | awk '{print $3}') (local relay) | PAKE + AES-GCM | ${r1}s | ${r2}s |"
fi
echo
echo "Machine: $(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m), $(uname -s) $(uname -r). Date: $(date +%F)."
