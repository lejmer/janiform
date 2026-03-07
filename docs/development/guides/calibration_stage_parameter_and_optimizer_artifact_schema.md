# Calibration Stage Parameter And Optimizer Artifact Schema

This guide defines the machine-readable stage-level parameter artifacts emitted for persisted calibration runs.

## Scope

- The artifact applies only to calibration runs with `execution_mode=disk`.
- Persisted artifacts are optional overall.
- CLI-facing calibration workflows default to persisted artifacts ON through `execution_mode=disk`.
- Imported package usage defaults to persisted artifacts OFF through `execution_mode=memory` unless explicitly overridden.
- One artifact is emitted per successful stage output.
- The artifact contains fitted parameters, stage boundary references, seed metadata, solver settings, and optimizer-state metadata when the stage uses the constrained optimizer adapter.
- Derived diagnostics are intentionally excluded from these artifacts.

## Canonical schema

Schema version:

- `calibration.artifact_registry.stage_parameter.v1`

Top-level fields:

- `schema_version`
- `run_id`
- `execution_mode`
- `stage`
- `artifact_reference`
- `schema_metadata`
- `output_schema_metadata`
- `stage_output_schema_version`
- `seed_metadata`
- `solver_settings`
- `stage_parameters`
- `optimizer_state` when the stage uses optimizer selection metadata

`schema_metadata` fields:

- `schema_family`: `calibration.artifact_registry.stage_parameter`
- `schema_generation`: `v1`
- `forward_extension_policy`: `breaking_changes_allowed_with_lockstep_in_repo_updates_pre_v0_1.v1`

`output_schema_metadata` fields:

- `schema_version`: `calibration.artifact_registry.stage_parameter.output_schema_metadata.v1`
- `required_artifact_fields`
- `required_seed_metadata_fields`
- `required_optimizer_state_fields`

`seed_metadata` fields:

- `run_global_seed`
- `stage_seed`
- `stage_seed_source`
- `stage_seed_derivation_policy`

`optimizer_state` fields:

- `schema_version`: `calibration.artifact_registry.stage_parameter.optimizer_state.v1`
- `optimizer_adapter_metadata`
- `input_candidate_count`
- `valid_candidate_count`
- `selected_candidate_raw_objective`
- `selected_candidate_penalized_objective`

`optimizer_adapter_metadata` captures:

- `schema_version`
- `backend_id`
- `random_seed`
- `selection_policy`
- `penalty_policy`
- `bounds_policy`
- `solver_settings`

## Stage payloads

`stage_parameters.payload_kind = seasonality`

- `dataset_id`
- `exchange_code`
- `trade_events_data_reference`
- `estimator_version`
- `timestamp_event_input_schema_version`
- `timestamp_event_preprocessed_schema_version`
- `intraday_estimate_schema_version`
- `day_of_week_estimate_schema_version`
- `day_of_year_estimate_schema_version`
- `composition_schema_version`
- `intraday_minutes_per_bin`
- `intraday_factor_by_bin`
- `day_of_week_factor`
- `day_of_year_factor`
- `normalization_metadata`

`stage_parameters.payload_kind = hawkes`

- `selected_candidate_id`
- `fitted_parameters`
- `seasonality_input_transport`
- `seasonality_dataset_id` for in-memory upstream boundaries
- `seasonality_artifact_reference` for persisted upstream boundaries

`stage_parameters.payload_kind = return_components`

- `selected_candidate_id`
- `fitted_parameters`
- `candidate_generation_policy`
- `objective_weighting_policy`
- `objective_metric_bucket_policy`
- `activity_input_transport`
- `activity_selected_hawkes_candidate_id` for in-memory upstream boundaries
- `activity_artifact_reference` for persisted upstream boundaries

Boundary policy:

- In-memory upstream boundaries keep the upstream selected identifier or dataset identifier and must omit the persisted artifact reference.
- Persisted upstream boundaries keep the upstream artifact reference and must omit the in-memory identifier field.

## Manifest linkage

- Each persisted stage parameter artifact is linked from the run manifest with `artifact_kind = stage_output`.
- The `artifact_reference` recorded in the artifact must match the URI linked from the manifest.
- If a stage declares one unique persisted handoff reference in the run spec, that reference becomes the canonical `artifact_reference`.
- Otherwise the default reference is `artifact://{stage}/{run_id}`.

## Validation rules

Validation rejects:

- unsupported `schema_version`
- `execution_mode` values other than `disk`
- blank `run_id`
- blank `artifact_reference`
- blank `stage_output_schema_version`
- `output_schema_metadata` field lists that do not match the canonical contract
- blank `seed_metadata.stage_seed_derivation_policy`
- invalid `solver_settings`
- `stage` values that do not match the stage payload variant
- empty fitted-factor vectors for seasonality
- missing optimizer-state metadata for Hawkes or return-components artifacts
- unexpected optimizer-state metadata for seasonality
- malformed upstream boundary fields for Hawkes or return-components payloads
- malformed optimizer adapter metadata

## Persistence paths

For a disk-mode run with checkpoint root `{artifact_root}` and `run_id={run_id}`:

- Seasonality stage artifact: `{artifact_root}/{run_id}/seasonality.stage_parameter.artifact.json`
- Hawkes stage artifact: `{artifact_root}/{run_id}/hawkes.stage_parameter.artifact.json`
- Return-components stage artifact: `{artifact_root}/{run_id}/return_components.stage_parameter.artifact.json`

Writes use the same atomic tempfile then rename policy as staged checkpoint persistence.
