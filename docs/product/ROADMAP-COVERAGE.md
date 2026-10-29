# Seyal Roadmap Coverage Ledger

**Status:** Complete roadmap ownership ledger  
**Canonical source:** `docs/product/FEATURES.md`  
**Rule:** Every capability has exactly one current roadmap disposition. Capability names and product semantics remain canonical in `FEATURES.md`; this file assigns roadmap ownership by stable ID.

## Disposition vocabulary

- **Known future implementation** — accepted capability with one implementation package/epic.
- **Spike/R&D before implementation** — architecture/product uncertainty must be closed by the named spike before implementation.
- **Explicitly deferred** — intentionally outside the current committed exit gate.
- **Superseded** — legacy shape must not be implemented; the named owner carries the replacement need.
- **Rejected** — deliberately outside current Seyal direction.

The 216 historical `F-*` rows are fully accounted for: **137 known future implementation, 31 spike/R&D, 25 explicitly deferred, 15 superseded, 8 rejected**. F-088a is outside that historical set. It is **Proposed under #1384** (contingent on Proposed ADR-025 acceptance) and does not change the rejected tally of 8 or F-088's Rejected disposition.

## F-* ownership

Ranges below are inclusive. Together they cover exactly the 216 IDs in the canonical RILL reconciliation table: F-001–029, F-030–047, F-050–089, F-090–104, F-110–124, F-130–164, F-170–180, F-190–205, F-210–231, F-240–250, plus F-252, F-253, F-254 and F-256. There are no duplicate owners. F-088a is an additional ID and is not part of that 216-ID cover.

| Capability IDs | Milestone | Current owner | Roadmap disposition |
|---|---|---|---|
| F-090 | M001 | #5 | Superseded |
| F-028, F-091–F-094, F-102 | M002 | #672 | Known future implementation |
| F-095 | M002 | #684 | Spike/R&D before implementation |
| F-099 | M002 | #685 | Spike/R&D before implementation |
| F-001–F-007, F-009–F-010, F-012–F-020, F-024, F-026, F-047, F-050, F-201–F-202, F-226, F-256 | M003 | #674 | Known future implementation |
| F-022 | M003 | #674 | Superseded |
| F-051, F-054–F-055, F-057, F-060, F-063–F-064, F-069–F-077, F-081, F-083–F-087, F-089, F-252–F-254 | M003 | #675 | Known future implementation |
| F-053 | M003 | #675 | Explicitly deferred |
| F-068, F-082 | M003 | #675 | Superseded |
| F-098 | M003 | #676 | Superseded |
| F-100–F-101, F-103–F-104, F-204, F-210–F-213, F-216–F-218 | M003 | #676 | Known future implementation |
| F-058, F-097 | M003 | #686 | Spike/R&D before implementation |
| F-045 | M004 | #666 | Explicitly deferred |
| F-030, F-033, F-042–F-043, F-223–F-224, F-229 | M004 | #677 | Known future implementation |
| F-031–F-032, F-034–F-036, F-039–F-041, F-044 | M004 | #687 | Spike/R&D before implementation |
| F-225, F-230 | M004 | #688 | Spike/R&D before implementation |
| F-122 | M005 | #667 | Explicitly deferred |
| F-029, F-152 | M005 | #678 | Known future implementation |
| F-130 | M005 | #678 | Superseded |
| F-037, F-131, F-136–F-139, F-143, F-149–F-151 | M005 | #679 | Known future implementation |
| F-135 | M005 | #679 | Superseded |
| F-011, F-110–F-121, F-123–F-124, F-132–F-134, F-162 | M005 | #680 | Known future implementation |
| F-161 | M005 | #680 | Superseded |
| F-079 | M005 | #681 | Superseded |
| F-144–F-145, F-154 | M005 | #681 | Known future implementation |
| F-008, F-021, F-046, F-052, F-059, F-061–F-062, F-065–F-066, F-155–F-156, F-163, F-198–F-199, F-214, F-221, F-249 | M006 | #668 | Explicitly deferred |
| F-067, F-078 | M006 | #682 | Superseded |
| F-140–F-142, F-147–F-148, F-153, F-203 | M006 | #682 | Known future implementation |
| F-025, F-056, F-146, F-190–F-193, F-195, F-240–F-248 | M006 | #683 | Known future implementation |
| F-194, F-222 | M006 | #683 | Superseded |
| F-096 | M006 | #689 | Spike/R&D before implementation |
| F-027, F-219–F-220 | M006 | #690 | Spike/R&D before implementation |
| F-038, F-157, F-177–F-178, F-197 | M007 | #669 | Explicitly deferred |
| F-231 | M007 | #669 | Known future implementation |
| F-227 | M007 | #691 | Spike/R&D before implementation |
| F-228 | M007 | #692 | Spike/R&D before implementation |
| F-170–F-176, F-180 | M007 | #693 | Spike/R&D before implementation |
| F-196 | M007 | #694 | Spike/R&D before implementation |
| F-215 | M007 | #694 | Superseded |
| F-158 | M008 | #670 | Known future implementation |
| F-080 | M008 | #695 | Superseded |
| F-159 | M008 | #695 | Spike/R&D before implementation |
| F-023, F-088, F-160, F-164, F-179, F-200, F-205, F-250 | — | `FEATURES.md` decision | Rejected |
| F-088a | — | Proposed ADR-025 / #1384 | Proposed (becomes Accepted direction on ADR-025 acceptance) |

