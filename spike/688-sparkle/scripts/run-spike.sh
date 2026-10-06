#!/bin/sh
# Drive G1–G5 for the isolated Sparkle spike. Artifacts stay under /tmp.
set -eu

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
REPO="$(CDPATH= cd -- "$ROOT/../.." && pwd)"
ART="/tmp/seyal-spike-688"
BIN_SIGN="/tmp/spike-688-sparkle/bin/sign_update"
PORT=8741
HTTPS_PORT=8742
# Sparkle/NSURLSession cannot adopt a throwaway root without an interactive
# trust prompt on this host. The same directory is also served over HTTPS for
# the curl/CA proof. Sparkle itself uses loopback HTTP plus the signed feed.
FEED_ORIGIN="http://127.0.0.1:${PORT}"
mkdir -p "$ART/keys" "$ART/www" "$ART/apps" "$ART/results" "$ART/logs"
chmod 700 "$ART/keys"
RESULTS="$ART/results"

redact() {
  sed -E \
    -e "s#$HOME#<home>#g" \
    -e "s#$REPO#<worktree>#g" \
    -e 's/Apple Development: [^"]+/Apple Development: <REDACTED>/g' \
    -e 's/Developer ID Application: [^"]+/Developer ID Application: <REDACTED>/g' \
    -e 's/TeamIdentifier=.*/TeamIdentifier=<REDACTED>/' \
    -e 's/Authority=.*/Authority=<REDACTED>/'
}

log() { printf '%s %s\n' "$(date -u +%H:%M:%S)" "$*"; }

write_policy() {
  bundle="$1"
  cat > "/tmp/seyal-spike-688/$bundle/policy.json"
}

write_feed_url() {
  bundle="$1"
  url="$2"
  mkdir -p "/tmp/seyal-spike-688/$bundle"
  printf '%s\n' "$url" > "/tmp/seyal-spike-688/$bundle/feed-url"
  : > "/tmp/seyal-spike-688/$bundle/command"
}

launch_app() {
  app="$1"
  open -n "$app"
}

app_pids() {
  app="$1"
  pgrep -f "$app/Contents/MacOS/SpikeHarness" || true
}

stop_app() {
  app="$1"
  pids="$(app_pids "$app")"
  if [ -n "$pids" ]; then
    # shellcheck disable=SC2086
    kill $pids 2>/dev/null || true
    sleep 1
    pids="$(app_pids "$app")"
    if [ -n "$pids" ]; then
      # shellcheck disable=SC2086
      kill -9 $pids 2>/dev/null || true
    fi
  fi
}

wait_log() {
  file="$1"
  pattern="$2"
  timeout="$3"
  end=$((SECONDS + timeout))
  while [ "$SECONDS" -lt "$end" ]; do
    if [ -f "$file" ] && grep -E -q "$pattern" "$file"; then
      return 0
    fi
    sleep 0.4
  done
  return 1
}

bundle_version() {
  /usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$1/Contents/Info.plist"
}

make_dmg() {
  src="$1"
  dmg="$2"
  stage="$(mktemp -d /tmp/seyal-spike-688-dmg.XXXXXX)"
  cp -R "$src" "$stage/SeyalSpike.app"
  rm -f "$dmg"
  hdiutil create -quiet -volname SeyalSpike -srcfolder "$stage" -ov -format UDZO "$dmg"
  rm -rf "$stage"
}

sign_archive() {
  file="$1"
  seed="$2"
  "$BIN_SIGN" --ed-key-file "$seed" "$file"
}

write_appcast() {
  out="$1"
  version="$2"
  dmg_name="$3"
  signature="$4"
  length="$5"
  seyal_value="$6"
  cat > "$out" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle" xmlns:seyal="https://seyal.dev/ns/update">
  <channel>
    <title>Seyal spike</title>
    <item>
      <title>${version}</title>
      <pubDate>Mon, 05 Oct 2026 12:00:00 +0000</pubDate>
      <sparkle:version>${version}</sparkle:version>
      <sparkle:shortVersionString>${version}.0</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>13.0.0</sparkle:minimumSystemVersion>
      <seyal:compatibility>${seyal_value}</seyal:compatibility>
      <enclosure
        url="${FEED_ORIGIN}/${dmg_name}"
        length="${length}"
        type="application/octet-stream"
        sparkle:edSignature="${signature}"
        sparkle:os="macos"/>
    </item>
  </channel>
</rss>
EOF
}

