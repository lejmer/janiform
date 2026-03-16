# Open Closed Gap Summary And Diff Report Contract

This guide defines the machine-readable summary artifact and behavior-diff report
emitted by the open and closed gap leakage regression harness.

## Report scope

The contract applies to:

- Rust entrypoint: `janiform::verification::run_open_closed_gap_leakage_regression_harness`
- Python entrypoint: `janiform.validation.run_open_closed_gap_leakage_regression_harness`

## Top-level report schema

- `schema_version`: `validation.open_closed_gap_leakage_regression_harness.report.v1`
- `determinism_policy`: `stable_input_order_and_event_timestamp_then_generation_order.v1`
- `output_schema_metadata`: machine-readable required field lists and policy identifiers
- `tolerance_config`:
  - `component_absolute_tolerance`
- `summary`: semantic mismatch summary with scenario-class coverage
- `summary_artifact`: per-scenario contribution artifact
- `behavior_diff_report`: per-scenario baseline diff report
- `case_reports`: scenario reports with semantic-mismatch diagnostics, contribution summary, and diff sections

## Output schema metadata

`output_schema_metadata` includes:

- `schema_version`: `validation.open_closed_gap_leakage_regression_harness.output_schema_metadata.v1`
- `summary_artifact_schema_version`: `validation.open_closed_gap_behavior.summary_artifact.v1`
- `behavior_diff_report_schema_version`: `validation.open_closed_gap_behavior.diff_report.v1`
- `baseline_expected_output_policy`: `market_state_gated_expected_contribution_baseline.v1`
- `environment_metadata_policy`: `exclude_environment_specific_metadata.v1`
- `canonical_component_vocabulary`: `trading_return`, `open_jump`, `closed_gap`, `activity_coupling`
- `canonical_semantic_bucket_vocabulary`: `trading_return`, `open_jump`, `closed_gap`, `activity_coupling`
- `required_case_report_fields`
- `required_summary_artifact_fields`
- `required_behavior_diff_report_fields`
- `required_behavior_diff_metric_fields`

The metadata explicitly avoids host-specific values such as filesystem paths, usernames,
or machine identifiers.

## Summary artifact

`summary_artifact` is deterministic and scenario-ordered:

- `schema_version`: `validation.open_closed_gap_behavior.summary_artifact.v1`
- `determinism_policy`: `preserve_input_case_order.v1`
- `total_case_count`
- `total_open_window_event_count`
- `total_closed_window_event_count`
- `scenario_class_coverage`
- `scenarios[]`, each with:
  - `fixture_id`
  - `exchange_code`
  - `scenario_class`
  - `deterministic_replay_metadata`
  - `issue_traceability`
  - `contributions`

Each `contributions` object separates canonical components:

- `expected_activity_coupling_total`
- `observed_activity_coupling_total`
- `expected_trading_return_total`
- `observed_trading_return_total`
- `expected_open_jump_total`
- `observed_open_jump_total`
- `expected_closed_gap_total`
- `observed_closed_gap_total`

## Behavior diff report

`behavior_diff_report` highlights regressions against the baseline contribution policy:

- `schema_version`: `validation.open_closed_gap_behavior.diff_report.v1`
- `determinism_policy`: `preserve_input_case_order_and_metric_order.v1`
- `baseline_expected_output_policy`: `market_state_gated_expected_contribution_baseline.v1`
- `summary`:
  - `total_case_count`
  - `regression_case_count`
  - `total_regression_metric_count`
  - `trading_return_regression_metric_count`
  - `open_jump_regression_metric_count`
  - `closed_gap_regression_metric_count`
  - `activity_coupling_regression_metric_count`
- `scenario_diffs[]`, each with:
  - `fixture_id`
  - `exchange_code`
  - `regression_metric_count`
  - `max_absolute_difference`
  - `metrics[]`

Each `metrics[]` entry contains:

- `metric_id`
- `semantic_bucket`
- `expected_value`
- `observed_value`
- `absolute_difference`
- `absolute_tolerance`
- `regression_detected`

Current metric ids:

- `trading_return_increment_total`
- `activity_coupling_j_t_increment_total`
- `open_jump_m_t_increment_total`
- `closed_gap_g_t_increment_total`

## Baseline comparison policy

`market_state_gated_expected_contribution_baseline.v1` defines expected values as:

- Activity contribution is the sum of all event severity marks.
- Trading return contribution is zero.
- Open-jump contribution is the sum of open-window coupled return jump marks.
- Closed-gap contribution is the sum of closed-window coupled return jump marks.

Observed values come from dispatch records, using per-component override fields when explicitly
supplied in regression inputs.
