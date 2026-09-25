#!/usr/bin/env python3
"""Structural-debt ratchet for handwritten production Rust and macOS Swift.

Enforces AGENTS.md module-cohesion signals as a continuous repository gate:

- new handwritten production file > HARD_LOC fails;
- grandfathered >HARD_LOC debt may remain only at a checked-in ceiling;
- a PR may not raise a ceiling without a narrowly scoped exception;
- reductions must lower the checked-in ceiling (ratchet down);
- changed files in the REVIEW_LOC..HARD_LOC band need an acknowledgement;
- arbitrary partN / numeric fragmentation names are rejected.

This gate measures structural size only. It does not authorize hot-path
indirection, new crates, or mechanical splits merely to satisfy LOC.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - Python < 3.11
    import tomli as tomllib  # type: ignore

DEFAULT_ROOT = Path(__file__).resolve().parents[1]
ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", DEFAULT_ROOT)).resolve()

SCHEMA = "seyal.structural-debt-baseline"
BASELINE_REL = Path("docs/engineering/structural-debt-baseline.toml")
REVIEW_LOC = 700
HARD_LOC = 1000

EXCLUDE_DIR_PARTS = frozenset(
    {
        "tests",
        "benches",
        "examples",
        "target",
        "Fixtures",
        "fixtures",
        "testdata",
        "test_data",
        "generated",
        "vendor",
        "third_party",
    }
)
EXCLUDE_NAME_MARKERS = ("generated",)
ARBITRARY_SPLIT_RE = re.compile(
    r"(^|[_-])part\d+([._-]|$)|(^|[_-])fragment\d+([._-]|$)",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class Grandfathered:
    path: str
    ceiling_loc: int
    issue: int
    note: str


@dataclass(frozen=True)
class ExceptionEntry:
    path: str
    ceiling_loc: int
    issue: int
    responsibility: str


@dataclass(frozen=True)
class CohesionAck:
    path: str
    issue: int
    reason: str


@dataclass(frozen=True)
class Baseline:
    grandfathered: dict[str, Grandfathered]
    exceptions: dict[str, ExceptionEntry]
    cohesion: dict[str, CohesionAck]


@dataclass(frozen=True)
class SourceFile:
    path: str
    loc: int
    changed: bool


def count_loc(path: Path) -> int:
    # Physical line count (including blanks/comments) — deterministic and
    # matches the inventory contributors see in editors/`wc -l`.
    with path.open("rb") as handle:
        return sum(1 for _ in handle)


def is_excluded(rel: Path) -> bool:
    if any(part in EXCLUDE_DIR_PARTS for part in rel.parts):
        return True
    name = rel.name
    low = name.lower()
    if any(marker in low for marker in EXCLUDE_NAME_MARKERS):
        return True
    if low.endswith(("_generated.rs", "_generated.swift")):
        return True
    if low.endswith(("_test.rs", "_tests.rs")) or low.startswith("test_"):
        return True
    if low.endswith(("tests.swift", "test.swift")):
        return True
    return False


def is_arbitrary_split_name(path: str) -> bool:
    stem = Path(path).stem
    return ARBITRARY_SPLIT_RE.search(stem) is not None


def iter_production_sources(root: Path) -> list[Path]:
    found: list[Path] = []
    crates = root / "crates"
    if crates.is_dir():
        for path in crates.rglob("*.rs"):
            rel = path.relative_to(root)
            if "src" not in rel.parts:
                continue
            if is_excluded(rel):
                continue
            found.append(path)
    sources = root / "macos" / "Seyal" / "Sources"
    if sources.is_dir():
        for path in sources.rglob("*.swift"):
            rel = path.relative_to(root)
            if is_excluded(rel):
                continue
            found.append(path)
    return sorted(found)


def load_baseline(path: Path) -> Baseline:
    if not path.is_file():
        raise SystemExit(f"Structural-debt ratchet failed: missing baseline {path}")
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    if data.get("schema") != SCHEMA:
        raise SystemExit(
            f"Structural-debt ratchet failed: baseline schema must be {SCHEMA!r}"
        )
    grandfathered: dict[str, Grandfathered] = {}
    for row in data.get("grandfathered", []):
        entry = Grandfathered(
            path=str(row["path"]),
            ceiling_loc=int(row["ceiling_loc"]),
            issue=int(row["issue"]),
            note=str(row.get("note", "")).strip(),
        )
        if entry.ceiling_loc < HARD_LOC:
            raise SystemExit(
                f"Structural-debt ratchet failed: grandfathered {entry.path} "
                f"ceiling_loc={entry.ceiling_loc} must be >= {HARD_LOC}"
            )
        if entry.issue <= 0 or not entry.note:
            raise SystemExit(
                f"Structural-debt ratchet failed: grandfathered {entry.path} "
                "requires positive issue and non-empty note"
            )
        if entry.path in grandfathered:
            raise SystemExit(
                f"Structural-debt ratchet failed: duplicate grandfathered path {entry.path}"
            )
        grandfathered[entry.path] = entry

    exceptions: dict[str, ExceptionEntry] = {}
    for row in data.get("exception", []):
        entry = ExceptionEntry(
            path=str(row["path"]),
            ceiling_loc=int(row["ceiling_loc"]),
            issue=int(row["issue"]),
            responsibility=str(row.get("responsibility", "")).strip(),
        )
        if entry.issue <= 0 or not entry.responsibility:
            raise SystemExit(
                f"Structural-debt ratchet failed: exception {entry.path} "
                "requires positive issue and non-empty responsibility"
            )
        if entry.path in exceptions:
            raise SystemExit(
                f"Structural-debt ratchet failed: duplicate exception path {entry.path}"
            )
        exceptions[entry.path] = entry

    cohesion: dict[str, CohesionAck] = {}
    for row in data.get("cohesion_acknowledgement", []):
        entry = CohesionAck(
            path=str(row["path"]),
            issue=int(row["issue"]),
            reason=str(row.get("reason", "")).strip(),
        )
        if entry.issue <= 0 or not entry.reason:
            raise SystemExit(
                f"Structural-debt ratchet failed: cohesion acknowledgement "
                f"{entry.path} requires positive issue and non-empty reason"
            )
        if entry.path in cohesion:
            raise SystemExit(
                f"Structural-debt ratchet failed: duplicate cohesion path {entry.path}"
            )
        cohesion[entry.path] = entry

    return Baseline(
        grandfathered=grandfathered, exceptions=exceptions, cohesion=cohesion
    )


def resolve_changed_paths(root: Path) -> set[str] | None:
    """Return changed production-relative paths, or None when unavailable.

    Override with SEYAL_STRUCTURAL_DEBT_CHANGED_FILES (comma-separated) for
    fixtures. Otherwise diff against SEYAL_STRUCTURAL_DEBT_BASE_SHA /
    SEYAL_STRUCTURAL_DEBT_BASE_REF / origin/master merge-base when git works.
    """
    override = os.environ.get("SEYAL_STRUCTURAL_DEBT_CHANGED_FILES")
    if override is not None:
        return {item.strip().replace("\\", "/") for item in override.split(",") if item.strip()}

    base_sha = os.environ.get("SEYAL_STRUCTURAL_DEBT_BASE_SHA", "").strip()
    base_ref = os.environ.get("SEYAL_STRUCTURAL_DEBT_BASE_REF", "").strip()
    if not base_sha and base_ref:
        base_sha = base_ref
    if not base_sha:
        github_base = os.environ.get("GITHUB_BASE_REF", "").strip()
        if github_base:
            base_sha = f"origin/{github_base}"
    if not base_sha:
        base_sha = "origin/master"

    try:
        merge_base = subprocess.check_output(
            ["git", "merge-base", "HEAD", base_sha],
            cwd=root,
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
        names = subprocess.check_output(
            ["git", "diff", "--name-only", f"{merge_base}...HEAD"],
            cwd=root,
            text=True,
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    return {line.strip().replace("\\", "/") for line in names.splitlines() if line.strip()}


def inventory(root: Path, changed: set[str] | None) -> list[SourceFile]:
    files: list[SourceFile] = []
    for path in iter_production_sources(root):
        rel = str(path.relative_to(root)).replace("\\", "/")
        loc = count_loc(path)
        is_changed = True if changed is None else rel in changed
        files.append(SourceFile(path=rel, loc=loc, changed=is_changed))
    return files


def resolution_hint(kind: str) -> str:
    if kind == "new_hard":
        return (
            "decompose by responsibility before merge, or add a narrowly scoped "
            "[[exception]] citing an owning Issue and cohesive responsibility"
        )
    if kind == "grow":
        return (
            "reduce the file below the checked-in ceiling, or add a narrowly scoped "
            "[[exception]] that raises the ceiling with an owning Issue"
        )
    if kind == "ratchet_down":
        return (
            "lower ceiling_loc in docs/engineering/structural-debt-baseline.toml "
            "to the new size (reductions must tighten the ratchet)"
        )
    if kind == "cohesion":
        return (
            "add a [[cohesion_acknowledgement]] entry with owning Issue + reason, "
            "or decompose by responsibility; do not invent part1/part2 splits"
        )
    if kind == "split":
        return (
            "rename/restructure by cohesive responsibility; arbitrary partN / "
            "fragmentN file splits are rejected"
        )
    if kind == "stale":
        return "remove the stale baseline/exception/acknowledgement entry"
    return "see docs/engineering/structural-debt-baseline.toml and AGENTS.md"


def evaluate(files: list[SourceFile], baseline: Baseline) -> list[str]:
    errors: list[str] = []
    present = {item.path for item in files}

    for path, entry in sorted(baseline.grandfathered.items()):
        if path not in present:
            errors.append(
                f"{path}: stale grandfathered entry (file missing); "
                f"ceiling={entry.ceiling_loc}; {resolution_hint('stale')}"
            )
    for path, entry in sorted(baseline.exceptions.items()):
        if path not in present:
            errors.append(
                f"{path}: stale exception entry (file missing); "
                f"ceiling={entry.ceiling_loc} issue=#{entry.issue}; "
                f"{resolution_hint('stale')}"
            )
    for path, entry in sorted(baseline.cohesion.items()):
        if path not in present:
            errors.append(
                f"{path}: stale cohesion acknowledgement (file missing); "
                f"issue=#{entry.issue}; {resolution_hint('stale')}"
            )

    for item in files:
        if is_arbitrary_split_name(item.path):
            errors.append(
                f"{item.path}: arbitrary numeric split name rejected "
                f"(loc={item.loc}); {resolution_hint('split')}"
            )

        exception = baseline.exceptions.get(item.path)
        grandfathered = baseline.grandfathered.get(item.path)
        ceiling = None
        if exception is not None:
            ceiling = exception.ceiling_loc
        elif grandfathered is not None:
            ceiling = grandfathered.ceiling_loc

        if ceiling is not None:
            if item.loc > ceiling:
                errors.append(
                    f"{item.path}: exceeds accepted ceiling "
                    f"(loc={item.loc} > ceiling={ceiling}); {resolution_hint('grow')}"
                )
            elif grandfathered is not None and exception is None and item.loc < ceiling:
                errors.append(
                    f"{item.path}: reduced below grandfathered ceiling "
                    f"(loc={item.loc} < ceiling={ceiling}); "
                    f"{resolution_hint('ratchet_down')}"
                )
            continue

        if item.loc > HARD_LOC:
            errors.append(
                f"{item.path}: new handwritten production file exceeds {HARD_LOC} LOC "
                f"(loc={item.loc}); {resolution_hint('new_hard')}"
            )
            continue

        if item.changed and REVIEW_LOC <= item.loc <= HARD_LOC:
            if item.path not in baseline.cohesion:
                errors.append(
                    f"{item.path}: changed file in cohesion-review band "
                    f"({REVIEW_LOC}-{HARD_LOC} LOC, loc={item.loc}) lacks "
                    f"[[cohesion_acknowledgement]]; {resolution_hint('cohesion')}"
                )

    return errors


def write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def write_baseline(
    path: Path,
    *,
    grandfathered: list[tuple[str, int]] | None = None,
    exceptions: list[tuple[str, int, int, str]] | None = None,
    cohesion: list[tuple[str, int, str]] | None = None,
) -> None:
    lines = [
        f'schema = "{SCHEMA}"',
        "version = 1",
        f"review_loc = {REVIEW_LOC}",
        f"hard_loc = {HARD_LOC}",
        "",
    ]
    for file_path, ceiling in grandfathered or []:
        lines.extend(
            [
                "[[grandfathered]]",
                f'path = "{file_path}"',
                f"ceiling_loc = {ceiling}",
                "issue = 1021",
                'note = "fixture grandfathered debt"',
                "",
            ]
        )
    for file_path, ceiling, issue, responsibility in exceptions or []:
        lines.extend(
            [
                "[[exception]]",
                f'path = "{file_path}"',
                f"ceiling_loc = {ceiling}",
                f"issue = {issue}",
                f'responsibility = "{responsibility}"',
                "",
            ]
        )
    for file_path, issue, reason in cohesion or []:
        lines.extend(
            [
                "[[cohesion_acknowledgement]]",
                f'path = "{file_path}"',
                f"issue = {issue}",
                f'reason = "{reason}"',
                "",
            ]
        )
    write(path, "\n".join(lines) + "\n")


def run_case(
    name: str,
    root: Path,
    *,
    expect_ok: bool,
    expected_substr: str | None = None,
    changed: str = "",
) -> None:
    env = os.environ.copy()
    env["SEYAL_VALIDATION_ROOT"] = str(root)
    env["SEYAL_STRUCTURAL_DEBT_CHANGED_FILES"] = changed
    result = subprocess.run(
        [sys.executable, str(DEFAULT_ROOT / "scripts/check-structural-debt.py")],
        cwd=root,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    ok = result.returncode == 0
    if expect_ok and not ok:
        raise SystemExit(f"[structural-debt self-test] {name} unexpectedly failed:\n{result.stdout}")
    if not expect_ok and ok:
        raise SystemExit(f"[structural-debt self-test] {name} unexpectedly passed")
    if expected_substr and expected_substr not in result.stdout:
        raise SystemExit(
            f"[structural-debt self-test] {name} missing {expected_substr!r}:\n{result.stdout}"
        )


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="seyal-structural-debt-") as tmp:
        base = Path(tmp)

        # 1. New file below review threshold.
        small = base / "small-ok"
        write(small / "crates/seyal-core/src/lib.rs", "x\n" * 40)
        write_baseline(small / BASELINE_REL)
        run_case("new-file-below-review", small, expect_ok=True, changed="crates/seyal-core/src/lib.rs")

        # 2. Changed file entering review range without acknowledgement.
        review = base / "review-missing"
        write(review / "crates/seyal-core/src/lib.rs", "x\n" * 750)
        write_baseline(review / BASELINE_REL)
        run_case(
            "changed-review-missing-ack",
            review,
            expect_ok=False,
            expected_substr="cohesion-review band",
            changed="crates/seyal-core/src/lib.rs",
        )

        # 2b. Same with acknowledgement.
        review_ok = base / "review-ack"
        write(review_ok / "crates/seyal-core/src/lib.rs", "x\n" * 750)
        write_baseline(
            review_ok / BASELINE_REL,
            cohesion=[("crates/seyal-core/src/lib.rs", 1021, "temporary cohesion hold")],
        )
        run_case(
            "changed-review-with-ack",
            review_ok,
            expect_ok=True,
            changed="crates/seyal-core/src/lib.rs",
        )

        # 3. New file >1000 LOC.
        huge = base / "new-huge"
        write(huge / "crates/seyal-core/src/lib.rs", "x\n" * 1200)
        write_baseline(huge / BASELINE_REL)
        run_case(
            "new-file-over-hard",
            huge,
            expect_ok=False,
            expected_substr="exceeds 1000 LOC",
            changed="crates/seyal-core/src/lib.rs",
        )

        # 4. Grandfathered >1000 unchanged.
        gf = base / "grandfathered-ok"
        write(gf / "crates/seyal-core/src/big.rs", "x\n" * 1500)
        write_baseline(gf / BASELINE_REL, grandfathered=[("crates/seyal-core/src/big.rs", 1500)])
        run_case("grandfathered-unchanged", gf, expect_ok=True, changed="")

        # 5. Grandfathered reduced but still >1000 without ceiling drop.
        reduced = base / "grandfathered-reduced"
        write(reduced / "crates/seyal-core/src/big.rs", "x\n" * 1200)
        write_baseline(
            reduced / BASELINE_REL,
            grandfathered=[("crates/seyal-core/src/big.rs", 1500)],
        )
        run_case(
            "grandfathered-reduced-needs-ratchet",
            reduced,
            expect_ok=False,
            expected_substr="reduced below grandfathered ceiling",
            changed="crates/seyal-core/src/big.rs",
        )

        # 5b. Reduction accepted after ceiling lowered.
        reduced_ok = base / "grandfathered-reduced-ok"
        write(reduced_ok / "crates/seyal-core/src/big.rs", "x\n" * 1200)
        write_baseline(
            reduced_ok / BASELINE_REL,
            grandfathered=[("crates/seyal-core/src/big.rs", 1200)],
        )
        run_case(
            "grandfathered-reduced-ceiling-updated",
            reduced_ok,
            expect_ok=True,
            changed="crates/seyal-core/src/big.rs",
        )

        # 6. Grandfathered grown beyond baseline.
        grown = base / "grandfathered-grown"
        write(grown / "crates/seyal-core/src/big.rs", "x\n" * 1600)
        write_baseline(
            grown / BASELINE_REL,
            grandfathered=[("crates/seyal-core/src/big.rs", 1500)],
        )
        run_case(
            "grandfathered-grown",
            grown,
            expect_ok=False,
            expected_substr="exceeds accepted ceiling",
            changed="crates/seyal-core/src/big.rs",
        )

        # 7. Generated/fixture exclusion.
        generated = base / "generated-excluded"
        write(generated / "crates/seyal-core/src/tables_generated.rs", "x\n" * 5000)
        write(generated / "crates/seyal-core/src/lib.rs", "x\n" * 10)
        write(generated / "crates/seyal-core/tests/huge.rs", "x\n" * 5000)
        write_baseline(generated / BASELINE_REL)
        run_case("generated-and-tests-excluded", generated, expect_ok=True, changed="")

        # 8. Valid narrowly scoped exception for new >1000 file.
        exc = base / "exception-ok"
        write(exc / "crates/seyal-core/src/special.rs", "x\n" * 1300)
        write_baseline(
            exc / BASELINE_REL,
            exceptions=[
                (
                    "crates/seyal-core/src/special.rs",
                    1300,
                    1021,
                    "single cohesive protocol table ownership",
                )
            ],
        )
        run_case(
            "valid-exception",
            exc,
            expect_ok=True,
            changed="crates/seyal-core/src/special.rs",
        )

        # 9. Stale / invalid exception entry.
        stale = base / "exception-stale"
        write(stale / "crates/seyal-core/src/lib.rs", "x\n" * 10)
        write_baseline(
            stale / BASELINE_REL,
            exceptions=[
                (
                    "crates/seyal-core/src/missing.rs",
                    1500,
                    1021,
                    "gone",
                )
            ],
        )
        run_case(
            "stale-exception",
            stale,
            expect_ok=False,
            expected_substr="stale exception entry",
            changed="",
        )

        invalid = base / "exception-invalid"
        write(invalid / "crates/seyal-core/src/special.rs", "x\n" * 1300)
        write(
            invalid / BASELINE_REL,
            "\n".join(
                [
                    f'schema = "{SCHEMA}"',
                    "version = 1",
                    "[[exception]]",
                    'path = "crates/seyal-core/src/special.rs"',
                    "ceiling_loc = 1300",
                    "issue = 1021",
                    'responsibility = ""',
                    "",
                ]
            ),
        )
        run_case(
            "invalid-exception",
            invalid,
            expect_ok=False,
            expected_substr="non-empty responsibility",
            changed="",
        )

        # 10. Arbitrary numeric split naming.
        split = base / "arbitrary-split"
        write(split / "crates/seyal-core/src/terminal_part1.rs", "x\n" * 40)
        write_baseline(split / BASELINE_REL)
        run_case(
            "arbitrary-split-name",
            split,
            expect_ok=False,
            expected_substr="arbitrary numeric split name",
            changed="crates/seyal-core/src/terminal_part1.rs",
        )

    print("[structural-debt self-test] all fixture cases passed.")
    return 0


def report_inventory(files: list[SourceFile], baseline: Baseline) -> None:
    print("Structural-debt inventory:")
    for item in files:
        state = "ok"
        if item.path in baseline.grandfathered:
            state = "grandfathered"
        elif item.path in baseline.exceptions:
            state = "exception"
        elif item.loc > HARD_LOC:
            state = "over-hard"
        elif item.loc >= REVIEW_LOC:
            state = "review-band"
        changed = "changed" if item.changed else "unchanged"
        print(f"  {item.path}: loc={item.loc} state={state} {changed}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run deterministic fixture cases and exit.",
    )
    parser.add_argument(
        "--inventory",
        action="store_true",
        help="Print classified inventory even when the ratchet passes.",
    )
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    baseline_path = ROOT / BASELINE_REL
    baseline = load_baseline(baseline_path)
    changed = resolve_changed_paths(ROOT)
    files = inventory(ROOT, changed)
    if args.inventory:
        report_inventory(files, baseline)

    errors = evaluate(files, baseline)
    if errors:
        print("Structural-debt ratchet failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        print(
            "Valid resolution paths: decompose by responsibility, reduce LOC and "
            "ratchet the checked-in ceiling down, or add a narrowly scoped "
            "[[exception]] / [[cohesion_acknowledgement]] tied to an owning Issue. "
            "Do not create partN splits, new crates, or hot-path indirection merely "
            "to satisfy this gate.",
            file=sys.stderr,
        )
        return 1

    print(
        f"Structural-debt ratchet passed "
        f"({len(files)} handwritten production files; "
        f"{len(baseline.grandfathered)} grandfathered)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
