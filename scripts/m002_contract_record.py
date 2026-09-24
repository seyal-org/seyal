#!/usr/bin/env python3
"""Shared M002 qualification record assembly.

Diagnostic collection remains PLATFORM_LIMITED. Controlled `--qualify`
writes a complete record.toml from candidate + distinct-baseline cohorts
and sets environment_status from live preflight. Inventory files are
never rewritten to VALID by this helper.
"""
from __future__ import annotations

import hashlib
import json
import os
import platform
import re
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "docs/evidence/M002-PERFORMANCE-CONTRACT-V1.toml"
VALIDATOR = ROOT / "scripts/check-m002-performance-contract.py"
CONTROLLED_PROVENANCE_MANIFEST_NAME = "controlled-provenance-manifest.json"
CONTROLLED_PROVENANCE_MANIFEST_SCHEMA = "seyal.m002.controlled-provenance-manifest"
HISTORY_MATRIX_LINES = (10000, 100000, 1000000)
HISTORY_MATRIX_POPULATIONS = (1, 10, 50, 100)
HISTORY_MATRIX_COLUMNS = (40, 48, 64, 80, 96, 132, 160)
HISTORY_MATRIX_WORKLOADS = ("ASCII", "styled", "CJK", "emoji-combining")
RESOURCE_GATES = {
    "resource_scaling_rss",
    "resource_scaling_fds",
    "resource_scaling_threads",
}
RESOURCE_POPULATIONS = (1, 10, 50, 100)


