use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::calibration::precheck_structured_hawkes_kernel_parameter_domain;
use crate::hawkes::{
    HawkesKernelParameterValidationReport, HawkesKernelParameters,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy,
};

use super::super::contracts::{
    CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
    CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY, CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
    CALIBRATION_STAGE_SEED_DERIVATION_POLICY, CalibrationExecutionMode,
    CalibrationOptimizerSettings, CalibrationRunSpec, CalibrationStage,
    CalibrationStageHandoffTransport, CalibrationStageSeedSource,
    resolve_calibration_stage_seed_metadata, stage_solver_settings_for_stage,
};
use super::super::objective::{
    CalibrationObjectiveTermDiagnostic, HawkesStageObjectiveTermInput,
    HawkesStageObjectiveTermInputError,
};
use super::super::optimizer_adapter::{
    CalibrationOptimizerAdapterCandidateInput, CalibrationOptimizerAdapterConfiguration,
    CalibrationOptimizerAdapterError, CalibrationOptimizerAdapterRunMetadata,
    CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
    run_constrained_optimizer_adapter,
};
use super::seasonality::{CalibrationStageExecutionStatus, SeasonalityStageNormalizedPayload};
/// Candidate parameterization and objective inputs for Hawkes stage fitting.
#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageCandidateInput {
    pub candidate_id: String,
    pub lambda_bar: f64,
    pub beta: f64,
    pub structured_alpha: HawkesStructuredAlphaParameterization,
    pub structured_rho: HawkesStructuredRhoParameterization,
    pub self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    pub stability_numeric_tolerance: f64,
    pub objective_terms: Vec<HawkesStageObjectiveTermInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageCandidateInputError {
    MissingCandidateId,
    InvalidLambdaBar {
        value: f64,
    },
    InvalidBeta {
        value: f64,
    },
    InvalidStabilityNumericTolerance {
        value: f64,
    },
    MissingObjectiveTerms,
    InvalidObjectiveTerm {
        term_id: String,
        source: HawkesStageObjectiveTermInputError,
    },
    InvalidWeightedObjectiveValue {
        value: f64,
    },
}

impl Display for HawkesStageCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => write!(formatter, "candidate_id must be non-empty"),
            Self::InvalidLambdaBar { value } => write!(
                formatter,
                "candidate lambda_bar must be finite for objective evaluation, found {value}"
            ),
            Self::InvalidBeta { value } => write!(
                formatter,
                "candidate beta must be finite for objective evaluation, found {value}"
            ),
            Self::InvalidStabilityNumericTolerance { value } => write!(
                formatter,
                "candidate stability_numeric_tolerance must be finite, found {value}"
            ),
            Self::MissingObjectiveTerms => {
                write!(
                    formatter,
                    "candidate objective_terms must include at least one weighted term"
                )
            }
            Self::InvalidObjectiveTerm { term_id, source } => {
                write!(formatter, "objective term `{term_id}` is invalid: {source}")
            }
            Self::InvalidWeightedObjectiveValue { value } => write!(
                formatter,
                "weighted objective sum must be finite, found {value}"
            ),
        }
    }
}

impl Error for HawkesStageCandidateInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidObjectiveTerm { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl HawkesStageCandidateInput {
    fn validate(&self) -> Result<(), HawkesStageCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(HawkesStageCandidateInputError::MissingCandidateId);
        }
        if !self.lambda_bar.is_finite() {
            return Err(HawkesStageCandidateInputError::InvalidLambdaBar {
                value: self.lambda_bar,
            });
        }
        if !self.beta.is_finite() {
            return Err(HawkesStageCandidateInputError::InvalidBeta { value: self.beta });
        }
        if !self.stability_numeric_tolerance.is_finite() {
            return Err(
                HawkesStageCandidateInputError::InvalidStabilityNumericTolerance {
                    value: self.stability_numeric_tolerance,
                },
            );
        }
        if self.objective_terms.is_empty() {
            return Err(HawkesStageCandidateInputError::MissingObjectiveTerms);
        }

        for objective_term in &self.objective_terms {
            objective_term.validate().map_err(|source| {
                HawkesStageCandidateInputError::InvalidObjectiveTerm {
                    term_id: objective_term.term_id.clone(),
                    source,
                }
            })?;
        }

        Ok(())
    }
}

/// Upstream seasonality handoff input resolved for Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub enum HawkesStageSeasonalityInput {
    InMemoryOutput {
        payload: SeasonalityStageNormalizedPayload,
    },
    PersistedArtifactReference {
        artifact_reference: String,
    },
}

impl HawkesStageSeasonalityInput {
    fn transport(&self) -> CalibrationStageHandoffTransport {
        match self {
            Self::InMemoryOutput { .. } => CalibrationStageHandoffTransport::InMemoryOutput,
            Self::PersistedArtifactReference { .. } => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        }
    }

    fn artifact_reference(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { .. } => None,
            Self::PersistedArtifactReference { artifact_reference } => Some(artifact_reference),
        }
    }

    fn dataset_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.dataset_id.as_str()),
            Self::PersistedArtifactReference { .. } => None,
        }
    }
}

