# Phase E — Composer + interaction (#1484)

- **Branch:** `mahboobmonnamd/issue/1484`

## Automated evidence

1. Existing ADR-009 live suite still green under native RegionIds (submit, multiline paste, DirectInput admission, forged markers).
2. Nested SSH host restore: after session Complete, `native_active_region()` equals host output RegionId (`composer_ssh_to_bash_…`).
3. DropPending cancels provisional OSC arm without leaving writes on an orphan pending region.

## Still required (G6)

- Headed: Enter, cd continuity, Tab completion, IME, mouse caret/selection/copy over RegionGrids.
- Running-Block DirectInput/Relay + focus/keybindings on Metal/Flow with NATIVE_REGIONS paint.
- Exact-head headed ledger — no GO from unit suites alone.
