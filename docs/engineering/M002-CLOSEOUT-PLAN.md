# M002 close-out execution plan

**Status:** planning authority for remaining M002 work. Not Done. Does **not** authorize new product slices, reopen closed #815–#823, or claim technical preview.

**Audience:** humans and coding agents about to touch #824 / #672, #673, or #837.

**Why this file exists:** M002 implementation PRs accumulated high review churn (repeated `CHANGES_REQUESTED`, exact-head re-reviews, evidence-honesty blockers). Close-out must be plan-first: each PR lands one independently reviewable outcome with honest relationship keywords and exact-head evidence, or it stays `Refs` only.

**Higher authority (do not override):**

1. `AGENTS.md`
2. Accepted architecture / ADRs (especially ADR-010, ADR-011, ADR-015)
3. SPEC-010 / SPEC-011 / SPEC-006 §21
4. [`docs/milestones/MILESTONE-002.md`](../milestones/MILESTONE-002.md) finding-set freeze
5. Live GitHub Issue state (assignee, open/closed) over stale prose in this file or in Issue bodies

---

## 1. Remaining chain (live snapshot)

Snapshot date for this plan: **2026-09-24**. Re-fetch GitHub before any pickup.

| Slice | Issue | Live state (this snapshot) | May start now? |
|---|---|---|---|
| M002.10 workload matrix / #672 close | [#824](https://github.com/seyal-org/seyal/issues/824) | **Open**, assignee **empty** (comments historically named @anulalbs; do not invent ownership) | Only after sole **human** owner is restored and Ready gates below pass |
| Compatibility parent | [#672](https://github.com/seyal-org/seyal/issues/672) | **Open**, no assignee | Closes only with #824 Done + independent close review |
| Performance contract rows | [#673](https://github.com/seyal-org/seyal/issues/673) | **Open**, sole assignee **@crdileep82** | Measurement/harness resume only by that owner (or explicit handoff). Independent of #824 closure |
| Unicode-heavy ARM64 | [#837](https://github.com/seyal-org/seyal/issues/837) | **Open**, no assignee | **Blocked** until accepted #673 ceilings exist |
| Epic | [#664](https://github.com/seyal-org/seyal/issues/664) | **Open** | Closes only after #824/#672 + #673 + #837 Done **and** milestone-validation on one freeze SHA |

Merged but **not Done** (relationship was `Refs` only):

- PR [#964](https://github.com/seyal-org/seyal/pull/964), [#984](https://github.com/seyal-org/seyal/pull/984) — #824 evidence / UI gates; closing PR still required
- PR [#919](https://github.com/seyal-org/seyal/pull/919), [#969](https://github.com/seyal-org/seyal/pull/969), [#985](https://github.com/seyal-org/seyal/pull/985), [#992](https://github.com/seyal-org/seyal/pull/992) — #673 contract/harness plumbing; PHYSICAL_ARM64 matrix still open

Do **not** create new M002 implementation Issues except for a mandatory FAIL/INCONCLUSIVE defect or an amended frozen finding (`MILESTONE-002.md` §5).

---

## 2. Development-readiness verdicts

Procedure: `.agents/skills/development-readiness` + Seyal Ready checkboxes in `ISSUE-PROTOCOL.md`.

### 2.1 #824 / #672 — `BLOCKED` (ownership) / `NOT_READY` (Done gates)

```text
verdict: BLOCKED
reason: no sole human assignee on live #824; Done criteria still unmet after Refs-only merges
blocking_items:
  - restore exactly one human owner (assignee or maintainer-acked Owner claim)
  - exclusive-Runtime headed Flow/Blocks XCUI classified PASS/FAIL (not INCONCLUSIVE)
  - ten headed/manual workload steps filled or owner-accepted PLATFORM_LIMITED / ENVIRONMENT_UNSUPPORTED
  - independent non-author close review with no unresolved red/orange blocker
  - final PR may use Closes #824 / Closes #672 only when the above are green on the exact merge SHA
next_capability: ownership resolution + issue-refinement of the closing package (not new product design)
resume_condition: sole human owner present AND closing evidence package drafted against MILESTONE-002 §6.1
named_slice_if_scope_cut: one closing evidence/remediation PR; do not start #673/#837 from it
```

Architecture for the matrix path is accepted. Remaining risk is **evidence honesty and closure keywords**, not a new engine.

### 2.2 #673 — `READY` for measurement slices only (owner-locked)

```text
verdict: READY
reason: versioned contract + validator/harness faithfulness (through PR #992) are on master;
        remaining work is measured PHYSICAL_ARM64 rows against that contract
blocking_items: none for architecture; ownership must remain @crdileep82 unless explicit handoff
next_capability: implement-issue (human owner @crdileep82) → thin vertical measurement PRs
resume_condition: N/A for owner; other humans/agents STOP and report assignee collision
named_slice_if_scope_cut:
  1) HistoryStore reflow PHYSICAL_ARM64 VALID row(s)
  2) remaining contract families (PTY→state, damage/projection, Metal/visible-frame,
     input admission, high-output, startup/idle scaling)
hard_to_reverse_decisions: none new — do not weaken #818 ceilings or invent softer evidence classes
```

`PLATFORM_LIMITED` / uncontrolled-developer-host rows are not #673 Done. Relative-tail FAIL on same-SHA noise must be diagnosed before treating a row as accepted (`MILESTONE-002.md` §6.2).

### 2.3 #837 — `BLOCKED`

```text
verdict: BLOCKED
reason: waits on accepted #673 ceilings; starting now hides the dependency
blocking_items: accepted #673 PHYSICAL_ARM64 ceilings + noise policy
next_capability: wait; then work-item-design / implement-issue for Unicode-heavy ARM64 only
resume_condition: #673 records accepted ceilings usable as #837 baseline
```

### 2.4 Epic #664 — `NOT_READY`

Milestone-validation on one freeze SHA only after #824/#672, #673, and #837 are Done. Do not open a “close M002” PR early.

---

## 3. Review-churn root causes (observed)

Drawn from independent re-reviews and PR threads on #819/#823/#824/#673 (especially PRs #964, #984, #969, #992 and the keyboard/history re-review series).

| Pattern | What reviewers keep rejecting | Required preflight |
|---|---|---|
| Premature `Closes` | Docs-only or partial evidence PR uses `Closes #N` | Default `Refs #N`. Upgrade to `Closes` only after Done checklist is green on the **exact merge candidate** |
| Evidence-class inflation | `PLATFORM_LIMITED` / dirty host labeled `PHYSICAL_ARM64` `VALID` | Record true class; AC + thermal controlled; clean tree; bind baseline cohorts to `baseline_sha` |
| Stale ledgers | SHA labels, fingerprints, or narrative disagree with HEAD | Regenerate ledgers on the tip; re-review exact head after every evidence edit |
| Harness ≠ product claim | Validator/plumbing PR claims a product gate passed | Say `Refs`; name what is still unmeasured |
| Ownership drift | Comment says assignee X while GitHub assignee is empty/other | Fresh-fetch assignees before pickup; stop on collision |
| Bundled outcomes | One PR mixes matrix close + perf rows + Unicode | One Issue → one PR; #673 may proceed without waiting on #824 |
| Self-approve / author “independent” review | Author posts Approve-shaped reviews | Non-author human reviewer for core/high-risk close |
| Re-open closed product slices | New work on #815–#823 because bodies still say Blocked | Obey freeze; only FAIL/INCONCLUSIVE mandatory defects create children |
| INCONCLUSIVE treated as PASS | Leftover `control.sock` / shared Runtime | Exclusive Runtime or classify honestly and leave Issue open |

These are process defects. Fixing them in planning is cheaper than another re-review loop.

---

## 4. Pre-PR checklist (mandatory before requesting review)

Copy into the PR body. Every box must be true or the PR stays draft / `Refs` only.

### 4.1 Claim and scope

- [ ] Fresh-fetched sole **human** assignee (or acknowledged `Owner:`) matches the branch namespace `<login>/issue/<number>`
- [ ] No other open PR claims the same Issue slice
- [ ] In-scope / out-of-scope match the Issue and `MILESTONE-002.md` §3–§4
- [ ] Relationship keyword chosen deliberately: `Refs` vs `Closes` / `Fixes` / `Resolves`
- [ ] PR does not amend an ADR and does not invent a second VT/grid/renderer authority

### 4.2 Evidence honesty

- [ ] Every perf/presentation/fuzz claim labeled `CI` | `NATIVE_HEADED` / controlled-host | `PHYSICAL_ARM64` | `PLATFORM_LIMITED`
- [ ] Exact production SHA in ledgers equals the merge candidate (or explicitly historical with reason)
- [ ] Dirty working tree never stamped as clean HEAD for measurement identity
- [ ] `#824` keeps `performance_claim=false`; `#673` owns release performance
- [ ] `#837` not started from an `#824` or incomplete `#673` PR

### 4.3 Verification already run (quote commands + outcomes)

- [ ] `make check` (and issue-specific `make ui-test` / fuzz / contract runners) on the tip
- [ ] Hosted Foundation Quality green on the tip when the change is CI-visible
- [ ] For headed gates: exclusive Runtime free, or result classified `INCONCLUSIVE` / `ENVIRONMENT_UNSUPPORTED` without closing the Issue
- [ ] For `#673` rows: five cohorts × 20 warmups × 100 samples, nearest-rank percentiles, baseline provenance bound to `baseline_sha`, power **and** thermal controlled for `PHYSICAL_ARM64` `VALID`

### 4.4 Review request

- [ ] Independent human reviewer identified (not the implementer, not a bot identity as the required reviewer)
- [ ] PR description lists residual open Done items if not closing
- [ ] No “force merge if self-review looks good” path for core/high-risk close

---

## 5. Ordered execution plan

Parallelism is allowed only where authority already says so.

```text
Track A — Compatibility close
  A0. Restore sole human owner on #824 (assignee or acked Owner claim)
  A1. Draft closing package against MILESTONE-002 §6.1 (evidence + any tiny in-scope fixes)
  A2. Run exclusive-Runtime headed XCUI + ten manual steps (or owner-accepted classifications)
  A3. Independent close review → Closes #824 / #672 only if green
  A4. Do not open #664 close from this track alone

Track B — Performance rows (may overlap Track A)
  B0. Confirm @crdileep82 still sole owner (or complete handoff)
  B1. Thin PRs: one family / honest evidence class per PR when possible
  B2. Land PHYSICAL_ARM64 VALID rows; diagnose relative-tail FAILs before accepting
  B3. #673 Done only when contract families required by MILESTONE-002 §6.2 are accepted

Track C — Unicode-heavy ARM64
  C0. Start only after Track B ceilings accepted
  C1. #837 measurements vs those ceilings; no renderer rewrite

Track D — Milestone validation
  D0. One freeze SHA
  D1. milestone-validation skill + MILESTONE-002 §8 ledger
  D2. Close #664 only when §8 is PASS
```

**Stop rules for agents**

- If asked to “just implement M002” without a named Ready Issue and human owner → return this plan; do not code.
- If `#824` or `#673` owner is another human → report and stop (`implement-issue` preflight).
- If architecture for a discovered defect is missing → `architecture-change`, not a silent workaround inside the closing PR.

---

## 6. Work-item shapes (when ownership is clear)

### 6.1 #824 closing package

```text
outcome: #672 matrix proven on permanent path; #824/#672 closable
why: M002.10 is the remaining compatibility child under the freeze
source_authority: MILESTONE-002 §6.1; #824; #672; SPEC-010/011; ADR-015
in_scope: headed/manual matrix evidence; small attributable fixes; terminfo/fuzz/security honesty
out_of_scope: #673 PHYSICAL_ARM64; #836 multilingual IME; new VT engine; M003 chrome
dependencies_and_blockers: sole human owner; exclusive Runtime for headed XCUI
ownership_boundary: validation/evidence + tiny fixes in existing terminal/host paths
acceptance_criteria:
  1. every required workload has automated or classified manual evidence
  2. exclusive-Runtime headed XCUI not INCONCLUSIVE
  3. independent close review has no red/orange blocker
  4. PR relationship is Closes only when 1–3 hold on exact head
required_evidence: exact-head make check / ui-test / CI; headed ledger; security/fuzz notes
readiness: REFINEMENT_REQUIRED until owner restored; then READY for the closing package only
```

### 6.2 #673 measurement slice (example: HistoryStore PHYSICAL_ARM64)

```text
outcome: one accepted HistoryStore reflow PHYSICAL_ARM64 VALID row set vs contract ceilings
why: contract is on master; rows are the remaining Done gate
source_authority: MILESTONE-002 §6.2; M002-PERFORMANCE-CONTRACT-V1; PERFORMANCE.md; #818 ceilings
in_scope: controlled-host collection, provenance, validator-clean record
out_of_scope: weakening ceilings; claiming key-to-photon without scanout; #837 Unicode matrix
dependencies_and_blockers: controlled Apple Silicon; owner @crdileep82
ownership_boundary: benches/scripts/docs/evidence only — no hot-path product redesign
acceptance_criteria:
  1. evidence class PHYSICAL_ARM64 VALID with AC+thermal+clean-tree+baseline provenance
  2. absolute ceilings pass; relative-tail policy applied with diagnosis if FAIL
  3. Refs #673 unless the full Issue Done checklist is met
readiness: READY for owner-locked slices
```

### 6.3 #837

Defer work-item design until §2.3 resume_condition is true.

---

## 7. What this plan deliberately does not do

- No production code, harness, or ledger edits in the planning PR that lands this file
- No new M002 product Issues
- No stealing `#673` from @crdileep82 or inventing an `#824` assignee
- No `Closes #664` / technical-preview claim

When the next human is ready to implement, enter `.agents/skills/implement-issue/SKILL.md` for the **named** Issue only, after confirming this plan’s Track A/B/C stop rules still hold on a fresh GitHub fetch.