/// Input payload for standalone or pipeline-integrated Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct HawkesStageExecutionInput {
    pub seasonality_input: HawkesStageSeasonalityInput,
    pub candidate_inputs: Vec<HawkesStageCandidateInput>,
}

impl HawkesStageExecutionInput {
    pub fn from_in_memory_seasonality_output(
        seasonality_payload: SeasonalityStageNormalizedPayload,
        candidate_inputs: Vec<HawkesStageCandidateInput>,
    ) -> Self {
        Self {
            seasonality_input: HawkesStageSeasonalityInput::InMemoryOutput {
                payload: seasonality_payload,
            },
            candidate_inputs,
        }
    }

    pub fn from_persisted_seasonality_artifact_reference(
        artifact_reference: impl Into<String>,
        candidate_inputs: Vec<HawkesStageCandidateInput>,
    ) -> Self {
        Self {
            seasonality_input: HawkesStageSeasonalityInput::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
            },
            candidate_inputs,
        }
    }
}

pub type HawkesStageObjectiveTermDiagnostic = CalibrationObjectiveTermDiagnostic;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageCandidateDiagnostic {
    pub candidate_id: String,
    pub raw_objective_value: f64,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub constraint_violation_count: usize,
    pub objective_terms: Vec<HawkesStageObjectiveTermDiagnostic>,
    pub validation_report: HawkesKernelParameterValidationReport,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageFittedParameters {
    pub kernel_parameters: HawkesKernelParameters,
    pub structured_alpha: HawkesStructuredAlphaParameterization,
    pub structured_rho: HawkesStructuredRhoParameterization,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    pub stability_numeric_tolerance: f64,
}

/// Stable Hawkes-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: HawkesStageFittedParameters,
    pub candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    pub seasonality_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasonality_dataset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seasonality_artifact_reference: Option<String>,
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
    pub stage_seed_derivation_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
}

/// Structured failure modes for Hawkes-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum HawkesStageExecutionFailure {
    InvalidRunSpec {
        error_message: String,
    },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    SeasonalityInputTransportMismatch {
        expected_transport: CalibrationStageHandoffTransport,
        provided_transport: CalibrationStageHandoffTransport,
    },
    MissingArtifactReferenceInSeasonalityInput,
    InvalidArtifactReferenceInSeasonalityInput {
        artifact_reference: String,
    },
    SeasonalityArtifactReferenceMismatch {
        expected_artifact_reference: String,
        provided_artifact_reference: String,
    },
    MissingHawkesCandidateInputs,
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        error_message: String,
    },
    ConstraintViolationsForAllCandidates {
        candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic>,
    },
    CandidateConstructionFailed {
        candidate_id: String,
        error_message: String,
    },
}

/// Structured Hawkes-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HawkesStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<HawkesStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<HawkesStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HawkesStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<HawkesStageExecutionFailure>,
}

