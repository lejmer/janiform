# Open Closed Gap Summary And Diff Report Contract

This guide defines the machine-readable summary artifact and behavior-diff report
emitted by the open and closed gap leakage regression harness.

## Report scope

The contract applies to:

- Rust entrypoint: `janiform_core::validation::run_open_closed_gap_leakage_regression_harness`
- Python entrypoint: `janiform.validation.run_open_closed_gap_leakage_regression_harness`

## Top-level report schema

- `schema_version`: `validation.open_closed_gap_leakage_regression_harness.report.v1`
- `determinism_policy`: `stable_input_order_and_event_timestamp_then_generation_order.v1`
- `output_schema_metadata`: machine-readable required field lists and policy identifiers
- `tolerance_config`:
  - `direct_gap_absolute_tolerance`
- `summary`: existing leakage gate summary
- `summary_artifact`: per-scenario contribution artifact
- `behavior_diff_report`: per-scenario baseline diff report
- `case_reports`: leakage case reports with embedded contribution and diff sections

## Output schema metadata

`output_schema_metadata` includes:

- `schema_version`: `validation.open_closed_gap_leakage_regression_harness.output_schema_metadata.v1`
- `summary_artifact_schema_version`: `validation.open_closed_gap_behavior.summary_artifact.v1`
- `behavior_diff_report_schema_version`: `validation.open_closed_gap_behavior.diff_report.v1`
- `baseline_expected_output_policy`: `market_state_gated_expected_contribution_baseline.v1`
- `environment_metadata_policy`: `exclude_environment_specific_metadata.v1`
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
- `scenarios[]`, each with:
  - `fixture_id`
  - `exchange_code`
  - `contributions`

Each `contributions` object separates activity and gap components:

- `expected_activity_coupling_j_t_increment_total`
- `observed_activity_coupling_j_t_increment_total`
- `expected_open_window_direct_gap_g_t_increment_total`
- `observed_open_window_direct_gap_g_t_increment_total`
- `expected_closed_window_direct_gap_g_t_increment_total`
- `observed_closed_window_direct_gap_g_t_increment_total`
- `expected_total_direct_gap_g_t_increment`
- `observed_total_direct_gap_g_t_increment`

## Behavior diff report

`behavior_diff_report` highlights regressions against the baseline contribution policy:

- `schema_version`: `validation.open_closed_gap_behavior.diff_report.v1`
- `determinism_policy`: `preserve_input_case_order_and_metric_order.v1`
- `baseline_expected_output_policy`: `market_state_gated_expected_contribution_baseline.v1`
- `summary`:
  - `total_case_count`
  - `regression_case_count`
  - `total_regression_metric_count`
- `scenario_diffs[]`, each with:
  - `fixture_id`
  - `exchange_code`
  - `regression_metric_count`
  - `max_absolute_difference`
  - `metrics[]`

Each `metrics[]` entry contains:

- `metric_id`
- `expected_value`
- `observed_value`
- `absolute_difference`
- `absolute_tolerance`
- `regression_detected`

Current metric ids:

- `activity_coupling_j_t_increment_total`
- `open_window_direct_gap_g_t_increment_total`
- `closed_window_direct_gap_g_t_increment_total`
- `total_direct_gap_g_t_increment`

## Baseline comparison policy

`market_state_gated_expected_contribution_baseline.v1` defines expected values as:

- Activity contribution is the sum of all event severity marks.
- Open-window direct gap contribution is zero.
- Closed-window direct gap contribution is the sum of closed-window coupled gap marks.
- Total direct gap contribution is open plus closed direct gap contribution.

Observed values come from dispatch records, using
`observed_direct_gap_increment_override` when explicitly supplied in regression inputs.