## SY-* ownership

| ID | Capability | Milestone | Current owner | Roadmap disposition |
|---|---|---|---|---|
| SY-001 | Seyal Resource Addressing | M003 | #674 | Known future implementation |
| SY-002 | Context-aware Seyal CLI | M006 | #683 | Known future implementation |
| SY-003 | Exact teammate handoff | M008 | #695 | Spike/R&D before implementation |
| SY-004 | Agent Worktree Awareness | M006 | #683 | Known future implementation |
| SY-005 | Safe worktree shell transition | M006 | #683 | Known future implementation |
| SY-006 | Tiered Agent Presence Detection | M005 | #679 | Known future implementation |
| SY-007 | Provider-neutral SCM/CI adapters | M006 | #683 | Known future implementation |
| SY-008 | Secure Remote Connection Multiplexing | M007 | #693 | Spike/R&D before implementation |
| SY-009 | Stable workspace ordering + attention projection | M005 | #680 | Known future implementation |
| SY-010 | Universal Seyal Integration CLI / Shell API | M006 | #683 | Known future implementation |
| SY-011 | Capability-scoped Control API | M006 | #683 | Known future implementation |
| SY-012 | Block References | M006 | #682 | Known future implementation |
| SY-013 | Command Library | M006 | #682 | Known future implementation |
| SY-014 | Parameterized Commands | M006 | #682 | Known future implementation |
| SY-015 | Promote command sequence to Workflow/Runbook | M006 | #682 | Known future implementation |
| SY-016 | Selective local-first sync | M007 | #694 | Spike/R&D before implementation |
| SY-017 | Key-to-photon latency contract | M002 | #673 | Known future implementation |
| SY-018 | Local Context Engine | M005 | #681 | Known future implementation |
| SY-019 | Local capability/rule router | M005 | #681 | Known future implementation |
| SY-020 | Durable local workflow DAG + effect/replay safety | M006 | #682 | Known future implementation |
| SY-021 | Multi-agent orchestration + writer isolation | M006 | #682 | Known future implementation |
| SY-022 | DevOps execution workspace | M006 | #683 | Known future implementation |
| SY-023 | Working-tree Changes inspector | M006 | #683 | Known future implementation |
| SY-024 | Agent evaluation, budgets and explainability | M005 | #681 | Known future implementation |
| SY-025 | Context Privacy Scope | M005 | #681 | Known future implementation |
| SY-026 | Quick Agent Popover | M005 | #680 | Known future implementation |

## Existing backlog issue reconciliation

`#640`–`#650` remain discovery/reconciliation buckets, not a second implementation roadmap.

| Existing issue | Current role / roadmap home |
|---|---|
| #640 | Hierarchy/navigation catalog -> primarily M003 #674; agent/resource/deferred rows split by the F-* table above. |
| #641 | Persistence/recovery catalog -> primarily M004 #677/#687; agent continuity M005; live handoff M007. |
| #642 | Input/Blocks catalog -> primarily M003 #675; advanced composer/workflow work M006. |
| #643 | Terminal fidelity catalog -> M002 #672/#673/#684/#685; shell integration M003 #686; graphics M006 #689. |
| #644 | Attention catalog -> M005 #680; password notification remains explicitly deferred. |
| #645 | Agent catalog -> M005 #678–#681, M006 #682/#683, later cloud/collaboration rows M007/M008. |
| #646 | Remote catalog -> M007 #693 plus deferred transport/network options under #669. |
| #647 | Extra surfaces catalog -> M006 #683, mobile M007 #694, deferred utility surfaces under #668/#669. F-194 is superseded, not rejected. |
| #648 | Appearance/config/security catalog -> M003 #676, M004 #677/#688, plugins M006 #690, platforms M007. |
| #649 | Coding catalog -> M006 #683. F-240 bounded editor and F-241 optional/lazy LSP are accepted; F-250 DAP is rejected. |
| #650 | Product backlog umbrella -> roadmap governance/coverage only; implementation authority is the package named per capability. |
| #262 | Seyal-native umbrella -> all SY-001..SY-026 are individually mapped above. |

