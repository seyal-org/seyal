# Repository and workspace structure

This document defines Seyal's physical repository layout and the accepted logical ownership/dependency boundaries. Passes 1–10 have materialized and validated the production Rust and native surfaces described below. Pass 10 (#727, closed) validated that surface; it did not invent a new layout.

## Principle

A crate/module exists only when it represents a real ownership, portability, process, ABI/dependency, or testing boundary.

The public Seyal repository is the single canonical OSS codebase. Headless, lightweight and full OSS forms are compositions of the same terminal/runtime authority, not separate repositories.

## Current physical Rust layout

M001 Passes 1–9 have seven justified production Rust ownership boundaries:

```text
/
├─ Cargo.toml
├─ Cargo.lock
└─ crates/
   ├─ seyal-core/              # stable IDs / shared value types only
   ├─ seyal-terminal/          # canonical portable terminal-semantics ownership boundary
   ├─ seyal-exec/              # TerminalExecution / PTY endpoint + child lifecycle boundary
   ├─ seyal-protocol/          # Candidate-D wire framing, display values, discovery validation
   ├─ seyal-runtime/           # per-user Runtime, attachments, projection producer, BlockTimeline
   ├─ seyal-render/            # portable prepared-surface normalization for Metal
   └─ seyal-client/            # disposable local attachment / DisplayCache commit path
```

`seyal-core` owns only stable identity/value types required across authority and protocol layers. It owns no PTY, VT, Runtime registry, protocol transport or renderer state.

`seyal-terminal` owns the permanent incremental VT/parser/state model introduced by Issue #38. `seyal-exec` owns PTY descriptor ownership, child lifecycle and `TerminalExecution`. `seyal-protocol` owns versioned Candidate-D framing and disposable display-value contracts. `seyal-runtime` owns the headless per-user Runtime, logical attachments, projection production and M001 Workspace/`BlockTimeline` metadata (no separate `seyal-workspace` crate exists yet). `seyal-render` owns portable prepared-surface normalization. `seyal-client` owns disposable local attachment state, atomic `DisplayCache` commit before native render, the portable headed Workspace/Tab/Pane product reducer, Flow/Raw/TUI presentation fencing, reconnect/recovery policy, composer/Block projection, chrome/inspector/attention projection, and portable theme/config resolution. It may depend on `seyal-core` identity types. It does not depend on `seyal-runtime` in production. Native hosts map resolved tokens to AppKit; this crate does not.

Do not create empty diagram-driven packages. A future physical `seyal-workspace` crate is justified only when Workspace/Block ownership needs a process/ABI boundary that Runtime composition cannot keep cleanly.

ADR-006 keeps the macOS PTY readiness-composition mechanism inside `seyal-exec`; the Runtime consumes safe reactor events and does not receive PTY ownership/raw descriptors.

## Current physical native macOS layout

`macos/Seyal` is the permanent native application boundary (**Swift + AppKit + Metal**). Pass 1 established the skeleton; Passes 6–9 added the permanent Metal terminal surface, Candidate-D client bridge, native input/resize/focus/IME, minimal Block presentation and detach/reconnect recovery.