parse_sig() {
  # sign_update prints: sparkle:edSignature="..." length="..."
  printf '%s\n' "$1" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p'
}

parse_len() {
  printf '%s\n' "$1" | sed -n 's/.*length="\([^"]*\)".*/\1/p'
}

log "generating throwaway ed25519 seeds"
swift -e 'import CryptoKit
import Foundation
for name in ["a","b"] {
  let key = Curve25519.Signing.PrivateKey()
  let seed = key.rawRepresentation.base64EncodedString()
  let pub = key.publicKey.rawRepresentation.base64EncodedString()
  print(name)
  print(seed)
  print(pub)
}' > "$ART/keys/generated.txt"
chmod 600 "$ART/keys/generated.txt"
SEED_A="$(sed -n '2p' "$ART/keys/generated.txt")"
PUB_A="$(sed -n '3p' "$ART/keys/generated.txt")"
SEED_B="$(sed -n '5p' "$ART/keys/generated.txt")"
PUB_B="$(sed -n '6p' "$ART/keys/generated.txt")"
printf '%s\n' "$SEED_A" > "$ART/keys/seed-a.b64"
printf '%s\n' "$SEED_B" > "$ART/keys/seed-b.b64"
chmod 600 "$ART/keys/seed-a.b64" "$ART/keys/seed-b.b64"
rm -f "$ART/keys/generated.txt"

KC="$ART/keys/spike.keychain-db"
rm -f "$KC"
security create-keychain -p 'spike-688-throwaway' "$KC"
security set-keychain-settings -lut 7200 "$KC"
security unlock-keychain -p 'spike-688-throwaway' "$KC"
security add-generic-password -a 'seyal-spike-688-a' -s 'spike-eddsa-seed' -w "$SEED_A" "$KC" >/dev/null
security add-generic-password -a 'seyal-spike-688-b' -s 'spike-eddsa-seed' -w "$SEED_B" "$KC" >/dev/null
# Confirm the seeds are in the throwaway keychain and not described here.
security find-generic-password -a 'seyal-spike-688-a' "$KC" >/dev/null
log "throwaway keychain ready"

openssl req -x509 -newkey rsa:2048 \
  -keyout "$ART/keys/feed.key" -out "$ART/keys/feed.crt" -days 2 -nodes \
  -subj "/CN=spike-688.local" \
  -addext "subjectAltName=IP:127.0.0.1,DNS:localhost" >/dev/null 2>&1
chmod 600 "$ART/keys/feed.key"

log "adding throwaway feed certificate to the login trust settings"
if perl -e 'alarm 5; exec @ARGV' security add-trusted-cert -r trustRoot -p ssl -k "$HOME/Library/Keychains/login.keychain-db" "$ART/keys/feed.crt"; then
  echo "cert-trust=added" > "$RESULTS/cert-trust.txt"
else
  echo "cert-trust=interactive-or-failed" > "$RESULTS/cert-trust.txt"
fi

