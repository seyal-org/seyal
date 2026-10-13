# M005 SPEC-020 BaselineCalibrationArtifact + §20 corpus pack

- **Issue:** #1294
- **Parent package:** #681
- **Parent epic:** #667
- **Unblocks ranking child:** #1275
- **Architecture:** ADR-016
- **Behavioral authority:** SPEC-020 §11 (profiles), §16 (BaselineCalibrationArtifact), §20 (calibration gate)
- **Distributable artifact:** [`m005-spec020-baseline-calibration-artifact-v1.toml`](m005-spec020-baseline-calibration-artifact-v1.toml)
- **Artifact version:** `seyal.spec020.baseline-calibration.v1`
- **Integrity (SHA-256 of the TOML artifact bytes):** `9d31ee776b06d288d914b2c55a8d2354459fa46894d237592ffc4508041b7ecf`
- **Purpose:** freeze a versioned, integrity-verified cold-start baseline and a reproducible §20 measurement procedure so #1275 can re-run `development-readiness`. This pack does **not** implement V1 ranking or compose `selection_kind` beyond pin/singleton.

## Decision summary

| Parameter | Frozen v1 value | Authority |
| --- | --- | --- |
| Artifact id / version | `seyal.spec020.baseline-calibration.v1` | SPEC-020 §16 |
| Integrity method | SHA-256 of [`m005-spec020-baseline-calibration-artifact-v1.toml`](m005-spec020-baseline-calibration-artifact-v1.toml) | SPEC-020 §16 |
| Factor order | Q, C, T, R, K, L, P | SPEC-020 §7 / §11 |
| Weight derivation | policy-identity equal base + named-factor emphasis (`emphasis_multiplier = 2`), then L1-normalize; **not** hand-tuned to examples; **not** corpus-tuned; **not** the synthetic POC table | SPEC-020 §11 / §20; #1294 |
| Balanced weights | numerators all `1` / denominator `7` (exact `1/7` each) | this pack |
| QualityFirst weights | Q=`2`, others=`1` / denominator `8` | this pack |
| CostAware weights | K=`2`, others=`1` / denominator `8` | this pack |
| LatencySensitive weights | L=`2`, others=`1` / denominator `8` | this pack |
| LocalFirst weights | P=`2`, others=`1` / denominator `8` | this pack |
| Claimed task-quality cohorts | **none** — every unsupported cohort is **Unknown** | SPEC-020 §16 |
| Cold-start quality priors / Beta α·β / sample-confidence `k` | **Unknown** (not frozen numerically; no invented quality constants) | SPEC-020 §8 / §16 |
| Rights-cleared frozen corpus | **absent** (`corpus_id` empty) | SPEC-020 §16 / §20 |
| §20 production metric results | all **Unknown** / `performance_claim=false` until a superseding corpus pack | SPEC-020 §20 |
| Clean install / learning-disabled parity | same artifact required | SPEC-020 §16 / §18 fixture 20 |
| Synthetic POC weights as defaults | **forbidden** (mechanics citation only) | #681 handoff; #1294 |
| Hard policy dimensions as calibration knobs | **forbidden** | SPEC-020 §5 / §11 |

Exact rational weights live in the TOML artifact. Decimal approximations (for review only):

| Profile | Q | C | T | R | K | L | P | Sum |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Balanced | 1/7 | 1/7 | 1/7 | 1/7 | 1/7 | 1/7 | 1/7 | 1 |
| QualityFirst | 1/4 | 1/8 | 1/8 | 1/8 | 1/8 | 1/8 | 1/8 | 1 |
| CostAware | 1/8 | 1/8 | 1/8 | 1/8 | 1/4 | 1/8 | 1/8 | 1 |
| LatencySensitive | 1/8 | 1/8 | 1/8 | 1/8 | 1/8 | 1/4 | 1/8 | 1 |
| LocalFirst | 1/8 | 1/8 | 1/8 | 1/8 | 1/8 | 1/8 | 1/4 | 1 |

Changing these values after #1275 starts implementing ranking requires a dated superseding evidence pack with methodology and independent review — not opportunistic edits inside ranking PRs.

## What this pack claims and does not claim

### Claims

- A versioned BaselineCalibrationArtifact exists under `docs/evidence/` with an integrity hash suitable for cold-start and local-learning-disabled installs (SPEC-020 §16).
- Five baseline profiles have frozen soft weights that sum to 1 over Q/C/T/R/K/L/P and are derived by a published policy-identity rule, not by copying the synthetic reference-POC table.
- All task-quality / route-quality cohorts are explicitly **Unknown**; no provider/model acceptance rates are invented.
- A reproducible SPEC-020 §20 measurement procedure is recorded for CI/acceptance to reference when a rights-cleared corpus exists.
- Honest provenance from commercial/OSS Agent Router R&D is cited with explicit non-claims.

### Non-claims

