#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cmd="${1:-}"
channel="$(sed -nE 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' rust-toolchain.toml 2>/dev/null | head -n1 || true)"

cargo_pinned() {
  [[ -n "$channel" ]] || { echo "rust-toolchain.toml does not declare a Rust channel" >&2; exit 1; }
  rustup run "$channel" cargo "$@"
}

runtime_failure_matrix() {
  cargo_pinned test -p seyal-runtime --locked --features test-fault-injection \
    --test local_ipc_failure_injection \
    --test runtime_adversarial \
    --test pass8_block_failures \
    --test display_publish_bookkeeping
  cargo_pinned test -p seyal-exec --locked --features test-fault-injection \
    --test macos_pty
}

case "$cmd" in
  bootstrap)
    bash scripts/bootstrap-toolchain.sh
    ;;
  bootstrap-agents)
    bash scripts/bootstrap-dev.sh
    ;;
  build)
    bash scripts/check-toolchain.sh
    cargo_pinned build --workspace --locked
    bash scripts/build-macos.sh
    ;;
  test)
    bash scripts/test-tooling.sh
    python3 scripts/test-workspace.py
    python3 scripts/test-harnesses.py
    python3 scripts/fuzz-smoke.py
    bash scripts/check-toolchain.sh
    cargo_pinned test --workspace --locked
    runtime_failure_matrix
    bash scripts/test-macos-ui.sh
    ;;
  ui-test)
    bash scripts/test-macos-ui.sh
    ;;
  check)
    bash scripts/check-toolchain.sh
    bash -n scripts/*.sh
    bash scripts/validate-governance.sh
    python3 scripts/check-doc-links.py
    python3 scripts/check-layering.py
    python3 scripts/check-hot-path.py
    python3 scripts/check-benchmark-contract.py
    python3 scripts/check-m002-performance-contract.py
    python3 scripts/run-m002-performance-contract.py --self-test
    python3 scripts/check-pass5-benchmark-coverage.py --self-test
    python3 scripts/check-pass7-benchmark-coverage.py --self-test
    python3 scripts/check-pass7-validation-matrix.py --self-test
    python3 scripts/check-pass9-production-budget.py --self-test
    python3 scripts/check-pass9-merge-acceptance.py --self-test
    python3 scripts/check-ui-test-policy.py
    python3 scripts/check-host-product-fixtures.py
    python3 scripts/check-thin-swift-boundary.py
    bash scripts/test-tooling.sh
    python3 scripts/test-workspace.py
    python3 scripts/test-harnesses.py
    python3 scripts/fuzz-smoke.py
    python3 scripts/test-ci-validators.py
    python3 scripts/test-m002-app-identity.py
    python3 scripts/test-m002-controlled-mode.py
    cargo_pinned fmt --all -- --check
    cargo_pinned clippy --workspace --all-targets --all-features -- -D warnings
    cargo_pinned test --workspace --locked
    runtime_failure_matrix
    ;;
  bench)
    bash scripts/check-toolchain.sh
    python3 scripts/check-benchmark-contract.py
    python3 scripts/check-m002-performance-contract.py
    python3 scripts/benchmark-smoke.py
    if find crates -type f -path '*/benches/*.rs' -print -quit 2>/dev/null | grep -q .; then
      # Darwin Unix-domain sockets have a 104-byte sun_path limit. The production
      # benchmarks deliberately use temporary Runtime directories; keep the
      # benchmark root deterministic and short instead of inheriting a potentially
      # long hosted-runner TMPDIR. Production runtime discovery is unchanged.
      if [[ "$(uname -s)" == "Darwin" ]]; then
        export TMPDIR=/tmp
      fi
      cargo_pinned bench --workspace --locked
      if [[ "$(uname -s)" == "Darwin" ]]; then
        pass5_log="$(mktemp -t seyal-pass5-benchmark.XXXXXX)"
        trap 'rm -f "$pass5_log"' EXIT
        cargo_pinned bench -p seyal-runtime --bench pass5_production_transport --features benchmark-instrumentation --locked 2>&1 | tee "$pass5_log"
        python3 scripts/check-pass5-benchmark-coverage.py "$pass5_log"
        rm -f "$pass5_log"
        trap - EXIT

        # Pass 7 measures its own native-input/client/Runtime/PTY and correlated
        # resize boundaries. It intentionally remains separate from Pass 5
        # Candidate-D transport and Pass 6 Metal renderer evidence.
        pass7_log="$(mktemp -t seyal-pass7-benchmark.XXXXXX)"
        trap 'rm -f "$pass7_log"' EXIT
        cargo_pinned bench -p seyal-client --bench pass7_input_resize --features benchmark-instrumentation --locked 2>&1 | tee "$pass7_log"
        python3 scripts/check-pass7-benchmark-coverage.py "$pass7_log"
        rm -f "$pass7_log"
        trap - EXIT

        # The Pass 7 matrix separately proves full-action PTY completion for
        # large commits and burst/contention/alternate-screen workload classes.
        # It keeps still-unmeasured failure/AppKit boundaries explicitly
        # NOT_CLAIMED instead of allowing partial evidence to masquerade as DoD.
        pass7_matrix_log="$(mktemp -t seyal-pass7-matrix.XXXXXX)"
        trap 'rm -f "$pass7_matrix_log"' EXIT
        cargo_pinned bench -p seyal-client --bench pass7_validation_matrix --features benchmark-instrumentation --locked 2>&1 | tee "$pass7_matrix_log"
        python3 scripts/check-pass7-validation-matrix.py "$pass7_matrix_log"
        rm -f "$pass7_matrix_log"
        trap - EXIT

        # Pass 8 measures only the fixed execution-level Block metadata seam.
        # The benchmark itself enforces the accepted absolute latency/RSS and
        # retirement/idle-resource gates against the exact production value,
        # client-cache and Runtime-timeline implementations.
        cargo_pinned bench -p seyal-client --bench pass8_block_metadata --features benchmark-instrumentation --locked
      else
        echo "[seyal Pass-5 benchmark coverage] measured Candidate-D validation skipped: production benchmark is macOS-only; validator self-test is enforced by make check."
        echo "[seyal Pass-6 renderer benchmark] native Metal measurement skipped: macOS-only."
        echo "[seyal Pass-7 input/resize benchmark] native measurement skipped: macOS-only."
        echo "[seyal Pass-7 validation matrix] native measurement skipped: macOS-only."
        echo "[seyal Pass-8 Block metadata benchmark] native measurement skipped: macOS-only."
        echo "[seyal Pass-9 renderer lifecycle] native measurement skipped: macOS-only."
      fi
    else
      echo "[seyal task] bench: harness metadata recorder passed; no production benchmark target exists yet and no performance result is claimed."
    fi
    ;;
  *)
    echo "usage: $0 {bootstrap|bootstrap-agents|build|test|ui-test|check|bench}" >&2
    exit 64
    ;;
esac