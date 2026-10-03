# M005 AB-1.5 — segment read protocol fields for real host

Evidence for Issue #1181 on branch `mahboobmonnamd/issue/1181`.

## Placement justification

SPEC-017 §10/`§12` `OutputRef`, `FingerprintRef`, and `RetentionPolicyRef`
live in `seyal-agent-core` so both `seyal-agent-protocol` (client decode) and
`seyal-agent-store` (durable encode) share one codec without a store↔protocol
edge. Protocol re-exports the read surface. Partial-segment merge and
`materialize_output_ref` stay on `AgentStore` beside `append_output_event`.
No PTY / TerminalState / scrollback ownership.

## Claims

| Claim | Evidence |
|---|---|
| §10 fingerprint/retention refs on real-host path | `output_ref.rs`; StandaloneProcessHost high-volume test |
| Clients honor refs without full byte materialization | `segment_metadata_round_trips_without_byte_materialization` |
| Cross-batch partial-segment merge + reconnect replay | `partial_segments_merge_across_batches_and_reconnect_replay` |
| High-volume stays segment-bounded | store `high_volume_append_stays_segment_bounded`; host test; integration E2E |
| Missing/corrupt fingerprint_ref explicit | `missing_and_corrupt_fingerprint_ref_are_explicit`; legacy kind=8 → `MissingFingerprint` |
| Secrets not in normal event payloads (§12) | opaque fingerprint path; public digest does not embed raw secret bytes |
| Dependency firewall unchanged | `python3 scripts/check-layering.py` |

## Local verification

```sh
cargo test -p seyal-agent-core --locked
cargo test -p seyal-agent-store --locked
cargo test -p seyal-agent-protocol --locked
cargo test -p seyal-agent-backend --locked --lib standalone_process_host
cargo test -p seyal-agent-backend --locked --test integration_path high_volume_output_uses_segments
cargo test -p seyal-agent-backend --locked
python3 scripts/check-layering.py
python3 scripts/check-structural-debt.py
```

`performance_claim=false` (append/replay cost note only; not a gated campaign claim).

## Documentation impact

Developer/protocol: `OutputRef` / fingerprint fields re-exported from
`seyal-agent-protocol`. User Guide: none. No ADR create/amend in this PR.
