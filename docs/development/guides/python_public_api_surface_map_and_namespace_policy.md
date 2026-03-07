# Python Public API Surface Map And Namespace Policy

Document version: `python.public_api_surface_map.pre_v0_1.milestone_1`

Status: active for the current pre-v0.1 milestone on March 7, 2026.

## Purpose

This document defines the public Python namespace for Janiform's current pre-v0.1 milestone.
It exists to keep the import surface minimal, task-oriented, and explicit while the simulation,
pricing, risk, and calibration workflows are still evolving.

## Public package root

The package root `janiform` is intentionally minimal.

- Public exports:
  - `__version__`
- Policy:
  - Do not add workflow functions, request types, response types, or helper classes to the root package.
  - Users should import from task-specific modules instead of relying on root-level re-exports.

## Public modules and ownership

### Primary workflow modules

- `janiform.simulation`
  - Ownership: seeded tick simulation workflow
  - Public symbols:
    - `HawkesKernelLoadings`
    - `HawkesKernelParameters`
    - `HawkesExcitationState`
    - `SeededTickEventSide`
    - `SeededTickEventStateSnapshotCapture`
    - `SeededTickEventStateSnapshot`
    - `SeededTickEventRecord`
    - `SeededTickEventTapeMetadata`
    - `SeededTickSimulationRequest`
    - `SeededTickSimulationResponse`
    - `SeededTickSimulationValidationFailure`
    - `SeededTickSimulationValidationError`
    - `load_hourly_factor_contract_for_simulation`
    - `run_seeded_tick_event_simulation`
    - `validate_seeded_tick_simulation_response`
    - `serialize_seeded_tick_simulation_response`
    - `deserialize_seeded_tick_simulation_response`
- `janiform.pricing`
  - Ownership: vanilla option pricing contract, execution input, native runner, and pricing-side hourly factor loading
  - Public symbols:
    - `GammaSeverityMarkConfiguration`
    - `NigSharedPlusShiftConfiguration`
    - `VanillaOptionPricingCalibrationHooks`
    - `VanillaOptionPricingExecutionInput`
    - `VanillaOptionPricingIntervalFactors`
    - `VanillaOptionPricingOptionalOutputSelection`
    - `VanillaOptionPayoffConfiguration`
    - `VanillaOptionPayoffType`
    - `VanillaOptionPricingDiagnostics`
    - `VanillaOptionPricingMarketInputs`
    - `VanillaOptionPricingModelParameters`
    - `VanillaOptionPricingRequest`
    - `VanillaOptionPricingResponse`
    - `VanillaOptionPricingRunDiagnostics`
    - `VanillaOptionPricingRunOutputSchemaMetadata`
    - `VanillaOptionPricingRunResult`
    - `VanillaOptionPricingValidationError`
    - `VanillaOptionPricingValidationFailure`
    - `load_hourly_factor_contract_for_pricing`
    - `run_vanilla_option_pricing`
    - `validate_vanilla_option_pricing_execution_input`
    - `validate_vanilla_option_pricing_request`
    - `validate_vanilla_option_pricing_response`
    - `validate_vanilla_option_pricing_run_result`
    - `serialize_vanilla_option_pricing_execution_input`
    - `serialize_vanilla_option_pricing_request`
    - `serialize_vanilla_option_pricing_response`
    - `serialize_vanilla_option_pricing_run_result`
    - `deserialize_vanilla_option_pricing_execution_input`
    - `deserialize_vanilla_option_pricing_request`
    - `deserialize_vanilla_option_pricing_response`
    - `deserialize_vanilla_option_pricing_run_result`
