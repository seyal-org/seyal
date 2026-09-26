#!/usr/bin/env python3
from __future__ import annotations

import os
from pathlib import Path

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()

REQUIRED_TOKENS = (
    "performance_claim=false",
    "Instant::now()",
)

UNICODE_RENDERER_TOKENS = (
    "m002_unicode_renderer",
    "baseline_unicode_pipeline=UNSUPPORTED_NONCOMPARABLE",
    "cache_phase=cold_miss",
    "cache_phase=warm_hit",
    "full_shaping_ns=",
    "font_fallback_runs=",
    "process_cpu_ns=",
    "process_rss_bytes=",
)


def main() -> None:
    errors: list[str] = []
    bench_files = sorted((ROOT / "crates").glob("*/benches/*.rs")) if (ROOT / "crates").exists() else []
    if not bench_files:
        errors.append("no production Rust benchmark targets found under crates/*/benches")
    for path in bench_files:
        source = path.read_text(encoding="utf-8")
        rel = path.relative_to(ROOT)
        for token in REQUIRED_TOKENS:
            if token not in source:
                errors.append(f"{rel} missing benchmark contract token {token!r}")
        if "performance_claim=true" in source:
            errors.append(f"{rel} must not make an unverified product performance claim")

    native_renderer = ROOT / "macos/Seyal/Sources/RendererValidation.swift"
    if native_renderer.exists():
        # Tokens may live in sibling responsibility files after cohesion splits;
        # the façade path must still exist as the public harness entry.
        sibling_sources = sorted(native_renderer.parent.glob("RendererValidation*.swift"))
        source = "\n".join(path.read_text(encoding="utf-8") for path in sibling_sources)
        rel = native_renderer.relative_to(ROOT)
        for token in UNICODE_RENDERER_TOKENS:
            if token not in source:
                errors.append(f"{rel} missing M002 Unicode renderer benchmark token {token!r}")
        if "m002_unicode_renderer performance_claim=true" in source:
            errors.append(f"{rel} must not make an unverified M002 performance claim")

    if errors:
        print("Benchmark contract violations:")
        for error in errors:
            print(f"  {error}")
        raise SystemExit(1)

    print(f"Benchmark contracts passed ({len(bench_files)} target(s)).")


if __name__ == "__main__":
    main()