python3 - <<PY &
import http.server, os, ssl, threading
os.chdir("${ART}/www")
http_server = http.server.ThreadingHTTPServer(("127.0.0.1", ${PORT}), http.server.SimpleHTTPRequestHandler)
https_server = http.server.ThreadingHTTPServer(("127.0.0.1", ${HTTPS_PORT}), http.server.SimpleHTTPRequestHandler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain("${ART}/keys/feed.crt", "${ART}/keys/feed.key")
https_server.socket = ctx.wrap_socket(https_server.socket, server_side=True)
threading.Thread(target=https_server.serve_forever, daemon=True).start()
http_server.serve_forever()
PY
echo $! > "$ART/https.pid"
cd "$ART/www"
log "https pid $(cat "$ART/https.pid")"

cleanup() {
  if [ -f "$ART/https.pid" ]; then
    kill "$(cat "$ART/https.pid")" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# Caller exports SPIKE_SPARKLE_FRAMEWORK and has built SpikeHarness.
build() {
  version="$1"
  bundle="$2"
  pubkey="$3"
  mode="$4"
  out="$ART/apps/${bundle}-${version}-${mode}.app"
  sh "$ROOT/scripts/build-app.sh" "$version" "$bundle" "$pubkey" "$out" "$mode" > "$RESULTS/build-${bundle}-${version}-${mode}.txt"
  printf '%s\n' "$out"
}

inventory() {
  app="$1"
  {
    echo "IDENTITY_LINE $(grep IDENTITY_CLASS "$RESULTS"/build-*.txt | head -1)"
    echo "NESTED_MACHO"
    find "$app" -type f -print0 | xargs -0 file | grep Mach-O || true
    echo "XPC_PRESENT"
    find "$app" -name '*.xpc' -print || echo none
    echo "VERIFY"
    codesign --verify --deep --strict --verbose=2 "$app" 2>&1 || true
    echo "APP_SIGN"
    codesign -dv --verbose=4 "$app" 2>&1 || true
    echo "HELPER_SIGN"
    codesign -dv --verbose=4 "$app/Contents/Helpers/seyal-runtime" 2>&1 || true
    echo "FRAMEWORK_SIGN"
    codesign -dv --verbose=4 "$app/Contents/Frameworks/Sparkle.framework" 2>&1 || true
  } | redact > "$RESULTS/g1-inventory.txt"
}

publish_update() {
  app="$1"
  version="$2"
  seed="$3"
  seyal_value="$4"
  feed_seed="${5:-$3}"
  name="SeyalSpike-${version}.dmg"
  make_dmg "$app" "$ART/www/$name"
  signed="$(sign_archive "$ART/www/$name" "$seed")"
  sig="$(parse_sig "$signed")"
  length="$(parse_len "$signed")"
  if [ -z "$sig" ] || [ -z "$length" ]; then
    echo "sign failed: $signed" >&2
    exit 1
  fi
  write_appcast "$ART/www/appcast.xml" "$version" "$name" "$sig" "$length" "$seyal_value"
  "$BIN_SIGN" --ed-key-file "$feed_seed" --disable-signing-warning "$ART/www/appcast.xml" >/dev/null
  cp "$ART/www/appcast.xml" "$RESULTS/appcast-${version}.xml"
  log "published ${name}"
}

install_tree() {
  bundle="$1"
  src="$2"
  dest="/tmp/seyal-spike-688/$bundle"
  stop_app "$dest/SeyalSpike.app" || true
  rm -rf "$dest"
  mkdir -p "$dest"
  cp -R "$src" "$dest/SeyalSpike.app"
  printf '%s\n' "$dest/SeyalSpike.app"
}

printf '%s\n' "$PUB_A" > "$ART/keys/pub-a.txt"
printf '%s\n' "$PUB_B" > "$ART/keys/pub-b.txt"
chmod 600 "$ART/keys/pub-a.txt" "$ART/keys/pub-b.txt"

if [ "${SPIKE_BOOTSTRAP_ONLY:-0}" = "1" ]; then
  log "bootstrap only; https left running"
  trap - EXIT
  wait
fi

HARNESS="$ROOT/.build/out/Products/Release/SpikeHarness"
FRAMEWORK="$(find "$ROOT/.build" -type d -path '*macos-arm64_x86_64/Sparkle.framework' | head -1)"
if [ -z "$FRAMEWORK" ]; then
  FRAMEWORK="$ROOT/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework"
fi
PROBE="$ROOT/probe/target/release/spike-688-probe"
if [ -z "$HARNESS" ] || [ -z "$FRAMEWORK" ] || [ ! -x "$PROBE" ]; then
  echo "missing harness, Sparkle.framework, or probe" >&2
  exit 1
fi
mkdir -p "$ROOT/.spike-build"
cp "$HARNESS" "$ROOT/.spike-build/SpikeHarness"
export SPIKE_SPARKLE_FRAMEWORK="$FRAMEWORK"
cp "$ROOT/Package.resolved" "$RESULTS/Package.resolved" 2>/dev/null || true

start_app() {
  app="$1"
  parent="$(dirname "$app")"
  : > "$parent/command"
  "$app/Contents/MacOS/SpikeHarness" >> "$parent/stdout.log" 2>&1 &
  echo $! > "$parent/app.pid"
}

send_cmd() {
  bundle="$1"
  printf '%s\n' "$2" > "/tmp/seyal-spike-688/$bundle/command"
}

summary() { printf '%s\n' "$*" >> "$RESULTS/summary.txt"; }

: > "$RESULTS/summary.txt"
log "building harness apps"
APP_G2_100="$(build 100 dev.seyal.spike688.g2 "$PUB_A" apple-development)"
APP_G2_101="$(build 101 dev.seyal.spike688.g2 "$PUB_A" apple-development)"
APP_G3_100="$(build 100 dev.seyal.spike688.g3 "$PUB_A" apple-development)"
APP_G3_101="$(build 101 dev.seyal.spike688.g3 "$PUB_A" apple-development)"
APP_G4_100="$(build 100 dev.seyal.spike688.g4 "$PUB_A" apple-development)"
APP_G4_101="$(build 101 dev.seyal.spike688.g4 "$PUB_A" apple-development)"
APP_G5_100="$(build 100 dev.seyal.spike688.g5 "$PUB_A" apple-development)"
APP_G5_101_ADHOC="$(build 101 dev.seyal.spike688.g5 "$PUB_A" adhoc)"
APP_G5_101_ROT="$(build 101 dev.seyal.spike688.g5 "$PUB_B" apple-development)"
APP_G5_102="$(build 102 dev.seyal.spike688.g5 "$PUB_B" apple-development)"
inventory "$APP_G2_100"
summary "G1 inventory=$RESULTS/g1-inventory.txt"

# --- G4 signed custom element, before any install ---
log "G4 signed feed"
publish_update "$APP_G4_101" 101 "$ART/keys/seed-a.b64" "min-hello=1;team-continuity=required"
cp "$ART/www/appcast.xml" "$ART/www/appcast-good.xml"
if curl -fsS --cacert "$ART/keys/feed.crt" "https://127.0.0.1:${HTTPS_PORT}/appcast-good.xml" -o "$RESULTS/https-appcast.xml"; then
  if cmp -s "$ART/www/appcast-good.xml" "$RESULTS/https-appcast.xml"; then
    summary "G4 https-feed-bytes=match"
  else
    summary "G4 https-feed-bytes=mismatch"
  fi
else
  summary "G4 https-feed-bytes=curl-failed"
fi
if "$BIN_SIGN" --verify --ed-key-file "$ART/keys/seed-a.b64" "$ART/www/appcast-good.xml" > "$RESULTS/g4-verify-good.txt" 2>&1; then
  summary "G4 tool-verify-good=pass"
else
  summary "G4 tool-verify-good=fail"
fi
python3 - <<'PY' "$ART/www/appcast-good.xml" "$ART/www/appcast-tampered.xml"
import pathlib, sys
src, dst = sys.argv[1:]
text = pathlib.Path(src).read_text()
text = text.replace("min-hello=1", "min-hello=999", 1)
pathlib.Path(dst).write_text(text)
PY
if "$BIN_SIGN" --verify --ed-key-file "$ART/keys/seed-a.b64" "$ART/www/appcast-tampered.xml" > "$RESULTS/g4-verify-tampered.txt" 2>&1; then
  summary "G4 tool-verify-tampered=unexpected-pass"
else
  summary "G4 tool-verify-tampered=rejected"
fi

G4APP="$(install_tree dev.seyal.spike688.g4 "$APP_G4_100")"
write_feed_url dev.seyal.spike688.g4 "$FEED_ORIGIN/appcast-good.xml"
write_policy dev.seyal.spike688.g4 <<'EOF'
{"allowDownload":false,"allowReadyInstall":false,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":true}
EOF
start_app "$G4APP"
G4LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g4/harness.log"
if wait_log "$G4LOG" "updater-started" 20; then
  send_cmd dev.seyal.spike688.g4 check
fi
if wait_log "$G4LOG" "delegate should-proceed" 40; then
  summary "G4 read-before-download=pass"
else
  summary "G4 read-before-download=fail"
fi
if grep -q "will-download" "$G4LOG"; then
  summary "G4 download-started-despite-block=yes"
else
  summary "G4 download-started-despite-block=no"
fi
stop_app "$G4APP"
cp "$ART/www/appcast-tampered.xml" "$ART/www/appcast.xml"
# Serve tampered under its own name; feed file points at it.
G4APP="$(install_tree dev.seyal.spike688.g4 "$APP_G4_100")"
write_feed_url dev.seyal.spike688.g4 "$FEED_ORIGIN/appcast-tampered.xml"
write_policy dev.seyal.spike688.g4 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}
EOF
start_app "$G4APP"
G4LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g4/harness.log"
if wait_log "$G4LOG" "updater-started" 20; then
  send_cmd dev.seyal.spike688.g4 check
fi
if wait_log "$G4LOG" "updater-error|delegate abort|cycle-finished error" 40; then
  summary "G4 tampered-feed-app=rejected"
else
  summary "G4 tampered-feed-app=not-rejected"
fi
if grep -q "will-download" "$G4LOG"; then
  summary "G4 tampered-feed-downloaded=yes"
else
  summary "G4 tampered-feed-downloaded=no"
fi
stop_app "$G4APP"
# Restore a good feed for later gates.
cp "$ART/www/appcast-good.xml" "$ART/www/appcast.xml"

# --- G2 resident runtime ---
log "G2 runtime survival"
publish_update "$APP_G2_101" 101 "$ART/keys/seed-a.b64" "min-hello=1;team-continuity=required"
G2APP="$(install_tree dev.seyal.spike688.g2 "$APP_G2_100")"
write_feed_url dev.seyal.spike688.g2 "$FEED_ORIGIN/appcast.xml"
write_policy dev.seyal.spike688.g2 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}
EOF
start_app "$G2APP"
G2LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g2/harness.log"
G2DIR="/tmp/seyal-spike-688/dev.seyal.spike688.g2"
if ! wait_log "$G2LOG" "runtime-spawned pid=" 25; then
  summary "G2 spawn=fail"
else
  RPID="$(sed -n 's/.*runtime-spawned pid=\([0-9]*\).*/\1/p' "$G2LOG" | head -1)"
  summary "G2 runtime-pid-before=$RPID"
  sleep 0.8
  if "$PROBE" prepare --runtime-dir "$G2DIR/runtime" --state "$RESULTS/g2-state.txt" > "$RESULTS/g2-prepare.txt" 2>&1; then
    summary "G2 prepare=pass"
  else
    summary "G2 prepare=fail"
  fi
  ps -ax -o pid,ppid,stat,command | awk -v p="$RPID" '$1==p || $2==p' | redact > "$RESULTS/g2-processes-before.txt" || true
  send_cmd dev.seyal.spike688.g2 check
  if wait_log "$G2LOG" "launch version=101" 180; then
    summary "G2 updated-to-101=pass"
  else
    summary "G2 updated-to-101=fail"
  fi
  sleep 1
  if kill -0 "$RPID" 2>/dev/null; then
    summary "G2 runtime-pid-alive-after-swap=yes"
  else
    summary "G2 runtime-pid-alive-after-swap=no"
  fi
  if grep -q "hello-attach existing-runtime" "$G2LOG"; then
    summary "G2 new-gui-hello-attach=pass"
  else
    summary "G2 new-gui-hello-attach=fail"
  fi
  if "$PROBE" reattach --runtime-dir "$G2DIR/runtime" --state "$RESULTS/g2-state.txt" > "$RESULTS/g2-reattach.txt" 2>&1; then
    summary "G2 probe-reattach=pass"
  else
    summary "G2 probe-reattach=fail"
  fi
  ps -ax -o pid,ppid,stat,command | awk -v p="$RPID" '$1==p || $2==p' | redact > "$RESULTS/g2-processes-after.txt" || true
  lsof -a -p "$RPID" -d txt 2>/dev/null | redact > "$RESULTS/g2-executable.txt" || true
  log show --style compact --last 8m --predicate 'eventMessage CONTAINS "CODE SIGNING" OR eventMessage CONTAINS "cs_invalid_page" OR eventMessage CONTAINS "CODESIGNING"' > "$ART/logs/g2-codesign-raw.txt" 2>&1 &
  LOGSHOW=$!
  sleep 20
  kill "$LOGSHOW" 2>/dev/null || true
  wait "$LOGSHOW" 2>/dev/null || true
  redact < "$ART/logs/g2-codesign-raw.txt" | tail -n 80 > "$RESULTS/g2-codesign-log.txt" || true
  if grep -q "$RPID" "$RESULTS/g2-codesign-log.txt"; then
    summary "G2 codesign-kill-log=seen"
  else
    summary "G2 codesign-kill-log=not-seen-for-pid"
  fi
  SWAP_EPOCH="$(date +%s)"
  echo "$RPID" > "$RESULTS/g2-runtime.pid"
  echo "$SWAP_EPOCH" > "$RESULTS/g2-swap.epoch"
  (
    while kill -0 "$RPID" 2>/dev/null; do
      now="$(date +%s)"
      kids="$(pgrep -P "$RPID" | wc -l | tr -d ' ')"
      echo "elapsed=$((now - SWAP_EPOCH)) pid=$RPID children=$kids" >> "$RESULTS/g2-soak.txt"
      sleep 20
    done
    echo "died elapsed=$(( $(date +%s) - SWAP_EPOCH ))" >> "$RESULTS/g2-soak.txt"
  ) >/dev/null 2>&1 &
  echo $! > "$RESULTS/g2-soak.pid"
fi

# --- G3 policy gates on a separate bundle ---
log "G3 policy gates"
publish_update "$APP_G3_101" 101 "$ART/keys/seed-a.b64" "min-hello=1;team-continuity=required"
run_g3() {
  name="$1"
  policy="$2"
  auto="$3"
  G3APP="$(install_tree dev.seyal.spike688.g3 "$APP_G3_100")"
  write_feed_url dev.seyal.spike688.g3 "$FEED_ORIGIN/appcast.xml"
  printf '%s\n' "$policy" > "/tmp/seyal-spike-688/dev.seyal.spike688.g3/policy.json"
  if [ "$auto" = "1" ]; then
    echo 1 > "/tmp/seyal-spike-688/dev.seyal.spike688.g3/auto-download"
  fi
  start_app "$G3APP"
  G3LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g3/harness.log"
  wait_log "$G3LOG" "updater-started" 20 || true
  send_cmd dev.seyal.spike688.g3 check
  wait_log "$G3LOG" "delegate cycle-finished|user-driver ready-choice|delegate will-install-on-quit|user-driver updater-error" 90 || true
  cp "$G3LOG" "$RESULTS/g3-${name}.log"
}

run_g3 manual-block '{"allowDownload":false,"allowReadyInstall":false,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}' 0
send_cmd dev.seyal.spike688.g3 quit || true
sleep 2
summary "G3 manual-block version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app || true

run_g3 ready-block '{"allowDownload":true,"allowReadyInstall":false,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}' 0
# Leave it running long enough for the impatient reminder interval (15s) plus slack.
sleep 25
cp /tmp/seyal-spike-688/dev.seyal.spike688.g3/harness.log "$RESULTS/g3-ready-block-after-wait.log"
send_cmd dev.seyal.spike688.g3 quit || true
sleep 3
summary "G3 ready-block-after-quit version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app || true

run_g3 install-on-quit '{"allowDownload":true,"allowReadyInstall":false,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}' 1
if wait_log /tmp/seyal-spike-688/dev.seyal.spike688.g3/harness.log "will-install-on-quit" 40; then
  summary "G3 install-on-quit-callback=seen"
else
  summary "G3 install-on-quit-callback=not-seen"
fi
send_cmd dev.seyal.spike688.g3 quit || true
sleep 4
summary "G3 install-on-quit-after-quit version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g3/SeyalSpike.app || true

# --- G5 trust ---
log "G5 trust semantics"
publish_update "$APP_G5_101_ADHOC" 101 "$ART/keys/seed-a.b64" "team=other"
G5APP="$(install_tree dev.seyal.spike688.g5 "$APP_G5_100")"
write_feed_url dev.seyal.spike688.g5 "$FEED_ORIGIN/appcast.xml"
write_policy dev.seyal.spike688.g5 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}
EOF
start_app "$G5APP"
G5LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g5/harness.log"
wait_log "$G5LOG" "updater-started" 20 || true
send_cmd dev.seyal.spike688.g5 check
if wait_log "$G5LOG" "launch version=101\\|updater-error\\|delegate abort\\|cycle-finished error" 180; then
  true
