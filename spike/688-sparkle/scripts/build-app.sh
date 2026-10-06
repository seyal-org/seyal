#!/bin/sh
# Assemble one non-sandboxed SpikeHarness.app around the SPM Sparkle 2.9.6
# framework with XPC services removed and the real seyal-runtime helper.
set -eu

VERSION="${1:?version}"
BUNDLE_ID="${2:?bundle id}"
PUBLIC_KEY="${3:?public key}"
OUT_APP="${4:?output app}"
SIGN_MODE="${5:-apple-development}"

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
REPO="$(CDPATH= cd -- "$ROOT/../.." && pwd)"
RUNTIME="$REPO/target/release/seyal-runtime"
WORK="$ROOT/.spike-build/work-$BUNDLE_ID-$VERSION"
FRAMEWORK_SRC="${SPIKE_SPARKLE_FRAMEWORK:?set SPIKE_SPARKLE_FRAMEWORK}"

if [ ! -x "$RUNTIME" ]; then
  echo "missing runtime helper" >&2
  exit 1
fi

IDENTITY_CLASS="Apple Development"
IDENTITY=""
if [ "$SIGN_MODE" = "apple-development" ]; then
  IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' | head -1)"
  if [ -z "$IDENTITY" ]; then
    IDENTITY="$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1)"
    IDENTITY_CLASS="Developer ID Application"
  fi
  if [ -z "$IDENTITY" ]; then
    echo "no Apple signing identity" >&2
    exit 1
  fi
fi

rm -rf "$WORK" "$OUT_APP"
mkdir -p "$WORK/SeyalSpike.app/Contents/MacOS" \
  "$WORK/SeyalSpike.app/Contents/Frameworks" \
  "$WORK/SeyalSpike.app/Contents/Helpers" \
  "$WORK/SeyalSpike.app/Contents/Resources"

cp "$ROOT/.spike-build/SpikeHarness" "$WORK/SeyalSpike.app/Contents/MacOS/SpikeHarness"
cp "$RUNTIME" "$WORK/SeyalSpike.app/Contents/Helpers/seyal-runtime"
chmod 755 "$WORK/SeyalSpike.app/Contents/MacOS/SpikeHarness" \
  "$WORK/SeyalSpike.app/Contents/Helpers/seyal-runtime"

# Copy the SPM framework, then drop the optional sandboxed XPC services.
cp -R "$FRAMEWORK_SRC" "$WORK/SeyalSpike.app/Contents/Frameworks/Sparkle.framework"
rm -rf "$WORK/SeyalSpike.app/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices" \
  "$WORK/SeyalSpike.app/Contents/Frameworks/Sparkle.framework/XPCServices"

cat > "$WORK/SeyalSpike.app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>SpikeHarness</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleName</key>
  <string>SeyalSpike</string>
  <key>CFBundleDisplayName</key>
  <string>SeyalSpike</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}.0</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSAppTransportSecurity</key>
  <dict>
    <key>NSAllowsLocalNetworking</key>
    <true/>
  </dict>
  <key>SUFeedURL</key>
  <string>https://127.0.0.1/spike-feed</string>
  <key>SUPublicEDKey</key>
  <string>${PUBLIC_KEY}</string>
  <key>SURequireSignedFeed</key>
  <true/>
  <key>SUVerifyUpdateBeforeExtraction</key>
  <true/>
  <key>SUSignedFeedFailureExpirationInterval</key>
  <integer>0</integer>
  <key>SUEnableSystemProfiling</key>
  <false/>
  <key>SUEnableAutomaticChecks</key>
  <true/>
  <key>SUAutomaticallyUpdate</key>
  <false/>
  <key>SUScheduledImpatientCheckInterval</key>
  <integer>15</integer>
</dict>
</plist>
EOF

BIN="$WORK/SeyalSpike.app/Contents/MacOS/SpikeHarness"
# Point Sparkle at the embedded framework and drop absolute SPM rpaths.
otool -l "$BIN" | awk '/cmd LC_RPATH/{getline; getline; print $2}' | while read -r rpath; do
  install_name_tool -delete_rpath "$rpath" "$BIN" || true
done
install_name_tool -add_rpath "@executable_path/../Frameworks" "$BIN"

sign_item() {
  target="$1"
  extra="${2:-}"
  if [ "$SIGN_MODE" = "adhoc" ]; then
    # shellcheck disable=SC2086
    codesign --force --sign - $extra "$target"
  else
    # shellcheck disable=SC2086
    codesign --force --sign "$IDENTITY" --options runtime --timestamp=none $extra "$target"
  fi
}

FW="$WORK/SeyalSpike.app/Contents/Frameworks/Sparkle.framework"
sign_item "$FW/Versions/B/Autoupdate"
if [ -d "$FW/Versions/B/Updater.app" ]; then
  sign_item "$FW/Versions/B/Updater.app" "--deep"
fi
sign_item "$FW"
sign_item "$WORK/SeyalSpike.app/Contents/Helpers/seyal-runtime" "--identifier dev.seyal.Seyal.runtime"
sign_item "$WORK/SeyalSpike.app"

mv "$WORK/SeyalSpike.app" "$OUT_APP"
echo "IDENTITY_CLASS=$IDENTITY_CLASS"
echo "SIGN_MODE=$SIGN_MODE"
echo "APP=$OUT_APP"