/// Execute the Hawkes stage with structured constraint checks for each candidate.
pub fn run_hawkes_calibration_stage(
    run_spec: &CalibrationRunSpec,
    stage_input: HawkesStageExecutionInput,
) -> HawkesStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Hawkes)
    {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Hawkes)
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if stage_input.candidate_inputs.is_empty() {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::MissingHawkesCandidateInputs,
        );
    }

    let expected_seasonality_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Seasonality
            && stage_handoff.to_stage == CalibrationStage::Hawkes
    });
    let expected_transport = expected_seasonality_handoff
        .map(|stage_handoff| stage_handoff.transport())
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.seasonality_input.transport();

    if provided_transport != expected_transport {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::SeasonalityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::MissingArtifactReferenceInSeasonalityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidArtifactReferenceInSeasonalityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_seasonality_handoff
            && let Some(expected_artifact_reference) = expected_stage_handoff.artifact_reference()
            && expected_artifact_reference != provided_artifact_reference
        {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::SeasonalityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.to_string(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let resolved_stage_seed =
        resolve_calibration_stage_seed_metadata(run_spec, CalibrationStage::Hawkes);
    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        resolved_stage_seed.stage_seed,
        stage_solver_settings.clone(),
        hawkes_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut seen_candidate_ids = BTreeSet::new();
    let mut validation_reports = Vec::with_capacity(stage_input.candidate_inputs.len());
    let mut adapter_candidate_inputs = Vec::with_capacity(stage_input.candidate_inputs.len());

    for candidate_input in &stage_input.candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }

        let validation_report = precheck_structured_hawkes_kernel_parameter_domain(
            candidate_input.lambda_bar,
            candidate_input.beta,
            candidate_input.structured_alpha,
            candidate_input.structured_rho,
            candidate_input.self_dominance_policy,
            candidate_input.stability_numeric_tolerance,
        );

        let external_constraint_violation_count = validation_report.summary.failed_check_count;
        validation_reports.push(validation_report);
        adapter_candidate_inputs.push(CalibrationOptimizerAdapterCandidateInput {
            candidate_id: candidate_input.candidate_id.clone(),
            objective_terms: candidate_input.objective_terms.clone(),
            parameter_values: vec![
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "lambda_bar".to_string(),
                    value: candidate_input.lambda_bar,
                },
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "beta".to_string(),
                    value: candidate_input.beta,
                },
                CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "stability_numeric_tolerance".to_string(),
                    value: candidate_input.stability_numeric_tolerance,
                },
            ],
            external_constraint_violation_count,
        });
    }

    let optimizer_adapter_output = match run_constrained_optimizer_adapter(
        &optimizer_adapter_configuration,
        &adapter_candidate_inputs,
    ) {
        Ok(output) => output,

        Err(CalibrationOptimizerAdapterError::InvalidCandidateInput {
            candidate_id,
            source,
        }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::DuplicateCandidateId { candidate_id },
            );
        }

        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut candidate_diagnostics: Vec<HawkesStageCandidateDiagnostic> =
        Vec::with_capacity(stage_input.candidate_inputs.len());
    for ((candidate_input, validation_report), optimizer_diagnostic) in stage_input
        .candidate_inputs
        .iter()
        .zip(validation_reports.into_iter())
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
    {
        candidate_diagnostics.push(HawkesStageCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: optimizer_diagnostic.raw_objective_value,
            constraint_penalty: optimizer_diagnostic.constraint_penalty,
            penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
            constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
            objective_terms: optimizer_diagnostic
                .objective_composition
                .objective_terms
                .clone(),
            validation_report,
        });
    }

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = stage_input
        .candidate_inputs
        .iter()
        .position(|candidate| candidate.candidate_id == *selected_candidate_id)
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message:
                    "selected candidate id was not found in stage_input.candidate_inputs"
                        .to_string(),
            },
        );
    };

    let selected_candidate_input = &stage_input.candidate_inputs[selected_candidate_position];
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];

    let Some(derived_alpha) = selected_candidate_diagnostic
        .validation_report
        .derived_alpha
    else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived alpha"
                        .to_string(),
            },
        );
    };

    let Some(derived_rho) = selected_candidate_diagnostic.validation_report.derived_rho else {
        return build_hawkes_stage_failure_report(
            run_spec,
            HawkesStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "selected candidate passed validation but did not provide derived rho"
                        .to_string(),
            },
        );
    };

    let kernel_parameters = match HawkesKernelParameters::try_new_with_stability_tolerance(
        selected_candidate_input.lambda_bar,
        selected_candidate_input.beta,
        derived_alpha,
        derived_rho,
        selected_candidate_input.stability_numeric_tolerance,
    ) {
        Ok(kernel_parameters) => kernel_parameters,

        Err(source) => {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let valid_candidate_count = optimizer_adapter_output
        .candidate_diagnostics
        .iter()
        .filter(|candidate_diagnostic| candidate_diagnostic.total_constraint_violation_count == 0)
        .count();

    let selected_candidate_raw_objective = selected_candidate_diagnostic.raw_objective_value;
    let selected_candidate_penalized_objective =
        selected_candidate_diagnostic.penalized_objective_value;

    let fitted_stage_payload = HawkesStageFittedPayload {
        schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: HawkesStageFittedParameters {
            kernel_parameters,
            structured_alpha: selected_candidate_input.structured_alpha,
            structured_rho: selected_candidate_input.structured_rho,
            self_dominance_policy: selected_candidate_input.self_dominance_policy,
            stability_numeric_tolerance: selected_candidate_input.stability_numeric_tolerance,
        },
        candidate_diagnostics,
        seasonality_input_transport: stage_input.seasonality_input.transport(),
        seasonality_dataset_id: stage_input
            .seasonality_input
            .dataset_id()
            .map(ToOwned::to_owned),
        seasonality_artifact_reference: stage_input
            .seasonality_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: build_hawkes_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(HawkesStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION,
            run_global_seed: resolved_stage_seed.run_global_seed,
            stage_seed: resolved_stage_seed.stage_seed,
            stage_seed_source: resolved_stage_seed.stage_seed_source,
            stage_seed_derivation_policy: CALIBRATION_STAGE_SEED_DERIVATION_POLICY,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: stage_input.candidate_inputs.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
        }),
        failure: None,
    }
}

fn hawkes_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![
        CalibrationOptimizerParameterBound {
            parameter_name: "lambda_bar".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "beta".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
        CalibrationOptimizerParameterBound {
            parameter_name: "stability_numeric_tolerance".to_string(),
            lower_inclusive: 0.0,
            upper_inclusive: f64::MAX,
        },
    ]
}

fn build_hawkes_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<HawkesStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Hawkes)
        .map(|stage_handoff| match stage_handoff.transport() {
            CalibrationStageHandoffTransport::InMemoryOutput => HawkesStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport(),
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                HawkesStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport(),
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference().map(str::to_string),
                }
            }
        })
        .collect()
}

fn build_hawkes_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: HawkesStageExecutionFailure,
) -> HawkesStageExecutionReport {
    HawkesStageExecutionReport {
        schema_version: CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Hawkes,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}
