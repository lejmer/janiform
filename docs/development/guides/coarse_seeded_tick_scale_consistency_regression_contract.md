# Coarse Seeded Tick Scale Consistency Regression Contract

This guide documents the seeded-tick aggregation and coarse-versus-fine regression helpers
implemented in `janiform_core::verification`.

## Scope

The contract covers:

- reconstructing coarse-comparable interval summaries from seeded-tick debug traces
- grouping fine seeded-tick intervals onto a compatible coarse grid
- building a deterministic machine-readable regression report over paired coarse and fine paths
- separating exact structural checks from approximate and distributional drift checks

This helper sits beside the existing scale-consistency decision and reporting infrastructure.
It does not replace the broader threshold-policy flow. It provides the lower-level regression
coverage needed to keep the public coarse simulation workflow aligned with aggregated seeded-tick
behavior.

## Rust Entry Points

Canonical exported items live in `janiform_core::verification`:

- `aggregate_seeded_tick_run_to_coarse_comparable_path`
- `build_coarse_seeded_tick_consistency_report`
- `SeededTickCoarseComparablePath`
- `SeededTickCoarseComparableIntervalRecord`
- `SeededTickCoarseComparablePathDiagnostics`
- `CoarseSeededTickConsistencyScenarioInput`
- `CoarseSeededTickPairedPathInput`
- `CoarseSeededTickConsistencyThresholds`
- `CoarseSeededTickConsistencyReport`

## Seeded Tick Aggregation

`aggregate_seeded_tick_run_to_coarse_comparable_path` requires:

- a `SeededTickSimulationRunConfig`
- the matching `SeededTickSimulationRunResult`
- a target `coarse_interval_delta`

The seeded-tick run must include full debug traces:

- `debug_trace_enabled = true`
- `sample_every_nth_transition = 1`

The aggregation helper reconstructs per-interval sufficient-statistics inputs by replaying:

- interval-boundary timestamps from the fine factor contract
- Hawkes drift between trace transitions
- endogenous buy and sell counts from trace transition kinds
- open-window and closed-window jump totals from cumulative routed jump diagnostics

The target coarse interval must be an exact integer multiple of the fine interval width.

## Report Semantics

`build_coarse_seeded_tick_consistency_report` consumes scenario inputs that contain paired:

- `CoarseSimulationPath`
- `SeededTickCoarseComparablePath`

The report separates comparisons into:

- `exact`
  - interval count alignment
  - boundary alignment
  - open-jump and closed-gap routing presence alignment
- `approximate`
  - mean cumulative `A+`, `A-`, `N+`, `N-`
  - mean terminal-state `L1` distance
- `distributional`
  - cumulative open-jump Wasserstein distance
  - cumulative closed-gap Wasserstein distance

Drift buckets are explicit:

- `structural_alignment`
- `sufficient_statistics_sampling`
- `direct_jump_generation`
- `state_carryover`

## Schema Identifiers

- comparable path:
  - `validation.seeded_tick.coarse_comparable_path.v1`
- comparable path diagnostics:
  - `validation.seeded_tick.coarse_comparable_path.diagnostics.v1`
- regression report:
  - `validation.coarse_seeded_tick_consistency.report.v1`
- regression output metadata:
  - `validation.coarse_seeded_tick_consistency.output_schema_metadata.v1`
- regression thresholds:
  - `validation.coarse_seeded_tick_consistency.thresholds.v1`

## Intended Use

Use this contract for regression coverage around:

- the hourly reference workflow
- compatible non-hourly interval grids
- drift triage when coarse simulation behavior moves away from aggregated seeded-tick behavior

Keep threshold changes explicit and versioned through the threshold struct carried in each report.
