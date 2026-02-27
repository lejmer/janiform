//! Calibration-side Hawkes entrypoints through shared provider and transition APIs.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::calendar::ExchangeCalendarRegistry;
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
use crate::pricing::{
    VanillaOptionPricingCalibrationHooks, VanillaOptionPricingCalibrationHooksError,
};
use crate::risk_neutral::{
    EsscherMartingaleConditionError, EsscherMartingaleConditionInput,
    EsscherMartingaleConditionReport, EsscherTransformError, NigEsscherTransformationInput,
    NigEsscherTransformedExponent, evaluate_esscher_martingale_condition,
    transform_nig_characteristic_exponent_under_esscher_tilt,
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
        let run_spec = Self {
            run_id: run_id.into(),
            data_inputs,
            execution_mode,
            selected_stages,
            seed_settings,
            optimizer_settings_by_stage,
        };
        run_spec.validate()?;
        Ok(run_spec)
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
        }
    }
}

impl Error for CalibrationRunSpecError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidDataInputs { source } => Some(source),
            Self::InvalidSeedSettings { source } => Some(source),
            Self::InvalidStageSolverSettings { source, .. } => Some(source),
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

#[cfg(test)]
mod tests {
    use crate::calibration::{
        CALIBRATION_RUN_CONTRACT_SCHEMA_VERSION, CALIBRATION_RUN_SPEC_SCHEMA_VERSION,
        CALIBRATION_SOLVER_SETTINGS_CAPTURE_POLICY, CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION,
        CalibrationExecutionMode, CalibrationOptimizerSettings, CalibrationRunContract,
        CalibrationRunContractError, CalibrationRunDataInputs, CalibrationRunSeedSettings,
        CalibrationRunSpec, CalibrationRunSpecError, CalibrationStage, CalibrationStageDependency,
        CalibrationStageGraph, CalibrationStageGraphError, CalibrationStageSeedOverride,
        CalibrationStageSolverSettings, OptionSurfaceCalibrationObservationError,
        build_option_surface_calibration_observation_from_pricing_hooks,
        evaluate_calibration_martingale_condition_under_esscher_tilt,
        precheck_hawkes_kernel_parameter_domain,
        precheck_structured_hawkes_kernel_parameter_domain,
        resolve_calibration_stage_execution_order,
        transform_nig_exponent_for_calibration_under_esscher_tilt,
    };
    use crate::hawkes::{
        HAWKES_KERNEL_PARAMETER_VALIDATION_REPORT_SCHEMA_VERSION, HawkesKernelLoadings,
        HawkesKernelValidationStatus, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    };
    use crate::nig::{ComplexScalar, NigParameters};
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
}