fi
sleep 2
cp "$G5LOG" "$RESULTS/g5-adhoc.log"
summary "G5 adhoc-update version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app || true

# Host pre-install team gate against the same ad-hoc archive.
G5APP="$(install_tree dev.seyal.spike688.g5 "$APP_G5_100")"
write_feed_url dev.seyal.spike688.g5 "$FEED_ORIGIN/appcast.xml"
write_policy dev.seyal.spike688.g5 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":true,"blockProceed":false}
EOF
start_app "$G5APP"
G5LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g5/harness.log"
wait_log "$G5LOG" "updater-started" 20 || true
send_cmd dev.seyal.spike688.g5 check
wait_log "$G5LOG" "team-continuity-blocked\\|updater-error\\|delegate abort\\|cycle-finished" 180 || true
sleep 1
cp "$G5LOG" "$RESULTS/g5-team-gate.log"
summary "G5 host-team-gate version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app || true

# EdDSA rotation: archive signed by key A, new app carries key B, Apple Development signature.
publish_update "$APP_G5_101_ROT" 101 "$ART/keys/seed-a.b64" "eddsa-rotation=key-b"
G5APP="$(install_tree dev.seyal.spike688.g5 "$APP_G5_100")"
write_feed_url dev.seyal.spike688.g5 "$FEED_ORIGIN/appcast.xml"
write_policy dev.seyal.spike688.g5 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}
EOF
start_app "$G5APP"
G5LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g5/harness.log"
wait_log "$G5LOG" "updater-started" 20 || true
send_cmd dev.seyal.spike688.g5 check
wait_log "$G5LOG" "launch version=101" 180 || true
sleep 1
summary "G5 rotation-step1 version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app 2>/dev/null || echo missing)"
# Second hop signed only by key B, which the rotated app now embeds.
publish_update "$APP_G5_102" 102 "$ART/keys/seed-b.b64" "eddsa-rotation=settled"
# The running 101 process reads the feed URL file on the next check.
send_cmd dev.seyal.spike688.g5 check || true
wait_log "$G5LOG" "launch version=102" 180 || true
sleep 1
cp "$G5LOG" "$RESULTS/g5-rotation.log"
summary "G5 rotation-step2 version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app || true

