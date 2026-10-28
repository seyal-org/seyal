#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "[seyal macOS UI test] skipped: native XCTest/XCUIAutomation requires macOS."
  exit 0
fi

bash scripts/check-macos-toolchain.sh

# The Pass 8 native application test starts the real, separately owned Runtime
# binary and then executes Seyal.app against it. Build the fixture through the
# pinned repository toolchain so `make ui-test` is self-contained.
channel="$(sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' rust-toolchain.toml | head -n1)"
[[ -n "$channel" ]] || { echo "rust-toolchain.toml does not declare a Rust channel" >&2; exit 1; }
# `--bin seyal-runtime` filters the whole cargo invocation, so `-p seyal-client`
# is skipped and Xcode links a stale `libseyal_client.a`. Build the staticlib
# and the Runtime helper as separate selections.
rustup run "$channel" cargo build -p seyal-client --locked
rustup run "$channel" cargo build -p seyal-runtime --bin seyal-runtime --locked

DERIVED_DATA="$ROOT/target/macos-ui-tests"
RESULT_BUNDLE="$ROOT/target/macos-ui-tests.xcresult"
rm -rf "$DERIVED_DATA" "$RESULT_BUNDLE"

xcodebuild \
  -project macos/Seyal/Seyal.xcodeproj \
  -scheme Seyal \
  -configuration Debug \
  -destination 'platform=macOS' \
  -derivedDataPath "$DERIVED_DATA" \
  ARCHS=arm64 \
  ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO \
  SWIFT_ACTIVE_COMPILATION_CONDITIONS='DEBUG $(inherited)' \
  build-for-testing

# Xcode's no-signing mode is required for reproducible local Rust linking, but
# macOS will refuse to load its generated XCTest/XCUI runner as an unsigned
# bundle. Sign only the disposable DerivedData products ad hoc, then execute
# the already-built test plan. This does not alter production signing.
#
# build-for-testing does not run scripts/build-macos.sh, so the permanent
# Contents/Helpers/seyal-runtime must be installed here. Identifier-preserving
# ad-hoc signatures are mandatory: BundledRuntimeLauncher rejects any helper
# whose codesign identifier is not exactly `dev.seyal.Seyal.runtime`, and a
# failed helper launch during endpointMissing permanently blocks recovery
# (including attach to an already-running external Runtime).
PRODUCTS="$DERIVED_DATA/Build/Products/Debug"
APP_BUNDLE="$PRODUCTS/Seyal.app"
HELPERS_DIR="$APP_BUNDLE/Contents/Helpers"
RUNTIME_BINARY="$ROOT/target/debug/seyal-runtime"
[[ -x "$RUNTIME_BINARY" ]] || { echo "missing seyal-runtime fixture: $RUNTIME_BINARY" >&2; exit 1; }
mkdir -p "$HELPERS_DIR"
install -m 755 "$RUNTIME_BINARY" "$HELPERS_DIR/seyal-runtime"
codesign --force --sign - --identifier dev.seyal.Seyal.runtime --timestamp=none \
  "$HELPERS_DIR/seyal-runtime"
codesign --force --sign - --identifier dev.seyal.Seyal --timestamp=none \
  "$APP_BUNDLE"
codesign --force --deep --sign - "$PRODUCTS/SeyalUITests-Runner.app"
helper_identifier="$(codesign -dvv "$HELPERS_DIR/seyal-runtime" 2>&1 | sed -n 's/^Identifier=//p')"
[[ "$helper_identifier" == "dev.seyal.Seyal.runtime" ]] || {
  echo "UI-test Runtime helper has unexpected identifier: $helper_identifier" >&2
  exit 1
}
codesign --verify --strict --all-architectures "$HELPERS_DIR/seyal-runtime"
codesign --verify --strict "$APP_BUNDLE"
codesign --verify --deep --strict "$PRODUCTS/SeyalUITests-Runner.app"

# xcodebuild only injects TEST_RUNNER_*-prefixed vars into the XCUI runner
# (prefix stripped). Forward the plain Gate 1 stress opt-in so
# `SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1 make ui-test` actually runs the 10-cycle
# case instead of silently XCTSkipping while the suite still reports PASS.
XCODEBUILD_TEST_ENV=()
if [[ "${SEYAL_UI_TEST_CLEAR_EXIT_STRESS:-}" == "1" ]]; then
  XCODEBUILD_TEST_ENV+=(TEST_RUNNER_SEYAL_UI_TEST_CLEAR_EXIT_STRESS=1)
fi

xcodebuild \
  -project macos/Seyal/Seyal.xcodeproj \
  -scheme Seyal \
  -configuration Debug \
  -destination 'platform=macOS' \
  -derivedDataPath "$DERIVED_DATA" \
  ARCHS=arm64 \
  ONLY_ACTIVE_ARCH=YES \
  -resultBundlePath "$RESULT_BUNDLE" \
  "${XCODEBUILD_TEST_ENV[@]}" \
  test-without-building

echo "[seyal macOS UI test] XCTest component + XCUIAutomation E2E passed."
