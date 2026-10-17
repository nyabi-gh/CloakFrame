#!/usr/bin/env bash
# Runs a program with network attempts logged (libnetwatch) and, unless --open, inside a
# sandbox that blocks outbound connections and DNS so nothing actually leaves the machine.
# While it runs, Internet sockets of the process are sampled with lsof. Afterwards, files
# the 1DS SDK would write (onnxruntime.db) are looked for.
#
#   netwatch/watch.sh [--open] <label> <out dir> -- <program> [args...]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
open=0
if [ "$1" = "--open" ]; then open=1; shift; fi
label=$1 out=$2; shift 2
[ "$1" = "--" ] && shift
mkdir -p "$out"
lib="$out/libnetwatch.dylib"
[ -f "$lib" ] || clang -O2 -dynamiclib -o "$lib" "$here/netwatch.c" -framework Network
log="$out/$label.netwatch.log"
sockets="$out/$label.lsof.log"
rm -f "$log" "$sockets"
marker="$out/$label.marker"
touch "$marker"
sleep 1

if [ $open = 1 ]; then
    env NETWATCH_LOG="$log" DYLD_INSERT_LIBRARIES="$lib" "$@" > "$out/$label.stdout" 2>&1 &
else
    sandbox-exec -f "$here/deny-network.sb" /usr/bin/env NETWATCH_LOG="$log" DYLD_INSERT_LIBRARIES="$lib" "$@" \
        > "$out/$label.stdout" 2>&1 &
fi
pid=$!
while kill -0 "$pid" 2>/dev/null; do
    for p in $(pgrep -P "$pid") "$pid"; do
        lsof -nP -a -p "$p" -i 2>/dev/null | tail -n +2 >> "$sockets" || true
    done
    sleep 0.2
done
wait "$pid" && status=0 || status=$?

attempts=$(grep -vc "netwatch loaded" "$log" 2>/dev/null || true)
touch "$sockets"
open_sockets=$(sort -u "$sockets" | wc -l | tr -d ' ')
stores=$({ find "$HOME/Library" "${TMPDIR:-/tmp}" "$PWD" -newer "$marker" -name 'onnxruntime.db*' 2>/dev/null || true; } | tee "$out/$label.stores.log" | wc -l | tr -d ' ')
loaded=$(grep -c "netwatch loaded" "$log" 2>/dev/null || true)
printf '%s exit=%s sandbox=%s watcher_loaded=%s network_attempts=%s internet_sockets=%s telemetry_store_files=%s\n' \
    "$label" "$status" "$([ $open = 1 ] && echo off || echo on)" "$loaded" "$attempts" "$open_sockets" "$stores"
