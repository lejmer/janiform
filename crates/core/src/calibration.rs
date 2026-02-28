//! Calibration-stage orchestration entrypoints and shared adapters.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::calendar::{
    ExchangeCalendarRegistry, TimestampEventInputRecord, TimestampEventSessionFilter,
};
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelLoadings,
    HawkesKernelParameterValidationReport, HawkesKernelParameters, HawkesSeasonalityIntervalError,
    HawkesStructuredAlphaParameterization, HawkesStructuredRhoParameterization,
    HawkesStructuredRhoSelfDominancePolicy, HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;
use crate::news::NewsProcessConfiguration;
use crate::nig::NigSharedPlusShiftConfiguration;
use crate::pricing::{
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingCalibrationHooksError,
};
use crate::risk_neutral::{
    EsscherMartingaleConditionError, EsscherMartingaleConditionInput,
    EsscherMartingaleConditionReport, EsscherTransformError, NigEsscherTransformationInput,
    NigEsscherTransformedExponent, evaluate_esscher_martingale_condition,
    transform_nig_characteristic_exponent_under_esscher_tilt,
};
use crate::seasonality::{
    DayOfWeekSeasonalityEstimatorSettings, DayOfYearSeasonalityEstimatorSettings,
    IntradaySeasonalityEstimatorSettings, ScheduledEventOverlayDefinition,
    SeasonalityDiagnosticsReportOutput, compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality, estimate_day_of_year_seasonality,
    estimate_intraday_seasonality, generate_seasonality_diagnostics_report,
};

/// Resolve interval-aligned hourly factors for the calibration path.
pub fn load_hourly_factor_contract_for_calibration(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}

/// Run pure Hawkes kernel-domain checks before expensive calibration loops.
pub fn precheck_hawkes_kernel_parameter_domain(
    lambda_bar: f64,
    beta: f64,
    alpha: HawkesKernelLoadings,
    rho: HawkesKernelLoadings,
) -> HawkesKernelParameterValidationReport {
    HawkesKernelParameters::validate_domain(lambda_bar, beta, alpha, rho)
}

/// Run structured base-plus-delta checks before calibration loops over structured parameters.
pub fn precheck_structured_hawkes_kernel_parameter_domain(
    lambda_bar: f64,
    beta: f64,
    structured_alpha: HawkesStructuredAlphaParameterization,
    structured_rho: HawkesStructuredRhoParameterization,
    self_dominance_policy: Option<HawkesStructuredRhoSelfDominancePolicy>,
    stability_numeric_tolerance: f64,
) -> HawkesKernelParameterValidationReport {
    HawkesKernelParameters::validate_structured_domain_with_stability_tolerance(
        lambda_bar,
        beta,
        structured_alpha,
        structured_rho,
        self_dominance_policy,
        stability_numeric_tolerance,
    )
}

/// Project state to interval intensities for calibration objective evaluation.
pub fn project_hawkes_intensities_for_calibration_interval(
    state: HawkesExcitationState,
    parameters: HawkesKernelParameters,
    hourly_factor_contract: &HourlyFactorContractOutput,
    interval_index: usize,
) -> Result<HawkesIntensityProjection, HawkesSeasonalityIntervalError> {
    state.project_intensities_for_hourly_interval(
        parameters.lambda_bar,
        hourly_factor_contract,
        interval_index,
    )
}

/// Apply one calibration-path Hawkes state transition using canonical call order.
///
/// Call order is delegated to `HawkesExcitationState::apply_transition_step`:
/// drift, then optional endogenous jump, then optional exogenous jump.
pub fn evolve_hawkes_state_for_calibration_step(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}

/// Evaluate one Esscher-transformed NIG exponent for calibration-side objectives.
pub fn transform_nig_exponent_for_calibration_under_esscher_tilt(
    input: NigEsscherTransformationInput,
) -> Result<NigEsscherTransformedExponent, EsscherTransformError> {
    transform_nig_characteristic_exponent_under_esscher_tilt(input)
}

/// Evaluate one Esscher martingale-condition residual for calibration-side objectives.
pub fn evaluate_calibration_martingale_condition_under_esscher_tilt(
    input: EsscherMartingaleConditionInput,
) -> Result<EsscherMartingaleConditionReport, EsscherMartingaleConditionError> {
    evaluate_esscher_martingale_condition(input)
}

/// Canonical calibration observation distilled from pricing calibration hooks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OptionSurfaceCalibrationObservation {
    pub option_price: f64,
    pub log_moneyness: f64,
    pub forward_moneyness: f64,
    pub maturity_interval: f64,
    pub integration_numeric_quality_is_poor: Option<bool>,
    pub integration_convergence_check_passed: Option<bool>,
    pub integration_truncation_check_passed: Option<bool>,
}

/// Consume one pricing calibration hook payload into the calibration observation shape.
pub fn build_option_surface_calibration_observation_from_pricing_hooks(
    hooks: VanillaOptionPricingCalibrationHooks,
) -> Result<OptionSurfaceCalibrationObservation, OptionSurfaceCalibrationObservationError> {
    hooks.validate().map_err(|source| {
        OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { source }
    })?;

    Ok(OptionSurfaceCalibrationObservation {
        option_price: hooks.option_price,
        log_moneyness: hooks.log_moneyness,
        forward_moneyness: hooks.forward_moneyness,
        maturity_interval: hooks.maturity_interval,
        integration_numeric_quality_is_poor: hooks.integration_numeric_quality_is_poor,
        integration_convergence_check_passed: hooks.integration_convergence_check_passed,
        integration_truncation_check_passed: hooks.integration_truncation_check_passed,
    })
}

/// Structured failure modes for pricing-hook calibration observation conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum OptionSurfaceCalibrationObservationError {
    InvalidPricingCalibrationHooks {
        source: VanillaOptionPricingCalibrationHooksError,
    },
}

impl Display for OptionSurfaceCalibrationObservationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => {
                write!(
                    formatter,
                    "pricing calibration hooks payload is invalid for calibration consumption: {source}"
                )
            }
        }
    }
}

impl Error for OptionSurfaceCalibrationObservationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidPricingCalibrationHooks { source } => Some(source),
        }
    }
}

pub const CALIBRATION_RUN_SPEC_SCHEMA_VERSION: &str = "calibration.orchestration.run_spec.v1";
pub const CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION: &str = "calibration.orchestration.stage_graph.v1";
pub const CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION: &str =
    "calibration.orchestration.run_contract.v1";
pub const CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY: &str =
    "capture_stage_solver_settings_per_run_contract.v1";

/// Caller-surface default policy for execution-mode resolution.
pub const CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY: &str =
    "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1";
pub const CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.seasonality.report.v1";
pub const CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.seasonality.v1";
pub const CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION: &str =
    "seasonality_estimator.v1";
pub const CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.hawkes.report.v1";
pub const CALIBRATION_HAWKES_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.hawkes.v1";
pub const CALIBRATION_HAWKES_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_execution.return_components.report.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_output.return_components.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY: &str =
    "explicit_run_spec_handoffs_mode_aware_transport.v1";
pub const CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY: &str =
    "weighted_sum_of_configured_objective_terms.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION: &str =
    "calibration.orchestration.stage_checkpoint.v1";
pub const CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY: &str =
    "checkpoint_uri_with_run_id_and_stage_token.v1";
pub const CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY: &str = "atomic_tempfile_then_rename.v1";
pub const CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION: &str =
    "calibration.artifact_registry.manifest.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStage {
    Seasonality,
    Hawkes,
    ReturnComponents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationExecutionMode {
    Memory,
    Disk,
}

/// Originating caller surface used for execution-mode defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationCallerSurface {
    Cli,
    PythonImportableApi,
}

/// Resolve the default execution mode for one caller surface.
pub const fn default_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
) -> CalibrationExecutionMode {
    match caller_surface {
        CalibrationCallerSurface::Cli => CalibrationExecutionMode::Disk,
        CalibrationCallerSurface::PythonImportableApi => CalibrationExecutionMode::Memory,
    }
}

/// Resolve execution mode using caller-surface defaults and optional override.
pub const fn resolve_calibration_execution_mode_for_surface(
    caller_surface: CalibrationCallerSurface,
    execution_mode_override: Option<CalibrationExecutionMode>,
) -> CalibrationExecutionMode {
    match execution_mode_override {
        Some(execution_mode) => execution_mode,
        None => default_calibration_execution_mode_for_surface(caller_surface),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunDataInputs {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_surface_data_reference: Option<String>,
}

impl CalibrationRunDataInputs {
    pub fn try_new(
        exchange_code: impl Into<String>,
        trade_events_data_reference: impl Into<String>,
        option_surface_data_reference: Option<String>,
    ) -> Result<Self, CalibrationRunDataInputsError> {
        let data_inputs = Self {
            exchange_code: exchange_code.into(),
            trade_events_data_reference: trade_events_data_reference.into(),
            option_surface_data_reference,
        };
        data_inputs.validate()?;
        Ok(data_inputs)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunDataInputsError> {
        if self.exchange_code.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingExchangeCode);
        }

        if self.trade_events_data_reference.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingTradeEventsDataReference);
        }

        if let Some(option_surface_data_reference) = &self.option_surface_data_reference
            && option_surface_data_reference.trim().is_empty()
        {
            return Err(CalibrationRunDataInputsError::EmptyOptionSurfaceDataReference);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunDataInputsError {
    MissingExchangeCode,
    MissingTradeEventsDataReference,
    EmptyOptionSurfaceDataReference,
}

impl Display for CalibrationRunDataInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeCode => {
                write!(
                    formatter,
                    "data_inputs.exchange_code must be a non-empty string"
                )
            }
            Self::MissingTradeEventsDataReference => write!(
                formatter,
                "data_inputs.trade_events_data_reference must be a non-empty string"
            ),
            Self::EmptyOptionSurfaceDataReference => write!(
                formatter,
                "data_inputs.option_surface_data_reference must be non-empty when provided"
            ),
        }
    }
}

