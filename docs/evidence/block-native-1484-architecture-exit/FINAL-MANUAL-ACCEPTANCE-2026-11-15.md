# PR #1485 final Block acceptance result — 2026-11-15

## Result

**Blocks are suspended and not accepted for production.** The current candidate
fails the product-owner's rendered-pixel acceptance. Preserve this checkpoint
for future Block work; do not describe the Block presentation as delivered or
rely on the architecture-exit evidence as product acceptance.

## Reproduction

- Worktree branch: `mahboobmonnamd/issue/1484`
- PR base head: `961538be77652f4c0359edbdfbffc67be4cd715b`
- Candidate rebuilt from the dirty worktree and relaunched on 2026-11-15.
- Workload: `echo sey1485-post-fix`, `seq 1 10000`, then `pwd`.
- User-observed symptom after long output: later simple commands cause prior
  output to flash/redraw in place, rather than producing duplicate lines.
- Rendered candidate retained the earlier echo header while the visible output
  tail changed; `pwd` did not appear as a distinct command Block. The frame
  contained large blank areas and did not show a coherent command/output Block.
- The symptom reproduced after a clean candidate relaunch. Earlier removal of
  synchronous GPU readback had not corrected the manual acceptance failure.

This is a failed visual acceptance, not a passing test or a claim that all
possible Block defects have been isolated. Focused Rust lifecycle tests and
Metal frame-publication tests passed, but they do not override the visible
failure. No full UI suite was run.

## Disposition

- Preserve the implementation and this evidence on the existing Issue #1484
  branch as a recoverable checkpoint.
- Suspend the Block presentation in the shipped path. Keep canonical Rust
  `PrimaryBlockEngine`, `RegionGrid`, VT state and Block metadata intact.
- Continue with a conventional Raw terminal presentation over the same
  canonical state. The Raw path must be verified independently for pixels,
  scrolling, input, selection/copy, TUI, panes/tabs and reconnect.
- Reconsider Blocks only after fresh independent source review and headed
  acceptance of separate command Blocks, long streaming output, stable scroll,
  SSH interactive input and measured input-to-pixel latency.
