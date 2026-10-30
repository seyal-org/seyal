# Risk-tiered PR validation pilot — 30-day record

Start the observation window on the day the policy PR merges. This file is a measurement log, not evidence that the pilot has succeeded.

## Measures

- Docs-only PR count and share of all PRs.
- Elapsed time from PR open to first submitted review and to merge. These are queue/wait measurements, not active review time.
- Number of submitted review rounds before merge.
- Rust/macOS CI minutes skipped for docs-only PRs, using actual workflow job durations.
- Failures found by product gates, docs/policy checks, and any defects later traced to an incorrectly classified docs-only change.
- Changed-file count per PR, with ADR acceptance PRs reported separately.
- Optional reviewer-reported active review time, labeled self-reported and never inferred from timestamps.

## Daily or weekly entries

| Date / window | Docs-only PRs / all PRs | Median open→first review | Median open→merge | Median review rounds | Rust/macOS minutes skipped | Docs-only escapes | Median files / ADR files | Notes |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Not started — begins after merge | — | — | — | — | — | — | — | No baseline has been fabricated. |

## Day-30 decision

Complete after 30 calendar days of observation:

- Keep, adjust, or revert the docs-only exemption, with evidence:
- Did any incorrect classification or escaped defect occur?
- Did review/CI elapsed time or review rounds improve? State sample sizes and limitations:
- Which paths or checks should change, if any?
- Decision date and human reviewer:
