#!/usr/bin/env python3
"""Self-test for retained M003 presentation workload fixtures (#1005).

Proves each fixture starts, terminates within its documented bound, emits every
required marker, and emits no seyal-m003-* marker outside allowed prefixes.
External child-process workloads only — no Seyal product/runtime coupling.
"""

from __future__ import annotations

import os
import pty
import re
import select
import subprocess
import sys
import time
import tomllib
from pathlib import Path

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()
FIXTURE_DIR = ROOT / "tests" / "fixtures" / "m003-presentation"
MANIFEST_PATH = FIXTURE_DIR / "manifest.toml"
MARKER_RE = re.compile(rb"seyal-m003-[^\r\n]*")


def fail(message: str) -> None:
    raise SystemExit(f"[seyal m003-presentation fixtures] ERROR: {message}")


def load_manifest() -> dict:
    if not MANIFEST_PATH.is_file():
        fail(f"missing manifest: {MANIFEST_PATH.relative_to(ROOT)}")
    with MANIFEST_PATH.open("rb") as handle:
        data = tomllib.load(handle)
    if data.get("version") != 1:
        fail(f"unsupported manifest version: {data.get('version')!r}")
    fixtures = data.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        fail("manifest fixtures list is empty")
    return data


def extract_markers(blob: bytes) -> list[str]:
    return [match.group(0).decode("utf-8", errors="replace") for match in MARKER_RE.finditer(blob)]


def run_pipe(script: Path, timeout: float, stdin_data: bytes) -> tuple[int, bytes]:
    completed = subprocess.run(
        ["/bin/bash", "--noprofile", "--norc", str(script)],
        input=stdin_data,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
        cwd=str(ROOT),
        env={
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "HOME": "/nonexistent",
            "LANG": "C",
            "LC_ALL": "C",
            "SEYAL_M003_ALTSCREEN_TIMEOUT": "2",
        },
    )
    return completed.returncode, completed.stdout + completed.stderr


def run_tty(script: Path, timeout: float, stdin_data: bytes) -> tuple[int, bytes]:
    master_fd, slave_fd = pty.openpty()
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": "/nonexistent",
        "LANG": "C",
        "LC_ALL": "C",
        "TERM": "xterm-256color",
        "COLUMNS": "80",
        "LINES": "24",
        "SEYAL_M003_ALTSCREEN_TIMEOUT": "2",
    }
    proc = subprocess.Popen(
        ["/bin/bash", "--noprofile", "--norc", str(script)],
        stdin=slave_fd,
        stdout=slave_fd,
        stderr=slave_fd,
        cwd=str(ROOT),
        env=env,
        close_fds=True,
    )
    os.close(slave_fd)

    chunks: list[bytes] = []
    deadline = time.monotonic() + timeout
    stdin_view = memoryview(stdin_data)
    stdin_offset = 0
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                proc.kill()
                proc.wait(timeout=2)
                fail(f"{script.name} exceeded timeout {timeout}s")
            readable, writable, _ = select.select(
                [master_fd],
                [master_fd] if stdin_offset < len(stdin_view) else [],
                [],
                min(remaining, 0.2),
            )
            if master_fd in readable:
                try:
                    data = os.read(master_fd, 4096)
                except OSError:
                    data = b""
                if not data:
                    break
                chunks.append(data)
            if master_fd in writable and stdin_offset < len(stdin_view):
                try:
                    written = os.write(master_fd, stdin_view[stdin_offset : stdin_offset + 256])
                except OSError:
                    written = 0
                stdin_offset += written
            if proc.poll() is not None and not readable:
                # Drain any remaining buffered output once the child exits.
                drain_deadline = time.monotonic() + 0.5
                while time.monotonic() < drain_deadline:
                    ready, _, _ = select.select([master_fd], [], [], 0.05)
                    if not ready:
                        break
                    try:
                        data = os.read(master_fd, 4096)
                    except OSError:
                        break
                    if not data:
                        break
                    chunks.append(data)
                break
    finally:
        os.close(master_fd)
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=2)

    code = proc.wait(timeout=2)
    return code, b"".join(chunks)


