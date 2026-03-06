# Scale Consistency Report And Drift Diagnostics Schema

This guide documents the concise report layer implemented in:

- `janiform_core::verification::build_scale_consistency_report`
- `janiform_core::verification::render_scale_consistency_report_summary`

## Scope

- Convert machine-readable threshold decisions into a review-oriented report without losing per-metric delta context
- Classify likely mismatch sources into `returns`, `volatility`, and `event_derived` drift categories
- Keep report payloads environment-agnostic so persisted outputs remain portable across `memory` and `disk` workflows

## Input contract

The report builder consumes a validated `ScaleConsistencyDecisionBatchReport`.

- `schema_version` must be `validation.scale_consistency.decision_report.v1`
- `ordering_policy` must be `preserve_metric_delta_report_scenario_order_and_metric_order.v1`

The report builder preserves the incoming scenario order and each scenario's metric-decision order.

## Report schema

- `schema_version`: `validation.scale_consistency.report.v1`
- `ordering_policy`: `preserve_decision_report_scenario_order_metric_order_and_canonical_drift_category_order.v1`
- `classification_policy`:
  `group_failed_metric_mismatches_by_returns_volatility_and_event_derived_categories_and_rank_primary_sources_by_failed_metric_count_then_max_absolute_delta_then_category.v1`
- `output_schema_metadata`
- `decision_policy`
- `governance_scope_policy`
- `benchmark_input_policy`
- `threshold_policy_id`
- `threshold_policy_version`
- `engine_benchmark_input`
- `status`
- `summary`
- `scenario_reports[]`

`output_schema_metadata` includes:

- `schema_version`: `validation.scale_consistency.report.output_schema_metadata.v1`
- `environment_metadata_policy`:
  `exclude_environment_specific_fields_from_scale_consistency_report_payloads.v1`
- required-field inventories for the top-level report, summary, scenario reports, drift diagnostics, drift metric references, metric decisions, and threshold context

## Scenario report fields

Each scenario report includes:

- scenario identifiers and interval context
- pass or fail `status`
- `failed_metric_count`
- `primary_drift_category`
- `non_drift_failure_reason_codes`
- `metric_decisions[]`
- `drift_diagnostics[]`

`metric_decisions[]` are copied from the decision report so per-scenario metric deltas, thresholds, and pass or fail outcomes remain machine-readable in one artifact.

## Drift diagnostic semantics

Drift diagnostics include only failed metrics with direct mismatch evidence:

- `absolute_delta_threshold_exceeded`
- `metric_computation_missing`

Metrics that fail only because the engine benchmark prerequisite failed are not assigned to a drift category. Those reasons are retained in `non_drift_failure_reason_codes`.

Each drift diagnostic provides:

- `drift_category`
- `failed_metric_count`
- `missing_metric_count`
- `max_absolute_delta`
- `metric_references[]`

`metric_references[]` identify the failed metric and threshold involved in that category.

## Primary drift category

When a scenario has multiple drift diagnostics, the primary category is chosen deterministically by:

1. highest `failed_metric_count`
2. highest `max_absolute_delta`
3. canonical category order: `returns`, then `volatility`, then `event_derived`

This keeps repeated reports stable while still surfacing the strongest likely source first.

## Human-readable summary

`render_scale_consistency_report_summary` produces a compact plain-text summary intended for review workflows. It includes:

- report status and aggregate counts
- one line per scenario
- drift-diagnostic lines in canonical category order
- explicit reporting when a failure is uncategorized or when a scenario has no drift diagnostics