impl Error for CalibrationRunDataInputsError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationStageSeedOverride {
    pub stage: CalibrationStage,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunSeedSettings {
    pub global_seed: u64,
    #[serde(default)]
    pub stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
}

impl CalibrationRunSeedSettings {
    pub fn try_new(
        global_seed: u64,
        stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
    ) -> Result<Self, CalibrationRunSeedSettingsError> {
        let seed_settings = Self {
            global_seed,
            stage_seed_overrides,
        };
        seed_settings.validate()?;
        Ok(seed_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunSeedSettingsError> {
        let mut stage_overrides = BTreeSet::new();
        for stage_seed_override in &self.stage_seed_overrides {
            if !stage_overrides.insert(stage_seed_override.stage) {
                return Err(
                    CalibrationRunSeedSettingsError::DuplicateStageSeedOverride {
                        stage: stage_seed_override.stage,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunSeedSettingsError {
    DuplicateStageSeedOverride { stage: CalibrationStage },
}

impl Display for CalibrationRunSeedSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateStageSeedOverride { stage } => write!(
                formatter,
                "seed_settings.stage_seed_overrides contains duplicate stage `{stage:?}`"
            ),
        }
    }
}

impl Error for CalibrationRunSeedSettingsError {}

/// Stage-to-stage handoff transport for orchestration contracts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageHandoffTransport {
    InMemoryOutput,
    PersistedArtifactReference,
}

/// One directed stage handoff contract entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationStageHandoff {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

impl CalibrationStageHandoff {
    fn validate(
        &self,
        execution_mode: CalibrationExecutionMode,
        selected_stages: &BTreeSet<CalibrationStage>,
    ) -> Result<(), CalibrationStageHandoffError> {
        if self.from_stage == self.to_stage {
            return Err(CalibrationStageHandoffError::SelfHandoff {
                stage: self.from_stage,
            });
        }

        if !selected_stages.contains(&self.from_stage) {
            return Err(CalibrationStageHandoffError::FromStageIsNotSelected {
                stage: self.from_stage,
            });
        }

        if !selected_stages.contains(&self.to_stage) {
            return Err(CalibrationStageHandoffError::ToStageIsNotSelected {
                stage: self.to_stage,
            });
        }

        match self.transport {
            CalibrationStageHandoffTransport::InMemoryOutput => {
                if self.artifact_reference.is_some() {
                    return Err(
                        CalibrationStageHandoffError::UnexpectedArtifactReferenceForInMemoryHandoff {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                }
            }
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                if execution_mode != CalibrationExecutionMode::Disk {
                    return Err(
                        CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                }

                let Some(artifact_reference) = &self.artifact_reference else {
                    return Err(
                        CalibrationStageHandoffError::MissingArtifactReferenceForPersistedHandoff {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                };
                if artifact_reference.trim().is_empty() {
                    return Err(
                        CalibrationStageHandoffError::EmptyArtifactReferenceForPersistedHandoff {
                            from_stage: self.from_stage,
                            to_stage: self.to_stage,
                        },
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageHandoffError {
    SelfHandoff {
        stage: CalibrationStage,
    },
    FromStageIsNotSelected {
        stage: CalibrationStage,
    },
    ToStageIsNotSelected {
        stage: CalibrationStage,
    },
    UnexpectedArtifactReferenceForInMemoryHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    PersistedArtifactHandoffRequiresDiskMode {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    MissingArtifactReferenceForPersistedHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    EmptyArtifactReferenceForPersistedHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
}

impl Display for CalibrationStageHandoffError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfHandoff { stage } => write!(
                formatter,
                "stage_handoffs contains invalid self handoff for stage `{stage:?}`"
            ),
            Self::FromStageIsNotSelected { stage } => write!(
                formatter,
                "stage_handoffs.from_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::ToStageIsNotSelected { stage } => write!(
                formatter,
                "stage_handoffs.to_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::UnexpectedArtifactReferenceForInMemoryHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` uses in_memory_output and must not include artifact_reference"
            ),
            Self::PersistedArtifactHandoffRequiresDiskMode {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` uses persisted_artifact_reference and requires execution_mode=disk"
            ),
            Self::MissingArtifactReferenceForPersistedHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` uses persisted_artifact_reference and must include artifact_reference"
            ),
            Self::EmptyArtifactReferenceForPersistedHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs entry `{from_stage:?}` -> `{to_stage:?}` has empty artifact_reference"
            ),
        }
    }
}

impl Error for CalibrationStageHandoffError {}

/// Resume source contract for calibration orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationResumeSource {
    SameProcess,
    PersistedCheckpointReference,
}

/// Resume configuration for staged calibration orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationResumeSettings {
    pub source: CalibrationResumeSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from_stage: Option<CalibrationStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_reference: Option<String>,
}

impl CalibrationResumeSettings {
    fn validate(
        &self,
        execution_mode: CalibrationExecutionMode,
        selected_stages: &BTreeSet<CalibrationStage>,
    ) -> Result<(), CalibrationResumeSettingsError> {
        if let Some(resume_from_stage) = self.resume_from_stage
            && !selected_stages.contains(&resume_from_stage)
        {
            return Err(CalibrationResumeSettingsError::ResumeFromUnselectedStage {
                stage: resume_from_stage,
            });
        }

        match self.source {
            CalibrationResumeSource::SameProcess => {
                if execution_mode != CalibrationExecutionMode::Memory {
                    return Err(
                        CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode,
                    );
                }

                if self.checkpoint_reference.is_some() {
                    return Err(
                        CalibrationResumeSettingsError::UnexpectedCheckpointReferenceForSameProcessResume,
                    );
                }
            }
            CalibrationResumeSource::PersistedCheckpointReference => {
                if execution_mode != CalibrationExecutionMode::Disk {
                    return Err(
                        CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode,
                    );
                }

                let Some(checkpoint_reference) = &self.checkpoint_reference else {
                    return Err(
                        CalibrationResumeSettingsError::MissingCheckpointReferenceForPersistedResume,
                    );
                };
                if checkpoint_reference.trim().is_empty() {
                    return Err(
                        CalibrationResumeSettingsError::EmptyCheckpointReferenceForPersistedResume,
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationResumeSettingsError {
    ResumeFromUnselectedStage { stage: CalibrationStage },
    SameProcessResumeRequiresMemoryMode,
    PersistedCheckpointResumeRequiresDiskMode,
    UnexpectedCheckpointReferenceForSameProcessResume,
    MissingCheckpointReferenceForPersistedResume,
    EmptyCheckpointReferenceForPersistedResume,
}

impl Display for CalibrationResumeSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ResumeFromUnselectedStage { stage } => write!(
                formatter,
                "resume_settings.resume_from_stage `{stage:?}` must be selected in selected_stages"
            ),
            Self::SameProcessResumeRequiresMemoryMode => write!(
                formatter,
                "resume_settings.source=same_process is only supported when execution_mode=memory"
            ),
            Self::PersistedCheckpointResumeRequiresDiskMode => write!(
                formatter,
                "resume_settings.source=persisted_checkpoint_reference is only supported when execution_mode=disk"
            ),
            Self::UnexpectedCheckpointReferenceForSameProcessResume => write!(
                formatter,
                "resume_settings.source=same_process must not include checkpoint_reference"
            ),
            Self::MissingCheckpointReferenceForPersistedResume => write!(
                formatter,
                "resume_settings.source=persisted_checkpoint_reference must include checkpoint_reference"
            ),
            Self::EmptyCheckpointReferenceForPersistedResume => write!(
                formatter,
                "resume_settings.checkpoint_reference must be non-empty when provided"
            ),
        }
    }
}

impl Error for CalibrationResumeSettingsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerSettings {
    pub solver_name: String,
    pub max_iterations: usize,
    pub absolute_tolerance: f64,
    pub relative_tolerance: f64,
    pub constraint_penalty_weight: f64,
}

impl CalibrationOptimizerSettings {
    pub fn try_new(
        solver_name: impl Into<String>,
        max_iterations: usize,
        absolute_tolerance: f64,
        relative_tolerance: f64,
        constraint_penalty_weight: f64,
    ) -> Result<Self, CalibrationOptimizerSettingsError> {
        let optimizer_settings = Self {
            solver_name: solver_name.into(),
            max_iterations,
            absolute_tolerance,
            relative_tolerance,
            constraint_penalty_weight,
        };
        optimizer_settings.validate()?;
        Ok(optimizer_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationOptimizerSettingsError> {
        if self.solver_name.trim().is_empty() {
            return Err(CalibrationOptimizerSettingsError::MissingSolverName);
        }
        if self.max_iterations == 0 {
            return Err(CalibrationOptimizerSettingsError::InvalidMaxIterations {
                value: self.max_iterations,
            });
        }
        if !self.absolute_tolerance.is_finite() || self.absolute_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidAbsoluteTolerance {
                    value: self.absolute_tolerance,
                },
            );
        }
        if !self.relative_tolerance.is_finite() || self.relative_tolerance <= 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidRelativeTolerance {
                    value: self.relative_tolerance,
                },
            );
        }
        if !self.constraint_penalty_weight.is_finite() || self.constraint_penalty_weight < 0.0 {
            return Err(
                CalibrationOptimizerSettingsError::InvalidConstraintPenaltyWeight {
                    value: self.constraint_penalty_weight,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerSettingsError {
    MissingSolverName,
    InvalidMaxIterations { value: usize },
    InvalidAbsoluteTolerance { value: f64 },
    InvalidRelativeTolerance { value: f64 },
    InvalidConstraintPenaltyWeight { value: f64 },
}

impl Display for CalibrationOptimizerSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSolverName => {
                write!(formatter, "solver_name must be a non-empty string")
            }
            Self::InvalidMaxIterations { value } => write!(
                formatter,
                "max_iterations must be strictly positive, found {value}"
            ),
            Self::InvalidAbsoluteTolerance { value } => write!(
                formatter,
                "absolute_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidRelativeTolerance { value } => write!(
                formatter,
                "relative_tolerance must be finite and strictly positive, found {value}"
            ),
            Self::InvalidConstraintPenaltyWeight { value } => write!(
                formatter,
                "constraint_penalty_weight must be finite and non-negative, found {value}"
            ),
        }
    }
}

impl Error for CalibrationOptimizerSettingsError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStageSolverSettings {
    pub stage: CalibrationStage,
    pub optimizer_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRunSpec {
    pub run_id: String,
    pub data_inputs: CalibrationRunDataInputs,
    pub execution_mode: CalibrationExecutionMode,
    pub selected_stages: Vec<CalibrationStage>,
    pub seed_settings: CalibrationRunSeedSettings,
    pub optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_handoffs: Vec<CalibrationStageHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_settings: Option<CalibrationResumeSettings>,
}

impl CalibrationRunSpec {
    pub fn try_new(
        run_id: impl Into<String>,
        data_inputs: CalibrationRunDataInputs,
        execution_mode: CalibrationExecutionMode,
        selected_stages: Vec<CalibrationStage>,
        seed_settings: CalibrationRunSeedSettings,
        optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
    ) -> Result<Self, CalibrationRunSpecError> {
        Self::try_new_with_mode_extensions(
            run_id,
            data_inputs,
            execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
            Vec::new(),
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    /// Build a run spec with explicit mode-aware extension fields.
    pub fn try_new_with_mode_extensions(
        run_id: impl Into<String>,
        data_inputs: CalibrationRunDataInputs,
        execution_mode: CalibrationExecutionMode,
        selected_stages: Vec<CalibrationStage>,
        seed_settings: CalibrationRunSeedSettings,
        optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
        stage_handoffs: Vec<CalibrationStageHandoff>,
        resume_settings: Option<CalibrationResumeSettings>,
    ) -> Result<Self, CalibrationRunSpecError> {
        let run_spec = Self {
            run_id: run_id.into(),
            data_inputs,
            execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
            stage_handoffs,
            resume_settings,
        };
        run_spec.validate()?;
        Ok(run_spec)
    }

    #[allow(clippy::too_many_arguments)]
    /// Build a run spec with caller-surface execution-mode defaulting.
    pub fn try_new_for_caller_surface(
        run_id: impl Into<String>,
        data_inputs: CalibrationRunDataInputs,
        caller_surface: CalibrationCallerSurface,
        execution_mode_override: Option<CalibrationExecutionMode>,
        selected_stages: Vec<CalibrationStage>,
        seed_settings: CalibrationRunSeedSettings,
        optimizer_settings_by_stage: Vec<CalibrationStageSolverSettings>,
        stage_handoffs: Vec<CalibrationStageHandoff>,
        resume_settings: Option<CalibrationResumeSettings>,
    ) -> Result<Self, CalibrationRunSpecError> {
        let resolved_execution_mode =
            resolve_calibration_execution_mode_for_surface(caller_surface, execution_mode_override);
        Self::try_new_with_mode_extensions(
            run_id,
            data_inputs,
            resolved_execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
            stage_handoffs,
            resume_settings,
        )
    }

    pub fn validate(&self) -> Result<(), CalibrationRunSpecError> {
        if self.run_id.trim().is_empty() {
            return Err(CalibrationRunSpecError::MissingRunId);
        }

        self.data_inputs
            .validate()
            .map_err(|source| CalibrationRunSpecError::InvalidDataInputs { source })?;

        if self.selected_stages.is_empty() {
            return Err(CalibrationRunSpecError::MissingSelectedStages);
        }

        let mut selected_stages = BTreeSet::new();
        for stage in &self.selected_stages {
            if !selected_stages.insert(*stage) {
                return Err(CalibrationRunSpecError::DuplicateSelectedStage { stage: *stage });
            }
        }

        self.seed_settings
            .validate()
            .map_err(|source| CalibrationRunSpecError::InvalidSeedSettings { source })?;

        for stage_seed_override in &self.seed_settings.stage_seed_overrides {
            if !selected_stages.contains(&stage_seed_override.stage) {
                return Err(CalibrationRunSpecError::SeedOverrideForUnselectedStage {
                    stage: stage_seed_override.stage,
                });
            }
        }

        let mut configured_stage_settings = BTreeMap::new();
        for stage_solver_settings in &self.optimizer_settings_by_stage {
            stage_solver_settings
                .optimizer_settings
                .validate()
                .map_err(
                    |source| CalibrationRunSpecError::InvalidStageSolverSettings {
                        stage: stage_solver_settings.stage,
                        source,
                    },
                )?;

            if configured_stage_settings
                .insert(stage_solver_settings.stage, ())
                .is_some()
            {
                return Err(CalibrationRunSpecError::DuplicateStageSolverSettings {
                    stage: stage_solver_settings.stage,
                });
            }
        }

        for selected_stage in &self.selected_stages {
            if !configured_stage_settings.contains_key(selected_stage) {
                return Err(CalibrationRunSpecError::MissingStageSolverSettings {
                    stage: *selected_stage,
                });
            }
        }

        for configured_stage in configured_stage_settings.keys() {
            if !selected_stages.contains(configured_stage) {
                return Err(CalibrationRunSpecError::UnexpectedStageSolverSettings {
                    stage: *configured_stage,
                });
            }
        }

        let mut stage_handoff_pairs = BTreeSet::new();
        for stage_handoff in &self.stage_handoffs {
            stage_handoff
                .validate(self.execution_mode, &selected_stages)
                .map_err(|source| CalibrationRunSpecError::InvalidStageHandoff { source })?;

            if !stage_handoff_pairs.insert((stage_handoff.from_stage, stage_handoff.to_stage)) {
                return Err(CalibrationRunSpecError::DuplicateStageHandoff {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                });
            }
        }

        if let Some(resume_settings) = &self.resume_settings {
            resume_settings
                .validate(self.execution_mode, &selected_stages)
                .map_err(|source| CalibrationRunSpecError::InvalidResumeSettings { source })?;
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_RUN_SPEC_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationRunSpecError {
    MissingRunId,
    InvalidDataInputs {
        source: CalibrationRunDataInputsError,
    },
    MissingSelectedStages,
    DuplicateSelectedStage {
        stage: CalibrationStage,
    },
    InvalidSeedSettings {
        source: CalibrationRunSeedSettingsError,
    },
    SeedOverrideForUnselectedStage {
        stage: CalibrationStage,
    },
    InvalidStageSolverSettings {
        stage: CalibrationStage,
        source: CalibrationOptimizerSettingsError,
    },
    DuplicateStageSolverSettings {
        stage: CalibrationStage,
    },
    MissingStageSolverSettings {
        stage: CalibrationStage,
    },
    UnexpectedStageSolverSettings {
        stage: CalibrationStage,
    },
    InvalidStageHandoff {
        source: CalibrationStageHandoffError,
    },
    DuplicateStageHandoff {
        from_stage: CalibrationStage,
        to_stage: CalibrationStage,
    },
    InvalidResumeSettings {
        source: CalibrationResumeSettingsError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationRunSpecErrorClassification {
    InvalidInput,
    ModeIncompatibleOptions,
}

impl CalibrationRunSpecError {
    /// Return a deterministic error class for caller-side handling.
    pub const fn classification(&self) -> CalibrationRunSpecErrorClassification {
        match self {
            Self::InvalidStageHandoff { source } => match source {
                CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode {
                    ..
                } => CalibrationRunSpecErrorClassification::ModeIncompatibleOptions,
                _ => CalibrationRunSpecErrorClassification::InvalidInput,
            },
            Self::InvalidResumeSettings { source } => match source {
                CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
                | CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode => {
                    CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
                }
                _ => CalibrationRunSpecErrorClassification::InvalidInput,
            },
            _ => CalibrationRunSpecErrorClassification::InvalidInput,
        }
    }
}

impl Display for CalibrationRunSpecError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRunId => write!(formatter, "run_id must be a non-empty string"),
            Self::InvalidDataInputs { source } => {
                write!(formatter, "data_inputs payload is invalid: {source}")
            }
            Self::MissingSelectedStages => write!(
                formatter,
                "selected_stages must include at least one calibration stage"
            ),
            Self::DuplicateSelectedStage { stage } => write!(
                formatter,
                "selected_stages contains duplicate stage `{stage:?}`"
            ),
            Self::InvalidSeedSettings { source } => {
                write!(formatter, "seed_settings payload is invalid: {source}")
            }
            Self::SeedOverrideForUnselectedStage { stage } => write!(
                formatter,
                "seed_settings.stage_seed_overrides contains stage `{stage:?}` that is not selected"
            ),
            Self::InvalidStageSolverSettings { stage, source } => write!(
                formatter,
                "optimizer_settings_by_stage entry for stage `{stage:?}` is invalid: {source}"
            ),
            Self::DuplicateStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage contains duplicate stage `{stage:?}`"
            ),
            Self::MissingStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage must include solver settings for selected stage `{stage:?}`"
            ),
            Self::UnexpectedStageSolverSettings { stage } => write!(
                formatter,
                "optimizer_settings_by_stage contains stage `{stage:?}` that is not selected"
            ),
            Self::InvalidStageHandoff { source } => {
                write!(formatter, "stage_handoffs payload is invalid: {source}")
            }
            Self::DuplicateStageHandoff {
                from_stage,
                to_stage,
            } => write!(
                formatter,
                "stage_handoffs contains duplicate entry `{from_stage:?}` -> `{to_stage:?}`"
            ),
            Self::InvalidResumeSettings { source } => {
                write!(formatter, "resume_settings payload is invalid: {source}")
            }
        }
    }
}

impl Error for CalibrationRunSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDataInputs { source } => Some(source),
            Self::InvalidSeedSettings { source } => Some(source),
            Self::InvalidStageSolverSettings { source, .. } => Some(source),
            Self::InvalidStageHandoff { source } => Some(source),
            Self::InvalidResumeSettings { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CalibrationStageDependency {
    pub stage: CalibrationStage,
    pub depends_on: CalibrationStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CalibrationStageGraph {
    #[serde(default)]
    pub dependencies: Vec<CalibrationStageDependency>,
}

impl CalibrationStageGraph {
    pub const fn new(dependencies: Vec<CalibrationStageDependency>) -> Self {
        Self { dependencies }
    }

    pub fn resolve_execution_order(
        &self,
        selected_stages: &[CalibrationStage],
    ) -> Result<Vec<CalibrationStage>, CalibrationStageGraphError> {
        if selected_stages.is_empty() {
            return Err(CalibrationStageGraphError::MissingSelectedStages);
        }

        let mut stage_positions = BTreeMap::new();
        for (position, stage) in selected_stages.iter().copied().enumerate() {
            if stage_positions.insert(stage, position).is_some() {
                return Err(CalibrationStageGraphError::DuplicateSelectedStage { stage });
            }
        }

        let stage_count = selected_stages.len();
        let mut indegree_by_position = vec![0usize; stage_count];
        let mut adjacency_by_position = vec![Vec::<usize>::new(); stage_count];
        let mut seen_dependencies = BTreeSet::new();

        for dependency in &self.dependencies {
            let Some(&stage_position) = stage_positions.get(&dependency.stage) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "stage is not selected in run_spec.selected_stages",
                });
            };
            let Some(&depends_on_position) = stage_positions.get(&dependency.depends_on) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "depends_on stage is not selected in run_spec.selected_stages",
                });
            };

            if dependency.stage == dependency.depends_on {
                return Err(CalibrationStageGraphError::SelfDependency {
                    stage: dependency.stage,
                });
            }

            if !seen_dependencies.insert(*dependency) {
                return Err(CalibrationStageGraphError::DuplicateDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                });
            }

            indegree_by_position[stage_position] =
                indegree_by_position[stage_position].saturating_add(1);

            adjacency_by_position[depends_on_position].push(stage_position);
        }

        for dependencies in &mut adjacency_by_position {
            dependencies.sort_unstable();
        }

        let mut ready_queue = VecDeque::new();
        for (position, indegree) in indegree_by_position.iter().copied().enumerate() {
            if indegree == 0 {
                ready_queue.push_back(position);
            }
        }

        let mut execution_positions = Vec::with_capacity(stage_count);
        while let Some(current_position) = ready_queue.pop_front() {
            execution_positions.push(current_position);
            for dependent_position in adjacency_by_position[current_position].iter().copied() {
                if indegree_by_position[dependent_position] == 0 {
                    continue;
                }

                indegree_by_position[dependent_position] -= 1;

                if indegree_by_position[dependent_position] == 0 {
                    ready_queue.push_back(dependent_position);
                }
            }
        }

        if execution_positions.len() != stage_count {
            let cyclic_stages = indegree_by_position
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(position, indegree)| {
                    if indegree > 0 {
                        Some(selected_stages[position])
                    } else {
                        None
                    }
                })
                .collect();

            return Err(CalibrationStageGraphError::CyclicDependency {
                stages: cyclic_stages,
            });
        }

        let execution_order = execution_positions
            .iter()
            .copied()
            .map(|position| selected_stages[position])
            .collect();
        Ok(execution_order)
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageGraphError {
    MissingSelectedStages,
    DuplicateSelectedStage {
        stage: CalibrationStage,
    },
    UnsupportedDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
        reason: &'static str,
    },
    SelfDependency {
        stage: CalibrationStage,
    },
    DuplicateDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
    },
    CyclicDependency {
        stages: Vec<CalibrationStage>,
    },
}

impl Display for CalibrationStageGraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSelectedStages => write!(
                formatter,
                "stage graph validation requires at least one selected stage"
            ),
            Self::DuplicateSelectedStage { stage } => write!(
                formatter,
                "run_spec.selected_stages contains duplicate stage `{stage:?}`"
            ),
            Self::UnsupportedDependency {
                stage,
                depends_on,
                reason,
            } => write!(
                formatter,
                "unsupported stage dependency `{stage:?}` -> `{depends_on:?}`: {reason}"
            ),
            Self::SelfDependency { stage } => write!(
                formatter,
                "stage graph contains self dependency for stage `{stage:?}`"
            ),
            Self::DuplicateDependency { stage, depends_on } => write!(
                formatter,
                "stage graph contains duplicate dependency `{stage:?}` -> `{depends_on:?}`"
            ),
            Self::CyclicDependency { stages } => write!(
                formatter,
                "stage graph contains cyclic dependency involving stages: {stages:?}"
            ),
        }
    }
}

impl Error for CalibrationStageGraphError {}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationRunContractError {
    InvalidRunSpec { source: CalibrationRunSpecError },
    InvalidStageGraph { source: CalibrationStageGraphError },
}

impl Display for CalibrationRunContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRunSpec { source } => {
                write!(formatter, "run_spec is invalid: {source}")
            }
            Self::InvalidStageGraph { source } => {
                write!(formatter, "stage_graph is invalid: {source}")
            }
        }
    }
}

impl Error for CalibrationRunContractError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidRunSpec { source } => Some(source),
            Self::InvalidStageGraph { source } => Some(source),
        }
    }
}

pub fn resolve_calibration_stage_execution_order(
    run_spec: &CalibrationRunSpec,
    stage_graph: &CalibrationStageGraph,
) -> Result<Vec<CalibrationStage>, CalibrationRunContractError> {
    run_spec
        .validate()
        .map_err(|source| CalibrationRunContractError::InvalidRunSpec { source })?;

    stage_graph
        .resolve_execution_order(&run_spec.selected_stages)
        .map_err(|source| CalibrationRunContractError::InvalidStageGraph { source })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRunContract {
    pub schema_version: String,
    pub run_spec_schema_version: String,
    pub stage_graph_schema_version: String,
    pub solver_settings_capture_policy: String,
    pub run_spec: CalibrationRunSpec,
    pub stage_graph: CalibrationStageGraph,
    pub execution_stage_order: Vec<CalibrationStage>,
    pub stage_solver_settings: Vec<CalibrationStageSolverSettings>,
}

impl CalibrationRunContract {
    pub fn try_new(
        run_spec: CalibrationRunSpec,
        stage_graph: CalibrationStageGraph,
    ) -> Result<Self, CalibrationRunContractError> {
        let execution_stage_order =
            resolve_calibration_stage_execution_order(&run_spec, &stage_graph)?;

        Ok(Self {
            schema_version: CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION.to_string(),
            run_spec_schema_version: CALIBRATION_RUN_SPEC_SCHEMA_VERSION.to_string(),
            stage_graph_schema_version: CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION.to_string(),
            solver_settings_capture_policy: CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY.to_string(),
            stage_solver_settings: run_spec.optimizer_settings_by_stage.clone(),
            run_spec,
            stage_graph,
            execution_stage_order,
        })
    }
}

/// Resolve the seed used for one stage after applying stage-level overrides.
pub fn resolve_calibration_stage_seed(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> u64 {
    run_spec
        .seed_settings
        .stage_seed_overrides
        .iter()
        .find(|stage_seed_override| stage_seed_override.stage == stage)
        .map(|stage_seed_override| stage_seed_override.seed)
        .unwrap_or(run_spec.seed_settings.global_seed)
}

/// Configurable knobs for deterministic seasonality stage execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeasonalityStageExecutionSettings {
    pub session_filter: TimestampEventSessionFilter,
    pub sparse_hour_event_threshold: usize,
    pub intraday_settings: IntradaySeasonalityEstimatorSettings,
    pub day_of_week_settings: DayOfWeekSeasonalityEstimatorSettings,
    pub day_of_year_settings: DayOfYearSeasonalityEstimatorSettings,
}

impl Default for SeasonalityStageExecutionSettings {
    fn default() -> Self {
        Self {
            session_filter: TimestampEventSessionFilter::OpenOnly,
            sparse_hour_event_threshold: 1,
            intraday_settings: IntradaySeasonalityEstimatorSettings::default(),
            day_of_week_settings: DayOfWeekSeasonalityEstimatorSettings::default(),
            day_of_year_settings: DayOfYearSeasonalityEstimatorSettings::default(),
        }
    }
}

/// Input payload for standalone or pipeline-integrated seasonality-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonalityStageExecutionInput {
    pub timestamp_event_records: Vec<TimestampEventInputRecord>,
    pub scheduled_event_overlays: Vec<ScheduledEventOverlayDefinition>,
    pub settings: SeasonalityStageExecutionSettings,
    pub dataset_id_override: Option<String>,
    pub estimator_version_override: Option<String>,
}

impl SeasonalityStageExecutionInput {
    pub fn from_timestamp_event_records(
        timestamp_event_records: Vec<TimestampEventInputRecord>,
    ) -> Self {
        Self {
            timestamp_event_records,
            scheduled_event_overlays: Vec::new(),
            settings: SeasonalityStageExecutionSettings::default(),
            dataset_id_override: None,
            estimator_version_override: None,
        }
    }
}

/// Success and failure status for stage execution reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageExecutionStatus {
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageNormalizationMetadata {
    pub normalization_policy: String,
    pub fit_window_event_count: usize,
    pub target_mean_factor: f64,
    pub pre_normalization_mean_factor: f64,
    pub normalization_constant: f64,
    pub normalization_log_offset: f64,
    pub post_normalization_mean_factor: f64,
    pub mean_anchor_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageOverlayMetadata {
    pub overlays_enabled: bool,
    pub configured_overlay_count: usize,
    pub enabled_overlay_count: usize,
    pub active_overlay_event_count: usize,
    pub precedence_policy: String,
    pub merge_policy: String,
    pub window_boundary_policy: String,
    pub configured_overlay_ids_in_order: Vec<String>,
    pub applied_overlay_ids_in_order: Vec<String>,
    pub overlay_source_labels_in_order: Vec<String>,
}

/// Stable seasonality-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageNormalizedPayload {
    pub schema_version: &'static str,
    pub dataset_id: String,
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    pub timestamp_event_input_schema_version: String,
    pub timestamp_event_preprocessed_schema_version: String,
    pub intraday_estimate_schema_version: String,
    pub day_of_week_estimate_schema_version: String,
    pub day_of_year_estimate_schema_version: String,
    pub composition_schema_version: String,
    pub source_record_positions: Vec<u64>,
    pub timestamp_utc_nanos: Vec<i64>,
    pub exchange_local_minute_of_day: Vec<u16>,
    pub exchange_local_day_of_week: Vec<u8>,
    pub exchange_local_day_of_year: Vec<u16>,
    pub market_state_codes: Vec<u8>,
    pub intraday_minutes_per_bin: u16,
    pub intraday_factor_by_bin: Vec<f64>,
    pub day_of_week_factor: Vec<f64>,
    pub day_of_year_factor: Vec<f64>,
    pub normalized_combined_log_component_by_event: Vec<f64>,
    pub normalized_combined_factor_by_event: Vec<f64>,
    pub normalization_metadata: SeasonalityStageNormalizationMetadata,
    pub overlay_metadata: SeasonalityStageOverlayMetadata,
    pub diagnostics_report: SeasonalityDiagnosticsReportOutput,
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub stage_seed: u64,
    pub solver_settings: CalibrationOptimizerSettings,
    pub dataset_id: String,
    pub estimator_version: String,
    pub input_record_count: usize,
    pub accepted_record_count: usize,
    pub filtered_record_count: usize,
    pub diagnostics_report_schema_version: String,
    pub diagnostics_snapshot_fingerprint_algorithm: String,
    pub diagnostics_snapshot_fingerprint_hex: String,
    pub normalization_policy: String,
}

/// Structured failure modes for seasonality-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum SeasonalityStageExecutionFailure {
    InvalidRunSpec { error_message: String },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    MissingTimestampEventRecords,
    EmptyDatasetId,
    EmptyEstimatorVersion,
    NoAcceptedTimestampEventsAfterPreprocessing,
    TimestampPreprocessingFailed { error_message: String },
    IntradayEstimationFailed { error_message: String },
    DayOfWeekEstimationFailed { error_message: String },
    DayOfYearEstimationFailed { error_message: String },
    DeterministicCompositionFailed { error_message: String },
    DiagnosticsReportFailed { error_message: String },
}

/// Structured seasonality-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeasonalityStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<SeasonalityStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub normalized_stage_payload: Option<SeasonalityStageNormalizedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SeasonalityStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<SeasonalityStageExecutionFailure>,
}

/// Execute the deterministic seasonality stage and emit normalized handoff-ready output.
pub fn run_seasonality_calibration_stage(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage_input: SeasonalityStageExecutionInput,
) -> SeasonalityStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::Seasonality)
    {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::Seasonality)
    else {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    let SeasonalityStageExecutionInput {
        timestamp_event_records,
        scheduled_event_overlays,
        settings,
        dataset_id_override,
        estimator_version_override,
    } = stage_input;

    if timestamp_event_records.is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::MissingTimestampEventRecords,
        );
    }

    let input_record_count = timestamp_event_records.len();
    let resolved_dataset_id = dataset_id_override
        .unwrap_or_else(|| run_spec.data_inputs.trade_events_data_reference.clone());
    if resolved_dataset_id.trim().is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyDatasetId,
        );
    }

    let resolved_estimator_version = estimator_version_override
        .unwrap_or_else(|| CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION.to_string());
    if resolved_estimator_version.trim().is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::EmptyEstimatorVersion,
        );
    }

    let preprocessed = match calendar_registry.preprocess_timestamp_events(
        &run_spec.data_inputs.exchange_code,
        timestamp_event_records,
        settings.session_filter,
        settings.sparse_hour_event_threshold,
    ) {
        Ok(preprocessed) => preprocessed,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::TimestampPreprocessingFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    if preprocessed.timestamp_utc_nanos.is_empty() {
        return build_seasonality_stage_failure_report(
            run_spec,
            SeasonalityStageExecutionFailure::NoAcceptedTimestampEventsAfterPreprocessing,
        );
    }

    let intraday_estimate = match estimate_intraday_seasonality(
        &preprocessed.exchange_local_minute_of_day,
        settings.intraday_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::IntradayEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let day_of_week_estimate = match estimate_day_of_week_seasonality(
        &preprocessed.exchange_local_day_of_week,
        settings.day_of_week_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DayOfWeekEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let day_of_year_estimate = match estimate_day_of_year_seasonality(
        &preprocessed.exchange_local_day_of_year,
        settings.day_of_year_settings,
    ) {
        Ok(estimate) => estimate,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DayOfYearEstimationFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let composition = match compose_deterministic_seasonality_signal(
        &preprocessed.timestamp_utc_nanos,
        &preprocessed.exchange_local_minute_of_day,
        &preprocessed.exchange_local_day_of_week,
        &preprocessed.exchange_local_day_of_year,
        intraday_estimate.minutes_per_bin,
        &intraday_estimate.intraday_log_component_by_bin,
        &day_of_week_estimate.day_of_week_log_component,
        &day_of_year_estimate.day_of_year_log_component,
        &scheduled_event_overlays,
    ) {
        Ok(composition) => composition,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DeterministicCompositionFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let diagnostics_report = match generate_seasonality_diagnostics_report(
        crate::seasonality::SeasonalityDiagnosticsReportInput {
            dataset_id: &resolved_dataset_id,
            input_schema_version: preprocessed.input_schema_version,
            preprocessed_schema_version: preprocessed.schema_version,
            exchange_code: &preprocessed.exchange_code,
            exchange_timezone_name: &preprocessed.exchange_timezone_name,
            source_record_positions: &preprocessed.source_record_positions,
            timestamp_utc_nanos: &preprocessed.timestamp_utc_nanos,
            exchange_local_minute_of_day: &preprocessed.exchange_local_minute_of_day,
            exchange_local_day_of_week: &preprocessed.exchange_local_day_of_week,
            exchange_local_day_of_year: &preprocessed.exchange_local_day_of_year,
            market_state_codes: &preprocessed.market_state_codes,
            intraday_estimate: &intraday_estimate,
            day_of_week_estimate: &day_of_week_estimate,
            day_of_year_estimate: &day_of_year_estimate,
            composition: &composition,
            estimator_version: &resolved_estimator_version,
        },
    ) {
        Ok(report) => report,
        Err(source) => {
            return build_seasonality_stage_failure_report(
                run_spec,
                SeasonalityStageExecutionFailure::DiagnosticsReportFailed {
                    error_message: source.to_string(),
                },
            );
        }
    };

    let normalization_metadata = SeasonalityStageNormalizationMetadata {
        normalization_policy: composition
            .diagnostics
            .normalization_metadata
            .normalization_policy
            .to_string(),
        fit_window_event_count: composition
            .diagnostics
            .normalization_metadata
            .fit_window_event_count,
        target_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .target_mean_factor,
        pre_normalization_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .pre_normalization_mean_factor,
        normalization_constant: composition
            .diagnostics
            .normalization_metadata
            .normalization_constant,
        normalization_log_offset: composition
            .diagnostics
            .normalization_metadata
            .normalization_log_offset,
        post_normalization_mean_factor: composition
            .diagnostics
            .normalization_metadata
            .post_normalization_mean_factor,
        mean_anchor_tolerance: composition
            .diagnostics
            .normalization_metadata
            .mean_anchor_tolerance,
    };

    let overlay_metadata = SeasonalityStageOverlayMetadata {
        overlays_enabled: composition
            .diagnostics
            .overlay_provenance_metadata
            .overlays_enabled,
        configured_overlay_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_count,
        enabled_overlay_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .enabled_overlay_count,
        active_overlay_event_count: composition
            .diagnostics
            .overlay_provenance_metadata
            .active_overlay_event_count,
        precedence_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .precedence_policy
            .to_string(),
        merge_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .merge_policy
            .to_string(),
        window_boundary_policy: composition
            .diagnostics
            .overlay_provenance_metadata
            .window_boundary_policy
            .to_string(),
        configured_overlay_ids_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .configured_overlay_ids_in_order
            .clone(),
        applied_overlay_ids_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .applied_overlay_ids_in_order
            .clone(),
        overlay_source_labels_in_order: composition
            .diagnostics
            .overlay_provenance_metadata
            .overlay_source_labels_in_order
            .clone(),
    };

    let normalized_stage_payload = SeasonalityStageNormalizedPayload {
        schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
        dataset_id: resolved_dataset_id.clone(),
        exchange_code: preprocessed.exchange_code.clone(),
        trade_events_data_reference: run_spec.data_inputs.trade_events_data_reference.clone(),
        timestamp_event_input_schema_version: preprocessed.input_schema_version.to_string(),
        timestamp_event_preprocessed_schema_version: preprocessed.schema_version.to_string(),
        intraday_estimate_schema_version: intraday_estimate.schema_version.to_string(),
        day_of_week_estimate_schema_version: day_of_week_estimate.schema_version.to_string(),
        day_of_year_estimate_schema_version: day_of_year_estimate.schema_version.to_string(),
        composition_schema_version: composition.schema_version.to_string(),
        source_record_positions: preprocessed.source_record_positions.clone(),
        timestamp_utc_nanos: preprocessed.timestamp_utc_nanos.clone(),
        exchange_local_minute_of_day: preprocessed.exchange_local_minute_of_day.clone(),
        exchange_local_day_of_week: preprocessed.exchange_local_day_of_week.clone(),
        exchange_local_day_of_year: preprocessed.exchange_local_day_of_year.clone(),
        market_state_codes: preprocessed.market_state_codes.clone(),
        intraday_minutes_per_bin: intraday_estimate.minutes_per_bin,
        intraday_factor_by_bin: intraday_estimate.intraday_factor_by_bin.clone(),
        day_of_week_factor: day_of_week_estimate.day_of_week_factor.clone(),
        day_of_year_factor: day_of_year_estimate.day_of_year_factor.clone(),
        normalized_combined_log_component_by_event: composition
            .combined_log_component_by_event
            .clone(),
        normalized_combined_factor_by_event: composition.combined_factor_by_event.clone(),
        normalization_metadata: normalization_metadata.clone(),
        overlay_metadata,
        diagnostics_report: diagnostics_report.clone(),
    };

    let handoffs = build_seasonality_stage_handoff_outputs(run_spec);
    let stage_seed = resolve_calibration_stage_seed(run_spec, CalibrationStage::Seasonality);

    SeasonalityStageExecutionReport {
        schema_version: CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Seasonality,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
        handoffs,
        normalized_stage_payload: Some(normalized_stage_payload),
        metadata: Some(SeasonalityStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_SEASONALITY_STAGE_OUTPUT_SCHEMA_VERSION,
            stage_seed,
            solver_settings: stage_solver_settings,
            dataset_id: resolved_dataset_id,
            estimator_version: resolved_estimator_version,
            input_record_count,
            accepted_record_count: preprocessed.diagnostics.accepted_record_count,
            filtered_record_count: preprocessed.diagnostics.filtered_record_count,
            diagnostics_report_schema_version: diagnostics_report.schema_version.to_string(),
            diagnostics_snapshot_fingerprint_algorithm: diagnostics_report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_algorithm
                .to_string(),
            diagnostics_snapshot_fingerprint_hex: diagnostics_report
                .reproducibility_metadata
                .input_snapshot
                .snapshot_fingerprint_hex
                .clone(),
            normalization_policy: normalization_metadata.normalization_policy,
        }),
        failure: None,
    }
}

fn build_seasonality_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<SeasonalityStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::Seasonality)
        .map(|stage_handoff| match stage_handoff.transport {
            CalibrationStageHandoffTransport::InMemoryOutput => SeasonalityStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport,
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                SeasonalityStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport,
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference.clone(),
                }
            }
        })
        .collect()
}

