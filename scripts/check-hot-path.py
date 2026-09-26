#!/usr/bin/env python3
from __future__ import annotations

import os
import re
from pathlib import Path

ROOT = Path(os.environ.get("SEYAL_VALIDATION_ROOT", Path(__file__).resolve().parents[1])).resolve()

HOT_FUNCTIONS = {
    # TerminalState feed/finish after #1020 responsibility split (was terminal.rs).
    "crates/seyal-terminal/src/terminal/state.rs": ["feed", "finish_input"],
    # Runtime dispatch ownership after #799: poll_once remains in the facade,
    # while control/read/write service loops live in reactor_io.
    "crates/seyal-runtime/src/runtime/mod.rs": ["poll_once"],
    "crates/seyal-runtime/src/runtime/reactor_io.rs": [
        "drain_control",
        "service_reads",
        "service_writes",
    ],
    "crates/seyal-runtime/src/input.rs": ["try_submit"],
    # Candidate-D display encode/publish after #1020 split (was display.rs).
    "crates/seyal-runtime/src/display/encode_v1.rs": [
        "encode_snapshot",
        "encode_delta",
        "encode_rows",
    ],
    "crates/seyal-runtime/src/display/encode_v2.rs": [
        "encode_snapshot_v2",
        "encode_delta_v2",
        "encode_cells_v2",
    ],
    "crates/seyal-runtime/src/runtime/local/display_publish.rs": ["publish_display_updates"],
}

# Metal prepare/present: first `update` is the NativePreparedFrame prepare path.
# Required only while a native host tree exists. #883 must restore these files;
# do not stub them here.
NATIVE_HOT_FUNCTIONS = {
    "macos/Seyal/Sources/MetalTerminalRenderer.swift": ["update", "present"],
}

NATIVE_HOST_ROOT = Path("macos/Seyal")

FORBIDDEN = {
    "blocking lock": ("Mutex<", "RwLock<", ".lock()", ".read()", ".write()"),
    "thread/process hop": ("thread::spawn", "std::thread", "process::Command", "Command::new("),
    "blocking sleep": ("thread::sleep", "std::thread::sleep"),
    "serialization": ("serde_json", "json!(", "bincode", "postcard"),
    "network/filesystem I/O": ("TcpStream", "UnixStream", "std::fs", "File::open", "File::create"),
    "avoidable allocation": ("Vec::new()", "vec![", ".to_vec()", ".to_owned()", "String::new()", "String::from(", "format!("),
    "unbounded channel": ("mpsc::channel(", "channel::<"),
}


def extract_function(source: str, name: str) -> str | None:
    # Rust `fn` and Swift `func` production entrypoints share one registry.
    match = re.search(
        rf"\b(?:fn|func)\s+{re.escape(name)}\s*\([^)]*\)[^{{]*\{{",
        source,
        re.S,
    )
    if not match:
        return None
    start = match.start()
    brace = source.find("{", match.start())
    depth = 0
    for index in range(brace, len(source)):
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    return None


def native_host_present() -> bool:
    return (ROOT / NATIVE_HOST_ROOT).is_dir()


def validate_native_recovery_ownership(errors: list[str]) -> None:
    if not native_host_present():
        return
    # Pass 9 recovery policy is owned by the Rust RecoveryCoordinator (#1065).
    # The surface only requests/cancels episodes and reports presentation
    # stages; the chrome effect executor opens/adopts and reports outcomes.
    guarded: dict[str, tuple[str, ...]] = {
        "macos/Seyal/Sources/MetalSurfaceView.swift": (),
        "macos/Seyal/Sources/MetalSurfaceView+Recovery.swift": (
            "SEYAL_APP_ACTION_BEGIN_RECOVERY",
            "SEYAL_APP_ACTION_CANCEL_RECOVERY",
            "SEYAL_APP_ACTION_ADVANCE_RECOVERY_STAGE",
            "func startAutomaticBridgeRecoveryIfNeeded()",
        ),
        "macos/Seyal/Sources/ProductChromeHostView+Recovery.swift": (
            "SEYAL_APP_ACTION_COMPLETE_RECOVERY",
            "openRuntimeRecoveryHandle(",
            "adoptRecoveredHandle(",
        ),
    }
    for relpath, required_tokens in guarded.items():
        path = ROOT / relpath
        if not path.exists():
            errors.append(f"missing guarded native lifecycle file: {relpath}")
            continue
        text = path.read_text(encoding="utf-8")
        # A direct bridge.start() from AppKit creates an extra connection
        # attempt and a fresh timeout outside the exact seven-attempt/
        # one-second episode, and may block the main actor.
        if re.search(r"\bbridge\??\.start\s*\(", text):
            errors.append(
                f"{relpath} performs a direct bridge.start(); production startup/recovery must be RecoveryCoordinator-owned"
            )
        for required in required_tokens:
            if required not in text:
                errors.append(f"{relpath} is missing Rust recovery boundary {required!r}")

    bridge_relpath = "macos/Seyal/Sources/RustDisplayBridge.swift"
    bridge_path = ROOT / bridge_relpath
    if not bridge_path.exists():
        errors.append(f"missing guarded native bridge file: {bridge_relpath}")
        return
    bridge = bridge_path.read_text(encoding="utf-8")

    # RustDisplayBridge owns one disposable client/socket only. It must never
    # remember or execute a self-reconnect request after teardown; otherwise a
    # dead live socket can bypass the RecoveryCoordinator and receive a fresh timeout.
    if "reconnectRequested" in bridge:
        errors.append(
            f"{bridge_relpath} retains bridge-owned reconnect state; lifecycle recovery must be Rust RecoveryCoordinator-owned"
        )
    teardown_match = re.search(
        r"private\s+func\s+teardownCompleted\s*\(\s*\)\s*\{(?P<body>.*?)\n\s*\}",
        bridge,
        re.S,
    )
    if teardown_match is None:
        errors.append(f"{bridge_relpath} is missing teardownCompleted()")
    elif re.search(r"\bstart\s*\(", teardown_match.group("body")):
        errors.append(
            f"{bridge_relpath}::teardownCompleted reopens a client; it may only publish teardown completion"
        )



def main() -> None:
    errors: list[str] = []
    registry = dict(HOT_FUNCTIONS)
    if native_host_present():
        registry.update(NATIVE_HOT_FUNCTIONS)
    for relpath, functions in registry.items():
        path = ROOT / relpath
        if not path.exists():
            errors.append(f"missing guarded hot-path file: {relpath}")
            continue
        source = path.read_text(encoding="utf-8")
        for function in functions:
            body = extract_function(source, function)
            if body is None:
                errors.append(f"missing guarded hot-path function: {relpath}::{function}")
                continue
            for category, patterns in FORBIDDEN.items():
                for pattern in patterns:
                    if pattern in body:
                        errors.append(
                            f"{relpath}::{function} contains forbidden {category} primitive {pattern!r}"
                        )

    validate_native_recovery_ownership(errors)

    if errors:
        print("Hot-path performance guardrail violations:")
        for error in errors:
            print(f"  {error}")
        raise SystemExit(1)

    print("Hot-path performance guardrails passed.")


if __name__ == "__main__":
    main()
