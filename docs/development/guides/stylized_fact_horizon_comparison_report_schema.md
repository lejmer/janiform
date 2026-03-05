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

## Report bundle generation

The canonical report bundle generator is implemented in:

- `janiform_core::verification::run_stylized_fact_horizon_comparison_report_generator`

It composes one comparison report with deterministic table and plot-ready payloads and optional
disk-mode artifact export.

### Bundle schema

- `schema_version`: `validation.stylized_fact.horizon_comparison.report_bundle.v1`
- `determinism_policy`: `stable_run_horizon_metric_order_with_metric_catalog_classification.v1`
- `comparison_report`
- `table_summary`
- `plot_payload`
- `artifact_export` (`null` in memory mode and populated when exported in disk mode)

### Table summary schema

- `schema_version`: `validation.stylized_fact.horizon_comparison.table_summary.v1`
- deterministic `rows[]` ordered by `(calibration_run_id, horizon_label, metric_id, metric_variant)`
- each row carries:
  - metric identifiers and metric-class labels (`primary_robust`, `secondary_moment`)
  - uncertainty fields (`absolute_delta`, `standardized_delta`, confidence intervals)
  - sample coverage and minimum-sufficiency fields
  - explicit `insufficient_data` reason metadata and decision reason codes

### Plot payload schema

- `schema_version`: `validation.stylized_fact.horizon_comparison.plot_payload.v1`
- renderer-agnostic `points[]` preserving:
  - metric identifiers and metric-class labels
  - uncertainty intervals and deltas
  - sample coverage fields
  - explicit insufficient-data reason metadata

### Artifact export policy

- Export is optional and controlled by `generation_options.artifact_export`.
- Export is allowed only when `generation_options.execution_mode = disk`.
- File names are deterministic from configured `file_stem`:
  - `{file_stem}.report_bundle.json`
  - `{file_stem}.table_summary.json`
  - `{file_stem}.plot_payload.json`
- Export directory is fully caller-configured via `export_directory` with no hardcoded paths.
