#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "[seyal macOS build] skipped: Seyal.app builds only on macOS."
  exit 0
fi

bash scripts/check-macos-toolchain.sh

CONFIGURATION="${SEYAL_MACOS_CONFIGURATION:-Debug}"
case "$CONFIGURATION" in
  Debug|Release) ;;
  *)
    echo "[seyal macOS build] unsupported configuration: $CONFIGURATION" >&2
    exit 64
    ;;
esac

# M001 builds the native app for the active host architecture. The Rust static
# library and Xcode target must agree; asking Xcode for an additional slice
# would require a separately cross-compiled Rust archive and a deliberate
# universal-binary packaging contract, which is outside Pass 6.
MACOS_ARCH="${SEYAL_MACOS_ARCH:-$(uname -m)}"
case "$MACOS_ARCH" in
  arm64|x86_64) ;;
  *)
    echo "[seyal macOS build] unsupported architecture: $MACOS_ARCH" >&2
    exit 64
    ;;
esac

channel="$(sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' rust-toolchain.toml | head -n1)"
[[ -n "$channel" ]] || { echo "rust-toolchain.toml does not declare a Rust channel" >&2; exit 1; }

rust_host_arch="$(rustup run "$channel" rustc -vV | sed -nE 's/^host: ([^-]+)-.*/\1/p')"
case "$rust_host_arch" in
  aarch64) rust_xcode_arch=arm64 ;;
  x86_64) rust_xcode_arch=x86_64 ;;
  *) rust_xcode_arch="$rust_host_arch" ;;
esac
[[ "$rust_xcode_arch" == "$MACOS_ARCH" ]] || {
  echo "[seyal macOS build] Rust host architecture $rust_host_arch ($rust_xcode_arch for Xcode) does not match requested Xcode architecture $MACOS_ARCH" >&2
  exit 1
}

cargo_args=(build -p seyal-client -p seyal-runtime --locked)
# C ABI linked into Swift: never unwind across the bridge (see seyal-client::ffi).
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C panic=abort"
if [[ "$CONFIGURATION" == "Release" ]]; then
  cargo_args+=(--release)
fi
rustup run "$channel" cargo "${cargo_args[@]}"

DERIVED_DATA="${ROOT}/target/macos-derived-data"
SWIFT_ACTIVE_COMPILATION_CONDITIONS='$(inherited)'
if [[ "$CONFIGURATION" == "Debug" ]]; then
  SWIFT_ACTIVE_COMPILATION_CONDITIONS='DEBUG $(inherited)'
fi

xcodebuild \
  -project macos/Seyal/Seyal.xcodeproj \
  -scheme Seyal \
  -configuration "$CONFIGURATION" \
  -derivedDataPath "$DERIVED_DATA" \
  ARCHS="$MACOS_ARCH" \
  ONLY_ACTIVE_ARCH=YES \
  CODE_SIGNING_ALLOWED=NO \
  SWIFT_ACTIVE_COMPILATION_CONDITIONS="$SWIFT_ACTIVE_COMPILATION_CONDITIONS" \
  build

# Runtime is a permanent bundled helper. Keep the launch path inside the
# signed app bundle; the GUI must never discover or launch an arbitrary shell
# command as a substitute for this helper.
APP_BUNDLE="${DERIVED_DATA}/Build/Products/${CONFIGURATION}/Seyal.app"
HELPERS_DIR="${APP_BUNDLE}/Contents/Helpers"
mkdir -p "$HELPERS_DIR"
RUNTIME_BINARY="${ROOT}/target/$([[ "$CONFIGURATION" == "Release" ]] && echo release || echo debug)/seyal-runtime"
[[ -x "$RUNTIME_BINARY" ]] || { echo "bundled Runtime binary is missing: $RUNTIME_BINARY" >&2; exit 1; }
install -m 755 "$RUNTIME_BINARY" "${HELPERS_DIR}/seyal-runtime"
[[ -x "${HELPERS_DIR}/seyal-runtime" ]] || { echo "failed to package bundled Runtime helper" >&2; exit 1; }

# xcodebuild test can leave hollow SeyalTests.xctest under Contents/PlugIns in
# the shared derived-data app. That is not a production subcomponent and makes
# codesign report "bundle format unrecognized". Strip before sealing.
PLUGINS_DIR="${APP_BUNDLE}/Contents/PlugIns"
if [[ -d "$PLUGINS_DIR" ]]; then
  find "$PLUGINS_DIR" -maxdepth 1 -name '*.xctest' -exec rm -rf {} +
  rmdir "$PLUGINS_DIR" 2>/dev/null || true
fi

# Sign the nested helper before sealing the outer app. Debug's ad-hoc allowance
# is compiled out of Release; distribution builds must name an Apple-issued
# identity whose Team ID is inherited by both signatures.
if [[ "$CONFIGURATION" == "Debug" ]]; then
  CODESIGN_IDENTITY="-"
  codesign --force --sign "$CODESIGN_IDENTITY" --identifier dev.seyal.Seyal.runtime \
    --timestamp=none "${HELPERS_DIR}/seyal-runtime"
  codesign --force --sign "$CODESIGN_IDENTITY" --identifier dev.seyal.Seyal \
    --timestamp=none "$APP_BUNDLE"
else
  CODESIGN_IDENTITY="${SEYAL_CODESIGN_IDENTITY:-}"
  [[ -n "$CODESIGN_IDENTITY" ]] || {
    echo "Release packaging requires SEYAL_CODESIGN_IDENTITY" >&2
    exit 1
  }
  codesign --force --sign "$CODESIGN_IDENTITY" --identifier dev.seyal.Seyal.runtime \
    --options runtime --timestamp "${HELPERS_DIR}/seyal-runtime"
  codesign --force --sign "$CODESIGN_IDENTITY" --identifier dev.seyal.Seyal \
    --options runtime --timestamp "$APP_BUNDLE"
fi

codesign --verify --strict --all-architectures "${HELPERS_DIR}/seyal-runtime"
codesign --verify --strict --all-architectures "$APP_BUNDLE"
helper_identifier="$(codesign -dvv "${HELPERS_DIR}/seyal-runtime" 2>&1 | sed -n 's/^Identifier=//p')"
[[ "$helper_identifier" == "dev.seyal.Seyal.runtime" ]] || {
  echo "bundled Runtime helper has unexpected identifier: $helper_identifier" >&2
  exit 1
}

printf '[seyal macOS build] built %s (%s)\n' "${DERIVED_DATA}/Build/Products/${CONFIGURATION}/Seyal.app" "$MACOS_ARCH"
