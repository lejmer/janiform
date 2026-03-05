# Stylized Fact Horizon Comparison Report Schema

This guide documents the canonical horizon-aware stylized-fact comparison contract implemented in:

- `janiform_core::verification::run_stylized_fact_horizon_comparison_engine`

The comparison engine is independent from plotting and UI rendering.

## Scope

- Align empirical and simulated diagnostics by explicit horizon label and metric key
- Avoid implicit interpolation between horizons
- Emit machine-readable delta and decision reports for single runs and batched runs
- Support uncertainty-aware comparison fields and explicit `insufficient_data` outcomes

## Report schema

Top-level report:

- `schema_version`: `validation.stylized_fact.horizon_comparison.report.v1`
- `decision_policy`
- `uncertainty_policy`
- `insufficient_data_status_code`: `insufficient_data`
- `output_schema_metadata`
- `metric_catalog_id`
- `metric_catalog_version`
- `threshold_policy_id`
- `status`: `pass`, `fail`, or `insufficient_data`
- `summary`
- `run_reports[]`

Per-run report:

- `calibration_run_id`
- `empirical_dataset_id`
- `simulated_dataset_id`
- `status`
- `decision_counts`
- `total_horizon_count`
- `total_metric_count`
- `horizons[]`

Per-horizon report:

- `horizon_label`
- `empirical_horizon_duration_nanos`
- `simulated_horizon_duration_nanos`
- `alignment_status`
- `status`
- `decision_counts`
- `metrics[]`

Per-metric report:

- `metric_id`
- `metric_name`
- `metric_variant` (optional)
- `alignment_status`
- `status`
- `empirical`
- `simulated`
- `absolute_delta` (optional)
- `uncertainty_context`
- `threshold_context`
- `horizon_label`
- `decision_reason_codes[]`

## Decision semantics

Decision states:

- `pass`
- `fail`
- `insufficient_data`

Metric-level decision flow:

1. Align by `(horizon_label, metric_id, metric_variant)` without interpolation.
2. If one side is missing or horizon durations mismatch, emit `insufficient_data`.
3. Apply sample-sufficiency checks to each side.
4. If either side is insufficient, emit `insufficient_data`.
5. Otherwise evaluate thresholds on:
   - `absolute_delta`
   - `standardized_delta` when available and configured
6. Emit `fail` when any configured threshold is exceeded, else `pass`.

Run and batch decisions are aggregated from metric and run outcomes:

- Any `fail` dominates
- Else any `insufficient_data` yields `insufficient_data`
- Else `pass`

## Uncertainty context

For matched and sufficient metric pairs, the report includes:

- `empirical_interval_half_width`
- `simulated_interval_half_width`
- `pooled_interval_half_width`
- `standardized_delta`

`standardized_delta` is emitted when both confidence intervals are present and produce a positive pooled half-width.

## Machine-readable metadata

`output_schema_metadata` carries required field lists and canonical vocabularies for:

- report layers
- decision states
- alignment statuses
- dataset metric statuses
- sufficiency reason codes
- decision reason codes