def run(command: list[str], *, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    merged = os.environ.copy() if env is None else env
    merged.setdefault("CARGO_TERM_COLOR", "never")
    return subprocess.run(
        command,
        cwd=ROOT,
        env=merged,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )


def toml_str(value: str) -> str:
    return json.dumps(value, ensure_ascii=True)


def load_contract() -> dict:
    return tomllib.loads(CONTRACT.read_text(encoding="utf-8"))


def history_matrix_configurations() -> list[tuple[int, int, int, str]]:
    return [
        (lines, population, columns, workload)
        for lines in HISTORY_MATRIX_LINES
        for population in HISTORY_MATRIX_POPULATIONS
        for columns in HISTORY_MATRIX_COLUMNS
        for workload in HISTORY_MATRIX_WORKLOADS
    ]


def normalize_history_workload(workload: str) -> str:
    return {
        "ASCII": "ascii",
        "ascii": "ascii",
        "styled": "styled",
        "CJK": "cjk",
        "cjk": "cjk",
        "emoji-combining": "emoji-combining",
    }[workload]


def matrix_config_id(lines: int, population: int, columns: int, workload: str) -> str:
    return f"{lines}-{population}-{columns}-{normalize_history_workload(workload)}"


def nearest_rank(values: list[float], percentile: int) -> float:
    ordered = sorted(values)
    rank = max(1, (len(ordered) * percentile + 99) // 100)
    return ordered[rank - 1]


def load_samples(directory: Path) -> list[float]:
    values: list[float] = []
    for path in sorted(directory.glob("*.toml")):
        cohort = tomllib.loads(path.read_text(encoding="utf-8"))
        samples = cohort.get("samples")
        if not isinstance(samples, list):
            raise SystemExit(f"invalid cohort file {path}")
        values.extend(float(value) for value in samples)
    if len(values) != 500:
        raise SystemExit(f"{directory} must contain exactly 500 samples across five cohorts")
    return values


def git_sha() -> str:
    result = run(["git", "rev-parse", "HEAD"])
    sha = result.stdout.strip()
    if result.returncode != 0 or len(sha) != 40:
        raise SystemExit("cannot resolve production SHA")
    return sha


def rustc_version() -> str:
    result = run(["rustc", "--version"])
    return result.stdout.strip() or "unknown"


def hardware() -> str:
    if sys.platform == "darwin":
        model = run(["sysctl", "-n", "hw.model"]).stdout.strip()
        machine = run(["uname", "-m"]).stdout.strip()
        return f"{model} {machine}".strip() or platform.platform()
    return platform.platform()


def power_thermal_state() -> str:
    override = os.environ.get("SEYAL_M002_POWER_THERMAL", "").strip()
    if override:
        return override
    if sys.platform != "darwin":
        return "uncontrolled-developer-host"
    battery = run(["pmset", "-g", "batt"])
    text = battery.stdout.casefold()
    if "discharging" in text or "battery power" in text:
        return "uncontrolled-developer-host-battery-discharging"
    if "ac power" in text or "charged" in text or "charging" in text:
        return "ac-power-developer-host"
    return "uncontrolled-developer-host"


def environment_is_valid(state: str) -> bool:
    lowered = state.casefold()
    if os.environ.get("SEYAL_M002_CONTROLLED_HOST") != "1":
        return False
    if "developer-host" in lowered:
        return False
    return not any(
        token in lowered
        for token in ("uncontrolled", "battery", "discharging", "thermal", "debug")
    )


def require_clean_worktree() -> None:
    status = run(["git", "status", "--porcelain"])
    if status.returncode != 0 or status.stdout.strip():
        raise SystemExit("qualify mode rejected a dirty worktree; freeze F must be a clean checkout")


def re_full_sha(value: str) -> bool:
    return bool(re.fullmatch(r"[0-9a-fA-F]{40}", value))


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require_cargo_release_identity() -> str:
    if os.environ.get("CARGO_PROFILE") == "dev":
        raise SystemExit("qualify mode rejected a Debug cargo profile")
    return "release"


def find_bench_binary(package: str, bench: str) -> Path | None:
    deps = ROOT / "target" / "release" / "deps"
    if not deps.is_dir():
        return None
    matches = sorted(
        (
            path
            for path in deps.glob(f"{bench}-*")
            if path.is_file() and os.access(path, os.X_OK) and path.suffix == ""
        ),
        key=lambda path: path.stat().st_mtime,
        reverse=True,
    )
    return matches[0] if matches else None


def write_matrix_manifest(path: Path, configs: list[tuple[int, int, int, str]]) -> None:
    expected = history_matrix_configurations()
    if len(configs) != 336 or set(configs) != set(expected):
        raise SystemExit("history matrix manifest must contain the accepted 336 configurations")
    lines = ["configuration_count = 336", "complete = true", "configurations = ["]
    for lines_n, population, columns, workload in configs:
        lines.append(f"  {toml_str(matrix_config_id(lines_n, population, columns, workload))},")
    lines.append("]")
    lines.append("")
    path.write_text("\n".join(lines), encoding="utf-8")


def write_controlled_provenance_manifest(
    directory: Path,
    *,
    sha: str,
    host: str = "TESTHOST-0001",
    clean: bool = True,
    ac: bool = True,
    thermal: bool = True,
    host_confirmed: bool = True,
) -> None:
    """Stamp controlled-collection provenance next to raw cohort files.

    PHYSICAL_ARM64 VALID validation re-reads this manifest; PLATFORM_LIMITED
    diagnostic rows do not require it. Callers that assemble a VALID fixture
    (including self-tests) must stamp both candidate and baseline cohorts.
    """
    directory.mkdir(parents=True, exist_ok=True)
    manifest = {
        "schema": CONTROLLED_PROVENANCE_MANIFEST_SCHEMA,
        "version": 1,
        "sha": sha,
        "clean_source_tree_confirmed": clean,
        "clean_source_tree_detail": "clean source tree" if clean else "working tree has uncommitted changes",
        "ac_power_confirmed": ac,
        "ac_power_detail": "AC Power" if ac else "host is running on battery power, not AC",
        "thermal_stability_confirmed": thermal,
        "thermal_stability_detail": "CPU_Speed_Limit=100" if thermal else "host is thermally throttled",
        "host_identity_confirmed": host_confirmed,
        "host_identity": host if host_confirmed else None,
        "collected_at": datetime.now(timezone.utc).isoformat(),
    }
    (directory / CONTROLLED_PROVENANCE_MANIFEST_NAME).write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def stamp_cohort_commits(directory: Path, sha: str) -> None:
    """Bind every cohort.toml under directory to sha via a `commit` field."""
    if not re_full_sha(sha):
        raise SystemExit(f"cannot stamp cohort commits with non-SHA value {sha!r}")
    for path in sorted(directory.glob("*.toml")):
        text = path.read_text(encoding="utf-8")
        if re.search(r"(?m)^commit = ", text):
            path.write_text(
                re.sub(r"(?m)^commit = .*$\n?", f"commit = {toml_str(sha)}\n", text, count=1),
                encoding="utf-8",
            )
        elif text.startswith("cohort = "):
            first, _, rest = text.partition("\n")
            path.write_text(f"{first}\ncommit = {toml_str(sha)}\n{rest}", encoding="utf-8")
        else:
            path.write_text(f"commit = {toml_str(sha)}\n{text}", encoding="utf-8")


def prepare_controlled_cohort_bundle(directory: Path, *, sha: str, host: str = "TESTHOST-0001") -> None:
    """Make a cohort directory eligible for PHYSICAL_ARM64 VALID binding checks."""
    stamp_cohort_commits(directory, sha)
    write_controlled_provenance_manifest(directory, sha=sha, host=host)


def write_qualification_record(
    *,
    gate: str,
    production_sha: str,
    baseline_sha: str,
    evidence_root: Path,
    raw_log: Path,
    candidate: Path,
    baseline: Path,
    workload: str,
    topology: str,
    display: str,
    power_thermal: str,
    qualify: bool,
    matrix_complete: bool,
    matrix_manifest: Path | None,
    binary_sha256: str,
    extra_topology: str = "",
) -> tuple[Path, str]:
    if not re_full_sha(production_sha) or not re_full_sha(baseline_sha):
        raise SystemExit("qualification records require full 40-character SHAs")
    if qualify and production_sha == baseline_sha:
        raise SystemExit("qualify mode rejected a same-SHA baseline; A/A is diagnostic only")
    contract = load_contract()
    gate_spec = contract["gates"][gate]
    candidate_values = load_samples(candidate)
    baseline_values = load_samples(baseline)
    p50, p95, p99 = (nearest_rank(candidate_values, p) for p in (50, 95, 99))
    b50, b95, b99 = (nearest_rank(baseline_values, p) for p in (50, 95, 99))
    ceilings = (gate_spec["p50"], gate_spec["p95"], gate_spec["p99"])
    allowed = gate_spec["relative_regression_percent"]
    absolute_ok = p50 <= ceilings[0] and p95 <= ceilings[1] and p99 <= ceilings[2]
    if any(base == 0 and cand > 0 for cand, base in zip((p50, p95, p99), (b50, b95, b99))):
        relative_ok = False
    else:
        relative_ok = (
            p50 <= b50 * (1 + allowed / 100)
            and p95 <= b95 * (1 + allowed / 100)
            and p99 <= b99 * (1 + allowed / 100)
        )
    numeric_status = "PASS" if absolute_ok and relative_ok else "FAIL"
    if qualify and environment_is_valid(power_thermal) and not binary_sha256:
        raise SystemExit("VALID qualification requires a hashed Release binary")
    if qualify and gate.startswith("history_") and not matrix_complete:
        raise SystemExit("history VALID qualification requires --full-matrix and a 336-configuration manifest")
    valid_env = qualify and environment_is_valid(power_thermal)
    if valid_env:
        environment_status = "VALID"
        platform_limit_reason = ""
        status = numeric_status
    else:
        environment_status = "PLATFORM_LIMITED"
        platform_limit_reason = (
            power_thermal
            if not environment_is_valid(power_thermal)
            else "diagnostic collection is not a release evaluation"
        )
        status = "PLATFORM_LIMITED"
    if matrix_complete:
        if matrix_manifest is None or not matrix_manifest.is_file():
            raise SystemExit("matrix_complete=true requires a 336-configuration manifest")
        manifest = tomllib.loads(matrix_manifest.read_text(encoding="utf-8"))
        if manifest.get("configuration_count") != 336 or not manifest.get("complete"):
            raise SystemExit("history matrix manifest is incomplete")
        configs = manifest.get("configurations")
        if not isinstance(configs, list) or len(set(configs)) != 336:
            raise SystemExit("history matrix manifest must list 336 unique configurations")
    rel = lambda path: path.relative_to(ROOT).as_posix()
    record = evidence_root / "record.toml"
    workload_hash = hashlib.sha256(workload.encode()).hexdigest()
    topology_value = topology if not extra_topology else f"{topology}; {extra_topology}"
    fields = [
        "contract_schema = 'seyal.m002.performance-contract'",
        "contract_version = 1",
        f"production_sha = {toml_str(production_sha)}",
        f"harness_sha = {toml_str(production_sha)}",
        f"baseline_sha = {toml_str(baseline_sha)}",
        "build_mode = 'release'",
        f"os_version = {toml_str(platform.platform())}",
        f"toolchain = {toml_str(rustc_version())}",
        f"hardware = {toml_str(hardware())}",
        f"display = {toml_str(display)}",
        f"power_thermal_state = {toml_str(power_thermal)}",
        f"workload_hash = {toml_str(workload_hash)}",
        f"topology = {toml_str(topology_value)}",
        f"evidence_class = {toml_str(gate_spec['evidence_class'])}",
        f"gate = {toml_str(gate)}",
        f"metric = {toml_str(gate)}",
        f"boundary = {toml_str(gate_spec['boundary'])}",
        f"unit = {toml_str(gate_spec['unit'])}",
        "percentile_method = 'nearest-rank'",
        "sample_count = 500",
        "cohort_count = 5",
        f"environment_status = {toml_str(environment_status)}",
        f"platform_limit_reason = {toml_str(platform_limit_reason)}",
        "comparator = 'less_equal'",
        f"status = {toml_str(status)}",
        f"numeric_status = {toml_str(numeric_status)}",
        f"p50 = {p50!r}",
        f"p95 = {p95!r}",
        f"p99 = {p99!r}",
        f"baseline_p50 = {b50!r}",
        f"baseline_p95 = {b95!r}",
        f"baseline_p99 = {b99!r}",
        f"relative_regression_percent = {allowed}",
        f"raw_log = {toml_str(rel(raw_log))}",
        f"raw_cohorts = {toml_str(rel(candidate) + '/')}",
        f"baseline_raw_cohorts = {toml_str(rel(baseline) + '/')}",
        f"qualification_mode = {toml_str('controlled' if qualify else 'diagnostic')}",
        f"matrix_complete = {'true' if matrix_complete else 'false'}",
        f"binary_identity_status = {toml_str('hashed-release' if binary_sha256 else 'unhashed-release-profile')}",
    ]
    if matrix_manifest is not None:
        fields.append(f"matrix_manifest = {toml_str(rel(matrix_manifest))}")
    if binary_sha256:
        fields.append(f"binary_sha256 = {toml_str(binary_sha256)}")
    fields.append("")
    record.write_text("\n".join(fields), encoding="utf-8")
    checked = run(["python3", str(VALIDATOR), "--record", str(record)])
    sys.stdout.write(checked.stdout)
    if checked.returncode != 0:
        raise SystemExit(f"validator rejected {record}:\n{checked.stdout}")
    expected = f"M002 performance result: {status} metric={gate}"
    if expected not in checked.stdout:
        raise SystemExit(f"validator did not evaluate {gate} as {status}:\n{checked.stdout}")
    return record, status
