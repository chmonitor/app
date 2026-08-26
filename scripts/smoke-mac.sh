#!/usr/bin/env bash
# GUI smoke test for chmonitor desktop app on macOS.
# Launches with fixture data (CHM_SMOKE=1), verifies the process stays up,
# captures a screenshot of the app window, and fails on an early exit or panic.
#
# Usage: scripts/smoke-mac.sh
set -euo pipefail

SHOTS_DIR="${SHOTS_DIR:-shots}"
WAIT_SECS="${WAIT_SECS:-6}"

log() { printf '[smoke-mac] %s\n' "$*"; }
fail() { printf '[smoke-mac] FAIL: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || fail "cargo not on PATH"
[[ "$(uname -s)" == Darwin ]] || fail "this script is macOS-only (see scripts/smoke.sh for Linux)"

mkdir -p "$SHOTS_DIR"

log "building debug binary…"
cargo build -p chm-app

BIN="target/debug/chm-app"
[[ -x "$BIN" ]] || fail "binary missing at $BIN"

LOG="$(mktemp /tmp/chm-smoke.XXXXXX)"
env CHM_SMOKE=1 RUST_LOG=info "$BIN" >"$LOG" 2>&1 &
APP_PID=$!
trap 'kill $APP_PID 2>/dev/null || true' EXIT

for _ in $(seq "$WAIT_SECS"); do
    if grep -q "shell ready" "$LOG" 2>/dev/null; then
        break
    fi
    kill -0 "$APP_PID" 2>/dev/null || { tail -30 "$LOG"; fail "app exited before shell ready"; }
    sleep 1
done
kill -0 "$APP_PID" 2>/dev/null || { tail -30 "$LOG"; fail "app exited early"; }
log "app alive after ${WAIT_SECS}s (pid $APP_PID)"

# Give the first frame a moment to paint, then capture the app window
# (full-screen -x is a fallback if we cannot resolve the CGWindow id).
sleep 2
OUT="$SHOTS_DIR/01-macos-overview.png"
if command -v screencapture >/dev/null; then
    WID=""
    if command -v swift >/dev/null; then
        WID="$(swift -e '
import CoreGraphics
let opts = CGWindowListOption.optionOnScreenOnly.union(.excludeDesktopElements)
guard let info = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else { fatalError("no windows") }
for w in info {
    let owner = w[kCGWindowOwnerName as String] as? String ?? ""
    let num = w[kCGWindowNumber as String] as? Int ?? 0
    if owner == "chm-app" || owner == "chmonitor" {
        print(num)
        break
    }
}
' 2>/dev/null || true)"
    fi
    if [[ -n "$WID" ]]; then
        screencapture -l"$WID" "$OUT" || log "WARN: screencapture -l$WID failed"
    else
        log "WARN: no chm-app window id — capturing full screen"
        screencapture -x "$OUT" || log "WARN: screencapture failed"
    fi
    [[ -s "$OUT" ]] && log "shot: $OUT ($(wc -c <"$OUT") bytes)"
else
    log "WARN: screencapture not on PATH — skipping screenshot"
fi

kill -0 "$APP_PID" 2>/dev/null || fail "app died during capture"
if grep -qE "panicked at|RUST_BACKTRACE" "$LOG"; then
    tail -20 "$LOG"; fail "panic found in app log"
fi
if ! grep -q "shell ready" "$LOG"; then
    tail -20 "$LOG"; fail "never printed 'shell ready'"
fi

log "PASS — log $LOG"
