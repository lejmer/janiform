# Calibration Optimizer Adapter And Objective Composition Schema

This guide documents the machine-readable contracts used by staged calibration for weighted objective composition and constrained candidate selection.

## Scope

- Module: `crates/core/src/calibration.rs`
- Consumer stages:
  - Hawkes stage (`run_hawkes_calibration_stage`)
  - Return-components stage (`run_return_components_calibration_stage`)

## Objective Composition Contract

Schema constant:

- `CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION`
  - Value: `calibration.optimizer.objective_composition.v1`

Input type:

- `CalibrationObjectiveTermInput`
  - `term_id: String`
  - `weight: f64` (finite and non-negative)
  - `observed_score: f64` (finite)

Output type:

- `CalibrationObjectiveCompositionReport`
  - `schema_version: &'static str`
  - `weighted_objective_value: f64`
  - `objective_terms: Vec<CalibrationObjectiveTermDiagnostic>`

Per-term diagnostic type:

- `CalibrationObjectiveTermDiagnostic`
  - `term_id: String`
  - `weight: f64`
  - `observed_score: f64`
  - `weighted_contribution: f64`

Composition API:

- `compose_calibration_weighted_objective(&[CalibrationObjectiveTermInput])`

Failure modes:

- `CalibrationObjectiveCompositionError::MissingObjectiveTerms`
- `CalibrationObjectiveCompositionError::InvalidObjectiveTerm`
- `CalibrationObjectiveCompositionError::InvalidWeightedObjectiveValue`

## Constrained Optimizer Adapter Contract

Run metadata schema constant:

- `CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION`
  - Value: `calibration.optimizer.adapter_run.v1`

Policy constants:

- `CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY`
- `CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY`
- `CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY`

Configuration type:

- `CalibrationOptimizerAdapterConfiguration`
  - `backend_id: String`
  - `random_seed: u64`
  - `solver_settings: CalibrationOptimizerSettings`
  - `parameter_bounds: Vec<CalibrationOptimizerParameterBound>`

Bound type:

- `CalibrationOptimizerParameterBound`
  - `parameter_name: String`
  - `lower_inclusive: f64`
  - `upper_inclusive: f64`

Candidate input type:

- `CalibrationOptimizerAdapterCandidateInput`
  - `candidate_id: String`
  - `objective_terms: Vec<CalibrationObjectiveTermInput>`
  - `parameter_values: Vec<CalibrationOptimizerCandidateParameterValue>`
  - `external_constraint_violation_count: usize`

Adapter run API:

- `run_constrained_optimizer_adapter(&CalibrationOptimizerAdapterConfiguration, &[CalibrationOptimizerAdapterCandidateInput])`

Run output type:

- `CalibrationOptimizerAdapterRunOutput`
  - `metadata: CalibrationOptimizerAdapterRunMetadata`
  - `selected_candidate_id: Option<String>`
  - `selected_candidate_position: Option<usize>`
  - `candidate_diagnostics: Vec<CalibrationOptimizerAdapterCandidateDiagnostic>`

Candidate diagnostic type:

- `CalibrationOptimizerAdapterCandidateDiagnostic`
  - `candidate_id: String`
  - `objective_composition: CalibrationObjectiveCompositionReport`
  - `raw_objective_value: f64`
  - `external_constraint_violation_count: usize`
  - `bound_violation_count: usize`
  - `total_constraint_violation_count: usize`
  - `constraint_penalty: f64`
  - `penalized_objective_value: f64`
  - `bound_violations: Vec<CalibrationOptimizerBoundViolationDiagnostic>`
  - `seeded_tiebreak_key: u64`

## Stage Metadata Integration

Both stage metadata payloads include adapter metadata:

- `HawkesStageExecutionMetadata.optimizer_adapter_metadata`
- `ReturnComponentsStageExecutionMetadata.optimizer_adapter_metadata`

This preserves the exact adapter schema and policy metadata in stage execution reports.
