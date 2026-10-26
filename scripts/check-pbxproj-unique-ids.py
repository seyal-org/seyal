#!/usr/bin/env python3
"""Reject duplicate object definitions in Xcode project files."""

from __future__ import annotations

import argparse
import os
import re
import sys
import tempfile
from pathlib import Path

DEFAULT_ROOT = Path(__file__).resolve().parents[1]
OBJECT_MAP_START = re.compile(r"^[ \t]*objects[ \t]*=[ \t]*\{[ \t]*$")
OBJECT_DEFINITION = re.compile(
    r"^[ \t]*([A-F0-9]{24})(?:[ \t]*/\*.*?\*/)?[ \t]*=[ \t]*\{",
    re.IGNORECASE,
)
EXCLUDED_DIRECTORIES = {
    ".git",
    ".sdlc",
    ".venv",
    "node_modules",
    "target",
    "third_party",
    "vendor",
}
OBJECT_ID = "C1000000000000000000000A"


def brace_depth_after(line: str, depth: int, in_block_comment: bool) -> tuple[int, bool]:
    """Count OpenStep plist braces, ignoring comments and quoted strings."""
    in_string = False
    escaped = False
    index = 0
    while index < len(line):
        char = line[index]
        next_char = line[index + 1] if index + 1 < len(line) else ""
        if in_block_comment:
            if char == "*" and next_char == "/":
                in_block_comment = False
                index += 2
                continue
            index += 1
            continue
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            index += 1
            continue
        if char == "/" and next_char == "*":
            in_block_comment = True
            index += 2
            continue
        if char == "/" and next_char == "/":
            break
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        index += 1
    return depth, in_block_comment


def duplicate_definitions(source: str) -> list[tuple[str, list[int]]]:
    definitions: dict[str, list[int]] = {}
    in_objects = False
    depth = 0
    in_block_comment = False

    for line_number, line in enumerate(source.splitlines(), start=1):
        if not in_objects:
            if not OBJECT_MAP_START.match(line):
                continue
            in_objects = True

        if depth == 1 and not in_block_comment:
            match = OBJECT_DEFINITION.match(line)
            if match:
                definitions.setdefault(match.group(1).upper(), []).append(line_number)

        depth, in_block_comment = brace_depth_after(line, depth, in_block_comment)
        if depth == 0:
            in_objects = False

    return [
        (identifier, lines)
        for identifier, lines in sorted(definitions.items())
        if len(lines) > 1
    ]


def find_duplicates(root: Path) -> list[tuple[str, str, list[int]]]:
    problems: list[tuple[str, str, list[int]]] = []
    for directory, child_directories, filenames in os.walk(root):
        child_directories[:] = sorted(
            name for name in child_directories if name not in EXCLUDED_DIRECTORIES
        )
        if "project.pbxproj" not in filenames:
            continue
        project = Path(directory) / "project.pbxproj"
        for identifier, lines in duplicate_definitions(
            project.read_text(encoding="utf-8")
        ):
            problems.append(
                (project.relative_to(root).as_posix(), identifier, lines)
            )
    return sorted(problems)


def self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="seyal-pbxproj-self-test-") as temp:
        root = Path(temp)
        project = root / "macos/Seyal/Seyal.xcodeproj/project.pbxproj"
        project.parent.mkdir(parents=True)
        project.write_text(
            """// duplicate definitions must fail
{
  objects = {
    C1000000000000000000000A /* first */ = { isa = PBXBuildFile; };
    C1000000000000000000000A /* second */ = { isa = PBXFileReference; };
  };
}
""",
            encoding="utf-8",
        )
        duplicates = find_duplicates(root)
        expected_duplicates = [
            (
                "macos/Seyal/Seyal.xcodeproj/project.pbxproj",
                OBJECT_ID,
                [4, 5],
            )
        ]
        if duplicates != expected_duplicates:
            raise SystemExit(
                f"duplicate fixture was not rejected: {duplicates!r}"
            )

        project.write_text(
            """{
  objects = {
    C1000000000000000000000A /* unique definition */ = { isa = PBXBuildFile; };
    A80000000000000000000001 /* Project object */ = {
      attributes = {
        TargetAttributes = {
          C1000000000000000000000A = { CreatedOnToolsVersion = 16.0; };
        };
      };
    };
    files = (C1000000000000000000000A, C1000000000000000000000A);
  };
}
""",
            encoding="utf-8",
        )
        if find_duplicates(root):
            raise SystemExit("valid references and nested target attributes must pass")

    print("[pbxproj unique-ID self-test] duplicate and valid-reference fixtures passed.")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(os.environ.get("SEYAL_VALIDATION_ROOT", DEFAULT_ROOT)),
        help="repository root to validate (defaults to this checkout)",
    )
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    root = args.root.resolve()
    problems = find_duplicates(root)
    if problems:
        for path, identifier, lines in problems:
            first, *repeats = lines
            repeated = ", ".join(str(line) for line in repeats)
            print(
                f"{path}:{repeats[0]}: duplicate Xcode object ID {identifier}; "
                f"first defined at line {first}; repeated at lines {repeated}",
                file=sys.stderr,
            )
        return 1

    print("Xcode project object definitions are unique.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
