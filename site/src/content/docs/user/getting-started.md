---
title: Getting Started
description: Build and run Seyal from the current source tree.
---

Seyal does not yet publish a stable end-user release. For now, the supported path is a source checkout used by contributors and early testers.

## Prerequisites

- macOS on Apple Silicon for the current native target.
- Rust toolchain required by the repository.
- Xcode / macOS development tools required by the native host when that milestone is active.
- Git.

## Bootstrap

From the repository root:

```sh
make bootstrap
```

Use the canonical repository commands rather than calling internal build scripts directly:

```sh
make build
make test
make check
make bench
```

Some commands may intentionally report that a later milestone has not created a production component yet. That is preferable to documentation pretending an unfinished surface is available.

## Appearance configuration

At cold startup the production app loads local UI configuration once through the Rust config authority (no Swift TOML parser):

1. If `SEYAL_CONFIG` is set to a non-empty path, that file is used.
2. Otherwise Seyal reads `~/.config/seyal/config.toml` when present.

Missing or unreadable files keep built-in defaults and the app still starts. Invalid TOML keeps the same full-default fallback and surfaces bounded, non-secret diagnostics (for example via system log). There is no live reload in this slice: restart the app to pick up changes.

Accepted user-facing keys that this cold-start slice realizes in the production app:

```toml
[ui]
appearance = "system" # system | light | dark
reduced-material = false
utility-opacity = 1.0
window-padding = 0

[ui.font]
size = 12

[terminal]
padding = 8

[terminal.font]
size = 14

[input]
option_as_alt = false
```

Invalid keys or values are ignored or clamped; Seyal always starts from a complete resolved snapshot. Appearance preference (system/light/dark) and material intent (`reduced-material` / `utility-opacity`) are resolved in Rust; AppKit maps the typed appearance, font **sizes**, padding, and utility material preference onto native presentation. Font **family** / fallback lists may be present in the file for forward compatibility but are not applied by the host in this slice (system / monospaced system fonts are used). There is still no settings UI, cloud sync, or Lua overlay.

For a one-off override without editing a file, launch with `open --env` (shell `VAR=value open …` does not pass environment into the app):

```sh
open --env SEYAL_CONFIG=/tmp/seyal-config.toml target/macos-derived-data/Build/Products/Debug/Seyal.app
open --env SEYAL_UI_APPEARANCE=light target/macos-derived-data/Build/Products/Debug/Seyal.app
```

## Next

See **What is available now?** before relying on a feature described in product plans or architecture documents.