M001.1 freezes the current AppKit **product** shell as deprecated (not product authority), implements portable product behavior in Rust, removes the rejected shell from the supported path (#890; may temporarily omit a headed app), then replaces the host with a new thin adapter (#883). The freeze/authority map is `docs/engineering/M001.1-SWIFT-OWNERSHIP-PARITY-MANIFEST.md`; it does not replace this layout document.

```text
macos/Seyal/
├─ Seyal.xcodeproj/            # native macOS application target + shared scheme
├─ Info.plist
├─ README.md
├─ Sources/                    # AppKit shell, Metal renderer, client bridge, input/IME,
│                              # Block presentation, Runtime launch/recovery coordinators
└─ Tests/                      # XCTest / XCUI coverage for shell and design-system seams
```

No Objective-C/Objective-C++ source is required for the current platform boundary. Metal shaders use Metal Shading Language. The native host does not own a second VT/grid/PTY authority; it consumes derived Candidate-D / prepared-frame data across a coarse C-compatible Rust/native boundary.

A second native implementation language requires concrete evidence that Swift plus the approved coarse Rust/native boundary cannot satisfy the requirement.

## Target logical layout

```text
/
├─ AGENTS.md
├─ PRODUCT.md
├─ docs/
│  ├─ architecture/
│  ├─ specs/
│  ├─ milestones/
│  └─ engineering/
├─ crates/                     # created incrementally when a real boundary is required
│  ├─ seyal-core/              # stable IDs/common value types only if justified
│  ├─ seyal-terminal/          # VT/TerminalState/history/damage semantics
│  ├─ seyal-exec/              # TerminalEndpoint/PTY + child lifecycle + TerminalExecution
│  ├─ seyal-workspace/         # logical BlockTimeline/workspace metadata (currently inside Runtime)
│  ├─ seyal-protocol/          # versioned local/remote protocol + projection schema
│  ├─ seyal-render/            # portable render preparation, not Metal ownership
│  ├─ seyal-runtime/           # per-user authority/orchestration of executions/attachments
│  └─ seyal-client/            # disposable local attachment / display commit
├─ macos/
│  └─ Seyal/                   # AppKit/Swift/native Metal host
├─ tests/
│  ├─ fixtures/
│  │  └─ vt/
│  ├─ integration/
│  └─ conformance/
├─ fuzz/
├─ benches/
└─ scripts/
```

Names may be adjusted during active milestones only if evidence shows a smaller correct layout, but authority/dependency rules below may not be silently changed.

Do not create speculative composition-specific crates merely to mirror future product names. Composition roots should be introduced only when a real binary/application boundary exists.

## OSS variants

Conceptually, all variants consume the same foundational implementation:

```text
                       shared Seyal OSS core
                              │
                ┌─────────────┼─────────────┐
                ▼             ▼             ▼
            headless      lightweight      full OSS
             runtime        terminal        native app
```

The exact binaries/packages are decided by active milestones. The architectural rule is fixed: there is one PTY implementation, one VT/state model, one Runtime ownership model and one renderer architecture. Variants may omit layers they do not need; they do not fork those layers.

## Ownership

- `seyal-core`: stable identity/value types only. No PTY, VT, Runtime, transport or UI ownership.
- `seyal-terminal`: canonical terminal semantics only. No GUI, cloud, workspace Blocks or process ownership.
- `seyal-exec`: terminal endpoint/PTY, child lifecycle, `TerminalExecution`, and the macOS safe readiness-composition seam; consumes terminal semantics. Reactor registration never owns the execution.
- `seyal-workspace` (logical): Block/workspace metadata keyed by stable execution/history identities; no PTY/VT ownership. M001 keeps this composition inside `seyal-runtime`.
- `seyal-protocol`: versioned messages/projection types and validation; no authoritative terminal state.
- `seyal-render`: derived render preparation; no canonical VT/grid ownership.
- `seyal-runtime`: per-user authoritative execution registry, logical attachment ownership, bounded multi-execution orchestration and M001 BlockTimeline composition.
- `seyal-client`: disposable local attachment/client state, DisplayCache commit, portable headed product composition, Flow/Raw/TUI presentation fencing, reconnect/recovery policy, composer/Block projection, chrome/inspector/attention projection, and portable theme/config resolution; no Runtime production dependency.
- `macos/Seyal`: AppKit/native lifecycle/input/Metal surface; consumes derived projection only.

## Allowed dependency direction

Conceptually:

```text
core/value types
   ↓
terminal
   ↓
exec
   ↓
runtime

workspace → core/value types + terminal history identity interfaces
protocol  → core/value types + stable projection/value contracts
render    → protocol/derived projection types
client    → protocol + render
macOS app → client/render/native bridge
runtime   → exec + terminal + protocol producer (+ workspace composition)
```

Avoid circular dependencies. If `seyal-core` becomes a dumping ground, split or remove it; it may contain stable identity/value types only.

The current Cargo workspace is acyclic by construction under `scripts/check-layering.py`. Forbidden production edges include `seyal-client → seyal-runtime` and `seyal-protocol → seyal-runtime`. The public `Foundation Quality` `repository-policy` job runs this check on every PR and `scripts/test-ci-validators.py` proves controlled forbidden dependencies are rejected. Dev-dependencies are intentionally excluded so integration tests can compose the real Runtime without contaminating production architecture.

## Repository isolation

External/private consumers may depend on public Seyal OSS capabilities, but the dependency direction must never reverse:

```text
external/private consumer → Seyal OSS
Seyal OSS                 ↛ non-OSS/private implementation
```

The public repository does not contain private implementation details or entitlement-aware branches. A public extension seam is acceptable only when it is a coherent capability useful to any OSS user and required by a real milestone.

## Forbidden dependencies

- terminal → workspace/Blocks, agents, cloud, telemetry, UI/native frameworks
- exec → renderer/UI/cloud/runtime ownership
- runtime readiness → GUI/Swift, renderer, agents, cloud or non-OSS/private implementation
- workspace → PTY ownership or canonical VT mutation
- render → mutable Runtime/TerminalState internals
- client → Runtime production dependency
- macOS app → a second VT/grid implementation
- OSS production code → non-OSS/private packages or private entitlement state

Enforce these with Cargo workspace layering checks/lints or a small dependency-graph CI script as physical crates are introduced.

## Test/fixture/benchmark locations

- Unit tests live with owning modules.
- External/reference terminal fixtures live under `tests/fixtures/vt` with provenance.
- Cross-crate/runtime/native integration tests live under `tests/integration` or a justified harness package.
- Fuzz targets live under `fuzz/`.
- Reproducible performance workloads/results metadata live under `benches/` and documented artifacts; generated result blobs should not pollute production modules.

These locations were introduced incrementally by their owning Issues. Issue #11 established the general harness contracts; later passes activated production-specific fixtures, fuzz adapters and pass benchmarks. Pass 10 re-validates the aggregate evidence surface rather than inventing a parallel harness tree.

## Platform boundary

Portable terminal/runtime behavior is Rust. macOS-only AppKit/Metal/input/accessibility code stays under the macOS host. Darwin `kqueue` details stay in the existing macOS platform/exec boundary rather than leaking into portable Runtime domain logic. Do not create a generic cross-platform GUI or reactor framework before another platform is under active development.

The native-language default for the macOS host adapter is Swift. Swift may own only the thin platform list in ADR-015. Introduce Objective-C/Objective-C++ only when a reviewed Issue demonstrates a specific API or interoperability need that Swift cannot satisfy cleanly. Cross the Rust/native boundary with coarse C-compatible arrays/runs/batches rather than per-cell callbacks. Portable product/UI state must not be added to Swift.

## Agent instructions

Use root `AGENTS.md`. Add nested `AGENTS.md` only if a real subsystem later needs scoped instructions that would be harmful globally.

## Build interface

M001 Pass 1 Issue #8 pinned the deterministic toolchain and canonical root `make bootstrap/build/test/check/bench` interface. Subsequent passes activated that interface against the expanding Rust workspace and native `Seyal.app` surface. Issue #12 made the corresponding public CI and architecture dependency gates deterministic and self-validating. The current production crates and native path above are the accepted Passes 1–9 layout; Pass 10 validates them in place.

## Historical Pass-1 scaffolding note (superseded)

> **Superseded.** Early Pass-1 wording that described an empty/minimal scaffolding workspace, a non-terminal native skeleton, or “crates remain logical until later passes” is historical. The physical layout sections above are current authority.
