#!/usr/bin/env python3
"""Reject portable product authority in the thin macOS host."""

from __future__ import annotations

import os
import pathlib
import re
import sys

DEFAULT_ROOT = pathlib.Path(__file__).resolve().parents[1]
ROOT = pathlib.Path(os.environ.get("SEYAL_VALIDATION_ROOT", DEFAULT_ROOT)).resolve()
SOURCES = ROOT / "macos" / "Seyal" / "Sources"

# KEEP_NATIVE_GLUE may still mention leftover types. New host files are always scanned.
DEPRECATED_ALLOWLIST = {
    "RuntimeLifecycleRecoveryCoordinator.swift",
    "RustDisplayBridge.swift",
    "MetalSurfaceView.swift",
    "MetalTerminalRenderer.swift",
}

REQUIRED_HOST = {
    "AppDelegate.swift",
    "ProductChromeHostView.swift",
    "ComposerBridgeView.swift",
    "ThinPaneHostView.swift",
    "NativeThemeRealization.swift",
}

PRODUCT_AUTHORITY = (
    "SeyalShellState",
    "SeyalShellPreviewFactory",
    "SeyalShellProductionFactory",
    "PanePresentationSession",
    "enum InspectorMode",
    "enum LeftPanelMode",
    "func appendCommand(",
    # Dead remnant deleted in #1020/E7; keep scanning so a non-allowlisted host
    # file cannot reintroduce the portable composer-correlation product type.
    "ComposerRequestCorrelation",
)


def main() -> int:
    if not SOURCES.is_dir():
        print("Thin-Swift boundary failed: missing macos/Seyal/Sources", file=sys.stderr)
        return 1
    errors: list[str] = []
    present = {path.name for path in SOURCES.glob("*.swift")}
    for name in REQUIRED_HOST:
        if name not in present:
            errors.append(f"missing required thin-host source: {name}")
    for path in sorted(SOURCES.glob("*.swift")):
        if path.name in DEPRECATED_ALLOWLIST:
            continue
        text = path.read_text(encoding="utf-8")
        rel = path.relative_to(ROOT)
        for token in PRODUCT_AUTHORITY:
            if token in text:
                errors.append(f"{rel} introduces portable product authority token {token!r}")
        if re.search(r"enum\s+Workspace(Id|Mode|State)\b", text):
            errors.append(f"{rel} introduces a Swift Workspace product enum")
        if path.name == "ProductChromeHostView.swift":
            for token in ("NativeBlockRecord", "currentTimeline("):
                if token in text:
                    errors.append(
                        f"{rel} owns Block presentation from {token!r}; "
                        "paint seyal_app_block_row only"
                    )
    if errors:
        print("Thin-Swift boundary failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("Thin-Swift boundary passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
