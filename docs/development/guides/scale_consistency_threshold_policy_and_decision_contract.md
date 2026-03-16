# Scale Consistency Threshold Policy And Decision Contract

This guide documents the model-validation threshold policy and pass-fail decision engine implemented in:

- `janiform::verification::load_scale_consistency_threshold_policy_from_json`
- `janiform::verification::run_scale_consistency_threshold_decision_engine`

## Scope

- Externalize release and calibration governance thresholds for scale-consistency checks
- Apply interval-dependent metric thresholds to coarse-versus-aggregated-fine delta reports
- Preserve threshold provenance in decisions through explicit `threshold_id` and rationale fields
- Reuse Initiative 004 engine benchmark outputs as a prerequisite input instead of redefining engine-level thresholds

## Threshold policy schema

- `schema_version`: `validation.scale_consistency.threshold_policy.v1`
- `policy_id`
- `policy_version`
- `governance_scope_policy`:
  `model_validation_release_and_calibration_governance_thresholds_with_engine_benchmark_prerequisite_inputs.v1`
- `interval_threshold_bands[]`
- `change_log[]`

Per-interval band fields:

- `interval_grid_label`
- `delta_upper_bound`
- `metric_thresholds[]`

Per-metric threshold fields:

- `threshold_id`
- `metric_id`
- `metric_variant` (optional exact override, otherwise the metric-level default applies for all variants)
- `max_absolute_delta`
- `rationale`

Validation rules:

- `delta_upper_bound` values must be strictly increasing across bands
- `threshold_id` values must be unique across the full policy
- `(metric_id, metric_variant)` must be unique within each interval band
- every thresholded metric must exist in the stylized-fact catalog and be supported by the scale-consistency metric engine
- `change_log` must contain the active `policy_version`

## Decision inputs

The decision engine consumes:

- a `ScaleConsistencyMetricDeltaBatchReport`
- a validated `ScaleConsistencyThresholdPolicy`
- a `ScaleConsistencyEngineBenchmarkInput`

`ScaleConsistencyEngineBenchmarkInput` is intentionally compact and can be derived from the Initiative 004 coarse-fine engine regression report with:

- `ScaleConsistencyEngineBenchmarkInput::from_coarse_fine_consistency_report`

## Decision semantics

Per metric:

1. Select the interval threshold band using the scenario `coarse_interval_delta`
2. Resolve the metric threshold by exact `(metric_id, metric_variant)` match or metric-level fallback
3. Fail when the engine benchmark prerequisite is `fail`
4. Fail when the metric delta computation is missing
5. Otherwise fail only when `absolute_delta > max_absolute_delta`
6. Threshold boundaries are inclusive, so `absolute_delta == max_absolute_delta` passes

Per scenario and per batch:

- `fail` if any contained metric fails
- otherwise `pass`

## Decision report schema

- `schema_version`: `validation.scale_consistency.decision_report.v1`
- `ordering_policy`: `preserve_metric_delta_report_scenario_order_and_metric_order.v1`
- `decision_policy`
- `governance_scope_policy`
- `benchmark_input_policy`
- `threshold_policy_id`
- `threshold_policy_version`
- `engine_benchmark_input`
- `status`
- `summary`
- `scenario_reports[]`

Per metric decision fields include:

- metric identifiers and delta fields copied from the metric-delta report
- `status`
- `threshold_context`
- `decision_reason_codes[]`

`threshold_context` includes:

- `policy_id`
- `policy_version`
- `interval_threshold_band_label`
- `threshold_id`
- `threshold_rationale`
- `max_absolute_delta`
- `absolute_delta_exceeds_threshold`