- This pack does **not** implement SPEC-020 V1 ranking, compose ranking `selection_kind`, or close #1275 / #681.
- The SPEC-020 V1 reference POC (28/28 fixtures including §18; SHA-256 `920a4cc064b8bc9c234328938ad5d33575b7c58a8ecc1bfc302e3ca08fd3cb85`) proves **mechanics only**. Its weights, priors, costs, failure rates and baseline are synthetic and remain **non-defaults**.
- Context/memory calibration (#1244 / [`m005-context-memory-production-calibration.md`](m005-context-memory-production-calibration.md)) still does **not** freeze ranking weights; this pack is the ranking baseline authority.
- Commercial Experiment 1 (#63) disposition **D — invalid/blocked** (no treatment ran). This pack claims no treatment winner.
- Commercial LEAP / Stage B / paired-corpus work under #73 is **not** copied as OSS production defaults.
- §20 metric cells marked Unknown are not “approximately good” quality results. They are absent evidence.
- Capability/static provider metadata may establish eligibility facts; they are **not** substitutes for task-quality evidence (SPEC-020 §16).
- No terminal hot-path latency/RSS claim is made.

## Provenance (honest)

| Source | What it supports here | Honest limit |
| --- | --- | --- |
| Commercial closeout `SEYAL-AGENT-ROUTER-RD-CLOSEOUT-2026-09-28.md` | Contributor handoff: implement accepted contracts; do not copy synthetic POC weights/priors/rates | Not weight authority |
| Commercial synthesis `SEYAL-AI-AGENT-ROUTING-SOLUTION-RD-001.md` §7.6.8 | Mechanics POC 28/28; SHA-256 `920a4cc…` | Synthetic baseline; uncalibrated |
| OSS #1244 / `m005-context-memory-production-calibration.md` | Pattern for versioned evidence + measurement procedure; explicitly left V1 weights unfrozen | Not ranking calibration |
| OSS SPEC-020 Accepted (#838 / #55) | Behavioral authority for artifact shape, profiles, Unknown cohorts, §20 gate | Does not invent corpus numbers |
| Commercial #63 / #73 | Disposition D; LEAP not required for this freeze | No OSS production defaults |

Primary OSS handoff narrative remains the #681 Agent Router R&D comment chain; this pack only freezes the ranking baseline inputs that #1244 deliberately left open.

## Weight derivation (conservative freeze)

No rights-cleared frozen corpus with provider evaluations is available for OSS production defaults. #1294 therefore uses the Issue-allowed path: an **owner-accepted conservative weight freeze with documented non-POC derivation**.

1. Factor set is exactly SPEC-020 §7: `Q, C, T, R, K, L, P`.
2. **Balanced** starts at equal soft preference: numerator `1` for every factor, denominator `7`.
3. Named profiles multiply only the profile’s primary factor by `emphasis_multiplier = 2` and renormalize:
   - QualityFirst → Q
   - CostAware → K
   - LatencySensitive → L
   - LocalFirst → P
4. Hard constraints/floors are never weighted (SPEC-020 §5 / §11).
5. No selected routing examples were used to adjust numerators (SPEC-020 §20: do not hand-tune weights to selected examples).
6. Future corpus calibration may supersede this pack only by landing a new versioned artifact + evidence document with integrity hash; it must not silently rewrite v1.

This is policy-identity emphasis, not a claim that Balanced equal weights maximize acceptance or minimize cost on any real provider cohort.

## Cohorts and shrinkage

| Concern | v1 freeze |
| --- | --- |
| Claimed quality cohorts | **[]** (empty) |
| Unsupported cohorts | **Unknown** — do not invent quality constants |
| Multimodal / short context-dependent held-out claims | **not claimed** → Unknown |
| Beta prior α/β | **Unknown** until a superseding pack claims a rights-cleared cohort with evidence |
| `sample_confidence_k` | **Unknown** until a superseding pack claims a cohort with evidence |
| Numeric quality prior | **Unknown** — not a provider acceptance rate |
| Unknown cost | never treated as zero |
| Incompatible evidence generations | must not mix |

Cold-start and learning-disabled installs must load this artifact (or a superseding integrity-verified successor). Sparse local evidence, once ranking exists, shrinks toward this baseline only after applicability/freshness/confidence checks (SPEC-020 §16).

## Integrity verification

```sh
# From a clean checkout of seyal:
shasum -a 256 docs/evidence/m005-spec020-baseline-calibration-artifact-v1.toml
# Expected:
# 9d31ee776b06d288d914b2c55a8d2354459fa46894d237592ffc4508041b7ecf
```

Production ranking (#1275) must bind cold-start / learning-disabled behavior to this `artifact_id` + hash (or a recorded successor). A mismatched hash is a fail-closed configuration error, not a silent fallback to synthetic POC constants.

## SPEC-020 §20 measurement procedure

CI/acceptance and #1275 implementation must reference this procedure rather than ad-hoc local thresholds. Absolute production quality gates remain **Unknown** until a rights-cleared corpus pack supersedes the §20 results table.

### Environment metadata (required on every retained run)

| Field | Example source |
| --- | --- |
| `git rev-parse HEAD` | exact production or evidence head |
| Baseline artifact id + SHA-256 | this pack / successor |
| OS / kernel / arch | `uname -a` |
| `rustc` / `cargo` | `rustc -Vv`, toolchain file |
| Build profile | `release` for any performance-adjacent claim |
| Host class | `developer` / `ci` / `controlled-release` |
| Corpus id / rights clearance | required before any non-Unknown §20 result |
| Provider/model/harness generations | CapabilitySnapshot / RouteOffering generations |
| `performance_claim` | `true` only after owning Issue accepts the number as a gate |

### Frozen-corpus protocol

1. Select a rights-cleared, reproducible WorkItem corpus with explicit exclusions and cohort coverage notes (SPEC-020 §16).
2. Split calibration vs held-out evaluation; never hand-tune weights on held-out or selected anecdotes (SPEC-020 §20).
3. For each profile under test, run identical frozen inputs through the production ranking envelope (once #1275 exists): hard constraints → eligible offerings → ranking → immutable `RoutingDecision` → dispatch/fallback.
4. Record the metrics below with honest denominators (SPEC-019): rejected, unresolved, abandoned, and failed cohort work remains in denominators where the metric definition requires it. Missing cost is Unknown, not zero.
5. Publish results in a superseding `docs/evidence/` pack that updates the artifact hash/version when weights change.

### Metrics — how to measure

| Metric | Procedure sketch | Pass rule against this pack |
| --- | --- | --- |
| Acceptance rate | Accepted WorkItems / eligible WorkItems in corpus; declare missing-data treatment | Until corpus exists: result **Unknown**. Later packs set numeric gates with review |
| First-attempt acceptance | Accepted on Attempt 1 / eligible WorkItems | Unknown until corpus |
| Attempts per accepted WorkItem | Attempt count / accepted WorkItems (failed attempts remain attributed) | Unknown until corpus |
| Total AI cost per accepted WorkItem | Sum of attributed AI cost / accepted WorkItems; Unknown cost propagates | Unknown until corpus; never coerce Unknown→0 |
| Elapsed time | WorkItem-level elapsed distribution (p50/p95); attention wait ≠ active labor | Unknown until corpus |
| Fallback/retry rate | Material reroutes + bounded retries / Attempts; classify per SPEC-020 §15 | Unknown until corpus |
| No-route correctness | Manual/oracle labels for cases that must NoRoute vs must route; false route under hard deny is FAIL | Unknown until corpus; hard-policy fixtures remain mandatory for #1275 |
| Confidence calibration | Reliability of stated confidence vs observed acceptance by bucket | Unknown until corpus |
| Explanation stability | Identical frozen inputs → identical canonical explanation fields across N≥30 replays and candidate-order permutations where required by §18 | Mechanics may be tested with fixtures; corpus quality stability remains Unknown |

### §20 results (v1 freeze)

| Metric | Result | `performance_claim` |
| --- | --- | --- |
| Acceptance rate | Unknown | false |
| First-attempt acceptance | Unknown | false |
| Attempts per accepted WorkItem | Unknown | false |
| Total AI cost per accepted WorkItem | Unknown | false |
| Elapsed time | Unknown | false |
| Fallback/retry rate | Unknown | false |
| No-route correctness | Unknown | false |
| Confidence calibration | Unknown | false |
| Explanation stability | Unknown (no production ranking yet) | false |

These Unknown results are intentional. The pack freezes the **procedure** and the **conservative weight/cohort baseline** so ranking implementation has a non-synthetic default surface. It does not pretend a corpus was measured.

## Change control

1. Profile weight numerators/denominators and `artifact_version` in the TOML are normative for #1275 cold-start once that Issue is Ready and implementing.
2. A PR that needs different weights must land a superseding dated calibration (new artifact version + hash) with methodology — not bury constants inside ranking code.
3. Weights may be tightened or corpus-retuned only with reviewable evidence; silent replacement by synthetic POC tables is forbidden.
4. Filling §20 Unknown cells requires a rights-cleared corpus and an evidence update; do not invent provider quality to make the table look complete.
5. Hard policy/security/capability constraints remain non-exploration dimensions forever.

## Relationship to #1275 / #681

Landing this pack removes the **BaselineCalibrationArtifact / §20 evidence** dependency cited by #1275’s Ready re-check.

It does **not** by itself make #1275 Ready. #1275 must still re-run `development-readiness` (composition remains one routing envelope; pin/allow/deny stay hard; no second router) and only then move to Ready for ranking implementation. #681 remains open until its package children, including ranking composition, are Done.
