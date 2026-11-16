# M004 Market-Ready v0.1 Gate

**Purpose:** Define the first release at which Seyal may be presented as a serious everyday macOS terminal.  
**Release owner:** M004 epic #666  
**Terminal prerequisites:** M002 #664 and M003 #665

M004 is not a feature-count race. The release bar is that ordinary terminal users can switch to Seyal without discovering that fundamental shell/TUI/input/history/windowing/recovery/install behavior is missing or fragile.

## Classification

- **LAUNCH BLOCKER** — must be green for v0.1.
- **PRE-LAUNCH OPTIONAL** — desirable if ready, but absence does not invalidate the terminal.
- **POST-LAUNCH** — intentionally outside v0.1; do not pull it into the critical path without an explicit roadmap change.

## Market-readiness matrix

| Capability / workload | Class | Owner | Required evidence before M004 |
|---|---|---|---|
| zsh, bash, fish interactive shells | LAUNCH BLOCKER | #672 | Automated fixtures + long interactive smoke; prompt/edit/history/resize/title/cwd behavior must not corrupt state. |
| Vim/Neovim and ncurses TUIs | LAUNCH BLOCKER | #672 | Alternate-screen enter/leave, resize, cursor, colors, keyboard and mouse cases; no primary-screen corruption. |
| tmux/zellij as ordinary child workloads | LAUNCH BLOCKER | #672 | Nested terminal capability/mouse/resize/key tests; Seyal never treats child mux as Runtime authority. |
| SSH and nested SSH as shell workloads | LAUNCH BLOCKER | #672 | Remote shell/TUI workload matrix through ordinary PTY; remote product attachment is M007, not required. |
| git, Docker, kubectl, terraform and high-output DevOps CLIs | LAUNCH BLOCKER | #672/#673 | Correct ANSI/TUI rendering plus bounded high-output throughput/resource benchmark. |
| Unicode grapheme clusters, combining marks, emoji, width policy | LAUNCH BLOCKER | #684 -> M002 implementation | Golden grid/render cases, cursor/cell-width invariants, fuzz/property tests and representative fonts. |
| macOS IME/text input contract | LAUNCH BLOCKER | #684 -> M002/M003 | Native IME composition tests plus no corruption across Raw/TUI input ownership. |
| Scrollback with bounded retention | LAUNCH BLOCKER | #685 -> M002 implementation | Long-output, eviction, search/selection stability and memory-bound tests. |
| Resize reflow | LAUNCH BLOCKER | #685 -> M002 implementation | Golden reflow cases across wrapped/unwrapped/wide/combining content; repeated-resize stress. |
| Selection, rectangular selection, copy/paste | LAUNCH BLOCKER | #672/#675 | Mouse + keyboard tests in the Raw primary screen; TUI mouse arbitration preserved. |
| Mouse reporting and host override | LAUNCH BLOCKER | #672/#675 | SGR/legacy mode fixtures and Shift-host-selection arbitration. |
| Search retained terminal history | LAUNCH BLOCKER | #672/#675 | Search across bounded Raw history without blocking PTY progress. |
| OSC 8 hyperlinks and safe URL/path opening | LAUNCH BLOCKER | #672/#675 | Parser/state golden cases plus trust/sanitization/open-policy tests. |
| Native macOS windows | LAUNCH BLOCKER | #674 | Lifecycle/focus/close/quit tests; window close never silently kills unrelated Runtime state. |
| Tabs | LAUNCH BLOCKER | #674 | Create/switch/reorder/close with inactive executions remaining live. |
| Nested horizontal/vertical splits | LAUNCH BLOCKER | #674 | Layout/focus/resize/move tests; one PTY per terminal leaf. |
| Raw terminal input path | LAUNCH BLOCKER | #675 | Direct same-execution input, no composer/Block semantic interception in raw/TUI mode. |
| Blocks/composer presentation | POST-LAUNCH | #1493 (reopening decision required) | Suspended for Raw v1. Preserve existing code and contracts; do not expose Flow/Blocks as a resting presentation until a separate architecture decision and headed acceptance gate pass. |
| Fonts/fallback | LAUNCH BLOCKER | #676 | Configured font + fallback + supplementary-plane smoke; missing glyphs fail visibly, not corrupt layout. |
| Themes/light-dark/contrast | LAUNCH BLOCKER | #676 | Deterministic theme schema, OS mode transition, readable defaults and contrast checks. |
| Local TOML config | LAUNCH BLOCKER | #676 | Parse/validation/error-path tests; invalid config cannot brick terminal startup. |
| Keybindings | LAUNCH BLOCKER | #676 | Conflict detection/default navigation/raw forwarding tests. |
| Startup shell/environment/CWD policy | LAUNCH BLOCKER | #676 | Login/non-login and configured-shell fixtures; CWD inheritance semantics documented. |
| Native AppKit + Metal production path | LAUNCH BLOCKER | M001/#677 | Exact-head native smoke proves no fallback terminal renderer/state path. |
| Key-to-photon latency | LAUNCH BLOCKER | #673 | Numeric p50/p95/p99 budgets fixed before M002 implementation; same-hardware exact-head benchmark must pass. |
| PTY -> VT -> projection throughput | LAUNCH BLOCKER | #673 | High-output benchmark, no synchronous agent/persistence/cloud work; defined regression budget must pass. |
| Memory/resource scaling | LAUNCH BLOCKER | #673 | 1/10/50/100-execution scenarios separated into canonical-state cost vs host PTY limits; bounded history/caches. |
| Startup time | LAUNCH BLOCKER | #673/#677 | Cold/warm launch measurements with hardware/build metadata and regression threshold fixed before RC. |
| High-output responsiveness | LAUNCH BLOCKER | #673 | Sustained output while typing/resizing/switching panes; latency distribution and backlog/memory bounded. |
| GUI detach without killing shell | LAUNCH BLOCKER | #677/#687 | Close/quit/detach/reattach tests with stable identities; explicit terminate is distinct and guarded. |
| Runtime crash/recovery honesty | LAUNCH BLOCKER | #687 | Failure-injection matrix states what survives and what cannot; no claim of resurrecting a dead PTY. |
| Layout/history restore | LAUNCH BLOCKER | #687 | Versioned persistence, corruption/migration tests, bounded/redacted retained data, visible recovery failures. |
| Protocol mismatch/reconnect failure | LAUNCH BLOCKER | #687 | Version/failure-injection tests; no infinite reconnect loops or duplicate input/effects. |
| No-account local operation | LAUNCH BLOCKER | #677 | Fresh-machine launch and full terminal use without sign-in/network dependency. |
| Secret/redaction boundaries | LAUNCH BLOCKER | #677 | Tests for diagnostics/history/artifacts; secrets do not enter logs/telemetry by default. |
| Accessibility | LAUNCH BLOCKER | #677 | Keyboard-only navigation, focus/role/label checks, text/selection exposure appropriate to terminal semantics. |
| Diagnostics | LAUNCH BLOCKER | #677 | Local, bounded, privacy-safe diagnostic bundle; terminal remains usable when diagnostics fail. |
| User install docs and troubleshooting | LAUNCH BLOCKER | #677 | Fresh-machine documented install/uninstall/config/recovery walkthrough validated independently. |
| Signing and notarization | LAUNCH BLOCKER | #688 -> #677 | Clean-machine Gatekeeper validation and reproducible release record. |
| Update + rollback/recovery | LAUNCH BLOCKER | #688 -> #677 | Signed update path, interrupted/corrupt update tests and documented recovery; update failure never destroys Runtime/persistent data. |
| Kitty keyboard protocol breadth | PRE-LAUNCH OPTIONAL | #672 | Ship only if conformance is strong; normal target TUI use must not depend on claiming unsupported protocol breadth. |
| Rich automatic shell integration beyond Block correctness | PRE-LAUNCH OPTIONAL | #686 | Shells must work without injection; trusted integration is additive. |
| Command palette/quick-switcher richness | PRE-LAUNCH OPTIONAL | #674 | Basic tab/pane/workspace navigation remains mandatory. |
| Large bundled theme library / visual polish extras | PRE-LAUNCH OPTIONAL | #676 | Stable theme/config schema and readable defaults are mandatory; breadth is not. |
| Terminal graphics/image protocols | POST-LAUNCH | #689 | Explicit M006 strategy; not an M004 blocker. |
| Plugin marketplace / custom interpreted sidebars | POST-LAUNCH | #690/#668 | Extension security boundary first. |
| Agent platform / multi-agent orchestration | POST-LAUNCH | M005/M006 | Agents are a Seyal differentiator, not a prerequisite for terminal market readiness. |
| Bounded code editor/LSP/review/DevOps workspace surfaces | POST-LAUNCH | #683 | Ordinary DevOps CLIs must already work as terminal workloads in M004. |
| Remote Runtime product, mobile, Linux, Windows | POST-LAUNCH | M007 | SSH as a terminal workload is required; Seyal remote-product features are not. |
| Collaboration / enterprise control plane | POST-LAUNCH | M008/M009 | Built after stable local/remote/resource authority. |