fn stage_solver_settings_for_stage(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Option<CalibrationOptimizerSettings> {
    run_spec
        .optimizer_settings_by_stage
        .iter()
        .find(|stage_solver_settings| stage_solver_settings.stage == stage)
        .map(|stage_solver_settings| stage_solver_settings.optimizer_settings.clone())
}

fn build_seasonality_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: SeasonalityStageExecutionFailure,
) -> SeasonalityStageExecutionReport {
    SeasonalityStageExecutionReport {
        schema_version: CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::Seasonality,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_SEASONALITY_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        normalized_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

pub const CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION: &str =
    "calibration.optimizer.objective_composition.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION: &str =
    "calibration.optimizer.adapter_run.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY: &str =
    "minimum_penalized_then_minimum_raw_then_seeded_tiebreak.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY: &str =
    "constraint_penalty_weight_times_total_violation_count.v1";
pub const CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY: &str =
    "inclusive_named_parameter_bounds.v1";

/// Canonical weighted objective term shape shared across calibration stages.
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationObjectiveTermInput {
    pub term_id: String,
    pub weight: f64,
    pub observed_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationObjectiveTermInputError {
    MissingTermId,
    InvalidWeight { value: f64 },
    InvalidObservedScore { value: f64 },
}

impl Display for CalibrationObjectiveTermInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTermId => write!(formatter, "objective term id must be non-empty"),
            Self::InvalidWeight { value } => write!(
                formatter,
                "objective term weight must be finite and non-negative, found {value}"
            ),
            Self::InvalidObservedScore { value } => write!(
                formatter,
                "objective term observed score must be finite, found {value}"
            ),
        }
    }
}

impl Error for CalibrationObjectiveTermInputError {}

impl CalibrationObjectiveTermInput {
    fn validate(&self) -> Result<(), CalibrationObjectiveTermInputError> {
        if self.term_id.trim().is_empty() {
            return Err(CalibrationObjectiveTermInputError::MissingTermId);
        }
        if !self.weight.is_finite() || self.weight < 0.0 {
            return Err(CalibrationObjectiveTermInputError::InvalidWeight { value: self.weight });
        }
        if !self.observed_score.is_finite() {
            return Err(CalibrationObjectiveTermInputError::InvalidObservedScore {
                value: self.observed_score,
            });
        }

        Ok(())
    }
}

