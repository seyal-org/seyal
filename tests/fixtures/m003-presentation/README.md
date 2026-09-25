# M003 presentation workload fixtures

Retained local child-process workloads for later headed/unit/integration tests
under parent #868. These fixtures are **external application workloads only**.
They never become terminal/Block truth and must not change product, runtime,
renderer, VT, or `TerminalState` code.

Owning Issue: #1005  
Placement: `tests/fixtures/m003-presentation/` per `docs/engineering/TESTING.md`

## Safety

- Network-free
- No reads of user shell history, config, or credentials
- No home mutation
- Every workload has an explicit output and time bound (see `manifest.toml`)

## Fixtures

| id | script | invocation | exit | markers / bounds |
| --- | --- | --- | --- | --- |
| `short-normal` | `short-normal.sh` | `/bin/bash --noprofile --norc tests/fixtures/m003-presentation/short-normal.sh` | `0` | `seyal-m003-short-stdout`, `seyal-m003-short-stderr`, `seyal-m003-short-done`; timeout 5s |
| `long-output` | `long-output.sh` | `/bin/bash --noprofile --norc tests/fixtures/m003-presentation/long-output.sh` | `0` | exactly **200** `seyal-m003-long line=NNNN` lines + `seyal-m003-long-done lines=200`; timeout 10s |
| `alternate-screen` | `alternate-screen.sh` | `/bin/bash --noprofile --norc tests/fixtures/m003-presentation/alternate-screen.sh` | `0` | enter/active → bounded `read -t` (default 5s) → exit; EXIT trap emits `seyal-m003-altscreen-restored` and leaves DECSET 1049; timeout 15s |
| `resize-observable` | `resize-observable.sh` | `/bin/bash --noprofile --norc tests/fixtures/m003-presentation/resize-observable.sh` | `0` | reports `cols`/`rows` (clamped ≤80×24), draws `seyal-m003-resize-row` grid on alt-screen, restores; timeout 10s |

Authoritative field list: `manifest.toml`.

## Self-test

From the repository root:

```sh
python3 scripts/test-m003-presentation-fixtures.py
```

The self-test proves each fixture starts, terminates within its documented bound,
emits every required marker, and emits no `seyal-m003-*` marker outside the
documented allowed prefixes. It is wired into `make check` / `make test` via
`scripts/task.sh`.

## Consumption

Later #868 / presentation tests should invoke these scripts by path (or the
`invocation` strings in the manifest) instead of embedding ad-hoc shell in the
test. Do not copy product logic into these fixtures.