# Lost-key fallback: archive is not signed by key A. Apple Development is not Developer ID.
publish_update "$APP_G5_102" 102 "$ART/keys/seed-b.b64" "lost-key-fallback" "$ART/keys/seed-a.b64"
G5APP="$(install_tree dev.seyal.spike688.g5 "$APP_G5_100")"
write_feed_url dev.seyal.spike688.g5 "$FEED_ORIGIN/appcast.xml"
write_policy dev.seyal.spike688.g5 <<'EOF'
{"allowDownload":true,"allowReadyInstall":true,"callInstallOnQuitHandler":false,"enforceTeamContinuity":false,"blockProceed":false}
EOF
start_app "$G5APP"
G5LOG="/tmp/seyal-spike-688/dev.seyal.spike688.g5/harness.log"
wait_log "$G5LOG" "updater-started" 20 || true
send_cmd dev.seyal.spike688.g5 check
wait_log "$G5LOG" "updater-error\\|delegate abort\\|cycle-finished error\\|launch version=102" 120 || true
sleep 1
cp "$G5LOG" "$RESULTS/g5-lost-key.log"
summary "G5 lost-key-fallback version=$(bundle_version /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app 2>/dev/null || echo missing)"
stop_app /tmp/seyal-spike-688/dev.seyal.spike688.g5/SeyalSpike.app || true

if [ -f "$RESULTS/g2-soak.txt" ]; then
  tail -n 5 "$RESULTS/g2-soak.txt" >> "$RESULTS/summary.txt"
fi
log "gates finished"
cat "$RESULTS/summary.txt"