def validate_fixture(entry: dict) -> None:
    fixture_id = entry.get("id", "<missing-id>")
    script_name = entry.get("script")
    if not isinstance(script_name, str) or not script_name:
        fail(f"{fixture_id}: missing script")
    script = FIXTURE_DIR / script_name
    if not script.is_file():
        fail(f"{fixture_id}: missing script file {script_name}")

    syntax = subprocess.run(
        ["/bin/bash", "-n", str(script)],
        check=False,
        capture_output=True,
        text=True,
    )
    if syntax.returncode != 0:
        fail(f"{fixture_id}: bash -n failed: {syntax.stderr.strip()}")

    timeout = float(entry.get("timeout_seconds", 10))
    expected_exit = int(entry.get("expected_exit_code", 0))
    max_bytes = int(entry.get("max_output_bytes", 65536))
    needs_tty = bool(entry.get("needs_tty", False))
    stdin = entry.get("stdin", "")
    if not isinstance(stdin, str):
        fail(f"{fixture_id}: stdin must be a string")
    stdin_data = stdin.encode("utf-8")

    if needs_tty:
        code, blob = run_tty(script, timeout, stdin_data)
    else:
        try:
            code, blob = run_pipe(script, timeout, stdin_data)
        except subprocess.TimeoutExpired as exc:
            fail(f"{fixture_id}: exceeded timeout {timeout}s ({exc})")

    if code != expected_exit:
        fail(f"{fixture_id}: expected exit {expected_exit}, got {code}")
    if len(blob) > max_bytes:
        fail(f"{fixture_id}: output {len(blob)} bytes exceeds max_output_bytes={max_bytes}")

    markers = extract_markers(blob)
    if not markers:
        fail(f"{fixture_id}: produced no seyal-m003-* markers")

    allowed_prefixes = entry.get("allowed_marker_prefixes", [])
    if not isinstance(allowed_prefixes, list) or not allowed_prefixes:
        fail(f"{fixture_id}: allowed_marker_prefixes missing")
    for marker in markers:
        if not any(marker.startswith(prefix) for prefix in allowed_prefixes):
            fail(f"{fixture_id}: undocumented marker {marker!r}")

    required = entry.get("required_markers", [])
    if not isinstance(required, list) or not required:
        fail(f"{fixture_id}: required_markers missing")
    joined = "\n".join(markers)
    for needle in required:
        if needle not in joined and not any(marker.startswith(needle) for marker in markers):
            # Exact line match preferred; prefix match for cols=/rows= templates.
            if not any(needle in marker for marker in markers):
                fail(f"{fixture_id}: missing required marker {needle!r}; saw {markers!r}")

    for group in entry.get("optional_marker_groups", []) or []:
        if not isinstance(group, list) or len(group) < 2:
            fail(f"{fixture_id}: optional_marker_groups entries need ≥2 markers")
        hits = [item for item in group if item in markers]
        if len(hits) != 1:
            fail(f"{fixture_id}: expected exactly one of {group!r}, got {hits!r}")

    hard_lines = entry.get("hard_line_count")
    if hard_lines is not None:
        body = [m for m in markers if m.startswith("seyal-m003-long line=")]
        if len(body) != int(hard_lines):
            fail(f"{fixture_id}: expected {hard_lines} long-output body lines, got {len(body)}")
        for index, marker in enumerate(body, start=1):
            expected = f"seyal-m003-long line={index:04d}"
            if marker != expected:
                fail(f"{fixture_id}: line {index} was {marker!r}, expected {expected!r}")

    print(f"[seyal m003-presentation fixtures] {fixture_id}: ok ({len(markers)} markers, {len(blob)} bytes)")


def main() -> int:
    data = load_manifest()
    for entry in data["fixtures"]:
        if not isinstance(entry, dict):
            fail("fixture entry must be a table")
        validate_fixture(entry)
    print(
        f"[seyal m003-presentation fixtures] self-test passed "
        f"({len(data['fixtures'])} fixtures under {FIXTURE_DIR.relative_to(ROOT)})."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
