# ADR-025 amendment patch plan (#1384)

**Status:** APPLIED 2026-10-29 in this worktree on `mahboobmonnamd/issue/1384`. These edits belong in the docs-only architecture PR that accepts ADR-025, by a non-author maintainer, never in an implementation PR (ISSUE-PROTOCOL.md §"An ADR is required…"). Because ADR-025 is still **Proposed**, normative spec and UI sentences that would change accepted behavior are marked contingent on acceptance of ADR-025 (#1384), the same convention used for Proposed ADR-024. ADR-009's normative invariants were not rewritten; the dated pointer section is the cross-link. Line numbers below are indicative at the ADR-025 draft revision. Each edit cites ADR-025 as authority.

Ordering: accept ADR-025 (+ these amendments) first; then align companion specs where a separate spec PR is called out; then Ready implementation Issues (§12 of ADR-025).

---

## 1. `docs/product/FEATURES.md` — F-088 (line ~160)

**Current row:**

```
| F-088 | Warpify / in-band remote cmds | Rejected | No vendor helper that silently injects commands into SSH/subshell PTYs. |
```

**Replace with** (split the honest-posture distinction; keep "silent" rejected):

```
| F-088 | Silent Warpify / in-band remote inject | Rejected | No vendor helper or Seyal path may inject setup/commands into an SSH/subshell PTY without explicit consent. |
| F-088a | Consented session-only remote shell bootstrap | Accepted direction | Per ADR-025 (#1384): explicit per-session consent (or consent-derived per-host policy) bootstraps in-memory bash/zsh hooks over the existing session for secret-authenticated Blocks + retained composer; nothing persists remotely; decline → labeled relay / Raw. Persistent companion is F-088b / #1385. |
```

Also check `docs/product/ROADMAP-COVERAGE.md` line ~65 (`F-088` in the "Rejected" group): add F-088a to the "Accepted direction" accounting so the 216-row reconciliation and the rejected-group tally stay correct. Do not change F-088's rejected status there.

---

## 2. `docs/architecture/ADR-009-COMMAND-BLOCKS-COMPOSER-AND-TUI.md`

Add a new dated amendment section (not normative here — ADR-025 is its own ADR; this is the cross-link ADR-009 needs so it never contradicts ADR-025), after the "2026-10-18 amendment — composed ClientHello fallback" section (ends ~L1230):

- **New pointer section "2026-10-29 amendment — orthogonal composer input target (ADR-025, #1384)"**, Status: Accepted on merge of the ADR-025 acceptance PR. Content:
  - Composer eligibility (invariant 7, §"Composer eligibility (invariant 7) holds exactly when the state is `AtPrompt`…" ~L392) is **derived from the ADR-025 §3.2 input target**, not from local integration state alone. A `TrustedShell(session)` target makes the composer `Submit`; a `ForegroundProgram(b)` target may offer labeled `Relay`.
  - Mechanism 5 (~L380–L479): nested interactive shells "run inside the outer command's Running Block … emit no trusted marker" is amended by ADR-025 for the **consented** case: a session scope may become session-trusted with its own secret and run the mechanism-5 machine; the accepted-secret set holds the execution nonce **plus at most one session secret**.
  - Mechanism 2 (~L294): "the accepted secret" → "an accepted secret (execution nonce or active session secret)"; a marker is trusted only against the scope whose secret it carries.
  - Alternate-screen rules apply per scope (ADR-025 §7.2).
- **Edit the "Required implementation seams" list (~L649)** only via this amendment: add the ADR-025 seams (session overlay, session secret, target-generation fence, session Blocks) by reference, marked Proposed-until-implementation.

Do **not** edit ADR-009's normative invariants list directly in this PR beyond the pointer; ADR-025 owns the new behavior. Keep the `#1041` prompt-anchor section's Proposed status untouched.

Update ADR-009's **Status** line (L5) reopen-conditions/amendment ledger to record the ADR-025 cross-amendment date and PR.

---

## 3. `docs/specs/SPEC-008-M003-COMMAND-BLOCKS.md`

- **§4 "Composer and input rules" (~L174):** the bullet "while a normal foreground command occupies the shell, the composer is disabled or represented by real busy state" (~L193) → amend: composer availability follows the **ADR-025 §3.2 input target**; a `TrustedShell(session)` target is `Submit`; a `ForegroundProgram` target may be labeled `Relay`; neither is implied by Block `Running` alone. Add the target + target-generation terms to the §3.5 eligibility tuple (~L150).
- **§3.1/§3.2:** note that Relay is a line-level Flow mode distinct from Raw and never routes character-level input (keep ADR-009 invariant 11).
- **§5.5 "Supported shells and CWD trust boundary" (~L301):**
  - Row "Nested shell or SSH child" (~L311) → split: "Nested shell / SSH child (no consent)" stays Unsupported→Raw/relay; add "Consented session (SSH / `docker exec -it` / interactive bash|zsh), ADR-025" = session-trusted `A`/`C`/`D` with a session secret; session-scoped Blocks; decline → labeled relay / Raw.
  - "Accept-or-defer record (#686)" (~L315): move "remote/SSH Block … claims" from **Deferred** to **Accepted under ADR-025 (#1384), consent-gated**; keep live-CWD and Bash/fish-local deferred.
  - Add the default offer-hint argv list (SSH; nested ssh excluded) and state the hint never authorizes bootstrap.
- **§8 "Acceptance matrix" (~L363):** add rows — consented session (discrete session Blocks + retained composer); declined session → labeled relay, no guessed Block; forged remote `A`/`D` ignored (no secret match); depth-two session → relay/Raw; auth/password prompt → not offered / relay suppressed / Raw; session alternate-screen → TUI. Keep existing "Nested shell or SSH child" and "unsupported/nested SSH" rows for the no-consent path.
- **§10 "Non-goals" (~L417):** "Bash/fish/remote trusted integration … in M003" → carve out: remote trusted integration is **consent-gated per ADR-025**; local Bash/fish Flow integration and trusted live CWD remain non-goals.
- **Status/Architecture header (L3–L5):** add ADR-025 to the governing-architecture line.

(If scoped as a separate spec-alignment PR after acceptance, this file + SPEC-004/SPEC-009 move there; F-088, ADR-009 and README stay in the acceptance PR.)

---

## 4. `docs/specs/SPEC-004-M001-LOCAL-ATTACHMENT-PROJECTION.md`

- **§8 capability bits (~L249–L262):** allocate **bit 13** `CAP_SESSION_BOOTSTRAP` (`1 << 13`), gated on `CAP_COMMAND_BLOCKS`. Record it in the bit list and the "Types 1–34 all allocated…" paragraph (~L264).
- **§8 message types table (~L219):** allocate the next free types ≥ **42** for: `SessionOffer` (R→C), `SessionConsent` (C→R), `SessionStatus` (R→C), and an extended `ComposerStatus` variant carrying the input target + target generation (or a new `ComposerTarget` R→C frame). Keep exact-length framing and §7 header rules; fail-closed on malformed/oversized.
- **`BlockTimeline` (type 21) record:** add `scope` (Local/Session) and `trust` (LocalTrusted/SessionTrusted/Unverified) with explicit presence encoding, negotiated by `CAP_SESSION_BOOTSTRAP`; compose its ClientHello fallback **after** the duration/viewport/extended-key chain (ADR-009 #1355 order). Never infer schema from length.
- **Composer status fence:** extend the §3.5 (SPEC-008) tuple mirror so a `SessionStatus`/target frame is accepted only for the current `attachment_id` and a forward-moving revision/target generation (mirror `ComposerStatus` semantics in `crates/seyal-protocol/src/pass7/composer.rs`).
- Note: no SCM_RIGHTS / descriptor is sent for sessions; the session secret never crosses the client boundary (ADR-025 §6.1, §10 T15/T19).

---

## 5. `docs/specs/SPEC-009-M001-DETACH-RECONNECT.md`

- Reconnect/handoff sections (~L42, L483, L597 "Flow reconnect …"): after fresh attachment/controller/canonical validation, the Pane **re-derives composer mode from current facts (ADR-025 §3.2)**; a trusted session is not resurrected across Runtime replacement; the session secret is Runtime-held and never restored to a client. Add a bullet that an in-progress `Armed`/`Delivering` bootstrap fails closed across detach (ADR-025 §10 T15).

---

## 6. `docs/architecture/ui/M001-LIVE-TAIL-BEHAVIOR.md`

- **§6 "Composer while the foreground shell is busy" (~L52):** amend the preferred-presentation bullets (~L58) — the composer is not unconditionally retracted while a foreground child runs; if that child is a **consented session-trusted shell at its prompt**, the composer is `Submit` to that shell; if it is an eligible foreground program, a clearly labeled `Relay` may be offered (ADR-025 §3.2/§8). Retraction remains correct when the input target is `None`.
- **§7 "New Blocks while running" (~L64):** add that a session-trusted shell produces session-scoped Blocks nested under the host Running Block (ADR-025 §7.2); this is not a second same-shell execution path.
- **§11 "Distinction from TUI takeover":** unchanged; a session alternate-screen is still full TUI takeover.

**Related composer references to align in the same PR** (all define "busy" as negation of ADR-009 eligibility; update to the ADR-025 §3.2 input-target definition, which now includes `TrustedShell(session)` and labeled `Relay`):

- `docs/architecture/ui/SEYAL-COMPOSER-COMPONENT-SPEC.md` §11 (~L277–L293) and §12.
- `docs/architecture/ui/M001-CORE-TERMINAL-REFERENCE-SCREEN.md` §9.1 (~L310).
- `docs/architecture/ui/M001-FIRST-UI-DESIGN-AMENDMENT.md` (~L44, L58).
- `docs/architecture/ui/M001-MULTIPANE-VIEW.md` (~L72, L77).
- `docs/architecture/ui/M001-COMPOSER-HISTORY-FUZZY-SEARCH.md` (~L63, L68).
- `docs/architecture/ui/M001-CORE-TERMINAL-REFERENCE-INDEX.md` (~L43, L58) summary wording.

---

## 7. `docs/architecture/README.md`

- **ADR listing (after the ADR-024 entry ~L28 "17h."):** add
  `17i. ADR-025-CONSENTED-SESSION-REMOTE-SHELL-BOOTSTRAP.md — **Proposed** under #1384. Orthogonal Block-lifecycle / composer-input-target / presentation model; consented session-only in-band bash/zsh bootstrap for secret-authenticated session Blocks with a retained composer; decline → labeled relay / Raw; depth-one; silent inject stays rejected (F-088). Companion spec alignment (SPEC-008/004/009) and sibling #1385 follow acceptance.`
- **"## Authority" ownership bullets:** add an ADR-025 ownership bullet mirroring the ADR-009 one, scoped to the orthogonal input-target model and consented session bootstrap; note it does not create a second VT/grid/session engine and does not own the persistent companion (#1385).
- If the ADR-009 bullet (~L62) is edited for the cross-amendment pointer, keep it consistent with item 2 above.

---

## Not amended (confirm during review)

- `AGENTS.md` invariants — ADR-025 operates within them (one PTY/VT, no hot-path IPC/alloc, Rust product authority, headless survival); no change needed.
- `docs/architecture/ADR-005-PTY-EXECUTION-LIFECYCLE.md` — termination/reap unchanged (ADR-025 §10 T9); no edit.
- `docs/product/FEATURES.md` F-171/F-173 (SSH attach) — different capability (remote Seyal Runtime attach), not this ADR; no edit.
- Milestone files (`MILESTONE-002/003`, `MARKET-READY-M004.md`, `ROADMAP.md`) — SSH-as-child workload references are M002 conformance, not this decision; no edit unless the owning milestone pulls A2 implementation in.
