# Agent presence enforcement (SY-006 / ADR-012 §12)

Permanent production types for negotiated adapter capabilities and presence
observations live in `seyal-agent-core` (`presence` module), with wire codecs in
`seyal-agent-protocol` and the backend projection gate in
`seyal-agent-backend::PresenceEnforcementPlane`.

## Enforcement classes (ADR-012 names)

| Class | Meaning |
| --- | --- |
| `Observed` | Observe/report only |
| `UpstreamRequestable` | May request upstream behavior; must not claim local enforcement |
| `BackendEnforced` | Operation crosses a Seyal-owned typed authority boundary |

**Product synonym:** user-facing copy may say **SeyalEnforced** only when it
maps 1:1 to `BackendEnforced`. Implementation, wire encodings, and developer
docs use the ADR-012 names.

## SY-006 presence-source tiers

Highest confidence first:

1. `StructuredAdapter`
2. `OfficialHooks`
3. `ProcessShellSignals`
4. `LowConfidenceHeuristic`

Heuristic and process/shell presence **cannot** be labeled `BackendEnforced`
and cannot authorize approval, audit, billing, or model-selection claims.
Raw terminal text is never approval/control truth (ADR-012 §13). External CLI
effects observed only via terminal text / heuristic never become
`BackendEnforced` Action evidence (SPEC-016 / ADR-014).

Source-tier ceilings also cap claim authorization:

| Source | Max enforcement |
| --- | --- |
| `StructuredAdapter` | `BackendEnforced` (typed boundary required) |
| `OfficialHooks` | `UpstreamRequestable` |
| `ProcessShellSignals` / `LowConfidenceHeuristic` | `Observed` |

## Claim modes

- `Observe` — requires at least `Observed` (caps alone OK)
- `UpstreamRequest` — requires presence evidence and effective class
  `UpstreamRequestable` or `BackendEnforced`
- `LocalEnforcement` — requires presence evidence, typed backend boundary, and
  effective class `BackendEnforced`

**Effective enforcement** for privileged modes is
`min(capability, source.max_enforcement(), presence.enforcement)`.

Unsupported and unknown capabilities remain explicit; there is no implicit
“full control”. Duplicate `CapabilityId`s are rejected at decode/install.

## Install trust binding

| Path | Trust | `BackendEnforced` caps |
| --- | --- | --- |
| `apply_handshake_payload` / untrusted peer | `UntrustedPeer` | Rejected |
| `install_trusted_handshake_payload` / `set_capabilities` | `BackendPolicyTrusted` | Allowed after adapter-policy binding |

Wire decode alone is structural; installing into the live plane requires an
explicit trust binding.

## Out of scope here

Conformance catalog (#1277), replay/fake adapter (#1278), and real Claude/Codex
adapters (#1279–#1280) consume these types; they are not implemented in this
plane.
