#!/usr/bin/env bash
# Build chmonitor.app for macOS: icon, Info.plist, signed-ready bundle.
#
# Usage:
#   scripts/build-macos.sh                 # release profile → dist/macos/chmonitor.app
#   scripts/build-macos.sh --debug
#   scripts/build-macos.sh --beta
#   scripts/build-macos.sh --bin PATH --version 0.1.1 --out DIR
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE="release"
BIN=""
VERSION="${CHM_VERSION:-}"
OUT="$ROOT/dist/macos"
SKIP_BUILD=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) PROFILE=dev; shift ;;
        --release) PROFILE=release; shift ;;
        --beta) PROFILE=beta; shift ;;
        --bin) BIN="$2"; SKIP_BUILD=1; shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,12p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

APP_NAME="chmonitor"
BIN_NAME="chm-app"
BUNDLE_ID="io.chmonitor.desktop"
if [[ -z "$VERSION" ]]; then
    VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)"
fi

log() { printf '[build-macos] %s\n' "$*" >&2; }

# --- icon ---
ICON_SRC="$ROOT/assets/icon/icon-1024.png"
[[ -f "$ICON_SRC" ]] || { echo "missing $ICON_SRC" >&2; exit 1; }

ICONSET="$(mktemp -d /tmp/chm-iconset.XXXXXX)"
trap 'rm -rf "$ICONSET"' EXIT
mkdir -p "$ICONSET/AppIcon.iconset"

# iconutil wants both 1x and @2x names.
copy_size() {
    local px="$1" name="$2"
    sips -z "$px" "$px" "$ICON_SRC" --out "$ICONSET/AppIcon.iconset/$name" >/dev/null
}
copy_size 16   icon_16x16.png
copy_size 32   icon_16x16@2x.png
copy_size 32   icon_32x32.png
copy_size 64   icon_32x32@2x.png
copy_size 128  icon_128x128.png
copy_size 256  icon_128x128@2x.png
copy_size 256  icon_256x256.png
copy_size 512  icon_256x256@2x.png
copy_size 512  icon_512x512.png
copy_size 1024 icon_512x512@2x.png

ICNS="$ICONSET/AppIcon.icns"
iconutil -c icns "$ICONSET/AppIcon.iconset" -o "$ICNS"
log "icon $ICNS ($(wc -c <"$ICNS") bytes)"

# --- binary ---
if [[ "$SKIP_BUILD" -eq 0 ]]; then
    log "cargo build --profile $PROFILE -p chm-app"
    cargo build --profile "$PROFILE" -p chm-app
    if [[ "$PROFILE" = "dev" ]]; then
        BIN="$ROOT/target/debug/$BIN_NAME"
    else
        BIN="$ROOT/target/$PROFILE/$BIN_NAME"
    fi
fi
[[ -x "$BIN" ]] || { echo "binary not executable: $BIN" >&2; exit 1; }

# --- bundle ---
mkdir -p "$OUT"
APP="$OUT/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$BIN_NAME"
chmod +x "$APP/Contents/MacOS/$BIN_NAME"
cp "$ICNS" "$APP/Contents/Resources/AppIcon.icns"
printf 'APPL????' > "$APP/Contents/PkgInfo"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleVersion</key>
  <string>$VERSION</string>
  <key>CFBundleShortVersionString</key>
  <string>$VERSION</string>
  <key>CFBundleExecutable</key>
  <string>$BIN_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon</string>
  <key>CFBundleIconName</key>
  <string>AppIcon</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticTermination</key>
  <false/>
</dict>
</plist>
PLIST

log "app $APP"
log "version $VERSION ($(file -b "$APP/Contents/MacOS/$BIN_NAME"))"
echo "$APP"