## Benchmark gate rules

#673 must publish numeric budgets before M002 implementation is declared Ready. M004 may not ship while key-to-photon, high-output throughput, startup or memory thresholds are still `TBD`.

Every performance result used for a release decision records:
- exact git SHA and build profile;
- hardware/OS/display-scale metadata;
- workload fixture and dimensions;
- p50/p95/p99 plus maximum where meaningful;
- steady-state and cold-start cases where relevant;
- memory/RSS/footprint and cache/history bounds;
- whether a result is a physical end-to-end measurement or an internal proxy.

A faster microbenchmark does not excuse a user-visible regression. A benchmark that hits a host PTY/process limit must label that separately from Seyal state/resource scaling.

## Exact-head release gate

The RC SHA must pass, without evidence borrowed from a different executable head:
1. build/check/unit/integration suites;
2. terminal conformance corpus;
3. retained fuzz/failure-injection gates;
4. native macOS real-shell/TUI smoke;
5. performance/resource gates;
6. persistence/reconnect/corruption recovery matrix;
7. signing/notarization/update tests;
8. fresh-machine install/docs walkthrough;
9. accessibility/privacy/security review.

Documentation-only follow-ups may reuse executable evidence only when they demonstrably cannot change executable outputs, and the release record names both SHAs.

## Competitive baseline

The M004 bar was calibrated against current mature-terminal fundamentals visible in official Ghostty, WezTerm, Kitty, iTerm2, Apple Terminal and Warp documentation during the August 2026 roadmap review. Those products differ in architecture and feature breadth, but collectively establish that reliable windows/tabs/splits, shell/TUI compatibility, Unicode/text handling, history/search/selection/mouse, configuration and responsive native terminal behavior are table stakes. Seyal does not need to duplicate every graphics, cloud, editor or AI feature before v0.1.

## Go/no-go rule

**GO** only when every LAUNCH BLOCKER row is green or an explicit roadmap amendment changes its classification with evidence. “Competitor does not have it”, “CI is mostly green”, “foundation exists”, or “manual smoke looked fine” is not sufficient to waive a blocker.