## #197 coding research children

| Child | Research capability | Milestone | Production home | Note |
|---|---|---|---|---|
| #201 | Palette file open | M006 | #683 | Bounded developer navigation. |
| #205 | Tabbed editor | M006 | #683 | Bounded editor; never terminal authority. |
| #209 | Editor R&D boundary | M006 | #683 | Research input before implementation. |
| #212 | Find/replace | M006 | #683 | F-242. |
| #216 | Syntax support | M006 | #683 | Editor presentation only. |
| #220 | Exact agent file/line | M006 | #683 | Uses stable resource/context identity. |
| #223 | Code review | M006 | #683 | F-243. |
| #228 | Code references | M006 | #683 | Typed references. |
| #231 | Code index | M005 | #681 | Context Engine prerequisite. |
| #236 | File tree | M006 | #683 | Root-bounded developer surface. |
| #239 | Agent changed files | M006 | #683 | Depends on M005 agent identity/context. |
| #243 | Diff -> build/test | M006 | #683 | Uses safe workflow execution #682. |
| #247 | Selected code context | M006 | #683 | Depends on #681. |
| #251 | Fix with agent | M006 | #683 | Uses M005 adapter/attention/context foundations. |
| #255 | Recurring defect memory | M006 | #683 | Retrieval/evaluation foundation from #681. |

## M001 specifications and accepted ADR implications

M001 is **Done / closed** and is not rewritten by this roadmap. Historical planning-baseline authority was #5 and `docs/milestones/MILESTONE-001.md`; Pass 6 implementation (#658 / PR #659) was unmerged at that planning baseline and later completed under the accepted M001 lineage.

| Authority | Roadmap enforcement |
|---|---|
| SPEC-001 M001 VT | M001 parser/state foundation; M002 extends breadth without replacing terminal authority. |
| SPEC-002 M001 PTY | PTY/child lifecycle stays at TerminalExecution; later panes/surfaces reuse it. |
| SPEC-003 M001 Runtime | Headless Runtime ownership remains beneath M003–M009. |
| SPEC-004 M001 Local Attachment Projection | Display client state remains derived/disposable for local and later remote clients. |
| SPEC-005 M001 Metal Renderer | Permanent macOS Metal renderer; no second GUI terminal state/renderer. |
| ADR-001 Local Display Projection | M003 UI, M004 restore and M007 thin clients project canonical Runtime state. |
| ADR-003 OSS/Commercial Repository Boundary | OSS never depends on proprietary code; M008/M009 private implementation consumes public seams one-way. |
| ADR-004 VT State Ownership | M002 Unicode/scrollback/compatibility modify one canonical TerminalState only. |
| ADR-005 PTY Execution Lifecycle | One PTY per terminal execution leaf; no popup/editor/agent shadow PTYs. |
| ADR-006 Runtime Reactor | Workspace/agent/persistence/cloud work never synchronously gates terminal progress. |
| ADR-007 Workspace Persistence & Agent Continuity | M004 recovery is honest about live-process limits; M005 continuity uses explicit durable identities. |
| ADR-008 Terminfo Capability Ownership | M002 advertises only behavior Seyal actually implements and proves. |

The accepted agent R&D program (#48, #51–#57 and `docs/architecture/agent-rd/*`) promotes to #678 (domain), #679 (adapter/presence), #681 (context/cache/evaluation/routing/privacy) and #682 (workflow/effect/replay/multi-agent). Security/performance isolation is a cross-cutting acceptance gate on #678–#683.

## Orphan audit rule

A roadmap audit fails if any new `F-*`, `SY-*`, #197 child, accepted ADR/spec requirement or newly accepted capability lacks exactly one disposition and one canonical owner/replacement decision here. Dependencies may be many; ownership must be one.