/// One weighted objective term used by Hawkes-stage candidate evaluation.
pub type HawkesStageObjectiveTermInput = CalibrationObjectiveTermInput;
pub type HawkesStageObjectiveTermInputError = CalibrationObjectiveTermInputError;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationObjectiveTermDiagnostic {
    pub term_id: String,
    pub weight: f64,
    pub observed_score: f64,
    pub weighted_contribution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationObjectiveCompositionReport {
    pub schema_version: &'static str,
    pub weighted_objective_value: f64,
    pub objective_terms: Vec<CalibrationObjectiveTermDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationObjectiveCompositionError {
    MissingObjectiveTerms,
    InvalidObjectiveTerm {
        term_id: String,
        source: CalibrationObjectiveTermInputError,
    },
    InvalidWeightedObjectiveValue {
        value: f64,
    },
}

impl Display for CalibrationObjectiveCompositionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingObjectiveTerms => {
                write!(
                    formatter,
                    "objective_terms must include at least one weighted term"
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

impl Error for CalibrationObjectiveCompositionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidObjectiveTerm { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn compose_calibration_weighted_objective(
    objective_terms: &[CalibrationObjectiveTermInput],
) -> Result<CalibrationObjectiveCompositionReport, CalibrationObjectiveCompositionError> {
    if objective_terms.is_empty() {
        return Err(CalibrationObjectiveCompositionError::MissingObjectiveTerms);
    }

    let mut weighted_objective_value = 0.0;
    let mut diagnostics = Vec::with_capacity(objective_terms.len());

    for objective_term in objective_terms {
        objective_term.validate().map_err(|source| {
            CalibrationObjectiveCompositionError::InvalidObjectiveTerm {
                term_id: objective_term.term_id.clone(),
                source,
            }
        })?;

        let weighted_contribution = objective_term.weight * objective_term.observed_score;
        if !weighted_contribution.is_finite() {
            return Err(
                CalibrationObjectiveCompositionError::InvalidWeightedObjectiveValue {
                    value: weighted_contribution,
                },
            );
        }

        weighted_objective_value += weighted_contribution;
        if !weighted_objective_value.is_finite() {
            return Err(
                CalibrationObjectiveCompositionError::InvalidWeightedObjectiveValue {
                    value: weighted_objective_value,
                },
            );
        }

        diagnostics.push(CalibrationObjectiveTermDiagnostic {
            term_id: objective_term.term_id.clone(),
            weight: objective_term.weight,
            observed_score: objective_term.observed_score,
            weighted_contribution,
        });
    }

    Ok(CalibrationObjectiveCompositionReport {
        schema_version: CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
        weighted_objective_value,
        objective_terms: diagnostics,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerParameterBound {
    pub parameter_name: String,
    pub lower_inclusive: f64,
    pub upper_inclusive: f64,
}

impl CalibrationOptimizerParameterBound {
    pub fn try_new(
        parameter_name: impl Into<String>,
        lower_inclusive: f64,
        upper_inclusive: f64,
    ) -> Result<Self, CalibrationOptimizerParameterBoundError> {
        let bound = Self {
            parameter_name: parameter_name.into(),
            lower_inclusive,
            upper_inclusive,
        };
        bound.validate()?;
        Ok(bound)
    }

    fn validate(&self) -> Result<(), CalibrationOptimizerParameterBoundError> {
        if self.parameter_name.trim().is_empty() {
            return Err(CalibrationOptimizerParameterBoundError::MissingParameterName);
        }
        if !self.lower_inclusive.is_finite() {
            return Err(CalibrationOptimizerParameterBoundError::InvalidLowerBound {
                value: self.lower_inclusive,
            });
        }
        if !self.upper_inclusive.is_finite() {
            return Err(CalibrationOptimizerParameterBoundError::InvalidUpperBound {
                value: self.upper_inclusive,
            });
        }
        if self.lower_inclusive > self.upper_inclusive {
            return Err(CalibrationOptimizerParameterBoundError::InvertedBounds {
                lower_inclusive: self.lower_inclusive,
                upper_inclusive: self.upper_inclusive,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerParameterBoundError {
    MissingParameterName,
    InvalidLowerBound {
        value: f64,
    },
    InvalidUpperBound {
        value: f64,
    },
    InvertedBounds {
        lower_inclusive: f64,
        upper_inclusive: f64,
    },
}

impl Display for CalibrationOptimizerParameterBoundError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingParameterName => {
                write!(
                    formatter,
                    "optimizer bound parameter_name must be non-empty"
                )
            }
            Self::InvalidLowerBound { value } => write!(
                formatter,
                "optimizer bound lower_inclusive must be finite, found {value}"
            ),
            Self::InvalidUpperBound { value } => write!(
                formatter,
                "optimizer bound upper_inclusive must be finite, found {value}"
            ),
            Self::InvertedBounds {
                lower_inclusive,
                upper_inclusive,
            } => write!(
                formatter,
                "optimizer bound lower_inclusive ({lower_inclusive}) must be <= upper_inclusive ({upper_inclusive})"
            ),
        }
    }
}

impl Error for CalibrationOptimizerParameterBoundError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationOptimizerAdapterConfiguration {
    pub backend_id: String,
    pub random_seed: u64,
    pub solver_settings: CalibrationOptimizerSettings,
    #[serde(default)]
    pub parameter_bounds: Vec<CalibrationOptimizerParameterBound>,
}

impl CalibrationOptimizerAdapterConfiguration {
    pub fn try_new(
        backend_id: impl Into<String>,
        random_seed: u64,
        solver_settings: CalibrationOptimizerSettings,
        parameter_bounds: Vec<CalibrationOptimizerParameterBound>,
    ) -> Result<Self, CalibrationOptimizerAdapterConfigurationError> {
        let configuration = Self {
            backend_id: backend_id.into(),
            random_seed,
            solver_settings,
            parameter_bounds,
        };
        configuration.validate()?;
        Ok(configuration)
    }

    fn validate(&self) -> Result<(), CalibrationOptimizerAdapterConfigurationError> {
        if self.backend_id.trim().is_empty() {
            return Err(CalibrationOptimizerAdapterConfigurationError::MissingBackendId);
        }

        self.solver_settings.validate().map_err(|source| {
            CalibrationOptimizerAdapterConfigurationError::InvalidSolverSettings { source }
        })?;

        let mut seen_parameter_names = BTreeSet::new();
        for bound in &self.parameter_bounds {
            bound.validate().map_err(|source| {
                CalibrationOptimizerAdapterConfigurationError::InvalidParameterBound {
                    parameter_name: bound.parameter_name.clone(),
                    source,
                }
            })?;
            if !seen_parameter_names.insert(bound.parameter_name.as_str()) {
                return Err(
                    CalibrationOptimizerAdapterConfigurationError::DuplicateParameterBound {
                        parameter_name: bound.parameter_name.clone(),
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterConfigurationError {
    MissingBackendId,
    InvalidSolverSettings {
        source: CalibrationOptimizerSettingsError,
    },
    InvalidParameterBound {
        parameter_name: String,
        source: CalibrationOptimizerParameterBoundError,
    },
    DuplicateParameterBound {
        parameter_name: String,
    },
}

impl Display for CalibrationOptimizerAdapterConfigurationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingBackendId => {
                write!(formatter, "optimizer adapter backend_id must be non-empty")
            }
            Self::InvalidSolverSettings { source } => {
                write!(
                    formatter,
                    "optimizer adapter solver_settings are invalid: {source}"
                )
            }
            Self::InvalidParameterBound {
                parameter_name,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter parameter bound `{parameter_name}` is invalid: {source}"
                )
            }
            Self::DuplicateParameterBound { parameter_name } => write!(
                formatter,
                "optimizer adapter contains duplicate bound for parameter `{parameter_name}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterConfigurationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidParameterBound { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationOptimizerCandidateParameterValue {
    pub parameter_name: String,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationOptimizerAdapterCandidateInput {
    pub candidate_id: String,
    pub objective_terms: Vec<CalibrationObjectiveTermInput>,
    pub parameter_values: Vec<CalibrationOptimizerCandidateParameterValue>,
    pub external_constraint_violation_count: usize,
}

impl CalibrationOptimizerAdapterCandidateInput {
    fn validate(&self) -> Result<(), CalibrationOptimizerAdapterCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(CalibrationOptimizerAdapterCandidateInputError::MissingCandidateId);
        }
        if self.objective_terms.is_empty() {
            return Err(CalibrationOptimizerAdapterCandidateInputError::MissingObjectiveTerms);
        }

        let mut seen_parameter_names = BTreeSet::new();
        for parameter_value in &self.parameter_values {
            if parameter_value.parameter_name.trim().is_empty() {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::MissingParameterValueName {
                        candidate_id: self.candidate_id.clone(),
                    },
                );
            }

            if !parameter_value.value.is_finite() {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::InvalidParameterValue {
                        candidate_id: self.candidate_id.clone(),
                        parameter_name: parameter_value.parameter_name.clone(),
                        value: parameter_value.value,
                    },
                );
            }

            if !seen_parameter_names.insert(parameter_value.parameter_name.as_str()) {
                return Err(
                    CalibrationOptimizerAdapterCandidateInputError::DuplicateParameterValue {
                        candidate_id: self.candidate_id.clone(),
                        parameter_name: parameter_value.parameter_name.clone(),
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterCandidateInputError {
    MissingCandidateId,
    MissingObjectiveTerms,
    MissingParameterValueName {
        candidate_id: String,
    },
    DuplicateParameterValue {
        candidate_id: String,
        parameter_name: String,
    },
    InvalidParameterValue {
        candidate_id: String,
        parameter_name: String,
        value: f64,
    },
}

impl Display for CalibrationOptimizerAdapterCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => {
                write!(
                    formatter,
                    "optimizer adapter candidate_id must be non-empty"
                )
            }
            Self::MissingObjectiveTerms => write!(
                formatter,
                "optimizer adapter candidate objective_terms must include at least one term"
            ),
            Self::MissingParameterValueName { candidate_id } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has parameter_values entry with empty parameter_name"
            ),
            Self::DuplicateParameterValue {
                candidate_id,
                parameter_name,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has duplicate parameter value for `{parameter_name}`"
            ),
            Self::InvalidParameterValue {
                candidate_id,
                parameter_name,
                value,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` has non-finite value `{value}` for parameter `{parameter_name}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterCandidateInputError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationOptimizerBoundViolationKind {
    BelowLowerBound,
    AboveUpperBound,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerBoundViolationDiagnostic {
    pub parameter_name: String,
    pub value: f64,
    pub lower_inclusive: f64,
    pub upper_inclusive: f64,
    pub violation_kind: CalibrationOptimizerBoundViolationKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerAdapterCandidateDiagnostic {
    pub candidate_id: String,
    pub objective_composition: CalibrationObjectiveCompositionReport,
    pub raw_objective_value: f64,
    pub external_constraint_violation_count: usize,
    pub bound_violation_count: usize,
    pub total_constraint_violation_count: usize,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub bound_violations: Vec<CalibrationOptimizerBoundViolationDiagnostic>,
    pub seeded_tiebreak_key: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerAdapterRunMetadata {
    pub schema_version: &'static str,
    pub backend_id: String,
    pub random_seed: u64,
    pub selection_policy: &'static str,
    pub penalty_policy: &'static str,
    pub bounds_policy: &'static str,
    pub solver_settings: CalibrationOptimizerSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationOptimizerAdapterRunOutput {
    pub metadata: CalibrationOptimizerAdapterRunMetadata,
    pub selected_candidate_id: Option<String>,
    pub selected_candidate_position: Option<usize>,
    pub candidate_diagnostics: Vec<CalibrationOptimizerAdapterCandidateDiagnostic>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationOptimizerAdapterError {
    InvalidConfiguration {
        source: CalibrationOptimizerAdapterConfigurationError,
    },
    MissingCandidates,
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        source: CalibrationOptimizerAdapterCandidateInputError,
    },
    InvalidObjectiveComposition {
        candidate_id: String,
        source: CalibrationObjectiveCompositionError,
    },
    MissingBoundedParameterValue {
        candidate_id: String,
        parameter_name: String,
    },
    InvalidPenalizedObjectiveValue {
        candidate_id: String,
        value: f64,
    },
}

impl Display for CalibrationOptimizerAdapterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration { source } => {
                write!(
                    formatter,
                    "optimizer adapter configuration is invalid: {source}"
                )
            }
            Self::MissingCandidates => write!(
                formatter,
                "optimizer adapter invocation requires at least one candidate"
            ),
            Self::DuplicateCandidateId { candidate_id } => write!(
                formatter,
                "optimizer adapter candidates contain duplicate candidate_id `{candidate_id}`"
            ),
            Self::InvalidCandidateInput {
                candidate_id,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter candidate `{candidate_id}` is invalid: {source}"
                )
            }
            Self::InvalidObjectiveComposition {
                candidate_id,
                source,
            } => {
                write!(
                    formatter,
                    "optimizer adapter candidate `{candidate_id}` objective composition failed: {source}"
                )
            }
            Self::MissingBoundedParameterValue {
                candidate_id,
                parameter_name,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` is missing required bounded parameter `{parameter_name}`"
            ),
            Self::InvalidPenalizedObjectiveValue {
                candidate_id,
                value,
            } => write!(
                formatter,
                "optimizer adapter candidate `{candidate_id}` produced non-finite penalized objective value `{value}`"
            ),
        }
    }
}

impl Error for CalibrationOptimizerAdapterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration { source } => Some(source),
            Self::InvalidCandidateInput { source, .. } => Some(source),
            Self::InvalidObjectiveComposition { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn run_constrained_optimizer_adapter(
    configuration: &CalibrationOptimizerAdapterConfiguration,
    candidate_inputs: &[CalibrationOptimizerAdapterCandidateInput],
) -> Result<CalibrationOptimizerAdapterRunOutput, CalibrationOptimizerAdapterError> {
    configuration
        .validate()
        .map_err(|source| CalibrationOptimizerAdapterError::InvalidConfiguration { source })?;

    if candidate_inputs.is_empty() {
        return Err(CalibrationOptimizerAdapterError::MissingCandidates);
    }

    let mut seen_candidate_ids = BTreeSet::new();
    let mut candidate_diagnostics = Vec::with_capacity(candidate_inputs.len());

    for candidate_input in candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.as_str()) {
            return Err(CalibrationOptimizerAdapterError::DuplicateCandidateId {
                candidate_id: candidate_input.candidate_id.clone(),
            });
        }

        candidate_input.validate().map_err(|source| {
            CalibrationOptimizerAdapterError::InvalidCandidateInput {
                candidate_id: candidate_input.candidate_id.clone(),
                source,
            }
        })?;

        let objective_composition =
            compose_calibration_weighted_objective(candidate_input.objective_terms.as_slice())
                .map_err(|source| {
                    CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
                        candidate_id: candidate_input.candidate_id.clone(),
                        source,
                    }
                })?;

        let parameter_values = candidate_input
            .parameter_values
            .iter()
            .map(|parameter_value| {
                (
                    parameter_value.parameter_name.as_str(),
                    parameter_value.value,
                )
            })
            .collect::<BTreeMap<_, _>>();

        let mut bound_violations = Vec::new();
        for parameter_bound in &configuration.parameter_bounds {
            let Some(parameter_value) =
                parameter_values.get(parameter_bound.parameter_name.as_str())
            else {
                return Err(
                    CalibrationOptimizerAdapterError::MissingBoundedParameterValue {
                        candidate_id: candidate_input.candidate_id.clone(),
                        parameter_name: parameter_bound.parameter_name.clone(),
                    },
                );
            };

            if *parameter_value < parameter_bound.lower_inclusive {
                bound_violations.push(CalibrationOptimizerBoundViolationDiagnostic {
                    parameter_name: parameter_bound.parameter_name.clone(),
                    value: *parameter_value,
                    lower_inclusive: parameter_bound.lower_inclusive,
                    upper_inclusive: parameter_bound.upper_inclusive,
                    violation_kind: CalibrationOptimizerBoundViolationKind::BelowLowerBound,
                });
            } else if *parameter_value > parameter_bound.upper_inclusive {
                bound_violations.push(CalibrationOptimizerBoundViolationDiagnostic {
                    parameter_name: parameter_bound.parameter_name.clone(),
                    value: *parameter_value,
                    lower_inclusive: parameter_bound.lower_inclusive,
                    upper_inclusive: parameter_bound.upper_inclusive,
                    violation_kind: CalibrationOptimizerBoundViolationKind::AboveUpperBound,
                });
            }
        }

        let bound_violation_count = bound_violations.len();
        let total_constraint_violation_count =
            candidate_input.external_constraint_violation_count + bound_violation_count;
        let constraint_penalty = configuration.solver_settings.constraint_penalty_weight
            * total_constraint_violation_count as f64;
        let penalized_objective_value =
            objective_composition.weighted_objective_value + constraint_penalty;

        if !penalized_objective_value.is_finite() {
            return Err(
                CalibrationOptimizerAdapterError::InvalidPenalizedObjectiveValue {
                    candidate_id: candidate_input.candidate_id.clone(),
                    value: penalized_objective_value,
                },
            );
        }

        candidate_diagnostics.push(CalibrationOptimizerAdapterCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: objective_composition.weighted_objective_value,
            objective_composition,
            external_constraint_violation_count: candidate_input
                .external_constraint_violation_count,
            bound_violation_count,
            total_constraint_violation_count,
            constraint_penalty,
            penalized_objective_value,
            bound_violations,
            seeded_tiebreak_key: seeded_tiebreak_key(
                configuration.random_seed,
                candidate_input.candidate_id.as_str(),
            ),
        });
    }

    let selected_candidate_position = candidate_diagnostics
        .iter()
        .enumerate()
        .filter(|(_, candidate_diagnostic)| {
            candidate_diagnostic.total_constraint_violation_count == 0
        })
        .map(|(candidate_position, _)| candidate_position)
        .reduce(|best_position, candidate_position| {
            let candidate = &candidate_diagnostics[candidate_position];
            let current_best = &candidate_diagnostics[best_position];

            match candidate
                .penalized_objective_value
                .total_cmp(&current_best.penalized_objective_value)
            {
                std::cmp::Ordering::Less => candidate_position,
                std::cmp::Ordering::Greater => best_position,
                std::cmp::Ordering::Equal => match candidate
                    .raw_objective_value
                    .total_cmp(&current_best.raw_objective_value)
                {
                    std::cmp::Ordering::Less => candidate_position,
                    std::cmp::Ordering::Greater => best_position,
                    std::cmp::Ordering::Equal => match candidate
                        .seeded_tiebreak_key
                        .cmp(&current_best.seeded_tiebreak_key)
                    {
                        std::cmp::Ordering::Less => candidate_position,
                        std::cmp::Ordering::Greater => best_position,
                        std::cmp::Ordering::Equal => {
                            if candidate.candidate_id < current_best.candidate_id {
                                candidate_position
                            } else {
                                best_position
                            }
                        }
                    },
                },
            }
        });

    Ok(CalibrationOptimizerAdapterRunOutput {
        metadata: CalibrationOptimizerAdapterRunMetadata {
            schema_version: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
            backend_id: configuration.backend_id.clone(),
            random_seed: configuration.random_seed,
            selection_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_SELECTION_POLICY,
            penalty_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_PENALTY_POLICY,
            bounds_policy: CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_BOUNDS_POLICY,
            solver_settings: configuration.solver_settings.clone(),
        },
        selected_candidate_id: selected_candidate_position
            .map(|position| candidate_diagnostics[position].candidate_id.clone()),
        selected_candidate_position,
        candidate_diagnostics,
    })
}

fn seeded_tiebreak_key(seed: u64, candidate_id: &str) -> u64 {
    let mut mixed = seed ^ 0x6A09_E667_F3BC_C909;
    for byte in candidate_id.as_bytes() {
        mixed = mixed.rotate_left(7) ^ (*byte as u64);
        mixed = mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }

    splitmix64_output_mixer(mixed)
}

const fn splitmix64_output_mixer(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

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
    pub stage_seed: u64,
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
        .map(|stage_handoff| stage_handoff.transport)
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
            && let Some(expected_artifact_reference) = &expected_stage_handoff.artifact_reference
            && expected_artifact_reference != &provided_artifact_reference
        {
            return build_hawkes_stage_failure_report(
                run_spec,
                HawkesStageExecutionFailure::SeasonalityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.clone(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let stage_seed = resolve_calibration_stage_seed(run_spec, CalibrationStage::Hawkes);
    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
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
            stage_seed,
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
        .map(|stage_handoff| match stage_handoff.transport {
            CalibrationStageHandoffTransport::InMemoryOutput => HawkesStageHandoffOutput {
                from_stage: stage_handoff.from_stage,
                to_stage: stage_handoff.to_stage,
                transport: stage_handoff.transport,
                in_memory_payload_available: true,
                artifact_reference: None,
            },
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                HawkesStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport,
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference.clone(),
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

/// One weighted objective term used by return-components-stage candidate evaluation.
pub type ReturnComponentsStageObjectiveTermInput = HawkesStageObjectiveTermInput;

/// One weighted objective-term diagnostic emitted by return-components-stage evaluation.
pub type ReturnComponentsStageObjectiveTermDiagnostic = HawkesStageObjectiveTermDiagnostic;

/// Candidate parameterization and objective inputs for return-components-stage fitting.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageCandidateInput {
    pub candidate_id: String,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
    pub objective_terms: Vec<ReturnComponentsStageObjectiveTermInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageCandidateInputError {
    MissingCandidateId,
    MissingObjectiveTerms,
    InvalidObjectiveTerm {
        term_id: String,
        source: HawkesStageObjectiveTermInputError,
    },
    InvalidWeightedObjectiveValue {
        value: f64,
    },
}

impl Display for ReturnComponentsStageCandidateInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCandidateId => write!(formatter, "candidate_id must be non-empty"),
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

impl Error for ReturnComponentsStageCandidateInputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidObjectiveTerm { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl ReturnComponentsStageCandidateInput {
    fn validate(&self) -> Result<(), ReturnComponentsStageCandidateInputError> {
        if self.candidate_id.trim().is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingCandidateId);
        }
        if self.objective_terms.is_empty() {
            return Err(ReturnComponentsStageCandidateInputError::MissingObjectiveTerms);
        }

        for objective_term in &self.objective_terms {
            objective_term.validate().map_err(|source| {
                ReturnComponentsStageCandidateInputError::InvalidObjectiveTerm {
                    term_id: objective_term.term_id.clone(),
                    source,
                }
            })?;
        }

        Ok(())
    }
}

/// Upstream activity-side handoff input resolved for return-components-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ReturnComponentsStageActivityInput {
    InMemoryOutput { payload: HawkesStageFittedPayload },
    PersistedArtifactReference { artifact_reference: String },
}

impl ReturnComponentsStageActivityInput {
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

    fn selected_hawkes_candidate_id(&self) -> Option<&str> {
        match self {
            Self::InMemoryOutput { payload } => Some(payload.selected_candidate_id.as_str()),
            Self::PersistedArtifactReference { .. } => None,
        }
    }
}

/// Input payload for standalone or pipeline-integrated return-components-stage execution.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnComponentsStageExecutionInput {
    pub activity_input: ReturnComponentsStageActivityInput,
    pub candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
}

impl ReturnComponentsStageExecutionInput {
    pub fn from_in_memory_activity_output(
        hawkes_payload: HawkesStageFittedPayload,
        candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    ) -> Self {
        Self {
            activity_input: ReturnComponentsStageActivityInput::InMemoryOutput {
                payload: hawkes_payload,
            },
            candidate_inputs,
        }
    }

    pub fn from_persisted_activity_artifact_reference(
        artifact_reference: impl Into<String>,
        candidate_inputs: Vec<ReturnComponentsStageCandidateInput>,
    ) -> Self {
        Self {
            activity_input: ReturnComponentsStageActivityInput::PersistedArtifactReference {
                artifact_reference: artifact_reference.into(),
            },
            candidate_inputs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageConstraintViolationDiagnostic {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageCandidateDiagnostic {
    pub candidate_id: String,
    pub raw_objective_value: f64,
    pub constraint_penalty: f64,
    pub penalized_objective_value: f64,
    pub constraint_violation_count: usize,
    pub constraint_violations: Vec<ReturnComponentsStageConstraintViolationDiagnostic>,
    pub objective_terms: Vec<ReturnComponentsStageObjectiveTermDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageFittedParameters {
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub news_process_configuration: NewsProcessConfiguration,
}

/// Stable return-components-stage payload consumed by downstream calibration stages.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageFittedPayload {
    pub schema_version: &'static str,
    pub selected_candidate_id: String,
    pub fitted_parameters: ReturnComponentsStageFittedParameters,
    pub candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic>,
    pub objective_weighting_policy: &'static str,
    pub activity_input_transport: CalibrationStageHandoffTransport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_selected_hawkes_candidate_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_artifact_reference: Option<String>,
}

/// Stage-handoff transfer output resolved from the run-spec contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageHandoffOutput {
    pub from_stage: CalibrationStage,
    pub to_stage: CalibrationStage,
    pub transport: CalibrationStageHandoffTransport,
    pub in_memory_payload_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_reference: Option<String>,
}

/// Deterministic execution metadata exposed for reproducibility and downstream audit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExecutionMetadata {
    pub stage_output_schema_version: &'static str,
    pub stage_seed: u64,
    pub solver_settings: CalibrationOptimizerSettings,
    pub optimizer_adapter_metadata: CalibrationOptimizerAdapterRunMetadata,
    pub input_candidate_count: usize,
    pub valid_candidate_count: usize,
    pub selected_candidate_raw_objective: f64,
    pub selected_candidate_penalized_objective: f64,
    pub objective_weighting_policy: &'static str,
}

/// Structured failure modes for return-components-stage execution.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum ReturnComponentsStageExecutionFailure {
    InvalidRunSpec {
        error_message: String,
    },
    StageIsNotSelectedInRunSpec,
    MissingStageSolverSettings,
    ActivityInputTransportMismatch {
        expected_transport: CalibrationStageHandoffTransport,
        provided_transport: CalibrationStageHandoffTransport,
    },
    MissingArtifactReferenceInActivityInput,
    InvalidArtifactReferenceInActivityInput {
        artifact_reference: String,
    },
    ActivityArtifactReferenceMismatch {
        expected_artifact_reference: String,
        provided_artifact_reference: String,
    },
    MissingReturnComponentsCandidateInputs,
    DuplicateCandidateId {
        candidate_id: String,
    },
    InvalidCandidateInput {
        candidate_id: String,
        error_message: String,
    },
    ConstraintViolationsForAllCandidates {
        candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic>,
    },
    CandidateConstructionFailed {
        candidate_id: String,
        error_message: String,
    },
}

/// Structured return-components-stage report for partial or full staged calibration workflows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReturnComponentsStageExecutionReport {
    pub schema_version: &'static str,
    pub stage: CalibrationStage,
    pub status: CalibrationStageExecutionStatus,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub handoff_policy: &'static str,
    pub handoffs: Vec<ReturnComponentsStageHandoffOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fitted_stage_payload: Option<ReturnComponentsStageFittedPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ReturnComponentsStageExecutionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ReturnComponentsStageExecutionFailure>,
}

/// Execute the return-components stage with NIG and coupled-gap parameter constraints.
pub fn run_return_components_calibration_stage(
    run_spec: &CalibrationRunSpec,
    stage_input: ReturnComponentsStageExecutionInput,
) -> ReturnComponentsStageExecutionReport {
    if let Err(source) = run_spec.validate() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::InvalidRunSpec {
                error_message: format!("{source} (classification={:?})", source.classification()),
            },
        );
    }

    if !run_spec
        .selected_stages
        .iter()
        .copied()
        .any(|selected_stage| selected_stage == CalibrationStage::ReturnComponents)
    {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::StageIsNotSelectedInRunSpec,
        );
    }

    let Some(stage_solver_settings) =
        stage_solver_settings_for_stage(run_spec, CalibrationStage::ReturnComponents)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::MissingStageSolverSettings,
        );
    };

    if stage_input.candidate_inputs.is_empty() {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::MissingReturnComponentsCandidateInputs,
        );
    }

    let expected_activity_handoff = run_spec.stage_handoffs.iter().find(|stage_handoff| {
        stage_handoff.from_stage == CalibrationStage::Hawkes
            && stage_handoff.to_stage == CalibrationStage::ReturnComponents
    });
    let expected_transport = expected_activity_handoff
        .map(|stage_handoff| stage_handoff.transport)
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        });
    let provided_transport = stage_input.activity_input.transport();

    if provided_transport != expected_transport {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch {
                expected_transport,
                provided_transport,
            },
        );
    }

    if provided_transport == CalibrationStageHandoffTransport::PersistedArtifactReference {
        let Some(provided_artifact_reference) = stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned)
        else {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::MissingArtifactReferenceInActivityInput,
            );
        };

        if provided_artifact_reference.trim().is_empty() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidArtifactReferenceInActivityInput {
                    artifact_reference: provided_artifact_reference,
                },
            );
        }

        if let Some(expected_stage_handoff) = expected_activity_handoff
            && let Some(expected_artifact_reference) = &expected_stage_handoff.artifact_reference
            && expected_artifact_reference != &provided_artifact_reference
        {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::ActivityArtifactReferenceMismatch {
                    expected_artifact_reference: expected_artifact_reference.clone(),
                    provided_artifact_reference,
                },
            );
        }
    }

    let stage_seed = resolve_calibration_stage_seed(run_spec, CalibrationStage::ReturnComponents);
    let optimizer_adapter_configuration = match CalibrationOptimizerAdapterConfiguration::try_new(
        "candidate_enumeration.v1",
        stage_seed,
        stage_solver_settings.clone(),
        return_components_stage_optimizer_bounds(),
    ) {
        Ok(configuration) => configuration,

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter_configuration".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut seen_candidate_ids = BTreeSet::new();
    let mut external_constraint_diagnostics =
        Vec::<Vec<ReturnComponentsStageConstraintViolationDiagnostic>>::with_capacity(
            stage_input.candidate_inputs.len(),
        );
    let mut adapter_candidate_inputs = Vec::with_capacity(stage_input.candidate_inputs.len());
    for candidate_input in &stage_input.candidate_inputs {
        if !seen_candidate_ids.insert(candidate_input.candidate_id.clone()) {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId {
                    candidate_id: candidate_input.candidate_id.clone(),
                },
            );
        }

        if let Err(source) = candidate_input.validate() {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id: candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }

        let constraint_violations =
            collect_return_components_stage_constraint_violations(candidate_input);
        let external_constraint_violation_count = constraint_violations.len();
        external_constraint_diagnostics.push(constraint_violations);

        adapter_candidate_inputs.push(CalibrationOptimizerAdapterCandidateInput {
            candidate_id: candidate_input.candidate_id.clone(),
            objective_terms: candidate_input.objective_terms.clone(),
            parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
                value: candidate_input
                    .nig_shared_plus_shift_configuration
                    .delta_beta,
            }],
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
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::InvalidObjectiveComposition {
            candidate_id,
            source,
        }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::InvalidCandidateInput {
                    candidate_id,
                    error_message: source.to_string(),
                },
            );
        }

        Err(CalibrationOptimizerAdapterError::DuplicateCandidateId { candidate_id }) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::DuplicateCandidateId { candidate_id },
            );
        }

        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: "optimizer_adapter".to_string(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    let mut candidate_diagnostics: Vec<ReturnComponentsStageCandidateDiagnostic> =
        Vec::with_capacity(stage_input.candidate_inputs.len());
    for ((candidate_input, constraint_violations), optimizer_diagnostic) in stage_input
        .candidate_inputs
        .iter()
        .zip(external_constraint_diagnostics.into_iter())
        .zip(optimizer_adapter_output.candidate_diagnostics.iter())
    {
        let mut combined_constraint_violations = constraint_violations;
        for bound_violation in &optimizer_diagnostic.bound_violations {
            combined_constraint_violations.push(
                ReturnComponentsStageConstraintViolationDiagnostic {
                    field: bound_violation.parameter_name.clone(),
                    message: format!(
                        "value {} violated inclusive bounds [{}, {}] ({:?})",
                        bound_violation.value,
                        bound_violation.lower_inclusive,
                        bound_violation.upper_inclusive,
                        bound_violation.violation_kind
                    ),
                },
            );
        }

        candidate_diagnostics.push(ReturnComponentsStageCandidateDiagnostic {
            candidate_id: candidate_input.candidate_id.clone(),
            raw_objective_value: optimizer_diagnostic.raw_objective_value,
            constraint_penalty: optimizer_diagnostic.constraint_penalty,
            penalized_objective_value: optimizer_diagnostic.penalized_objective_value,
            constraint_violation_count: optimizer_diagnostic.total_constraint_violation_count,
            constraint_violations: combined_constraint_violations,
            objective_terms: optimizer_diagnostic
                .objective_composition
                .objective_terms
                .clone(),
        });
    }

    let Some(selected_candidate_id) = optimizer_adapter_output.selected_candidate_id.as_ref()
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            },
        );
    };

    let Some(selected_candidate_position) = stage_input
        .candidate_inputs
        .iter()
        .position(|candidate| candidate.candidate_id == *selected_candidate_id)
    else {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: "optimizer_adapter_selection".to_string(),
                error_message:
                    "selected candidate id was not found in stage_input.candidate_inputs"
                        .to_string(),
            },
        );
    };

    let selected_candidate_input = &stage_input.candidate_inputs[selected_candidate_position];
    let selected_candidate_diagnostic = &candidate_diagnostics[selected_candidate_position];
    let nig_configuration = match NigSharedPlusShiftConfiguration::try_new(
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .alpha,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .beta_0,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .mu,
        selected_candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                    candidate_id: selected_candidate_input.candidate_id.clone(),
                    error_message: source.to_string(),
                },
            );
        }
    };

    if !nig_configuration.delta_beta.is_finite() || nig_configuration.delta_beta >= 0.0 {
        return build_return_components_stage_failure_report(
            run_spec,
            ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
                candidate_id: selected_candidate_input.candidate_id.clone(),
                error_message:
                    "nig_shared_plus_shift_configuration.delta_beta must be finite and strictly negative for return skew asymmetry".to_string(),
            },
        );
    }

    let news_process_configuration = match NewsProcessConfiguration::try_new(
        selected_candidate_input
            .news_process_configuration
            .intensity,
        selected_candidate_input
            .news_process_configuration
            .gamma_severity_marks,
        selected_candidate_input
            .news_process_configuration
            .double_exponential_base_shock,
        selected_candidate_input
            .news_process_configuration
            .coupled_gap_marks,
    ) {
        Ok(configuration) => configuration,
        Err(source) => {
            return build_return_components_stage_failure_report(
                run_spec,
                ReturnComponentsStageExecutionFailure::CandidateConstructionFailed {
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

    let fitted_stage_payload = ReturnComponentsStageFittedPayload {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
        selected_candidate_id: selected_candidate_input.candidate_id.clone(),
        fitted_parameters: ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: nig_configuration,
            news_process_configuration,
        },
        candidate_diagnostics,
        objective_weighting_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
        activity_input_transport: stage_input.activity_input.transport(),
        activity_selected_hawkes_candidate_id: stage_input
            .activity_input
            .selected_hawkes_candidate_id()
            .map(ToOwned::to_owned),
        activity_artifact_reference: stage_input
            .activity_input
            .artifact_reference()
            .map(ToOwned::to_owned),
    };

    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Success,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: build_return_components_stage_handoff_outputs(run_spec),
        fitted_stage_payload: Some(fitted_stage_payload),
        metadata: Some(ReturnComponentsStageExecutionMetadata {
            stage_output_schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_OUTPUT_SCHEMA_VERSION,
            stage_seed,
            solver_settings: stage_solver_settings,
            optimizer_adapter_metadata: optimizer_adapter_output.metadata.clone(),
            input_candidate_count: stage_input.candidate_inputs.len(),
            valid_candidate_count,
            selected_candidate_raw_objective,
            selected_candidate_penalized_objective,
            objective_weighting_policy:
                CALIBRATION_RETURN_COMPONENTS_STAGE_OBJECTIVE_WEIGHTING_POLICY,
        }),
        failure: None,
    }
}

fn collect_return_components_stage_constraint_violations(
    candidate_input: &ReturnComponentsStageCandidateInput,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = Vec::new();

    if !candidate_input
        .nig_shared_plus_shift_configuration
        .delta_beta
        .is_finite()
        || candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta
            >= 0.0
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
            message:
                "delta_beta must be finite and strictly negative to preserve sell-side skew asymmetry"
                    .to_string(),
        });
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        candidate_input.nig_shared_plus_shift_configuration.alpha,
        candidate_input.nig_shared_plus_shift_configuration.beta_0,
        candidate_input.nig_shared_plus_shift_configuration.delta,
        candidate_input.nig_shared_plus_shift_configuration.mu,
        candidate_input
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if let Err(source) = NewsProcessConfiguration::try_new(
        candidate_input.news_process_configuration.intensity,
        candidate_input
            .news_process_configuration
            .gamma_severity_marks,
        candidate_input
            .news_process_configuration
            .double_exponential_base_shock,
        candidate_input.news_process_configuration.coupled_gap_marks,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "news_process_configuration".to_string(),
            message: source.to_string(),
        });
    }

    violations
}

fn return_components_stage_optimizer_bounds() -> Vec<CalibrationOptimizerParameterBound> {
    vec![CalibrationOptimizerParameterBound {
        parameter_name: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
        lower_inclusive: -f64::MAX,
        upper_inclusive: 0.0,
    }]
}

fn build_return_components_stage_handoff_outputs(
    run_spec: &CalibrationRunSpec,
) -> Vec<ReturnComponentsStageHandoffOutput> {
    run_spec
        .stage_handoffs
        .iter()
        .filter(|stage_handoff| stage_handoff.from_stage == CalibrationStage::ReturnComponents)
        .map(|stage_handoff| match stage_handoff.transport {
            CalibrationStageHandoffTransport::InMemoryOutput => {
                ReturnComponentsStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport,
                    in_memory_payload_available: true,
                    artifact_reference: None,
                }
            }
            CalibrationStageHandoffTransport::PersistedArtifactReference => {
                ReturnComponentsStageHandoffOutput {
                    from_stage: stage_handoff.from_stage,
                    to_stage: stage_handoff.to_stage,
                    transport: stage_handoff.transport,
                    in_memory_payload_available: false,
                    artifact_reference: stage_handoff.artifact_reference.clone(),
                }
            }
        })
        .collect()
}

fn build_return_components_stage_failure_report(
    run_spec: &CalibrationRunSpec,
    failure: ReturnComponentsStageExecutionFailure,
) -> ReturnComponentsStageExecutionReport {
    ReturnComponentsStageExecutionReport {
        schema_version: CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        stage: CalibrationStage::ReturnComponents,
        status: CalibrationStageExecutionStatus::Failure,
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        handoff_policy: CALIBRATION_RETURN_COMPONENTS_STAGE_HANDOFF_POLICY,
        handoffs: Vec::new(),
        fitted_stage_payload: None,
        metadata: None,
        failure: Some(failure),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationStageCheckpointMetadata {
    pub schema_version: String,
    pub checkpoint_reference_policy: String,
    pub disk_write_policy: String,
    pub artifact_manifest_schema_version: String,
    pub checkpoint_reference: String,
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub run_contract_schema_version: String,
    pub run_spec_schema_version: String,
    pub stage_graph_schema_version: String,
    pub execution_stage_order: Vec<CalibrationStage>,
    pub completed_stage: CalibrationStage,
    pub completed_stage_position: usize,
    pub completed_stage_output_schema_version: String,
    pub completed_stage_seed: u64,
    pub completed_stage_solver_settings: CalibrationOptimizerSettings,
}

impl CalibrationStageCheckpointMetadata {
    fn validate_compatibility_with_run_contract(
        &self,
        run_contract: &CalibrationRunContract,
    ) -> Result<(), CalibrationStageCheckpointCompatibilityError> {
        if self.schema_version != CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedCheckpointSchemaVersion {
                    expected: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
                    found: self.schema_version.clone(),
                },
            );
        }
        if self.checkpoint_reference_policy != CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedReferencePolicy {
                    expected: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
                    found: self.checkpoint_reference_policy.clone(),
                },
            );
        }
        if self.disk_write_policy != CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedDiskWritePolicy {
                    expected: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
                    found: self.disk_write_policy.clone(),
                },
            );
        }
        if self.artifact_manifest_schema_version != CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION {
            return Err(
                CalibrationStageCheckpointCompatibilityError::UnsupportedArtifactManifestSchemaVersion {
                    expected: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
                    found: self.artifact_manifest_schema_version.clone(),
                },
            );
        }
        if self.run_id != run_contract.run_spec.run_id {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunIdMismatch {
                    expected: run_contract.run_spec.run_id.clone(),
                    found: self.run_id.clone(),
                },
            );
        }
        if self.execution_mode != run_contract.run_spec.execution_mode {
            return Err(
                CalibrationStageCheckpointCompatibilityError::ExecutionModeMismatch {
                    expected: run_contract.run_spec.execution_mode,
                    found: self.execution_mode,
                },
            );
        }
        if self.run_contract_schema_version != run_contract.schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunContractSchemaVersionMismatch {
                    expected: run_contract.schema_version.clone(),
                    found: self.run_contract_schema_version.clone(),
                },
            );
        }
        if self.run_spec_schema_version != run_contract.run_spec_schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::RunSpecSchemaVersionMismatch {
                    expected: run_contract.run_spec_schema_version.clone(),
                    found: self.run_spec_schema_version.clone(),
                },
            );
        }
        if self.stage_graph_schema_version != run_contract.stage_graph_schema_version {
            return Err(
                CalibrationStageCheckpointCompatibilityError::StageGraphSchemaVersionMismatch {
                    expected: run_contract.stage_graph_schema_version.clone(),
                    found: self.stage_graph_schema_version.clone(),
                },
            );
        }
        if self.execution_stage_order != run_contract.execution_stage_order {
            return Err(
                CalibrationStageCheckpointCompatibilityError::ExecutionStageOrderMismatch {
                    expected: run_contract.execution_stage_order.clone(),
                    found: self.execution_stage_order.clone(),
                },
            );
        }

        let Some(expected_stage_position) = self
            .execution_stage_order
            .iter()
            .position(|stage| *stage == self.completed_stage)
        else {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStageIsNotInExecutionOrder {
                    completed_stage: self.completed_stage,
                },
            );
        };

        if self.completed_stage_position != expected_stage_position {
            return Err(
                CalibrationStageCheckpointCompatibilityError::CompletedStagePositionMismatch {
                    completed_stage: self.completed_stage,
                    expected_position: expected_stage_position,
                    found_position: self.completed_stage_position,
                },
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageCheckpointCompatibilityError {
    UnsupportedCheckpointSchemaVersion {
        expected: String,
        found: String,
    },
    UnsupportedReferencePolicy {
        expected: String,
        found: String,
    },
    UnsupportedDiskWritePolicy {
        expected: String,
        found: String,
    },
    UnsupportedArtifactManifestSchemaVersion {
        expected: String,
        found: String,
    },
    RunIdMismatch {
        expected: String,
        found: String,
    },
    ExecutionModeMismatch {
        expected: CalibrationExecutionMode,
        found: CalibrationExecutionMode,
    },
    RunContractSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    RunSpecSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    StageGraphSchemaVersionMismatch {
        expected: String,
        found: String,
    },
    ExecutionStageOrderMismatch {
        expected: Vec<CalibrationStage>,
        found: Vec<CalibrationStage>,
    },
    CompletedStageIsNotInExecutionOrder {
        completed_stage: CalibrationStage,
    },
    CompletedStagePositionMismatch {
        completed_stage: CalibrationStage,
        expected_position: usize,
        found_position: usize,
    },
}

impl Display for CalibrationStageCheckpointCompatibilityError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedCheckpointSchemaVersion { expected, found } => write!(
                formatter,
                "checkpoint schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedReferencePolicy { expected, found } => write!(
                formatter,
                "checkpoint reference policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedDiskWritePolicy { expected, found } => write!(
                formatter,
                "checkpoint disk write policy is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::UnsupportedArtifactManifestSchemaVersion { expected, found } => write!(
                formatter,
                "checkpoint artifact manifest schema is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunIdMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_id is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::ExecutionModeMismatch { expected, found } => write!(
                formatter,
                "checkpoint execution_mode is incompatible, expected `{expected:?}`, found `{found:?}`"
            ),
            Self::RunContractSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_contract schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::RunSpecSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint run_spec schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::StageGraphSchemaVersionMismatch { expected, found } => write!(
                formatter,
                "checkpoint stage_graph schema version is incompatible, expected `{expected}`, found `{found}`"
            ),
            Self::ExecutionStageOrderMismatch { expected, found } => write!(
                formatter,
                "checkpoint execution order is incompatible, expected `{expected:?}`, found `{found:?}`"
            ),
            Self::CompletedStageIsNotInExecutionOrder { completed_stage } => write!(
                formatter,
                "checkpoint completed_stage `{completed_stage:?}` is not present in checkpoint execution order"
            ),
            Self::CompletedStagePositionMismatch {
                completed_stage,
                expected_position,
                found_position,
            } => write!(
                formatter,
                "checkpoint completed_stage `{completed_stage:?}` is at unexpected position, expected `{expected_position}`, found `{found_position}`"
            ),
        }
    }
}

impl Error for CalibrationStageCheckpointCompatibilityError {}

#[derive(Debug, Clone, PartialEq, Default)]
struct CalibrationInMemoryStagePayloadState {
    seasonality_stage_payload: Option<SeasonalityStageNormalizedPayload>,
    hawkes_stage_payload: Option<HawkesStageFittedPayload>,
    return_components_stage_payload: Option<ReturnComponentsStageFittedPayload>,
}

#[derive(Debug, Clone, Default)]
struct CalibrationInMemoryCheckpointRunState {
    checkpoint_by_stage: BTreeMap<CalibrationStage, CalibrationStageCheckpointMetadata>,
    last_successful_stage: Option<CalibrationStage>,
    payload_state: CalibrationInMemoryStagePayloadState,
}

#[derive(Debug, Clone, Default)]
pub struct CalibrationMemoryCheckpointStore {
    run_state_by_id: BTreeMap<String, CalibrationInMemoryCheckpointRunState>,
}

#[derive(Debug, Clone)]
struct LoadedInMemoryCheckpointState {
    checkpoint_metadata: CalibrationStageCheckpointMetadata,
    payload_state: CalibrationInMemoryStagePayloadState,
}

impl CalibrationMemoryCheckpointStore {
    pub const fn new() -> Self {
        Self {
            run_state_by_id: BTreeMap::new(),
        }
    }

    pub fn checkpoint_count_for_run(&self, run_id: &str) -> usize {
        self.run_state_by_id
            .get(run_id)
            .map(|state| state.checkpoint_by_stage.len())
            .unwrap_or(0)
    }

    pub fn checkpoint_for_stage(
        &self,
        run_id: &str,
        stage: CalibrationStage,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        self.run_state_by_id
            .get(run_id)
            .and_then(|state| state.checkpoint_by_stage.get(&stage))
    }

    pub fn last_checkpoint_for_run(
        &self,
        run_id: &str,
    ) -> Option<&CalibrationStageCheckpointMetadata> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;

        run_state.checkpoint_by_stage.get(&last_stage)
    }

    fn load_resume_state(&self, run_id: &str) -> Option<LoadedInMemoryCheckpointState> {
        let run_state = self.run_state_by_id.get(run_id)?;
        let last_stage = run_state.last_successful_stage?;
        let checkpoint_metadata = run_state.checkpoint_by_stage.get(&last_stage)?.clone();

        Some(LoadedInMemoryCheckpointState {
            checkpoint_metadata,
            payload_state: run_state.payload_state.clone(),
        })
    }

    fn record_stage_checkpoint(
        &mut self,
        checkpoint_metadata: CalibrationStageCheckpointMetadata,
        payload_state: &CalibrationInMemoryStagePayloadState,
    ) {
        let run_state = self
            .run_state_by_id
            .entry(checkpoint_metadata.run_id.clone())
            .or_default();

        run_state.checkpoint_by_stage.insert(
            checkpoint_metadata.completed_stage,
            checkpoint_metadata.clone(),
        );
        run_state.last_successful_stage = Some(checkpoint_metadata.completed_stage);
        run_state.payload_state = payload_state.clone();
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalibrationStagedRunInputs {
    pub seasonality_stage_input: Option<SeasonalityStageExecutionInput>,
    pub hawkes_candidate_inputs: Option<Vec<HawkesStageCandidateInput>>,
    pub return_components_candidate_inputs: Option<Vec<ReturnComponentsStageCandidateInput>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "stage_report_kind",
    content = "report",
    rename_all = "snake_case"
)]
pub enum CalibrationStagedRunStageReport {
    Seasonality(SeasonalityStageExecutionReport),
    Hawkes(HawkesStageExecutionReport),
    ReturnComponents(ReturnComponentsStageExecutionReport),
}

impl CalibrationStagedRunStageReport {
    const fn stage(&self) -> CalibrationStage {
        match self {
            Self::Seasonality(_) => CalibrationStage::Seasonality,
            Self::Hawkes(_) => CalibrationStage::Hawkes,
            Self::ReturnComponents(_) => CalibrationStage::ReturnComponents,
        }
    }

    const fn status(&self) -> CalibrationStageExecutionStatus {
        match self {
            Self::Seasonality(report) => report.status,
            Self::Hawkes(report) => report.status,
            Self::ReturnComponents(report) => report.status,
        }
    }

    fn checkpoint_components(&self) -> Option<(String, u64, CalibrationOptimizerSettings)> {
        match self {
            Self::Seasonality(report) => report.metadata.as_ref().map(|metadata| {
                (
                    metadata.stage_output_schema_version.to_string(),
                    metadata.stage_seed,
                    metadata.solver_settings.clone(),
                )
            }),
            Self::Hawkes(report) => report.metadata.as_ref().map(|metadata| {
                (
                    metadata.stage_output_schema_version.to_string(),
                    metadata.stage_seed,
                    metadata.solver_settings.clone(),
                )
            }),
            Self::ReturnComponents(report) => report.metadata.as_ref().map(|metadata| {
                (
                    metadata.stage_output_schema_version.to_string(),
                    metadata.stage_seed,
                    metadata.solver_settings.clone(),
                )
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "failure_kind", rename_all = "snake_case")]
pub enum CalibrationStagedRunFailure {
    InvalidRunContract {
        error_message: String,
    },
    MissingMemoryCheckpointStore,
    MissingDiskCheckpointDirectory,
    MissingResumeCheckpointForRun {
        run_id: String,
    },
    MissingStageInput {
        stage: CalibrationStage,
    },
    MissingInMemoryUpstreamPayload {
        required_stage: CalibrationStage,
        consuming_stage: CalibrationStage,
    },
    InvalidCheckpointReference {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointLoadFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointCompatibilityFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    CheckpointPersistFailed {
        checkpoint_reference: String,
        error_message: String,
    },
    MissingCheckpointMetadataForSuccessfulStage {
        stage: CalibrationStage,
    },
    StageExecutionFailed {
        stage: CalibrationStage,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationStagedRunReport {
    pub run_id: String,
    pub execution_mode: CalibrationExecutionMode,
    pub status: CalibrationStageExecutionStatus,
    pub stage_reports: Vec<CalibrationStagedRunStageReport>,
    pub emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_checkpoint_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_from_last_successful_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_from_stage: Option<CalibrationStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<CalibrationStagedRunFailure>,
}

pub fn calibration_checkpoint_reference_for_stage(run_id: &str, stage: CalibrationStage) -> String {
    format!("checkpoint://{run_id}/{}", calibration_stage_token(stage))
}

pub fn calibration_last_success_checkpoint_reference(run_id: &str) -> String {
    format!("checkpoint://{run_id}/last-success")
}

/// Execute staged calibration with deterministic checkpoint emission after each successful stage.
///
/// Resume behavior is mode-aware by contract:
/// - `execution_mode=memory` supports resume only through `CalibrationMemoryCheckpointStore`
/// - `execution_mode=disk` supports cross-process resume from persisted checkpoint references
pub fn run_staged_calibration_with_checkpointing(
    run_contract: &CalibrationRunContract,
    calendar_registry: &ExchangeCalendarRegistry,
    mut staged_run_inputs: CalibrationStagedRunInputs,
    mut memory_checkpoint_store: Option<&mut CalibrationMemoryCheckpointStore>,
    disk_checkpoint_directory: Option<&Path>,
) -> CalibrationStagedRunReport {
    if let Err(error_message) = validate_run_contract_for_staged_execution(run_contract) {
        return build_staged_run_failure_report(
            run_contract,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            CalibrationStagedRunFailure::InvalidRunContract { error_message },
        );
    }

    let run_spec = &run_contract.run_spec;
    if run_spec.execution_mode == CalibrationExecutionMode::Disk
        && disk_checkpoint_directory.is_none()
    {
        return build_staged_run_failure_report(
            run_contract,
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
            CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
        );
    }

    let execution_stage_order = run_contract.execution_stage_order.clone();

    let mut stage_reports = Vec::new();
    let mut emitted_checkpoints = Vec::new();
    let mut payload_state = CalibrationInMemoryStagePayloadState::default();
    let mut resumed_from_checkpoint_reference = None;
    let mut resumed_from_last_successful_stage = None;
    let mut start_position = 0usize;

    if let Some(resume_settings) = &run_spec.resume_settings {
        let loaded_checkpoint = match resume_settings.source {
            CalibrationResumeSource::SameProcess => {
                let Some(checkpoint_store) = memory_checkpoint_store.as_deref() else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingMemoryCheckpointStore,
                    );
                };

                let Some(loaded_state) = checkpoint_store.load_resume_state(&run_spec.run_id)
                else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingResumeCheckpointForRun {
                            run_id: run_spec.run_id.clone(),
                        },
                    );
                };

                loaded_state
            }
            CalibrationResumeSource::PersistedCheckpointReference => {
                let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        None,
                        None,
                        None,
                        CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
                    );
                };
                let checkpoint_reference = resume_settings
                    .checkpoint_reference
                    .as_deref()
                    .unwrap_or_default()
                    .to_string();
                let checkpoint_metadata = match load_stage_checkpoint_from_disk(
                    disk_checkpoint_directory,
                    checkpoint_reference.as_str(),
                ) {
                    Ok(checkpoint_metadata) => checkpoint_metadata,
                    Err(CalibrationDiskCheckpointError::InvalidCheckpointReference {
                        checkpoint_reference,
                        source,
                    }) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            None,
                            None,
                            None,
                            CalibrationStagedRunFailure::InvalidCheckpointReference {
                                checkpoint_reference,
                                error_message: source.to_string(),
                            },
                        );
                    }
                    Err(source) => {
                        return build_staged_run_failure_report(
                            run_contract,
                            stage_reports,
                            emitted_checkpoints,
                            None,
                            None,
                            None,
                            CalibrationStagedRunFailure::CheckpointLoadFailed {
                                checkpoint_reference,
                                error_message: source.to_string(),
                            },
                        );
                    }
                };

                LoadedInMemoryCheckpointState {
                    checkpoint_metadata,
                    payload_state: CalibrationInMemoryStagePayloadState::default(),
                }
            }
        };

        if let Err(source) = loaded_checkpoint
            .checkpoint_metadata
            .validate_compatibility_with_run_contract(run_contract)
        {
            let resume_reference = resume_settings
                .checkpoint_reference
                .as_deref()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    loaded_checkpoint
                        .checkpoint_metadata
                        .checkpoint_reference
                        .clone()
                });

            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                None,
                None,
                None,
                CalibrationStagedRunFailure::CheckpointCompatibilityFailed {
                    checkpoint_reference: resume_reference,
                    error_message: source.to_string(),
                },
            );
        }

        resumed_from_checkpoint_reference = Some(
            resume_settings
                .checkpoint_reference
                .as_deref()
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    loaded_checkpoint
                        .checkpoint_metadata
                        .checkpoint_reference
                        .clone()
                }),
        );
        resumed_from_last_successful_stage =
            Some(loaded_checkpoint.checkpoint_metadata.completed_stage);
        payload_state = loaded_checkpoint.payload_state;

        start_position = if let Some(resume_from_stage) = resume_settings.resume_from_stage {
            let Some(position) = execution_stage_order
                .iter()
                .position(|stage| *stage == resume_from_stage)
            else {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    None,
                    CalibrationStagedRunFailure::CheckpointCompatibilityFailed {
                        checkpoint_reference: calibration_last_success_checkpoint_reference(
                            run_spec.run_id.as_str(),
                        ),
                        error_message: format!(
                            "resume_from_stage `{resume_from_stage:?}` is not in execution order `{:?}`",
                            execution_stage_order
                        ),
                    },
                );
            };

            position
        } else {
            loaded_checkpoint
                .checkpoint_metadata
                .completed_stage_position
                .saturating_add(1)
        };
    }

    if start_position >= execution_stage_order.len() {
        return CalibrationStagedRunReport {
            run_id: run_spec.run_id.clone(),
            execution_mode: run_spec.execution_mode,
            status: CalibrationStageExecutionStatus::Success,
            stage_reports,
            emitted_checkpoints,
            resumed_from_checkpoint_reference,
            resumed_from_last_successful_stage,
            started_from_stage: None,
            failure: None,
        };
    }

    let started_from_stage = execution_stage_order.get(start_position).copied();
    for (stage_position, stage) in execution_stage_order
        .iter()
        .copied()
        .enumerate()
        .skip(start_position)
    {
        let stage_report = match run_stage_for_staged_execution(
            run_spec,
            calendar_registry,
            stage,
            &mut staged_run_inputs,
            &payload_state,
        ) {
            Ok(stage_report) => stage_report,
            Err(failure) => {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    failure,
                );
            }
        };

        let stage_status = stage_report.status();
        stage_reports.push(stage_report.clone());

        if stage_status == CalibrationStageExecutionStatus::Failure {
            return build_staged_run_failure_report(
                run_contract,
                stage_reports,
                emitted_checkpoints,
                resumed_from_checkpoint_reference,
                resumed_from_last_successful_stage,
                started_from_stage,
                CalibrationStagedRunFailure::StageExecutionFailed { stage },
            );
        }

        update_in_memory_payload_state(&mut payload_state, &stage_report);

        let checkpoint_metadata =
            match build_stage_checkpoint_metadata(run_contract, &stage_report, stage_position) {
                Ok(checkpoint_metadata) => checkpoint_metadata,
                Err(failure) => {
                    return build_staged_run_failure_report(
                        run_contract,
                        stage_reports,
                        emitted_checkpoints,
                        resumed_from_checkpoint_reference,
                        resumed_from_last_successful_stage,
                        started_from_stage,
                        failure,
                    );
                }
            };

        if run_spec.execution_mode == CalibrationExecutionMode::Memory {
            if let Some(checkpoint_store) = memory_checkpoint_store.as_deref_mut() {
                checkpoint_store
                    .record_stage_checkpoint(checkpoint_metadata.clone(), &payload_state);
            }
        } else {
            let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::MissingDiskCheckpointDirectory,
                );
            };

            if let Err(source) =
                persist_stage_checkpoint_to_disk(disk_checkpoint_directory, &checkpoint_metadata)
            {
                return build_staged_run_failure_report(
                    run_contract,
                    stage_reports,
                    emitted_checkpoints,
                    resumed_from_checkpoint_reference,
                    resumed_from_last_successful_stage,
                    started_from_stage,
                    CalibrationStagedRunFailure::CheckpointPersistFailed {
                        checkpoint_reference: checkpoint_metadata.checkpoint_reference.clone(),
                        error_message: source.to_string(),
                    },
                );
            }
        }

        emitted_checkpoints.push(checkpoint_metadata);
    }

    CalibrationStagedRunReport {
        run_id: run_spec.run_id.clone(),
        execution_mode: run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Success,
        stage_reports,
        emitted_checkpoints,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: None,
    }
}

fn validate_run_contract_for_staged_execution(
    run_contract: &CalibrationRunContract,
) -> Result<(), String> {
    if run_contract.schema_version != CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.schema_version must be `{}`, found `{}`",
            CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, run_contract.schema_version
        ));
    }
    if run_contract.run_spec_schema_version != CALIBRATION_RUN_SPEC_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.run_spec_schema_version must be `{}`, found `{}`",
            CALIBRATION_RUN_SPEC_SCHEMA_VERSION, run_contract.run_spec_schema_version
        ));
    }
    if run_contract.stage_graph_schema_version != CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION {
        return Err(format!(
            "run_contract.stage_graph_schema_version must be `{}`, found `{}`",
            CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, run_contract.stage_graph_schema_version
        ));
    }

    let resolved_execution_order = resolve_calibration_stage_execution_order(
        &run_contract.run_spec,
        &run_contract.stage_graph,
    )
    .map_err(|source| source.to_string())?;
    if resolved_execution_order != run_contract.execution_stage_order {
        return Err(format!(
            "run_contract.execution_stage_order is stale, expected `{:?}`, found `{:?}`",
            resolved_execution_order, run_contract.execution_stage_order
        ));
    }

    Ok(())
}

fn run_stage_for_staged_execution(
    run_spec: &CalibrationRunSpec,
    calendar_registry: &ExchangeCalendarRegistry,
    stage: CalibrationStage,
    staged_run_inputs: &mut CalibrationStagedRunInputs,
    payload_state: &CalibrationInMemoryStagePayloadState,
) -> Result<CalibrationStagedRunStageReport, CalibrationStagedRunFailure> {
    match stage {
        CalibrationStage::Seasonality => {
            let Some(seasonality_stage_input) = staged_run_inputs.seasonality_stage_input.take()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Seasonality,
                });
            };

            Ok(CalibrationStagedRunStageReport::Seasonality(
                run_seasonality_calibration_stage(
                    run_spec,
                    calendar_registry,
                    seasonality_stage_input,
                ),
            ))
        }
        CalibrationStage::Hawkes => {
            let Some(candidate_inputs) = staged_run_inputs.hawkes_candidate_inputs.take() else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::Hawkes,
                });
            };
            let seasonality_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
            );
            let seasonality_input = match seasonality_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(seasonality_stage_payload) =
                        payload_state.seasonality_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Seasonality,
                                consuming_stage: CalibrationStage::Hawkes,
                            },
                        );
                    };

                    HawkesStageSeasonalityInput::InMemoryOutput {
                        payload: seasonality_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    HawkesStageSeasonalityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Seasonality,
                            CalibrationStage::Hawkes,
                        ),
                    }
                }
            };

            Ok(CalibrationStagedRunStageReport::Hawkes(
                run_hawkes_calibration_stage(
                    run_spec,
                    HawkesStageExecutionInput {
                        seasonality_input,
                        candidate_inputs,
                    },
                ),
            ))
        }
        CalibrationStage::ReturnComponents => {
            let Some(candidate_inputs) =
                staged_run_inputs.return_components_candidate_inputs.take()
            else {
                return Err(CalibrationStagedRunFailure::MissingStageInput {
                    stage: CalibrationStage::ReturnComponents,
                });
            };

            let activity_input_transport = expected_stage_handoff_transport(
                run_spec,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            );

            let activity_input = match activity_input_transport {
                CalibrationStageHandoffTransport::InMemoryOutput => {
                    let Some(hawkes_stage_payload) = payload_state.hawkes_stage_payload.clone()
                    else {
                        return Err(
                            CalibrationStagedRunFailure::MissingInMemoryUpstreamPayload {
                                required_stage: CalibrationStage::Hawkes,
                                consuming_stage: CalibrationStage::ReturnComponents,
                            },
                        );
                    };

                    ReturnComponentsStageActivityInput::InMemoryOutput {
                        payload: hawkes_stage_payload,
                    }
                }
                CalibrationStageHandoffTransport::PersistedArtifactReference => {
                    ReturnComponentsStageActivityInput::PersistedArtifactReference {
                        artifact_reference: expected_stage_handoff_artifact_reference(
                            run_spec,
                            CalibrationStage::Hawkes,
                            CalibrationStage::ReturnComponents,
                        ),
                    }
                }
            };

            Ok(CalibrationStagedRunStageReport::ReturnComponents(
                run_return_components_calibration_stage(
                    run_spec,
                    ReturnComponentsStageExecutionInput {
                        activity_input,
                        candidate_inputs,
                    },
                ),
            ))
        }
    }
}

fn expected_stage_handoff_transport(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> CalibrationStageHandoffTransport {
    run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .map(|handoff| handoff.transport)
        .unwrap_or(match run_spec.execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoffTransport::InMemoryOutput,
            CalibrationExecutionMode::Disk => {
                CalibrationStageHandoffTransport::PersistedArtifactReference
            }
        })
}

fn expected_stage_handoff_artifact_reference(
    run_spec: &CalibrationRunSpec,
    from_stage: CalibrationStage,
    to_stage: CalibrationStage,
) -> String {
    if let Some(artifact_reference) = run_spec
        .stage_handoffs
        .iter()
        .find(|handoff| handoff.from_stage == from_stage && handoff.to_stage == to_stage)
        .and_then(|handoff| handoff.artifact_reference.as_ref())
        && !artifact_reference.trim().is_empty()
    {
        return artifact_reference.clone();
    }

    format!(
        "artifact://{}/{}",
        calibration_stage_token(from_stage),
        run_spec.run_id
    )
}

fn update_in_memory_payload_state(
    payload_state: &mut CalibrationInMemoryStagePayloadState,
    stage_report: &CalibrationStagedRunStageReport,
) {
    match stage_report {
        CalibrationStagedRunStageReport::Seasonality(report) => {
            payload_state.seasonality_stage_payload = report.normalized_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::Hawkes(report) => {
            payload_state.hawkes_stage_payload = report.fitted_stage_payload.clone();
        }
        CalibrationStagedRunStageReport::ReturnComponents(report) => {
            payload_state.return_components_stage_payload = report.fitted_stage_payload.clone();
        }
    }
}

fn build_stage_checkpoint_metadata(
    run_contract: &CalibrationRunContract,
    stage_report: &CalibrationStagedRunStageReport,
    stage_position: usize,
) -> Result<CalibrationStageCheckpointMetadata, CalibrationStagedRunFailure> {
    let stage = stage_report.stage();
    let Some((stage_output_schema_version, stage_seed, stage_solver_settings)) =
        stage_report.checkpoint_components()
    else {
        return Err(
            CalibrationStagedRunFailure::MissingCheckpointMetadataForSuccessfulStage { stage },
        );
    };

    Ok(CalibrationStageCheckpointMetadata {
        schema_version: CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION.to_string(),
        checkpoint_reference_policy: CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY.to_string(),
        disk_write_policy: CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY.to_string(),
        artifact_manifest_schema_version: CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION.to_string(),
        checkpoint_reference: calibration_checkpoint_reference_for_stage(
            run_contract.run_spec.run_id.as_str(),
            stage,
        ),
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        run_contract_schema_version: run_contract.schema_version.clone(),
        run_spec_schema_version: run_contract.run_spec_schema_version.clone(),
        stage_graph_schema_version: run_contract.stage_graph_schema_version.clone(),
        execution_stage_order: run_contract.execution_stage_order.clone(),
        completed_stage: stage,
        completed_stage_position: stage_position,
        completed_stage_output_schema_version: stage_output_schema_version,
        completed_stage_seed: stage_seed,
        completed_stage_solver_settings: stage_solver_settings,
    })
}

fn build_staged_run_failure_report(
    run_contract: &CalibrationRunContract,
    stage_reports: Vec<CalibrationStagedRunStageReport>,
    emitted_checkpoints: Vec<CalibrationStageCheckpointMetadata>,
    resumed_from_checkpoint_reference: Option<String>,
    resumed_from_last_successful_stage: Option<CalibrationStage>,
    started_from_stage: Option<CalibrationStage>,
    failure: CalibrationStagedRunFailure,
) -> CalibrationStagedRunReport {
    CalibrationStagedRunReport {
        run_id: run_contract.run_spec.run_id.clone(),
        execution_mode: run_contract.run_spec.execution_mode,
        status: CalibrationStageExecutionStatus::Failure,
        stage_reports,
        emitted_checkpoints,
        resumed_from_checkpoint_reference,
        resumed_from_last_successful_stage,
        started_from_stage,
        failure: Some(failure),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CalibrationCheckpointReferenceTarget {
    LastSuccess,
    Stage(CalibrationStage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedCalibrationCheckpointReference {
    run_id: String,
    target: CalibrationCheckpointReferenceTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CalibrationCheckpointReferenceParseError {
    MissingPrefix,
    MissingRunId,
    MissingTarget,
    UnknownTarget { token: String },
    UnexpectedPathSegments,
}

impl Display for CalibrationCheckpointReferenceParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingPrefix => write!(
                formatter,
                "checkpoint reference must use `checkpoint://` prefix"
            ),
            Self::MissingRunId => write!(
                formatter,
                "checkpoint reference must include a non-empty run_id segment"
            ),
            Self::MissingTarget => write!(
                formatter,
                "checkpoint reference must include a stage token or `last-success`"
            ),
            Self::UnknownTarget { token } => write!(
                formatter,
                "checkpoint reference target `{token}` is unsupported"
            ),
            Self::UnexpectedPathSegments => write!(
                formatter,
                "checkpoint reference must have exactly two path segments after `checkpoint://`"
            ),
        }
    }
}

impl Error for CalibrationCheckpointReferenceParseError {}

fn parse_checkpoint_reference(
    checkpoint_reference: &str,
) -> Result<ParsedCalibrationCheckpointReference, CalibrationCheckpointReferenceParseError> {
    let Some(without_prefix) = checkpoint_reference.strip_prefix("checkpoint://") else {
        return Err(CalibrationCheckpointReferenceParseError::MissingPrefix);
    };

    let mut segments = without_prefix.split('/');
    let Some(run_id) = segments.next() else {
        return Err(CalibrationCheckpointReferenceParseError::MissingRunId);
    };
    if run_id.trim().is_empty() {
        return Err(CalibrationCheckpointReferenceParseError::MissingRunId);
    }

    let Some(target_segment) = segments.next() else {
        return Err(CalibrationCheckpointReferenceParseError::MissingTarget);
    };
    if target_segment.trim().is_empty() {
        return Err(CalibrationCheckpointReferenceParseError::MissingTarget);
    }
    if segments.next().is_some() {
        return Err(CalibrationCheckpointReferenceParseError::UnexpectedPathSegments);
    }

    let target = if target_segment == "last-success" {
        CalibrationCheckpointReferenceTarget::LastSuccess
    } else if let Some(stage) = calibration_stage_from_token(target_segment) {
        CalibrationCheckpointReferenceTarget::Stage(stage)
    } else {
        return Err(CalibrationCheckpointReferenceParseError::UnknownTarget {
            token: target_segment.to_string(),
        });
    };

    Ok(ParsedCalibrationCheckpointReference {
        run_id: run_id.to_string(),
        target,
    })
}

#[derive(Debug)]
enum CalibrationDiskCheckpointError {
    InvalidCheckpointReference {
        checkpoint_reference: String,
        source: CalibrationCheckpointReferenceParseError,
    },
    CreateDirectoryFailed {
        path: String,
        error_message: String,
    },
    ReadFailed {
        path: String,
        error_message: String,
    },
    WriteFailed {
        path: String,
        error_message: String,
    },
    RenameFailed {
        source_path: String,
        target_path: String,
        error_message: String,
    },
    SerializationFailed {
        error_message: String,
    },
    DeserializationFailed {
        path: String,
        error_message: String,
    },
}

impl Display for CalibrationDiskCheckpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCheckpointReference {
                checkpoint_reference,
                source,
            } => write!(
                formatter,
                "invalid checkpoint reference `{checkpoint_reference}`: {source}"
            ),
            Self::CreateDirectoryFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to create checkpoint directory `{path}`: {error_message}"
            ),
            Self::ReadFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to read checkpoint file `{path}`: {error_message}"
            ),
            Self::WriteFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to write checkpoint file `{path}`: {error_message}"
            ),
            Self::RenameFailed {
                source_path,
                target_path,
                error_message,
            } => write!(
                formatter,
                "failed to atomically rename checkpoint file `{source_path}` -> `{target_path}`: {error_message}"
            ),
            Self::SerializationFailed { error_message } => {
                write!(formatter, "failed to serialize checkpoint: {error_message}")
            }
            Self::DeserializationFailed {
                path,
                error_message,
            } => write!(
                formatter,
                "failed to deserialize checkpoint file `{path}`: {error_message}"
            ),
        }
    }
}

impl Error for CalibrationDiskCheckpointError {}

fn persist_stage_checkpoint_to_disk(
    disk_checkpoint_directory: &Path,
    checkpoint_metadata: &CalibrationStageCheckpointMetadata,
) -> Result<(), CalibrationDiskCheckpointError> {
    let run_directory = disk_checkpoint_directory.join(checkpoint_metadata.run_id.as_str());
    fs::create_dir_all(&run_directory).map_err(|source| {
        CalibrationDiskCheckpointError::CreateDirectoryFailed {
            path: run_directory.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    let stage_checkpoint_path = run_directory.join(format!(
        "{}.checkpoint.json",
        calibration_stage_token(checkpoint_metadata.completed_stage)
    ));
    write_checkpoint_file_atomically(&stage_checkpoint_path, checkpoint_metadata)?;

    let last_success_checkpoint_path = run_directory.join("last-success.checkpoint.json");
    write_checkpoint_file_atomically(&last_success_checkpoint_path, checkpoint_metadata)?;

    Ok(())
}

fn load_stage_checkpoint_from_disk(
    disk_checkpoint_directory: &Path,
    checkpoint_reference: &str,
) -> Result<CalibrationStageCheckpointMetadata, CalibrationDiskCheckpointError> {
    let parsed_reference = parse_checkpoint_reference(checkpoint_reference).map_err(|source| {
        CalibrationDiskCheckpointError::InvalidCheckpointReference {
            checkpoint_reference: checkpoint_reference.to_string(),
            source,
        }
    })?;

    let run_directory = disk_checkpoint_directory.join(parsed_reference.run_id.as_str());
    let checkpoint_path = match parsed_reference.target {
        CalibrationCheckpointReferenceTarget::LastSuccess => {
            run_directory.join("last-success.checkpoint.json")
        }
        CalibrationCheckpointReferenceTarget::Stage(stage) => run_directory.join(format!(
            "{}.checkpoint.json",
            calibration_stage_token(stage)
        )),
    };

    let content = fs::read_to_string(&checkpoint_path).map_err(|source| {
        CalibrationDiskCheckpointError::ReadFailed {
            path: checkpoint_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    serde_json::from_str(content.as_str()).map_err(|source| {
        CalibrationDiskCheckpointError::DeserializationFailed {
            path: checkpoint_path.display().to_string(),
            error_message: source.to_string(),
        }
    })
}

fn write_checkpoint_file_atomically(
    target_path: &Path,
    checkpoint_metadata: &CalibrationStageCheckpointMetadata,
) -> Result<(), CalibrationDiskCheckpointError> {
    let serialized = serde_json::to_vec_pretty(checkpoint_metadata).map_err(|source| {
        CalibrationDiskCheckpointError::SerializationFailed {
            error_message: source.to_string(),
        }
    })?;

    let temporary_path = target_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path).map_err(|source| {
        CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;
    temporary_file
        .write_all(serialized.as_slice())
        .map_err(|source| CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        })?;
    temporary_file
        .sync_all()
        .map_err(|source| CalibrationDiskCheckpointError::WriteFailed {
            path: temporary_path.display().to_string(),
            error_message: source.to_string(),
        })?;

    fs::rename(&temporary_path, target_path).map_err(|source| {
        CalibrationDiskCheckpointError::RenameFailed {
            source_path: temporary_path.display().to_string(),
            target_path: target_path.display().to_string(),
            error_message: source.to_string(),
        }
    })?;

    Ok(())
}

fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
    }
}

fn calibration_stage_from_token(token: &str) -> Option<CalibrationStage> {
    match token {
        "seasonality" => Some(CalibrationStage::Seasonality),
        "hawkes" => Some(CalibrationStage::Hawkes),
        "return_components" => Some(CalibrationStage::ReturnComponents),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{TimeZone, Utc};

    use crate::calendar::{
        ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, TimestampEventInputRecord,
    };
    use crate::calibration::{
        CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION,
        CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION,
        CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY, CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
        CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION,
        CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
        CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION,
        CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION,
        CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY,
        CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
        CalibrationCallerSurface, CalibrationExecutionMode, CalibrationMemoryCheckpointStore,
        CalibrationObjectiveTermInput, CalibrationOptimizerAdapterCandidateInput,
        CalibrationOptimizerAdapterConfiguration, CalibrationOptimizerAdapterError,
        CalibrationOptimizerCandidateParameterValue, CalibrationOptimizerParameterBound,
        CalibrationOptimizerSettings, CalibrationResumeSettings, CalibrationResumeSettingsError,
        CalibrationResumeSource, CalibrationRunContract, CalibrationRunContractError,
        CalibrationRunDataInputs, CalibrationRunSeedSettings, CalibrationRunSpec,
        CalibrationRunSpecError, CalibrationRunSpecErrorClassification, CalibrationStage,
        CalibrationStageDependency, CalibrationStageExecutionStatus, CalibrationStageGraph,
        CalibrationStageGraphError, CalibrationStageHandoff, CalibrationStageHandoffError,
        CalibrationStageHandoffTransport, CalibrationStageSeedOverride,
        CalibrationStageSolverSettings, CalibrationStagedRunFailure, CalibrationStagedRunInputs,
        HawkesStageCandidateInput, HawkesStageExecutionFailure, HawkesStageExecutionInput,
        HawkesStageObjectiveTermInput, OptionSurfaceCalibrationObservationError,
        ReturnComponentsStageCandidateInput, ReturnComponentsStageExecutionFailure,
        ReturnComponentsStageExecutionInput, ReturnComponentsStageObjectiveTermInput,
        SeasonalityStageExecutionFailure, SeasonalityStageExecutionInput,
        build_option_surface_calibration_observation_from_pricing_hooks,
        calibration_checkpoint_reference_for_stage, calibration_last_success_checkpoint_reference,
        compose_calibration_weighted_objective, default_calibration_execution_mode_for_surface,
        evaluate_calibration_martingale_condition_under_esscher_tilt,
        precheck_hawkes_kernel_parameter_domain,
        precheck_structured_hawkes_kernel_parameter_domain,
        resolve_calibration_execution_mode_for_surface, resolve_calibration_stage_execution_order,
        run_constrained_optimizer_adapter, run_hawkes_calibration_stage,
        run_return_components_calibration_stage, run_seasonality_calibration_stage,
        run_staged_calibration_with_checkpointing,
        transform_nig_exponent_for_calibration_under_esscher_tilt,
    };
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
        HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };
    use crate::news::{
        CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
        GammaSeverityMarkConfiguration, NewsIntensityConfiguration, NewsProcessConfiguration,
    };
    use crate::nig::{ComplexScalar, NigParameters, NigSharedPlusShiftConfiguration};
    use crate::pricing::VanillaOptionPricingCalibrationHooks;
    use crate::risk_neutral::{EsscherMartingaleConditionInput, NigEsscherTransformationInput};
    use serde_json::json;

    #[test]
    fn calibration_domain_precheck_report_shape_is_compatible_with_optimizer_consumers() {
        let report = precheck_hawkes_kernel_parameter_domain(
            0.85,
            1.25,
            HawkesKernelLoadings {
                plus_plus: 0.6,
                plus_minus: 0.1,
                minus_plus: 0.2,
                minus_minus: 0.7,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.05,
                minus_plus: 0.12,
                minus_minus: 0.0,
            },
        );

        assert_eq!(
            report.schema_version,
            HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(report.status, HawkesKernelValidationStatus::Pass);
        assert_eq!(report.summary.failed_check_count, 0);
        assert_eq!(
            report.summary.passed_check_count,
            report.summary.total_check_count
        );
        assert!(report.violations.is_empty());
    }

    #[test]
    fn calibration_structured_precheck_report_exposes_deterministic_failure_order() {
        let report = precheck_structured_hawkes_kernel_parameter_domain(
            0.0,
            -1.0,
            HawkesStructuredAlphaParameterization::new(0.0, -0.1, -0.1, -0.2),
            HawkesStructuredRhoParameterization::new(0.0, -0.2, -0.2, -0.3),
            Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
            -1.0,
        );

        assert_eq!(report.status, HawkesKernelValidationStatus::Fail);
        assert_eq!(
            report
                .violations
                .iter()
                .map(|violation| violation.field)
                .collect::<Vec<_>>(),
            vec![
                "lambda_bar",
                "beta",
                "structured_alpha.a_self",
                "structured_alpha.a_cross",
                "structured_alpha.delta_self",
                "structured_alpha.delta_cross",
                "alpha.plus_minus",
                "alpha.minus_plus",
                "alpha.minus_minus",
                "structured_rho.r_self",
                "structured_rho.r_cross",
                "structured_rho.delta_self_r",
                "structured_rho.delta_cross_r",
                "rho.plus_minus",
                "rho.minus_plus",
                "rho.minus_minus",
                "structured_rho.self_dominance_policy.minimum_self_to_cross_ratio",
                "stability.numeric_tolerance",
            ]
        );
    }

    #[test]
    fn objective_composition_reports_weighted_sum_and_per_term_diagnostics() {
        let objective_terms = vec![
            CalibrationObjectiveTermInput {
                term_id: "tail_fit".to_string(),
                weight: 2.0,
                observed_score: 0.25,
            },
            CalibrationObjectiveTermInput {
                term_id: "activity_clustering".to_string(),
                weight: 1.5,
                observed_score: 0.4,
            },
        ];

        let report = compose_calibration_weighted_objective(objective_terms.as_slice())
            .expect("objective composition should succeed");

        assert_eq!(
            report.schema_version,
            CALIBRATION_OBJECTIVE_COMPOSITION_SCHEMA_VERSION
        );
        assert_eq!(report.objective_terms.len(), 2);
        assert!((report.objective_terms[0].weighted_contribution - 0.5).abs() < 1e-12);
        assert!((report.objective_terms[1].weighted_contribution - 0.6).abs() < 1e-12);
        assert!((report.weighted_objective_value - 1.1).abs() < 1e-12);
    }

    #[test]
    fn constrained_optimizer_adapter_configuration_and_failure_paths_are_validated() {
        let solver_settings = CalibrationOptimizerSettings::try_new("lbfgs", 25, 1e-8, 1e-6, 10.0)
            .expect("solver settings should validate");
        let duplicate_bounds = vec![
            CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
                .expect("first bound should validate"),
            CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
                .expect("second bound should validate"),
        ];

        let duplicate_bound_error = CalibrationOptimizerAdapterConfiguration::try_new(
            "candidate_enumeration.v1",
            7,
            solver_settings.clone(),
            duplicate_bounds,
        )
        .expect_err("duplicate parameter bounds must be rejected");
        assert!(matches!(
            duplicate_bound_error,
            crate::calibration::CalibrationOptimizerAdapterConfigurationError::DuplicateParameterBound { .. }
        ));

        let valid_configuration = CalibrationOptimizerAdapterConfiguration::try_new(
            "candidate_enumeration.v1",
            7,
            solver_settings,
            vec![
                CalibrationOptimizerParameterBound::try_new("lambda_bar", 0.0, 10.0)
                    .expect("bound should validate"),
            ],
        )
        .expect("configuration should validate");

        let missing_parameter_error = run_constrained_optimizer_adapter(
            &valid_configuration,
            &[CalibrationOptimizerAdapterCandidateInput {
                candidate_id: "candidate_001".to_string(),
                objective_terms: vec![CalibrationObjectiveTermInput {
                    term_id: "objective_1".to_string(),
                    weight: 1.0,
                    observed_score: 0.5,
                }],
                parameter_values: Vec::new(),
                external_constraint_violation_count: 0,
            }],
        )
        .expect_err("missing parameter value for configured bound must fail");

        assert!(matches!(
            missing_parameter_error,
            CalibrationOptimizerAdapterError::MissingBoundedParameterValue { .. }
        ));
    }

    #[test]
    fn constrained_optimizer_adapter_is_deterministic_for_fixed_seed() {
        let configuration = CalibrationOptimizerAdapterConfiguration::try_new(
            "candidate_enumeration.v1",
            991,
            CalibrationOptimizerSettings::try_new("lbfgs", 50, 1e-8, 1e-6, 2.0)
                .expect("solver settings should validate"),
            vec![
                CalibrationOptimizerParameterBound::try_new("beta", 0.0, 10.0)
                    .expect("bound should validate"),
            ],
        )
        .expect("configuration should validate");

        let tie_candidates = vec![
            CalibrationOptimizerAdapterCandidateInput {
                candidate_id: "candidate_a".to_string(),
                objective_terms: vec![CalibrationObjectiveTermInput {
                    term_id: "objective".to_string(),
                    weight: 1.0,
                    observed_score: 1.0,
                }],
                parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "beta".to_string(),
                    value: 1.0,
                }],
                external_constraint_violation_count: 0,
            },
            CalibrationOptimizerAdapterCandidateInput {
                candidate_id: "candidate_b".to_string(),
                objective_terms: vec![CalibrationObjectiveTermInput {
                    term_id: "objective".to_string(),
                    weight: 1.0,
                    observed_score: 1.0,
                }],
                parameter_values: vec![CalibrationOptimizerCandidateParameterValue {
                    parameter_name: "beta".to_string(),
                    value: 1.0,
                }],
                external_constraint_violation_count: 0,
            },
        ];

        let first_output =
            run_constrained_optimizer_adapter(&configuration, tie_candidates.as_slice())
                .expect("optimizer adapter run should succeed");
        let second_output =
            run_constrained_optimizer_adapter(&configuration, tie_candidates.as_slice())
                .expect("second optimizer adapter run should succeed");

        assert_eq!(
            first_output.metadata.schema_version,
            CALIBRATION_CONSTRAINED_OPTIMIZER_ADAPTER_RUN_SCHEMA_VERSION
        );
        assert_eq!(first_output, second_output);
        assert!(first_output.selected_candidate_id.is_some());
    }

    #[test]
    fn calibration_esscher_transform_adapter_delegates_to_shared_module() {
        let parameters = NigParameters::try_new(2.3, -0.8, 0.7, 0.1)
            .expect("fixture parameters should construct");

        let transformed = transform_nig_exponent_for_calibration_under_esscher_tilt(
            NigEsscherTransformationInput::new(parameters, ComplexScalar::new(0.4, -0.2), -0.05),
        )
        .expect("Esscher transform should evaluate");

        assert!(transformed.transformed_exponent.is_finite());
    }

    #[test]
    fn calibration_martingale_adapter_delegates_to_shared_module() {
        let report = evaluate_calibration_martingale_condition_under_esscher_tilt(
            EsscherMartingaleConditionInput::new(ComplexScalar::new(0.12, 0.0), 0.1, 0.03),
        )
        .expect("martingale condition input should validate");

        assert!(report.is_satisfied);
        assert!(report.residual_norm <= report.tolerance);
    }

    #[test]
    fn calibration_consumer_observation_builder_accepts_valid_pricing_hooks() {
        let hooks = VanillaOptionPricingCalibrationHooks::try_new(
            8.5,
            -0.03,
            1.08,
            0.5,
            None,
            Some(false),
            Some(true),
            Some(true),
        )
        .expect("pricing hooks should construct");

        let observation = build_option_surface_calibration_observation_from_pricing_hooks(hooks)
            .expect("calibration observation conversion should succeed");

        assert_eq!(observation.option_price, hooks.option_price);
        assert_eq!(observation.log_moneyness, hooks.log_moneyness);
        assert_eq!(observation.forward_moneyness, hooks.forward_moneyness);
        assert_eq!(observation.maturity_interval, hooks.maturity_interval);
        assert_eq!(
            observation.integration_numeric_quality_is_poor,
            hooks.integration_numeric_quality_is_poor
        );
        assert_eq!(
            observation.integration_convergence_check_passed,
            hooks.integration_convergence_check_passed
        );
        assert_eq!(
            observation.integration_truncation_check_passed,
            hooks.integration_truncation_check_passed
        );
    }

    #[test]
    fn calibration_consumer_observation_builder_rejects_invalid_pricing_hooks() {
        let invalid_hooks = VanillaOptionPricingCalibrationHooks::new(
            8.5,
            -0.03,
            0.0,
            0.5,
            None,
            Some(false),
            Some(true),
            Some(true),
        );

        let error = build_option_surface_calibration_observation_from_pricing_hooks(invalid_hooks)
            .expect_err("invalid pricing hooks should fail calibration conversion");

        assert!(matches!(
            error,
            OptionSurfaceCalibrationObservationError::InvalidPricingCalibrationHooks { .. }
        ));
    }

    #[test]
    fn calibration_run_spec_round_trip_serialization_preserves_contract_fields() {
        let run_spec = build_valid_run_spec();

        assert_eq!(
            CalibrationRunSpec::schema_version(),
            CALIBRATION_RUN_SPEC_SCHEMA_VERSION
        );

        let serialized = serde_json::to_value(&run_spec).expect("run spec should serialize");
        assert_eq!(serialized["run_id"], json!("run-001"));
        assert_eq!(serialized["execution_mode"], json!("memory"));
        assert_eq!(serialized["selected_stages"][0], json!("seasonality"));
        assert_eq!(
            serialized["optimizer_settings_by_stage"][1]["stage"],
            json!("hawkes")
        );
        assert_eq!(serialized["seed_settings"]["global_seed"], json!(17));
        assert!(serialized.get("stage_handoffs").is_none());
        assert!(serialized.get("resume_settings").is_none());

        let parsed: CalibrationRunSpec =
            serde_json::from_value(serialized).expect("run spec should deserialize");
        assert_eq!(parsed, run_spec);
    }

    #[test]
    fn calibration_run_spec_deserialization_rejects_invalid_execution_mode() {
        let mut serialized = serde_json::to_value(build_valid_run_spec())
            .expect("run spec fixture should serialize");
        serialized["execution_mode"] = json!("unsupported_mode");

        let parse_result = serde_json::from_value::<CalibrationRunSpec>(serialized);
        assert!(parse_result.is_err());
    }

    #[test]
    fn calibration_execution_mode_defaults_are_surface_specific_and_documented() {
        assert_eq!(
            CALIBRATION_EXECUTION_MODE_DEFAULT_POLICY,
            "cli_defaults_to_disk_python_importable_api_defaults_to_memory.v1"
        );
        assert_eq!(
            default_calibration_execution_mode_for_surface(CalibrationCallerSurface::Cli),
            CalibrationExecutionMode::Disk
        );
        assert_eq!(
            default_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::PythonImportableApi
            ),
            CalibrationExecutionMode::Memory
        );
    }

    #[test]
    fn calibration_execution_mode_resolution_honors_explicit_overrides_per_surface() {
        assert_eq!(
            resolve_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::Cli,
                Some(CalibrationExecutionMode::Memory)
            ),
            CalibrationExecutionMode::Memory
        );
        assert_eq!(
            resolve_calibration_execution_mode_for_surface(
                CalibrationCallerSurface::PythonImportableApi,
                Some(CalibrationExecutionMode::Disk)
            ),
            CalibrationExecutionMode::Disk
        );
    }

    #[test]
    fn calibration_run_spec_for_caller_surface_applies_default_execution_mode() {
        let cli_spec = CalibrationRunSpec::try_new_for_caller_surface(
            "run-cli-default",
            build_data_inputs(),
            CalibrationCallerSurface::Cli,
            None,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            Vec::new(),
            None,
        )
        .expect("cli default run spec should validate");
        assert_eq!(cli_spec.execution_mode, CalibrationExecutionMode::Disk);

        let python_spec = CalibrationRunSpec::try_new_for_caller_surface(
            "run-python-default",
            build_data_inputs(),
            CalibrationCallerSurface::PythonImportableApi,
            None,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            Vec::new(),
            None,
        )
        .expect("python default run spec should validate");
        assert_eq!(python_spec.execution_mode, CalibrationExecutionMode::Memory);
    }

    #[test]
    fn calibration_run_spec_mode_extensions_support_valid_memory_and_disk_configurations() {
        let memory_run_spec = CalibrationRunSpec::try_new_with_mode_extensions(
            "run-memory",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            vec![CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::InMemoryOutput,
                artifact_reference: None,
            }],
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::SameProcess,
                resume_from_stage: Some(CalibrationStage::Hawkes),
                checkpoint_reference: None,
            }),
        )
        .expect("memory mode with in-memory handoff and same-process resume should validate");
        assert_eq!(
            memory_run_spec.execution_mode,
            CalibrationExecutionMode::Memory
        );

        let disk_run_spec = CalibrationRunSpec::try_new_with_mode_extensions(
            "run-disk",
            build_data_inputs(),
            CalibrationExecutionMode::Disk,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            vec![CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
                artifact_reference: Some("artifact://seasonality/stage-001".to_string()),
            }],
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::PersistedCheckpointReference,
                resume_from_stage: None,
                checkpoint_reference: Some("checkpoint://run-disk/last-success".to_string()),
            }),
        )
        .expect("disk mode with persisted handoff and persisted resume should validate");
        assert_eq!(disk_run_spec.execution_mode, CalibrationExecutionMode::Disk);
    }

    #[test]
    fn calibration_run_spec_rejects_mode_incompatible_option_settings_with_deterministic_classification()
     {
        let memory_with_persisted_handoff = CalibrationRunSpec::try_new_with_mode_extensions(
            "run-invalid-memory-handoff",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            vec![CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
                artifact_reference: Some("artifact://seasonality/stage-001".to_string()),
            }],
            None,
        )
        .expect_err("memory mode must reject persisted artifact handoff");

        assert_eq!(
            memory_with_persisted_handoff.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            memory_with_persisted_handoff,
            CalibrationRunSpecError::InvalidStageHandoff {
                source: CalibrationStageHandoffError::PersistedArtifactHandoffRequiresDiskMode { .. }
            }
        ));

        let memory_with_persisted_resume = CalibrationRunSpec::try_new_with_mode_extensions(
            "run-invalid-memory-resume",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            Vec::new(),
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::PersistedCheckpointReference,
                resume_from_stage: None,
                checkpoint_reference: Some("checkpoint://run-memory/last-success".to_string()),
            }),
        )
        .expect_err("memory mode must reject persisted checkpoint resume");

        assert_eq!(
            memory_with_persisted_resume.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            memory_with_persisted_resume,
            CalibrationRunSpecError::InvalidResumeSettings {
                source: CalibrationResumeSettingsError::PersistedCheckpointResumeRequiresDiskMode
            }
        ));

        let disk_with_same_process_resume = CalibrationRunSpec::try_new_with_mode_extensions(
            "run-invalid-disk-resume",
            build_data_inputs(),
            CalibrationExecutionMode::Disk,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            Vec::new(),
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::SameProcess,
                resume_from_stage: None,
                checkpoint_reference: None,
            }),
        )
        .expect_err("disk mode must reject same-process resume");

        assert_eq!(
            disk_with_same_process_resume.classification(),
            CalibrationRunSpecErrorClassification::ModeIncompatibleOptions
        );
        assert!(matches!(
            disk_with_same_process_resume,
            CalibrationRunSpecError::InvalidResumeSettings {
                source: CalibrationResumeSettingsError::SameProcessResumeRequiresMemoryMode
            }
        ));
    }

    #[test]
    fn calibration_stage_graph_validation_resolves_explicit_execution_order() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-ordered",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![
                CalibrationStage::ReturnComponents,
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate");

        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        assert_eq!(
            CalibrationStageGraph::schema_version(),
            CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
        );

        let serialized_graph =
            serde_json::to_value(&stage_graph).expect("stage graph should serialize");
        let parsed_graph: CalibrationStageGraph =
            serde_json::from_value(serialized_graph).expect("stage graph should deserialize");
        assert_eq!(parsed_graph, stage_graph);

        let execution_order = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect("valid graph should resolve an explicit topological execution order");

        assert_eq!(
            execution_order,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents
            ]
        );
    }

    #[test]
    fn calibration_stage_graph_rejects_unsupported_dependency_when_stage_is_unselected() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-unsupported-dependency",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
        )
        .expect("run spec should validate");
        let stage_graph = CalibrationStageGraph::new(vec![CalibrationStageDependency {
            stage: CalibrationStage::ReturnComponents,
            depends_on: CalibrationStage::Seasonality,
        }]);

        let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect_err("graph with unselected dependency stage must fail");
        assert!(matches!(
            error,
            CalibrationRunContractError::InvalidStageGraph {
                source: CalibrationStageGraphError::UnsupportedDependency {
                    stage: CalibrationStage::ReturnComponents,
                    depends_on: CalibrationStage::Seasonality,
                    ..
                }
            }
        ));
    }

    #[test]
    fn calibration_stage_graph_rejects_cycles() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-cycle",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate");

        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Seasonality,
                depends_on: CalibrationStage::ReturnComponents,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        let error = resolve_calibration_stage_execution_order(&run_spec, &stage_graph)
            .expect_err("cyclic graph must fail");
        assert!(matches!(
            error,
            CalibrationRunContractError::InvalidStageGraph {
                source: CalibrationStageGraphError::CyclicDependency { .. }
            }
        ));
    }

    #[test]
    fn calibration_run_contract_captures_solver_settings_for_auditability() {
        let run_spec = build_valid_run_spec();
        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        let run_contract = CalibrationRunContract::try_new(run_spec.clone(), stage_graph)
            .expect("valid run contract should build");
        assert_eq!(
            run_contract.schema_version,
            CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION
        );
        assert_eq!(
            run_contract.solver_settings_capture_policy,
            CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY
        );
        assert_eq!(
            run_contract.stage_solver_settings,
            run_spec.optimizer_settings_by_stage
        );
        assert_eq!(
            run_contract.execution_stage_order,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents
            ]
        );

        let serialized =
            serde_json::to_value(&run_contract).expect("run contract should serialize");
        assert_eq!(
            serialized["stage_solver_settings"]
                .as_array()
                .map(|v| v.len()),
            Some(3)
        );
        assert_eq!(
            serialized["stage_solver_settings"][0]["optimizer_settings"]["solver_name"],
            json!("lbfgs")
        );
    }

    #[test]
    fn calibration_run_contract_construction_supports_memory_and_disk_modes() {
        let stage_graph = CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ]);

        for execution_mode in [
            CalibrationExecutionMode::Memory,
            CalibrationExecutionMode::Disk,
        ] {
            let run_spec = build_valid_run_spec_with_mode(execution_mode);
            let run_contract = CalibrationRunContract::try_new(run_spec, stage_graph.clone())
                .expect("run contract should build for both execution modes");
            assert_eq!(run_contract.run_spec.execution_mode, execution_mode);
        }
    }

    #[test]
    fn calibration_run_spec_rejects_missing_solver_settings_for_selected_stage() {
        let run_spec = CalibrationRunSpec::try_new(
            "run-missing-solver",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            }],
        );

        assert!(matches!(
            run_spec,
            Err(CalibrationRunSpecError::MissingStageSolverSettings {
                stage: CalibrationStage::Hawkes
            })
        ));
    }

    #[test]
    fn staged_runner_emits_checkpoint_metadata_with_deterministic_identifiers_in_memory_mode() {
        let run_contract = build_run_contract_with_mode_and_resume(
            "run-checkpoint-memory",
            CalibrationExecutionMode::Memory,
            None,
        );
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let report = run_staged_calibration_with_checkpointing(
            &run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
        assert!(report.failure.is_none());
        assert_eq!(report.emitted_checkpoints.len(), 3);
        assert_eq!(
            checkpoint_store.checkpoint_count_for_run(run_contract.run_spec.run_id.as_str()),
            3
        );

        let first_checkpoint = report
            .emitted_checkpoints
            .first()
            .expect("first stage checkpoint should exist");
        assert_eq!(
            first_checkpoint.schema_version,
            CALIBRATION_STAGE_CHECKPOINT_SCHEMA_VERSION
        );
        assert_eq!(
            first_checkpoint.checkpoint_reference_policy,
            CALIBRATION_STAGE_CHECKPOINT_REFERENCE_POLICY
        );
        assert_eq!(
            first_checkpoint.disk_write_policy,
            CALIBRATION_DISK_CHECKPOINT_WRITE_POLICY
        );
        assert_eq!(
            first_checkpoint.artifact_manifest_schema_version,
            CALIBRATION_ARTIFACT_MANIFEST_SCHEMA_VERSION
        );
        assert_eq!(
            first_checkpoint.checkpoint_reference,
            calibration_checkpoint_reference_for_stage(
                run_contract.run_spec.run_id.as_str(),
                CalibrationStage::Seasonality
            )
        );
        assert_eq!(
            first_checkpoint.completed_stage,
            CalibrationStage::Seasonality
        );
        assert_eq!(first_checkpoint.completed_stage_position, 0);
        assert_eq!(
            checkpoint_store
                .last_checkpoint_for_run(run_contract.run_spec.run_id.as_str())
                .expect("store should expose last checkpoint")
                .completed_stage,
            CalibrationStage::ReturnComponents
        );
    }

    #[test]
    fn staged_runner_resume_from_last_successful_stage_works_in_memory_mode() {
        let run_id = "run-memory-resume-last";
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
        let initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            Some(&mut checkpoint_store),
            None,
        );
        assert_eq!(
            initial_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(initial_report.emitted_checkpoints.len(), 2);
        assert_eq!(
            initial_report.failure,
            Some(CalibrationStagedRunFailure::MissingStageInput {
                stage: CalibrationStage::ReturnComponents
            })
        );

        let resume_run_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::SameProcess,
                resume_from_stage: None,
                checkpoint_reference: None,
            }),
        );
        let resume_report = run_staged_calibration_with_checkpointing(
            &resume_run_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(
            resume_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_report.resumed_from_last_successful_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(
            resume_report.started_from_stage,
            Some(CalibrationStage::ReturnComponents)
        );
        assert_eq!(resume_report.stage_reports.len(), 1);
        assert_eq!(resume_report.emitted_checkpoints.len(), 1);
    }

    #[test]
    fn staged_runner_resume_from_selected_stage_works_in_memory_mode() {
        let run_id = "run-memory-resume-stage";
        let calendar_registry = calibration_stage_test_registry();
        let mut checkpoint_store = CalibrationMemoryCheckpointStore::new();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Memory, None);
        let _initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            Some(&mut checkpoint_store),
            None,
        );

        let resume_run_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::SameProcess,
                resume_from_stage: Some(CalibrationStage::Hawkes),
                checkpoint_reference: None,
            }),
        );
        let resume_report = run_staged_calibration_with_checkpointing(
            &resume_run_contract,
            &calendar_registry,
            staged_run_inputs(false, true, true),
            Some(&mut checkpoint_store),
            None,
        );

        assert_eq!(
            resume_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_report.started_from_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(resume_report.stage_reports.len(), 2);
        assert_eq!(resume_report.emitted_checkpoints.len(), 2);
    }

    #[test]
    fn staged_runner_resume_from_last_and_selected_stage_work_in_disk_mode() {
        let run_id = "run-disk-resume";
        let checkpoint_directory = temporary_checkpoint_directory("disk-resume");
        let calendar_registry = calibration_stage_test_registry();

        let initial_run_contract =
            build_run_contract_with_mode_and_resume(run_id, CalibrationExecutionMode::Disk, None);
        let initial_report = run_staged_calibration_with_checkpointing(
            &initial_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            None,
            Some(checkpoint_directory.as_path()),
        );
        assert_eq!(
            initial_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(initial_report.emitted_checkpoints.len(), 2);
        assert!(
            checkpoint_directory
                .join(run_id)
                .join("hawkes.checkpoint.json")
                .exists()
        );
        assert!(
            checkpoint_directory
                .join(run_id)
                .join("last-success.checkpoint.json")
                .exists()
        );

        let resume_from_last_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::PersistedCheckpointReference,
                resume_from_stage: None,
                checkpoint_reference: Some(calibration_last_success_checkpoint_reference(run_id)),
            }),
        );
        let resume_from_last_report = run_staged_calibration_with_checkpointing(
            &resume_from_last_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            Some(checkpoint_directory.as_path()),
        );

        assert_eq!(
            resume_from_last_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_from_last_report.started_from_stage,
            Some(CalibrationStage::ReturnComponents)
        );
        assert_eq!(resume_from_last_report.stage_reports.len(), 1);

        let resume_from_stage_contract = build_run_contract_with_mode_and_resume(
            run_id,
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::PersistedCheckpointReference,
                resume_from_stage: Some(CalibrationStage::Hawkes),
                checkpoint_reference: Some(calibration_last_success_checkpoint_reference(run_id)),
            }),
        );
        let resume_from_stage_report = run_staged_calibration_with_checkpointing(
            &resume_from_stage_contract,
            &calendar_registry,
            staged_run_inputs(false, true, true),
            None,
            Some(checkpoint_directory.as_path()),
        );
        assert_eq!(
            resume_from_stage_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(
            resume_from_stage_report.started_from_stage,
            Some(CalibrationStage::Hawkes)
        );
        assert_eq!(resume_from_stage_report.stage_reports.len(), 2);

        let _ = fs::remove_dir_all(checkpoint_directory);
    }

    #[test]
    fn staged_runner_rejects_incompatible_checkpoints_and_mode_specific_runtime_requirements() {
        let checkpoint_directory = temporary_checkpoint_directory("incompatible-checkpoint");
        let calendar_registry = calibration_stage_test_registry();

        let source_run_contract = build_run_contract_with_mode_and_resume(
            "run-source",
            CalibrationExecutionMode::Disk,
            None,
        );
        let _source_report = run_staged_calibration_with_checkpointing(
            &source_run_contract,
            &calendar_registry,
            staged_run_inputs(true, true, false),
            None,
            Some(checkpoint_directory.as_path()),
        );

        let incompatible_run_contract = build_run_contract_with_mode_and_resume(
            "run-target",
            CalibrationExecutionMode::Disk,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::PersistedCheckpointReference,
                resume_from_stage: None,
                checkpoint_reference: Some(calibration_last_success_checkpoint_reference(
                    "run-source",
                )),
            }),
        );
        let incompatible_report = run_staged_calibration_with_checkpointing(
            &incompatible_run_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            Some(checkpoint_directory.as_path()),
        );

        assert_eq!(
            incompatible_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            incompatible_report.failure,
            Some(CalibrationStagedRunFailure::CheckpointCompatibilityFailed { .. })
        ));

        let memory_resume_contract = build_run_contract_with_mode_and_resume(
            "run-memory-missing-store",
            CalibrationExecutionMode::Memory,
            Some(CalibrationResumeSettings {
                source: CalibrationResumeSource::SameProcess,
                resume_from_stage: None,
                checkpoint_reference: None,
            }),
        );
        let missing_store_report = run_staged_calibration_with_checkpointing(
            &memory_resume_contract,
            &calendar_registry,
            staged_run_inputs(false, false, true),
            None,
            None,
        );

        assert_eq!(
            missing_store_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(
            missing_store_report.failure,
            Some(CalibrationStagedRunFailure::MissingMemoryCheckpointStore)
        );

        let disk_missing_directory_contract = build_run_contract_with_mode_and_resume(
            "run-disk-missing-directory",
            CalibrationExecutionMode::Disk,
            None,
        );
        let missing_directory_report = run_staged_calibration_with_checkpointing(
            &disk_missing_directory_contract,
            &calendar_registry,
            staged_run_inputs(true, true, true),
            None,
            None,
        );

        assert_eq!(
            missing_directory_report.failure,
            Some(CalibrationStagedRunFailure::MissingDiskCheckpointDirectory)
        );

        let _ = fs::remove_dir_all(checkpoint_directory);
    }

    #[test]
    fn seasonality_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
        let calendar_registry = calibration_stage_test_registry();
        let input_events = calibration_stage_timestamp_fixture();

        let memory_report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(input_events.clone()),
        );
        let disk_report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Disk),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(input_events),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_SEASONALITY_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .normalized_stage_payload
            .as_ref()
            .expect("memory report should include normalized payload");
        let disk_payload = disk_report
            .normalized_stage_payload
            .as_ref()
            .expect("disk report should include normalized payload");
        assert_eq!(memory_payload, disk_payload);

        let memory_handoff = memory_report
            .handoffs
            .first()
            .expect("memory report should include one handoff");
        assert!(memory_handoff.in_memory_payload_available);
        assert!(memory_handoff.artifact_reference.is_none());

        let disk_handoff = disk_report
            .handoffs
            .first()
            .expect("disk report should include one handoff");
        assert!(!disk_handoff.in_memory_payload_available);
        assert_eq!(
            disk_handoff.artifact_reference.as_deref(),
            Some("artifact://seasonality/run-001")
        );
    }

    #[test]
    fn seasonality_stage_execution_reports_missing_input_and_invalid_config_failures() {
        let calendar_registry = calibration_stage_test_registry();
        let run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory);

        let missing_input_report = run_seasonality_calibration_stage(
            &run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(Vec::new()),
        );
        assert_eq!(
            missing_input_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert_eq!(
            missing_input_report.failure,
            Some(SeasonalityStageExecutionFailure::MissingTimestampEventRecords)
        );

        let mut invalid_config_input = SeasonalityStageExecutionInput::from_timestamp_event_records(
            calibration_stage_timestamp_fixture(),
        );
        invalid_config_input.settings.sparse_hour_event_threshold = 0;

        let invalid_config_report =
            run_seasonality_calibration_stage(&run_spec, &calendar_registry, invalid_config_input);
        assert_eq!(
            invalid_config_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            invalid_config_report.failure,
            Some(SeasonalityStageExecutionFailure::TimestampPreprocessingFailed { .. })
        ));
    }

    #[test]
    fn seasonality_stage_execution_metadata_is_deterministic_under_fixed_seed_and_config() {
        let calendar_registry = calibration_stage_test_registry();
        let memory_run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_seasonality_stage_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_seasonality_calibration_stage(
            &memory_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let second_memory_report = run_seasonality_calibration_stage(
            &memory_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let first_disk_report = run_seasonality_calibration_stage(
            &disk_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );
        let second_disk_report = run_seasonality_calibration_stage(
            &disk_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.diagnostics_snapshot_fingerprint_hex,
            first_disk_metadata.diagnostics_snapshot_fingerprint_hex
        );
        assert_eq!(
            first_memory_metadata.estimator_version,
            CALIBRATION_SEASONALITY_STAGE_DEFAULT_ESTIMATOR_VERSION
        );
    }

    #[test]
    fn seasonality_stage_execution_supports_standalone_partial_pipeline_runs() {
        let calendar_registry = calibration_stage_test_registry();

        let standalone_seed_settings = CalibrationRunSeedSettings::try_new(
            17,
            vec![CalibrationStageSeedOverride {
                stage: CalibrationStage::Seasonality,
                seed: 101,
            }],
        )
        .expect("standalone seed settings should validate");

        let standalone_run_spec = CalibrationRunSpec::try_new(
            "run-seasonality-only",
            build_data_inputs(),
            CalibrationExecutionMode::Memory,
            vec![CalibrationStage::Seasonality],
            standalone_seed_settings,
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            }],
        )
        .expect("standalone seasonality run spec should validate");

        let report = run_seasonality_calibration_stage(
            &standalone_run_spec,
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Success);
        assert!(report.handoffs.is_empty());
        assert!(report.failure.is_none());
    }

    #[test]
    fn hawkes_stage_execution_succeeds_for_memory_and_disk_mode_handoffs() {
        let seasonality_payload = build_seasonality_stage_success_payload();

        let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload.clone(),
                hawkes_stage_candidate_fixture(),
            ),
        );
        let disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_HAWKES_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted stage payload");
        let disk_payload = disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted stage payload");

        assert_eq!(
            memory_payload.selected_candidate_id,
            "candidate_stable_low_loss"
        );
        assert_eq!(
            memory_payload.selected_candidate_id,
            disk_payload.selected_candidate_id
        );
        assert_eq!(
            memory_payload.fitted_parameters.kernel_parameters,
            disk_payload.fitted_parameters.kernel_parameters
        );
        assert_eq!(
            memory_payload.candidate_diagnostics,
            disk_payload.candidate_diagnostics
        );

        assert_eq!(
            memory_payload.seasonality_dataset_id.as_deref(),
            Some(seasonality_payload.dataset_id.as_str())
        );
        assert!(disk_payload.seasonality_dataset_id.is_none());
        assert_eq!(
            disk_payload.seasonality_artifact_reference.as_deref(),
            Some("artifact://seasonality/run-001")
        );

        let memory_handoff = memory_report
            .handoffs
            .first()
            .expect("memory report should include one handoff");
        assert!(memory_handoff.in_memory_payload_available);
        assert!(memory_handoff.artifact_reference.is_none());

        let disk_handoff = disk_report
            .handoffs
            .first()
            .expect("disk report should include one handoff");
        assert!(!disk_handoff.in_memory_payload_available);
        assert_eq!(
            disk_handoff.artifact_reference.as_deref(),
            Some("artifact://hawkes/run-001")
        );
    }

    #[test]
    fn hawkes_stage_execution_reports_constraint_violations_for_invalid_candidates() {
        let run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);

        let report = run_hawkes_calibration_stage(
            &run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                build_seasonality_stage_success_payload(),
                vec![hawkes_stage_constraint_violating_candidate_fixture()],
            ),
        );

        assert_eq!(report.status, CalibrationStageExecutionStatus::Failure);

        let failure = report
            .failure
            .as_ref()
            .expect("failure report should include details");
        match failure {
            HawkesStageExecutionFailure::ConstraintViolationsForAllCandidates {
                candidate_diagnostics,
            } => {
                assert_eq!(candidate_diagnostics.len(), 1);
                let candidate = &candidate_diagnostics[0];
                assert_eq!(candidate.candidate_id, "candidate_invalid_constraints");
                assert!(candidate.constraint_violation_count > 0);
                assert_eq!(
                    candidate.validation_report.status,
                    HawkesKernelValidationStatus::Fail
                );
            }
            _ => panic!("expected constraint failure diagnostics"),
        }
    }

    #[test]
    fn hawkes_stage_execution_is_deterministic_under_fixed_seed_and_solver_settings() {
        let seasonality_payload = build_seasonality_stage_success_payload();
        let memory_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec = build_hawkes_stage_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload.clone(),
                hawkes_stage_candidate_fixture(),
            ),
        );
        let second_memory_report = run_hawkes_calibration_stage(
            &memory_run_spec,
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload,
                hawkes_stage_candidate_fixture(),
            ),
        );
        let first_disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );
        let second_disk_report = run_hawkes_calibration_stage(
            &disk_run_spec,
            HawkesStageExecutionInput::from_persisted_seasonality_artifact_reference(
                "artifact://seasonality/run-001",
                hawkes_stage_candidate_fixture(),
            ),
        );

        let first_memory_payload = first_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted payload");
        let second_memory_payload = second_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("second memory report should include fitted payload");
        let first_disk_payload = first_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted payload");
        let second_disk_payload = second_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("second disk report should include fitted payload");

        assert_eq!(first_memory_payload, second_memory_payload);
        assert_eq!(first_disk_payload, second_disk_payload);
        assert_eq!(
            first_memory_payload.selected_candidate_id,
            first_disk_payload.selected_candidate_id
        );
        assert_eq!(
            first_memory_payload.fitted_parameters.kernel_parameters,
            first_disk_payload.fitted_parameters.kernel_parameters
        );
        assert_eq!(
            first_memory_payload.candidate_diagnostics,
            first_disk_payload.candidate_diagnostics
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.solver_settings,
            first_disk_metadata.solver_settings
        );
    }

    #[test]
    fn return_components_stage_execution_succeeds_for_memory_and_disk_pipeline_handoffs() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload.clone(),
                return_components_stage_candidate_fixture(),
            ),
        );
        let disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_fixture(),
            ),
        );

        assert_eq!(
            memory_report.schema_version,
            CALIBRATION_RETURN_COMPONENTS_STAGE_EXECUTION_REPORT_SCHEMA_VERSION
        );
        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);

        let memory_payload = memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted stage payload");
        let disk_payload = disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted stage payload");

        assert_eq!(
            memory_payload.selected_candidate_id,
            "candidate_return_low_loss"
        );
        assert_eq!(
            memory_payload.selected_candidate_id,
            disk_payload.selected_candidate_id
        );
        assert_eq!(
            memory_payload.fitted_parameters,
            disk_payload.fitted_parameters
        );
        assert_eq!(
            memory_payload.candidate_diagnostics,
            disk_payload.candidate_diagnostics
        );
        assert_eq!(
            memory_payload
                .activity_selected_hawkes_candidate_id
                .as_deref(),
            Some(hawkes_payload.selected_candidate_id.as_str())
        );
        assert!(disk_payload.activity_selected_hawkes_candidate_id.is_none());
        assert_eq!(
            disk_payload.activity_artifact_reference.as_deref(),
            Some("artifact://hawkes/run-001")
        );
    }

    #[test]
    fn return_components_stage_execution_supports_standalone_runs_in_memory_and_disk_modes() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_standalone_run_spec(CalibrationExecutionMode::Disk);

        let memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_fixture(),
            ),
        );
        let disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/standalone",
                return_components_stage_candidate_fixture(),
            ),
        );

        assert_eq!(
            memory_report.status,
            CalibrationStageExecutionStatus::Success
        );
        assert_eq!(disk_report.status, CalibrationStageExecutionStatus::Success);
        assert!(memory_report.handoffs.is_empty());
        assert!(disk_report.handoffs.is_empty());
        assert!(memory_report.failure.is_none());
        assert!(disk_report.failure.is_none());
    }

    #[test]
    fn return_components_stage_execution_reports_missing_dependency_stage_outputs() {
        let hawkes_payload = build_hawkes_stage_success_payload();

        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let memory_failure_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_fixture(),
            ),
        );
        assert_eq!(
            memory_failure_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            memory_failure_report.failure,
            Some(ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch { .. })
        ));

        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);
        let disk_failure_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_fixture(),
            ),
        );
        assert_eq!(
            disk_failure_report.status,
            CalibrationStageExecutionStatus::Failure
        );
        assert!(matches!(
            disk_failure_report.failure,
            Some(ReturnComponentsStageExecutionFailure::ActivityInputTransportMismatch { .. })
        ));
    }

    #[test]
    fn return_components_stage_execution_is_deterministic_under_fixed_settings() {
        let hawkes_payload = build_hawkes_stage_success_payload();
        let memory_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Memory);
        let disk_run_spec =
            build_return_components_stage_pipeline_run_spec(CalibrationExecutionMode::Disk);

        let first_memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload.clone(),
                return_components_stage_candidate_fixture(),
            ),
        );
        let second_memory_report = run_return_components_calibration_stage(
            &memory_run_spec,
            ReturnComponentsStageExecutionInput::from_in_memory_activity_output(
                hawkes_payload,
                return_components_stage_candidate_fixture(),
            ),
        );
        let first_disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_fixture(),
            ),
        );
        let second_disk_report = run_return_components_calibration_stage(
            &disk_run_spec,
            ReturnComponentsStageExecutionInput::from_persisted_activity_artifact_reference(
                "artifact://hawkes/run-001",
                return_components_stage_candidate_fixture(),
            ),
        );

        let first_memory_payload = first_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("memory report should include fitted payload");
        let second_memory_payload = second_memory_report
            .fitted_stage_payload
            .as_ref()
            .expect("second memory report should include fitted payload");
        let first_disk_payload = first_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("disk report should include fitted payload");
        let second_disk_payload = second_disk_report
            .fitted_stage_payload
            .as_ref()
            .expect("second disk report should include fitted payload");

        assert_eq!(first_memory_payload, second_memory_payload);
        assert_eq!(first_disk_payload, second_disk_payload);
        assert_eq!(
            first_memory_payload.selected_candidate_id,
            first_disk_payload.selected_candidate_id
        );
        assert_eq!(
            first_memory_payload.fitted_parameters,
            first_disk_payload.fitted_parameters
        );
        assert_eq!(
            first_memory_payload.candidate_diagnostics,
            first_disk_payload.candidate_diagnostics
        );

        let first_memory_metadata = first_memory_report
            .metadata
            .as_ref()
            .expect("memory report should include metadata");
        let second_memory_metadata = second_memory_report
            .metadata
            .as_ref()
            .expect("second memory report should include metadata");
        let first_disk_metadata = first_disk_report
            .metadata
            .as_ref()
            .expect("disk report should include metadata");
        let second_disk_metadata = second_disk_report
            .metadata
            .as_ref()
            .expect("second disk report should include metadata");

        assert_eq!(first_memory_metadata, second_memory_metadata);
        assert_eq!(first_disk_metadata, second_disk_metadata);
        assert_eq!(
            first_memory_metadata.stage_seed,
            first_disk_metadata.stage_seed
        );
        assert_eq!(
            first_memory_metadata.solver_settings,
            first_disk_metadata.solver_settings
        );
    }

    fn build_data_inputs() -> CalibrationRunDataInputs {
        CalibrationRunDataInputs::try_new(
            "XNYS",
            "dataset://synthetic/trades/v1",
            Some("dataset://synthetic/options/v1".to_string()),
        )
        .expect("data inputs should validate")
    }

    fn build_seed_settings() -> CalibrationRunSeedSettings {
        CalibrationRunSeedSettings::try_new(
            17,
            vec![
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Seasonality,
                    seed: 101,
                },
                CalibrationStageSeedOverride {
                    stage: CalibrationStage::Hawkes,
                    seed: 202,
                },
            ],
        )
        .expect("seed settings should validate")
    }

    fn build_solver_settings() -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new("lbfgs", 250, 1e-8, 1e-6, 10.0)
            .expect("solver settings should validate")
    }

    fn build_solver_settings_for_all_stages() -> Vec<CalibrationStageSolverSettings> {
        vec![
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Seasonality,
                optimizer_settings: build_solver_settings(),
            },
            CalibrationStageSolverSettings {
                stage: CalibrationStage::Hawkes,
                optimizer_settings: build_solver_settings(),
            },
            CalibrationStageSolverSettings {
                stage: CalibrationStage::ReturnComponents,
                optimizer_settings: build_solver_settings(),
            },
        ]
    }

    fn build_valid_run_spec() -> CalibrationRunSpec {
        build_valid_run_spec_with_mode(CalibrationExecutionMode::Memory)
    }

    fn build_valid_run_spec_with_mode(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        CalibrationRunSpec::try_new(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
        )
        .expect("run spec should validate")
    }

    fn build_run_contract_with_mode_and_resume(
        run_id: &str,
        execution_mode: CalibrationExecutionMode,
        resume_settings: Option<CalibrationResumeSettings>,
    ) -> CalibrationRunContract {
        let run_spec = CalibrationRunSpec::try_new_with_mode_extensions(
            run_id,
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
            Vec::new(),
            resume_settings,
        )
        .expect("run spec should validate");

        CalibrationRunContract::try_new(run_spec, default_stage_graph())
            .expect("run contract should validate")
    }

    fn default_stage_graph() -> CalibrationStageGraph {
        CalibrationStageGraph::new(vec![
            CalibrationStageDependency {
                stage: CalibrationStage::Hawkes,
                depends_on: CalibrationStage::Seasonality,
            },
            CalibrationStageDependency {
                stage: CalibrationStage::ReturnComponents,
                depends_on: CalibrationStage::Hawkes,
            },
        ])
    }

    fn staged_run_inputs(
        include_seasonality_input: bool,
        include_hawkes_candidates: bool,
        include_return_candidates: bool,
    ) -> CalibrationStagedRunInputs {
        CalibrationStagedRunInputs {
            seasonality_stage_input: if include_seasonality_input {
                Some(
                    SeasonalityStageExecutionInput::from_timestamp_event_records(
                        calibration_stage_timestamp_fixture(),
                    ),
                )
            } else {
                None
            },
            hawkes_candidate_inputs: if include_hawkes_candidates {
                Some(hawkes_stage_candidate_fixture())
            } else {
                None
            },
            return_components_candidate_inputs: if include_return_candidates {
                Some(return_components_stage_candidate_fixture())
            } else {
                None
            },
        }
    }

    fn temporary_checkpoint_directory(test_name: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX epoch")
            .as_nanos();
        let checkpoint_directory =
            std::env::temp_dir().join(format!("janiform-checkpoints-{test_name}-{unique_suffix}"));

        if checkpoint_directory.exists() {
            let _ = fs::remove_dir_all(checkpoint_directory.as_path());
        }

        fs::create_dir_all(checkpoint_directory.as_path())
            .expect("temporary checkpoint directory should be created");

        checkpoint_directory
    }

    fn build_seasonality_stage_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        let stage_handoff = match execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::InMemoryOutput,
                artifact_reference: None,
            },
            CalibrationExecutionMode::Disk => CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
                artifact_reference: Some("artifact://seasonality/run-001".to_string()),
            },
        };

        CalibrationRunSpec::try_new_with_mode_extensions(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![CalibrationStage::Seasonality, CalibrationStage::Hawkes],
            build_seed_settings(),
            vec![
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Seasonality,
                    optimizer_settings: build_solver_settings(),
                },
                CalibrationStageSolverSettings {
                    stage: CalibrationStage::Hawkes,
                    optimizer_settings: build_solver_settings(),
                },
            ],
            vec![stage_handoff],
            None,
        )
        .expect("seasonality stage run spec should validate")
    }

    fn build_hawkes_stage_run_spec(execution_mode: CalibrationExecutionMode) -> CalibrationRunSpec {
        let seasonality_handoff = match execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::InMemoryOutput,
                artifact_reference: None,
            },
            CalibrationExecutionMode::Disk => CalibrationStageHandoff {
                from_stage: CalibrationStage::Seasonality,
                to_stage: CalibrationStage::Hawkes,
                transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
                artifact_reference: Some("artifact://seasonality/run-001".to_string()),
            },
        };

        let hawkes_handoff = match execution_mode {
            CalibrationExecutionMode::Memory => CalibrationStageHandoff {
                from_stage: CalibrationStage::Hawkes,
                to_stage: CalibrationStage::ReturnComponents,
                transport: CalibrationStageHandoffTransport::InMemoryOutput,
                artifact_reference: None,
            },
            CalibrationExecutionMode::Disk => CalibrationStageHandoff {
                from_stage: CalibrationStage::Hawkes,
                to_stage: CalibrationStage::ReturnComponents,
                transport: CalibrationStageHandoffTransport::PersistedArtifactReference,
                artifact_reference: Some("artifact://hawkes/run-001".to_string()),
            },
        };

        CalibrationRunSpec::try_new_with_mode_extensions(
            "run-001",
            build_data_inputs(),
            execution_mode,
            vec![
                CalibrationStage::Seasonality,
                CalibrationStage::Hawkes,
                CalibrationStage::ReturnComponents,
            ],
            build_seed_settings(),
            build_solver_settings_for_all_stages(),
            vec![seasonality_handoff, hawkes_handoff],
            None,
        )
        .expect("hawkes stage run spec should validate")
    }

    fn build_return_components_stage_pipeline_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        build_hawkes_stage_run_spec(execution_mode)
    }

    fn build_return_components_stage_standalone_run_spec(
        execution_mode: CalibrationExecutionMode,
    ) -> CalibrationRunSpec {
        let seed_settings = CalibrationRunSeedSettings::try_new(
            23,
            vec![CalibrationStageSeedOverride {
                stage: CalibrationStage::ReturnComponents,
                seed: 303,
            }],
        )
        .expect("standalone return-components seed settings should validate");

        CalibrationRunSpec::try_new(
            "run-return-components-standalone",
            build_data_inputs(),
            execution_mode,
            vec![CalibrationStage::ReturnComponents],
            seed_settings,
            vec![CalibrationStageSolverSettings {
                stage: CalibrationStage::ReturnComponents,
                optimizer_settings: build_solver_settings(),
            }],
        )
        .expect("standalone return-components run spec should validate")
    }

    fn build_seasonality_stage_success_payload()
    -> crate::calibration::SeasonalityStageNormalizedPayload {
        let calendar_registry = calibration_stage_test_registry();
        let report = run_seasonality_calibration_stage(
            &build_seasonality_stage_run_spec(CalibrationExecutionMode::Memory),
            &calendar_registry,
            SeasonalityStageExecutionInput::from_timestamp_event_records(
                calibration_stage_timestamp_fixture(),
            ),
        );

        report
            .normalized_stage_payload
            .expect("seasonality stage fixture should produce normalized payload")
    }

    fn build_hawkes_stage_success_payload() -> crate::calibration::HawkesStageFittedPayload {
        let seasonality_payload = build_seasonality_stage_success_payload();
        let report = run_hawkes_calibration_stage(
            &build_hawkes_stage_run_spec(CalibrationExecutionMode::Memory),
            HawkesStageExecutionInput::from_in_memory_seasonality_output(
                seasonality_payload,
                hawkes_stage_candidate_fixture(),
            ),
        );

        report
            .fitted_stage_payload
            .expect("hawkes stage fixture should produce fitted payload")
    }

    fn hawkes_stage_candidate_fixture() -> Vec<HawkesStageCandidateInput> {
        vec![
            HawkesStageCandidateInput {
                candidate_id: "candidate_high_loss".to_string(),
                lambda_bar: 0.85,
                beta: 1.50,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.40, 0.10, 0.08, 0.10,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.35, 0.10, 0.05, 0.10),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                objective_terms: vec![
                    HawkesStageObjectiveTermInput {
                        term_id: "event_rate_residual".to_string(),
                        weight: 1.0,
                        observed_score: 2.8,
                    },
                    HawkesStageObjectiveTermInput {
                        term_id: "clustering_autocorrelation".to_string(),
                        weight: 1.0,
                        observed_score: 1.4,
                    },
                ],
            },
            HawkesStageCandidateInput {
                candidate_id: "candidate_stable_low_loss".to_string(),
                lambda_bar: 0.80,
                beta: 1.60,
                structured_alpha: HawkesStructuredAlphaParameterization::new(
                    0.35, 0.06, 0.07, 0.04,
                ),
                structured_rho: HawkesStructuredRhoParameterization::new(0.30, 0.08, 0.04, 0.05),
                self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(1.0)),
                stability_numeric_tolerance: 0.0,
                objective_terms: vec![
                    HawkesStageObjectiveTermInput {
                        term_id: "event_rate_residual".to_string(),
                        weight: 1.0,
                        observed_score: 1.6,
                    },
                    HawkesStageObjectiveTermInput {
                        term_id: "clustering_autocorrelation".to_string(),
                        weight: 1.0,
                        observed_score: 0.7,
                    },
                ],
            },
        ]
    }

    fn hawkes_stage_constraint_violating_candidate_fixture() -> HawkesStageCandidateInput {
        HawkesStageCandidateInput {
            candidate_id: "candidate_invalid_constraints".to_string(),
            lambda_bar: -0.2,
            beta: 0.0,
            structured_alpha: HawkesStructuredAlphaParameterization::new(-0.1, -0.1, -0.1, -0.1),
            structured_rho: HawkesStructuredRhoParameterization::new(-0.2, -0.1, -0.1, -0.1),
            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy::new(0.9)),
            stability_numeric_tolerance: -0.5,
            objective_terms: vec![HawkesStageObjectiveTermInput {
                term_id: "event_rate_residual".to_string(),
                weight: 1.0,
                observed_score: 1.0,
            }],
        }
    }

    fn return_components_stage_candidate_fixture() -> Vec<ReturnComponentsStageCandidateInput> {
        vec![
            ReturnComponentsStageCandidateInput {
                candidate_id: "candidate_return_high_loss".to_string(),
                nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
                    2.30, -0.80, 0.70, 0.10, -0.20,
                )
                .expect("high-loss NIG configuration should validate"),
                news_process_configuration: build_news_process_configuration(
                    0.08, 1.8, 0.7, 0.45, 8.0, 10.0, 0.22,
                ),
                objective_terms: vec![
                    ReturnComponentsStageObjectiveTermInput {
                        term_id: "tail_fit_residual".to_string(),
                        weight: 1.0,
                        observed_score: 1.2,
                    },
                    ReturnComponentsStageObjectiveTermInput {
                        term_id: "gap_quantile_residual".to_string(),
                        weight: 1.0,
                        observed_score: 0.8,
                    },
                ],
            },
            ReturnComponentsStageCandidateInput {
                candidate_id: "candidate_return_low_loss".to_string(),
                nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
                    2.45, -0.70, 0.62, 0.08, -0.15,
                )
                .expect("low-loss NIG configuration should validate"),
                news_process_configuration: build_news_process_configuration(
                    0.06, 1.6, 0.65, 0.50, 9.5, 9.0, 0.18,
                ),
                objective_terms: vec![
                    ReturnComponentsStageObjectiveTermInput {
                        term_id: "tail_fit_residual".to_string(),
                        weight: 1.0,
                        observed_score: 0.6,
                    },
                    ReturnComponentsStageObjectiveTermInput {
                        term_id: "gap_quantile_residual".to_string(),
                        weight: 1.0,
                        observed_score: 0.4,
                    },
                ],
            },
        ]
    }

    fn build_news_process_configuration(
        closed_window_baseline_intensity: f64,
        gamma_shape_k: f64,
        gamma_scale_theta: f64,
        probability_positive: f64,
        eta_plus: f64,
        eta_minus: f64,
        sigma_g: f64,
    ) -> NewsProcessConfiguration {
        NewsProcessConfiguration::try_new(
            NewsIntensityConfiguration::try_new(closed_window_baseline_intensity)
                .expect("news intensity configuration should validate"),
            GammaSeverityMarkConfiguration::try_new(gamma_shape_k, gamma_scale_theta)
                .expect("gamma severity mark configuration should validate"),
            DoubleExponentialBaseShockConfiguration::try_new(
                probability_positive,
                eta_plus,
                eta_minus,
            )
            .expect("double-exponential base-shock configuration should validate"),
            CoupledGapMarkConfiguration::new(sigma_g),
        )
        .expect("news process configuration should validate")
    }

    fn calibration_stage_test_registry() -> ExchangeCalendarRegistry {
        ExchangeCalendarRegistryLoader::load_from_json(
            r#"
{
  "schema_version": "calendar.registry.v1",
  "artifact_date": "2026-01-31",
  "exchanges": [
    {
      "schema_version": "calendar.v1",
      "exchange_code": "XNYS",
      "exchange_name": "New York Stock Exchange",
      "timezone_name": "America/New_York",
      "rule_metadata": {
        "holiday_calendar_code": "US_EQUITIES",
        "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
      },
      "regular_sessions": [
        {
          "weekday": "Monday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        },
        {
          "weekday": "Tuesday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        },
        {
          "weekday": "Wednesday",
          "windows": [{"label": "regular", "open_time": "09:30", "close_time": "16:00"}]
        }
      ],
      "holiday_dates": [],
      "early_close_sessions": []
    }
  ]
}
"#,
        )
        .expect("calendar registry fixture should load")
    }

    fn calibration_stage_timestamp_fixture() -> Vec<TimestampEventInputRecord> {
        vec![
            timestamp_input_record(0, 2026, 2, 2, 14, 30, 0),
            timestamp_input_record(1, 2026, 2, 2, 15, 30, 0),
            timestamp_input_record(2, 2026, 2, 3, 14, 30, 0),
            timestamp_input_record(3, 2026, 2, 3, 18, 45, 0),
            timestamp_input_record(4, 2026, 2, 4, 14, 30, 0),
            timestamp_input_record(5, 2026, 2, 4, 20, 30, 0),
        ]
    }

    fn timestamp_input_record(
        source_record_position: u64,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> TimestampEventInputRecord {
        TimestampEventInputRecord {
            source_record_position,
            timestamp_utc: Utc
                .with_ymd_and_hms(year, month, day, hour, minute, second)
                .single()
                .expect("test timestamp should be valid"),
        }
    }
}