- `janiform.risk`
  - Ownership: sufficient-statistics risk workflow contract
  - Public symbols:
    - `SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY`
    - `SufficientStatisticsRiskEquationSpecification`
    - `SufficientStatisticsRiskIntervalFactors`
    - `SufficientStatisticsRiskModelInputs`
    - `SufficientStatisticsRiskRunRequest`
    - `SufficientStatisticsIntervalStatistics`
    - `SufficientStatisticsRiskReturnComponents`
    - `SufficientStatisticsRiskPathRecord`
    - `SufficientStatisticsRiskPath`
    - `SufficientStatisticsRiskValidationError`
    - `SufficientStatisticsRiskValidationFailure`
    - `validate_sufficient_statistics_risk_run_request`
    - `validate_sufficient_statistics_risk_path`
    - `serialize_sufficient_statistics_risk_run_request`
    - `serialize_sufficient_statistics_risk_path`
    - `deserialize_sufficient_statistics_risk_run_request`
    - `deserialize_sufficient_statistics_risk_path`
- `janiform.calibration`
  - Ownership: staged calibration request and response contracts
  - Public symbols:
    - `CALIBRATION_RUN_SPEC_SCHEMA_VERSION`
    - `CALIBRATION_STAGED_RUN_REPORT_SCHEMA_VERSION`
    - `CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY`
    - `DEFAULT_PYTHON_IMPORTABLE_API_EXECUTION_MODE`
    - `CalibrationExecutionMode`
    - `CalibrationStage`
    - `CalibrationStageExecutionStatus`
    - `CalibrationHandoffTransport`
    - `CalibrationResumeSource`
    - `CalibrationRunDataInputs`
    - `CalibrationStageSeedOverride`
    - `CalibrationRunSeedSettings`
    - `CalibrationOptimizerSettings`
    - `CalibrationStageSolverSettings`
    - `CalibrationStageHandoff`
    - `CalibrationResumeSettings`
    - `StagedCalibrationRequest`
    - `CalibrationCheckpointMetadata`
    - `CalibrationFailureDetails`
    - `CalibrationStageReport`
    - `StagedCalibrationResponse`
    - `CalibrationValidationFailure`
    - `CalibrationValidationError`
    - `validate_staged_calibration_request`
    - `validate_staged_calibration_response`
    - `serialize_staged_calibration_request`
    - `serialize_staged_calibration_response`
    - `deserialize_staged_calibration_request`
    - `deserialize_staged_calibration_response`

### Supporting public modules

- `janiform.calendar`
  - Ownership: exchange calendar queries used by simulation, pricing, and later calibration
- `janiform.hourly_factors`
  - Ownership: interval-aligned `s_h` and `nu_h` contract materialization shared by simulation and pricing
- `janiform.seasonality`
  - Ownership: deterministic seasonality estimation and composition
- `janiform.validation`
  - Ownership: regression harnesses and validation artifacts
- `janiform.stylized_facts`
  - Ownership: stylized-fact comparison reporting

## Calibration namespace status

`janiform.calibration` is now public for typed request and response contracts.

The module intentionally stops at contract objects and serialization helpers in this milestone.
Top-level calibration orchestrator entrypoints still land under:

- `JAN-190 Workflow Orchestrator API Entry Points`

## Private namespace policy

The following namespaces are private implementation details and may change without warning inside the
pre-v0.1 line:

- any module whose name starts with `_`
- any submodule not listed in this document
- implementation helpers nested under public packages, such as `janiform.validation.harness`

Specific private modules for the current milestone:

- `janiform._native`
- `janiform._native_api`

Private modules must not be re-exported from `janiform` or from public module `__all__` lists.

## Contract evolution rules

- Pre-v0.1 correctness and clarity take priority over compatibility.
- Breaking API and schema changes are allowed when they simplify the model contract or fix semantics.
- New public symbols must be added to a task-specific public module, not to the package root.
- If a symbol is not listed in this document, treat it as private even if Python still allows importing it.
- When a public symbol moves modules, update in-repo call sites, tests, and docs in the same change.

## Review checklist for future API changes

- Is the symbol part of a user task rather than an implementation detail?
- Does it belong in an existing public module instead of the root package?
- Does it avoid leaking private native-adapter or harness internals?
- Are README and export regression tests updated in the same change?
