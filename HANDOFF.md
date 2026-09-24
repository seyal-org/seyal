# Handoff — Issue #994 pane/tab TerminalExecution provisioning contract

> **Agent scaffolding, not merge content.** This file exists only to hand the
> refinement result to the coordinating owner. Drop the commit that adds it
> before merging, or delete the file in the PR branch.

## Branch

`cursor/issue-994-provisioning-contract-ad70`, based on `origin/master`
(`7a66e63`). Refinement only: no Rust, Swift, test or build changes.

## What landed

| Commit | Content |
|---|---|
| `810a4bd` | `docs/architecture/ADR-019-EXECUTION-PROVISIONING-AND-DISPOSITION.md` (**Proposed**) + `docs/architecture/README.md` index/authority entry |
| `118313f` | scoped amendments: SPEC-004 §18, SPEC-003 §4.1/§5.2, SPEC-009 §8.2.1, each marked *normative only on ADR-019 acceptance* |
| `2bb7b6b` | `docs/milestones/M003-674-EXECUTION-PROVISIONING-CHILDREN.md` child drafts + one MILESTONE-003 §6.2 pointer |

## ADR numbering collision (needs a decision)

Two sibling refinements in the same M003 pass claimed numbers before this branch
was committed:

- #1000 → `ADR-017-NATIVE-WINDOW-TAB-LIFECYCLE.md`
- #1004 → `ADR-018-LOCAL-RESOURCE-ADDRESSING-NAVIGATION.md`

This branch therefore uses **ADR-019** and cites both as concurrent proposals
(§2.1). If merge order changes, renumber one document and its references; the
architecture-index entry deliberately uses the same low-conflict `17x.` insert
style the sibling branches used.

`docs/specs/SPEC-022` is also already claimed by #1004, which is one reason this
work amends SPEC-004 instead of creating a new specification.

## Suggested PR

**Title:** `docs(994): define pane/tab TerminalExecution provisioning and disposition contract`

**Body summary:**

- `Refs #994`, `Part of #674`. Refinement/architecture output only — no
  production code, so this PR must not close #994's parent or any implementation
  Issue.
- Proposes ADR-019: portable Rust product authority owns provisioning intent,
  the Runtime registry stays the single execution-creation authority and
  launch-policy owner, and `TabId`/`PaneId` never cross the local protocol.
- Adds scoped SPEC-004 §18 (capability bit 8, message types 35–38 with exact
  fixed-width layouts, validation order, bounds, two additive result codes),
  SPEC-003 §4.1/§5.2 and SPEC-009 §8.2.1 — all explicitly non-normative until
  ADR-019 is accepted.
- Makes close/detach/terminate explicit: closing presentation detaches, explicit
  termination is a separate Controller-fenced operation, and an execution whose
  provisioning intent died is disposed of deterministically instead of leaking.
- Publishes eight Ready-candidate child drafts (P1–P4, C1–C3, M1) with
  measurable acceptance and tests, plus the work deliberately left out (#676
  launch profiles, #686 trusted CWD, #923, #929, #936, SPEC-004 maxima).

## Reviewer attention

1. Whether provisioning should stay connection-scoped (any authenticated
   same-UID connection may create) while termination is Controller-fenced.
2. The disposal path for a never-bound execution (attach as Controller solely to
   terminate) versus the rejected creator-connection disposition (§11 alternative
   I).
3. The honest limits recorded in §14: unreferenced live executions can outlive
   every Pane until #929/#1000 reachability work lands, and at most 16 Panes can
   be attached under current SPEC-004 maxima.
4. Whether ADR-019 §2.1's boundary with #1000 is drawn where the owner wants it.
