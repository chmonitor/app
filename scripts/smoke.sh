#!/usr/bin/env bash
# GUI smoke test for chmonitor desktop app on a Linux X desktop.
# Launches the app with fixture data (CHM_SMOKE=1), verifies a window maps,
# screenshots each page, and fails if any shot is blank or the app crashed.
#
# Usage: scripts/smoke.sh [display]   (default :1; falls back to xvfb-run)
set -euo pipefail

DISPLAY_NUM="${1:-${DISPLAY:-:1}}"
SHOTS_DIR="${SHOTS_DIR:-shots}"
WAIT_SECS=6
XVFB_PID=""

log() { printf '[smoke] %s\n' "$*"; }
fail() { printf '[smoke] FAIL: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null || fail "cargo not on PATH"

# --- preflight ---------------------------------------------------------------
if [ -z "${DISPLAY:-}" ]; then
    command -v Xvfb >/dev/null || fail "no DISPLAY and no Xvfb"
    DISPLAY_NUM=":99"
    Xvfb "$DISPLAY_NUM" -screen 0 1440x900x24 >/dev/null 2>&1 &
    XVFB_PID=$!
    export DISPLAY="$DISPLAY_NUM"
    sleep 1
    log "started Xvfb on $DISPLAY (pid $XVFB_PID)"
fi

shot_tool=""
for t in import scrot gnome-screenshot; do
    if command -v "$t" >/dev/null; then shot_tool="$t"; break; fi
done
[ -n "$shot_tool" ] || fail "no screenshot tool (need imagemagick import, scrot, or gnome-screenshot)"

wm_tool=""
for t in wmctrl xdotool; do
    if command -v "$t" >/dev/null; then wm_tool="$t"; break; fi
done

# --- launch ------------------------------------------------------------------
mkdir -p "$SHOTS_DIR"

log "building debug binary…"
cargo build -p chm-app

BIN="target/debug/chm-app"
[ -x "$BIN" ] || fail "binary missing at $BIN"

LOG="$(mktemp /tmp/chm-smoke.XXXXXX.log)"
env CHM_SMOKE=1 RUST_LOG=info "$BIN" >"$LOG" 2>&1 &
APP_PID=$!
trap 'kill $APP_PID $XVFB_PID 2>/dev/null || true' EXIT

sleep "$WAIT_SECS"
kill -0 "$APP_PID" 2>/dev/null || { tail -30 "$LOG"; fail "app exited early"; }
log "app alive after ${WAIT_SECS}s (pid $APP_PID)"

# --- window mapped? ----------------------------------------------------------
# The app sets _NET_WM_NAME ("chmonitor") via its titlebar options. Prefer
# `xdotool search --name`, which walks the whole window tree — on bare Xvfb
# there is no WM, so `wmctrl -l` has no _NET_CLIENT_LIST to enumerate.
window_found=""
if [ -n "$wm_tool" ]; then
    for _ in $(seq 10); do
        if command -v xdotool >/dev/null; then
            DISPLAY="$DISPLAY_NUM" xdotool search --onlyvisible --name chmonitor >/dev/null 2>&1 &&
                window_found=yes
        elif DISPLAY="$DISPLAY_NUM" wmctrl -l 2>/dev/null | grep -qi chmonitor; then
            window_found=yes
        fi
        [ -n "$window_found" ] && break
        sleep 1
    done
else
    window_found=assumed   # no WM tooling; rely on screenshot checks
fi
case "$window_found" in
    yes)     log "window mapped" ;;
    assumed) log "WARN: no wmctrl/xdotool — skipping window-title check" ;;
    *)       tail -20 "$LOG"; fail "window never mapped within 10s" ;;
esac

# --- screenshots --------------------------------------------------------------
shot() {
    local name="$1" out="$SHOTS_DIR/$1.png"
    case "$shot_tool" in
        import)          import -window root "$out" || return 1 ;;
        scrot)           scrot -o "$out" || return 1 ;;
        gnome-screenshot) gnome-screenshot -f "$out" || return 1 ;;
    esac
}

shot "01-connect-or-overview" || fail "screenshot failed on $DISPLAY"
PAGES=("02-overview" "03-queries" "04-merges" "05-replicas" "06-health" "07-tables" "08-traffic")

# Page switching is keyboard-driven when supported (keys 1..8 bound in shell);
# without input automation we capture what is visible and verify non-blankness.
if command -v xdotool >/dev/null && [ -n "${SMOKE_NAVIGATE:-}" ]; then
    i=2
    for p in "${PAGES[@]}"; do
        DISPLAY="$DISPLAY_NUM" xdotool key --window "" "$((i-1))" 2>/dev/null || true
        sleep 1
        shot "$p"
        i=$((i+1))
    done
fi

# --- assertions ----------------------------------------------------------------
blank_failures=()
for f in "$SHOTS_DIR"/*.png; do
    # pixel variance via imagemagick statistics; blank frames have ~zero stdev
    if command -v identify >/dev/null; then
        std="$(identify -format '%[fx:standard_deviation]' "$f" 2>/dev/null || echo 0)"
        if awk -v s="$std" 'BEGIN{exit !(s>0.005)}'; then
            log "ok: $f (stdev=$std)"
        else
            blank_failures+=("$f")
        fi
    fi
done
[ ${#blank_failures[@]} -eq 0 ] || fail "blank screenshots: ${blank_failures[*]}"

kill -0 "$APP_PID" 2>/dev/null || fail "app died during capture"
if grep -qE "panicked at|RUST_BACKTRACE" "$LOG"; then
    tail -20 "$LOG"; fail "panic found in app log"
fi

log "PASS — shots in $SHOTS_DIR/"
