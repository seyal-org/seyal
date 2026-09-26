---
title: What is available now?
description: Separate implemented Seyal behavior from planned product capability.
---

Seyal is being built one vertical milestone at a time. This page exists so user documentation never turns architecture intent into a false shipping claim.

## Current rule

A capability is documented as available only after it is:

1. implemented,
2. tested,
3. demonstrable, and
4. benchmarked where relevant.

Until then, architecture and product documents may describe the intended direction, but the User Guide must label the capability as **under development**.

## Planned documentation areas

As implementation lands, this section will expand into guides for:

- shells and terminal compatibility,
- tabs, panes and workspaces,
- Blocks,
- configuration and themes (local TOML at `SEYAL_CONFIG` or `~/.config/seyal/config.toml` sets appearance, font sizes, padding, and material preference at cold startup; font family is not applied yet; there is no settings UI or Lua runtime yet),
- SSH and remote execution,
- persistent/detached execution,
- agent workflows and approvals,
- accessibility and keyboard control,
- troubleshooting and diagnostics.

## Screenshots and video

Screenshots are encouraged when they show a real, current UI. Do not use concept art as procedural evidence.

Tutorial video should be generated or recorded only after the relevant UI and interaction flow are stable. Every video should identify the Seyal version/commit it demonstrates so stale media can be retired deliberately.
