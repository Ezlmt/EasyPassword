#!/usr/bin/env bash
set -euo pipefail

APP_NAME="EasyPassword"
BUNDLE_ID="com.easypassword.app"

VERSION="$(python - <<'PY'
import re
from pathlib import Path
text = Path('Cargo.toml').read_text(encoding='utf-8')
m = re.search(r'^version\s*=\s*"([^"]+)"\s*$', text, flags=re.M)
if not m:
    raise SystemExit('error: could not parse version from Cargo.toml')
print(m.group(1))
PY
)"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$ROOT_DIR/dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_DIR/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"

if [[ "${OSTYPE:-}" != darwin* ]]; then
  echo "error: this script must be run on macOS" >&2
  exit 1
fi

cd "$ROOT_DIR"
cargo build --release

rm -rf "$APP_DIR"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

cp -f "$ROOT_DIR/target/release/easypassword" "$MACOS_DIR/easypassword"
chmod +x "$MACOS_DIR/easypassword"

cat > "$CONTENTS_DIR/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>easypassword</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>LSUIElement</key>
  <true/>
</dict>
</plist>
EOF

cat > "$CONTENTS_DIR/PkgInfo" <<EOF
APPL????
EOF

echo "created: $APP_DIR"
