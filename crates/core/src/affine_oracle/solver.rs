use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::complex_math::{ComplexScalar, complex_exponential};

use super::{
    EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
    EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
    EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY, ExponentialAffineTransformDVector,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputError,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformOdeRhs,
    ExponentialAffineTransformOutput, ExponentialAffineTransformOutputError,
    evaluate_exponential_affine_transform_ode_rhs,
};

const DEFAULT_EXPONENTIAL_AFFINE_TRANSFORM_SOLVER_STEP_COUNT: usize = 64;

/// Supported one-step numerical integration methods for affine transform ODE solving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformIntegratorMethod {
    FourthOrderRungeKutta,
    ForwardEuler,
}

impl Default for ExponentialAffineTransformIntegratorMethod {
    fn default() -> Self {
        Self::FourthOrderRungeKutta
    }
}

/// Debug telemetry emission mode for one-interval affine transform integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformTelemetryMode {
    Disabled,
    Enabled,
}

impl Default for ExponentialAffineTransformTelemetryMode {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Solver settings for one-interval affine transform integration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExponentialAffineTransformSolverSettings {
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub step_count: usize,
    pub telemetry_mode: ExponentialAffineTransformTelemetryMode,
}

impl Default for ExponentialAffineTransformSolverSettings {
    fn default() -> Self {
        Self {
            integrator_method: ExponentialAffineTransformIntegratorMethod::default(),
            step_count: DEFAULT_EXPONENTIAL_AFFINE_TRANSFORM_SOLVER_STEP_COUNT,
            telemetry_mode: ExponentialAffineTransformTelemetryMode::default(),
        }
    }
}

impl ExponentialAffineTransformSolverSettings {
    pub fn try_new(
        integrator_method: ExponentialAffineTransformIntegratorMethod,
        step_count: usize,
    ) -> Result<Self, ExponentialAffineTransformSolverSettingsError> {
        let settings = Self {
            integrator_method,
            step_count,
            telemetry_mode: ExponentialAffineTransformTelemetryMode::Disabled,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn with_telemetry_mode(
        self,
        telemetry_mode: ExponentialAffineTransformTelemetryMode,
    ) -> Self {
        Self {
            integrator_method: self.integrator_method,
            step_count: self.step_count,
            telemetry_mode,
        }
    }

    pub fn validate(self) -> Result<(), ExponentialAffineTransformSolverSettingsError> {
        if self.step_count == 0 {
            return Err(
                ExponentialAffineTransformSolverSettingsError::InvalidStepCount {
                    value: self.step_count,
                },
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformSolverSettingsError {
    InvalidStepCount { value: usize },
}

impl Display for ExponentialAffineTransformSolverSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStepCount { value } => write!(
                formatter,
                "solver step_count must be strictly positive, found {value}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformSolverSettingsError {}

/// Structured integration status for one affine transform solve call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformIntegrationStatus {
    Completed,
    DomainGuardViolation,
    BlowupTerminated,
}

/// Machine-readable reason code for guard-triggered integration termination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExponentialAffineTransformGuardFailureCode {
    DomainUpperBoundViolation,
    NonFiniteState,
    NonFiniteDomainMetric,
}

/// Guard-trigger metadata captured at deterministic integration checkpoints.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformGuardFailure {
    pub schema_version: &'static str,
    pub code: ExponentialAffineTransformGuardFailureCode,
    pub step_index: usize,
    pub time: f64,
    pub d_dot_r: ComplexScalar,
    pub d_dot_r_upper_bound: f64,
}

/// Deterministic boolean status flags mirroring the integration status enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformIntegrationStatusFlags {
    pub completed: bool,
    pub domain_guard_violation: bool,
    pub blowup_terminated: bool,
}

impl ExponentialAffineTransformIntegrationStatusFlags {
    pub const fn from_status(status: ExponentialAffineTransformIntegrationStatus) -> Self {
        match status {
            ExponentialAffineTransformIntegrationStatus::Completed => Self {
                completed: true,
                domain_guard_violation: false,
                blowup_terminated: false,
            },
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation => Self {
                completed: false,
                domain_guard_violation: true,
                blowup_terminated: false,
            },
            ExponentialAffineTransformIntegrationStatus::BlowupTerminated => Self {
                completed: false,
                domain_guard_violation: false,
                blowup_terminated: true,
            },
        }
    }
}

/// Machine-readable diagnostics schema metadata for integration outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExponentialAffineTransformIntegrationOutputSchemaMetadata {
    pub schema_version: &'static str,
    pub integration_metadata_schema_version: &'static str,
    pub guard_failure_schema_version: &'static str,
    pub telemetry_schema_version: &'static str,
    pub required_result_fields: &'static [&'static str],
    pub required_metadata_fields: &'static [&'static str],
    pub required_guard_failure_fields: &'static [&'static str],
    pub required_telemetry_fields: &'static [&'static str],
}

pub const EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA:
    ExponentialAffineTransformIntegrationOutputSchemaMetadata =
    ExponentialAffineTransformIntegrationOutputSchemaMetadata {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA_VERSION,
        integration_metadata_schema_version:
            EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        guard_failure_schema_version: EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        telemetry_schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
        required_result_fields: &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_RESULT_FIELDS,
        required_metadata_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_METADATA_FIELDS,
        required_guard_failure_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_GUARD_FAILURE_FIELDS,
        required_telemetry_fields:
            &EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_REQUIRED_TELEMETRY_FIELDS,
    };

/// Optional debug telemetry payload for integration quality triage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationTelemetry {
    pub schema_version: &'static str,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub checkpoint_count: usize,
    pub max_abs_c_value: Option<f64>,
    pub max_abs_d_component: Option<f64>,
    pub max_d_dot_r_real: Option<f64>,
    pub min_domain_margin: Option<f64>,
    pub guard_triggered: bool,
    pub guard_trigger: Option<ExponentialAffineTransformGuardFailure>,
}

/// Metadata describing one completed affine transform integration run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationMetadata {
    pub schema_version: &'static str,
    pub integrator_method: ExponentialAffineTransformIntegratorMethod,
    pub telemetry_mode: ExponentialAffineTransformTelemetryMode,
    pub status_flags: ExponentialAffineTransformIntegrationStatusFlags,
    pub news_intensity_usage_policy: &'static str,
    pub news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub configured_step_count: usize,
    pub completed_step_count: usize,
    pub step_size: f64,
    pub rhs_evaluation_count: usize,
    pub domain_guard_policy: &'static str,
    pub blowup_termination_policy: &'static str,
    pub guard_failure: Option<ExponentialAffineTransformGuardFailure>,
}

/// Result payload for one affine transform solve call.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ExponentialAffineTransformIntegrationResult {
    pub schema_version: &'static str,
    pub output_schema_metadata: ExponentialAffineTransformIntegrationOutputSchemaMetadata,
    pub status: ExponentialAffineTransformIntegrationStatus,
    pub metadata: ExponentialAffineTransformIntegrationMetadata,
    pub output: Option<ExponentialAffineTransformOutput>,
    pub telemetry: Option<ExponentialAffineTransformIntegrationTelemetry>,
}

/// Structured failure modes for affine transform solver execution.
#[derive(Debug, Clone, PartialEq)]
pub enum ExponentialAffineTransformSolverError {
    InvalidInput {
        source: ExponentialAffineTransformInputError,
    },
    InvalidSolverSettings {
        source: ExponentialAffineTransformSolverSettingsError,
    },
    InvalidOutput {
        source: ExponentialAffineTransformOutputError,
    },
}

impl Display for ExponentialAffineTransformSolverError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput { source } => {
                write!(
                    formatter,
                    "affine transform solver input is invalid: {source}"
                )
            }
            Self::InvalidSolverSettings { source } => write!(
                formatter,
                "affine transform solver settings are invalid: {source}"
            ),
            Self::InvalidOutput { source } => write!(
                formatter,
                "affine transform solver produced invalid output: {source}"
            ),
        }
    }
}

impl Error for ExponentialAffineTransformSolverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidInput { source } => Some(source),
            Self::InvalidSolverSettings { source } => Some(source),
            Self::InvalidOutput { source } => Some(source),
        }
    }
}

/// Solve one-interval affine transform ODE system using default stable settings.
pub fn solve_exponential_affine_transform(
    transform_input: ExponentialAffineTransformInput,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(transform_input, None)
}

/// Solve one-interval affine transform ODE system with explicit solver settings.
pub fn solve_exponential_affine_transform_with_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    solve_exponential_affine_transform_with_optional_settings(
        transform_input,
        Some(solver_settings),
    )
}

/// Solve one-interval affine transform ODE system through one consumer-neutral entrypoint.
///
/// Pricing and risk adapters call this function so solver behavior stays unified.
///
/// Pass `None` for `solver_settings` to use the default stable method.
///
/// # Examples
///
/// Pricing consumer
/// ```ignore
/// use janiform_core::pricing::solve_exponential_affine_transform;
///
/// let pricing_input = build_pricing_transform_input();
/// let solve_result = solve_exponential_affine_transform(pricing_input)?;
/// # Ok::<(), janiform_core::affine_oracle::ExponentialAffineTransformSolverError>(())
/// ```
///
/// Risk-sampling consumer with explicit settings
/// ```ignore
/// use janiform_core::affine_oracle::ExponentialAffineTransformSolverSettings;
/// use janiform_core::risk::solve_exponential_affine_transform_for_risk_sampling_with_settings;
///
/// let risk_input = build_risk_sampling_transform_input();
/// let solver_settings = ExponentialAffineTransformSolverSettings::default();
/// let solve_result = solve_exponential_affine_transform_for_risk_sampling_with_settings(
///     risk_input,
///     solver_settings,
/// )?;
/// # Ok::<(), janiform_core::affine_oracle::ExponentialAffineTransformSolverError>(())
/// ```
pub fn solve_exponential_affine_transform_with_optional_settings(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: Option<ExponentialAffineTransformSolverSettings>,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError> {
    let effective_solver_settings = solver_settings.unwrap_or_default();

    transform_input
        .validate()
        .map_err(|source| ExponentialAffineTransformSolverError::InvalidInput { source })?;
    effective_solver_settings.validate().map_err(|source| {
        ExponentialAffineTransformSolverError::InvalidSolverSettings { source }
    })?;

    match effective_solver_settings.integrator_method {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta => {
            run_affine_transform_integration_with_integrator(
                transform_input,
                effective_solver_settings,
                FourthOrderRungeKuttaIntegrator,
            )
        }
        ExponentialAffineTransformIntegratorMethod::ForwardEuler => {
            run_affine_transform_integration_with_integrator(
                transform_input,
                effective_solver_settings,
                ForwardEulerIntegrator,
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformIntegrationState {
    d_vector: ExponentialAffineTransformDVector,
    c_value: ComplexScalar,
}

impl ExponentialAffineTransformIntegrationState {
    fn initial() -> Self {
        Self {
            d_vector: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            c_value: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn add_scaled_rhs(self, rhs: ExponentialAffineTransformOdeRhs, scale: f64) -> Self {
        Self {
            d_vector: self.d_vector.add(rhs.d_derivative.scale(scale)),
            c_value: self.c_value.add(rhs.c_derivative.scale(scale)),
        }
    }
}

type ExponentialAffineTransformOdeRhsEvaluator = fn(
    ExponentialAffineTransformInput,
    ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformOdeRhs;

trait ExponentialAffineTransformIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod;
    fn rhs_evaluations_per_step(&self) -> usize;
    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ForwardEulerIntegrator;

impl ExponentialAffineTransformIntegrator for ForwardEulerIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod {
        ExponentialAffineTransformIntegratorMethod::ForwardEuler
    }

    fn rhs_evaluations_per_step(&self) -> usize {
        1
    }

    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState {
        let first_derivative = rhs_evaluator(transform_input, state.d_vector);
        state.add_scaled_rhs(first_derivative, step_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FourthOrderRungeKuttaIntegrator;

impl ExponentialAffineTransformIntegrator for FourthOrderRungeKuttaIntegrator {
    fn method(&self) -> ExponentialAffineTransformIntegratorMethod {
        ExponentialAffineTransformIntegratorMethod::FourthOrderRungeKutta
    }

    fn rhs_evaluations_per_step(&self) -> usize {
        4
    }

    fn advance_one_step(
        &self,
        transform_input: ExponentialAffineTransformInput,
        step_size: f64,
        state: ExponentialAffineTransformIntegrationState,
        rhs_evaluator: ExponentialAffineTransformOdeRhsEvaluator,
    ) -> ExponentialAffineTransformIntegrationState {
        let k1 = rhs_evaluator(transform_input, state.d_vector);
        let k2 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k1, 0.5 * step_size).d_vector,
        );
        let k3 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k2, 0.5 * step_size).d_vector,
        );
        let k4 = rhs_evaluator(
            transform_input,
            state.add_scaled_rhs(k3, step_size).d_vector,
        );

        let d_increment = k1
            .d_derivative
            .add(k2.d_derivative.scale(2.0))
            .add(k3.d_derivative.scale(2.0))
            .add(k4.d_derivative)
            .scale(step_size / 6.0);
        let c_increment = k1
            .c_derivative
            .add(k2.c_derivative.scale(2.0))
            .add(k3.c_derivative.scale(2.0))
            .add(k4.c_derivative)
            .scale(step_size / 6.0);

        ExponentialAffineTransformIntegrationState {
            d_vector: state.d_vector.add(d_increment),
            c_value: state.c_value.add(c_increment),
        }
    }
}

fn run_affine_transform_integration_with_integrator<TIntegrator>(
    transform_input: ExponentialAffineTransformInput,
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator: TIntegrator,
) -> Result<ExponentialAffineTransformIntegrationResult, ExponentialAffineTransformSolverError>
where
    TIntegrator: ExponentialAffineTransformIntegrator,
{
    let step_size = transform_input.delta / solver_settings.step_count as f64;
    let mut integration_state = ExponentialAffineTransformIntegrationState::initial();
    let mut completed_step_count = 0_usize;
    let mut rhs_evaluation_count = 0_usize;
    let mut telemetry_accumulator =
        ExponentialAffineTransformTelemetryAccumulator::enabled_or_none(solver_settings);

    let initial_checkpoint = evaluate_integration_guard_checkpoint(
        transform_input,
        integration_state,
        completed_step_count,
        step_size,
    );

    if let Some(telemetry_accumulator) = telemetry_accumulator.as_mut() {
        telemetry_accumulator.record_checkpoint(integration_state, initial_checkpoint);
    }

    if let Some(guard_failure) = initial_checkpoint.guard_failure {
        let telemetry = telemetry_accumulator
            .map(|accumulator| accumulator.to_payload(transform_input.news_intensity));
        return Ok(terminated_result_from_guard_failure(
            solver_settings,
            integrator,
            completed_step_count,
            rhs_evaluation_count,
            step_size,
            transform_input.news_intensity,
            guard_failure,
            telemetry,
        ));
    }

    for _ in 0..solver_settings.step_count {
        integration_state = integrator.advance_one_step(
            transform_input,
            step_size,
            integration_state,
            evaluate_exponential_affine_transform_ode_rhs,
        );
        completed_step_count += 1;
        rhs_evaluation_count =
            rhs_evaluation_count.saturating_add(integrator.rhs_evaluations_per_step());

        let checkpoint = evaluate_integration_guard_checkpoint(
            transform_input,
            integration_state,
            completed_step_count,
            step_size,
        );

        if let Some(telemetry_accumulator) = telemetry_accumulator.as_mut() {
            telemetry_accumulator.record_checkpoint(integration_state, checkpoint);
        }

        if let Some(guard_failure) = checkpoint.guard_failure {
            let telemetry = telemetry_accumulator
                .map(|accumulator| accumulator.to_payload(transform_input.news_intensity));
            return Ok(terminated_result_from_guard_failure(
                solver_settings,
                integrator,
                completed_step_count,
                rhs_evaluation_count,
                step_size,
                transform_input.news_intensity,
                guard_failure,
                telemetry,
            ));
        }
    }

    let c_at_delta = integration_state.c_value;
    let d_at_delta = integration_state.d_vector;
    let transform_exponent = c_at_delta.add(d_at_delta.dot_state(transform_input.initial_state));
    let transform_value = complex_exponential(transform_exponent);
    let output = ExponentialAffineTransformOutput::try_new(c_at_delta, d_at_delta, transform_value)
        .map_err(|source| ExponentialAffineTransformSolverError::InvalidOutput { source })?;
    let status = ExponentialAffineTransformIntegrationStatus::Completed;
    let telemetry = telemetry_accumulator
        .map(|accumulator| accumulator.to_payload(transform_input.news_intensity));

    Ok(ExponentialAffineTransformIntegrationResult {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        output_schema_metadata: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA,
        status,
        metadata: build_integration_metadata(
            solver_settings,
            integrator.method(),
            status,
            transform_input.news_intensity,
            completed_step_count,
            step_size,
            rhs_evaluation_count,
            None,
        ),
        output: Some(output),
        telemetry,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformGuardCheckpointEvaluation {
    guard_failure: Option<ExponentialAffineTransformGuardFailure>,
    d_dot_r: ComplexScalar,
    d_dot_r_upper_bound: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ExponentialAffineTransformTelemetryAccumulator {
    checkpoint_count: usize,
    max_abs_c_value: Option<f64>,
    max_abs_d_component: Option<f64>,
    max_d_dot_r_real: Option<f64>,
    min_domain_margin: Option<f64>,
    guard_trigger: Option<ExponentialAffineTransformGuardFailure>,
}

impl ExponentialAffineTransformTelemetryAccumulator {
    fn enabled_or_none(solver_settings: ExponentialAffineTransformSolverSettings) -> Option<Self> {
        match solver_settings.telemetry_mode {
            ExponentialAffineTransformTelemetryMode::Disabled => None,
            ExponentialAffineTransformTelemetryMode::Enabled => Some(Self {
                checkpoint_count: 0,
                max_abs_c_value: None,
                max_abs_d_component: None,
                max_d_dot_r_real: None,
                min_domain_margin: None,
                guard_trigger: None,
            }),
        }
    }

    fn record_checkpoint(
        &mut self,
        integration_state: ExponentialAffineTransformIntegrationState,
        checkpoint: ExponentialAffineTransformGuardCheckpointEvaluation,
    ) {
        self.checkpoint_count = self.checkpoint_count.saturating_add(1);

        if integration_state.c_value.is_finite() {
            self.max_abs_c_value =
                update_optional_max(self.max_abs_c_value, integration_state.c_value.norm());
        }

        if let Some(max_abs_d_component) = evaluate_max_abs_d_component(integration_state.d_vector)
        {
            self.max_abs_d_component =
                update_optional_max(self.max_abs_d_component, max_abs_d_component);
        }

        if checkpoint.d_dot_r.is_finite() {
            self.max_d_dot_r_real =
                update_optional_max(self.max_d_dot_r_real, checkpoint.d_dot_r.real);
            self.min_domain_margin = update_optional_min(
                self.min_domain_margin,
                checkpoint.d_dot_r_upper_bound - checkpoint.d_dot_r.real,
            );
        }

        if self.guard_trigger.is_none() {
            self.guard_trigger = checkpoint.guard_failure;
        }
    }

    fn to_payload(
        self,
        news_intensity: ExponentialAffineTransformNewsIntensityInput,
    ) -> ExponentialAffineTransformIntegrationTelemetry {
        ExponentialAffineTransformIntegrationTelemetry {
            schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_TELEMETRY_SCHEMA_VERSION,
            news_intensity,
            checkpoint_count: self.checkpoint_count,
            max_abs_c_value: self.max_abs_c_value,
            max_abs_d_component: self.max_abs_d_component,
            max_d_dot_r_real: self.max_d_dot_r_real,
            min_domain_margin: self.min_domain_margin,
            guard_triggered: self.guard_trigger.is_some(),
            guard_trigger: self.guard_trigger,
        }
    }
}

fn evaluate_integration_guard_checkpoint(
    transform_input: ExponentialAffineTransformInput,
    integration_state: ExponentialAffineTransformIntegrationState,
    completed_step_count: usize,
    step_size: f64,
) -> ExponentialAffineTransformGuardCheckpointEvaluation {
    let d_dot_r_upper_bound = 1.0
        / transform_input
            .equation_specification
            .gamma_severity_mark_configuration
            .scale_theta;
    let checkpoint_time = completed_step_count as f64 * step_size;

    if !integration_state.c_value.is_finite() || integration_state.d_vector.validate().is_err() {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::NonFiniteState,
                completed_step_count,
                checkpoint_time,
                ComplexScalar::new(f64::NAN, f64::NAN),
                d_dot_r_upper_bound,
            )),
            d_dot_r: ComplexScalar::new(f64::NAN, f64::NAN),
            d_dot_r_upper_bound,
        };
    }

    let d_dot_r = integration_state
        .d_vector
        .dot_loadings(transform_input.equation_specification.r());
    if !d_dot_r.is_finite() {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::NonFiniteDomainMetric,
                completed_step_count,
                checkpoint_time,
                d_dot_r,
                d_dot_r_upper_bound,
            )),
            d_dot_r,
            d_dot_r_upper_bound,
        };
    }

    if d_dot_r.real >= d_dot_r_upper_bound {
        return ExponentialAffineTransformGuardCheckpointEvaluation {
            guard_failure: Some(build_guard_failure(
                ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation,
                completed_step_count,
                checkpoint_time,
                d_dot_r,
                d_dot_r_upper_bound,
            )),
            d_dot_r,
            d_dot_r_upper_bound,
        };
    }

    ExponentialAffineTransformGuardCheckpointEvaluation {
        guard_failure: None,
        d_dot_r,
        d_dot_r_upper_bound,
    }
}

fn terminated_result_from_guard_failure<TIntegrator>(
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator: TIntegrator,
    completed_step_count: usize,
    rhs_evaluation_count: usize,
    step_size: f64,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    guard_failure: ExponentialAffineTransformGuardFailure,
    telemetry: Option<ExponentialAffineTransformIntegrationTelemetry>,
) -> ExponentialAffineTransformIntegrationResult
where
    TIntegrator: ExponentialAffineTransformIntegrator,
{
    let status = status_from_guard_failure_code(guard_failure.code);

    ExponentialAffineTransformIntegrationResult {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_RESULT_SCHEMA_VERSION,
        output_schema_metadata: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_OUTPUT_SCHEMA_METADATA,
        status,
        metadata: build_integration_metadata(
            solver_settings,
            integrator.method(),
            status,
            news_intensity,
            completed_step_count,
            step_size,
            rhs_evaluation_count,
            Some(guard_failure),
        ),
        output: None,
        telemetry,
    }
}

fn build_guard_failure(
    code: ExponentialAffineTransformGuardFailureCode,
    step_index: usize,
    time: f64,
    d_dot_r: ComplexScalar,
    d_dot_r_upper_bound: f64,
) -> ExponentialAffineTransformGuardFailure {
    ExponentialAffineTransformGuardFailure {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_GUARD_FAILURE_SCHEMA_VERSION,
        code,
        step_index,
        time,
        d_dot_r,
        d_dot_r_upper_bound,
    }
}

fn build_integration_metadata(
    solver_settings: ExponentialAffineTransformSolverSettings,
    integrator_method: ExponentialAffineTransformIntegratorMethod,
    status: ExponentialAffineTransformIntegrationStatus,
    news_intensity: ExponentialAffineTransformNewsIntensityInput,
    completed_step_count: usize,
    step_size: f64,
    rhs_evaluation_count: usize,
    guard_failure: Option<ExponentialAffineTransformGuardFailure>,
) -> ExponentialAffineTransformIntegrationMetadata {
    ExponentialAffineTransformIntegrationMetadata {
        schema_version: EXPONENTIAL_AFFINE_TRANSFORM_INTEGRATION_METADATA_SCHEMA_VERSION,
        integrator_method,
        telemetry_mode: solver_settings.telemetry_mode,
        status_flags: ExponentialAffineTransformIntegrationStatusFlags::from_status(status),
        news_intensity_usage_policy: EXPONENTIAL_AFFINE_TRANSFORM_NEWS_INTENSITY_USAGE_POLICY,
        news_intensity,
        configured_step_count: solver_settings.step_count,
        completed_step_count,
        step_size,
        rhs_evaluation_count,
        domain_guard_policy: EXPONENTIAL_AFFINE_TRANSFORM_DOMAIN_GUARD_POLICY,
        blowup_termination_policy: EXPONENTIAL_AFFINE_TRANSFORM_BLOWUP_TERMINATION_POLICY,
        guard_failure,
    }
}

fn status_from_guard_failure_code(
    guard_failure_code: ExponentialAffineTransformGuardFailureCode,
) -> ExponentialAffineTransformIntegrationStatus {
    match guard_failure_code {
        ExponentialAffineTransformGuardFailureCode::DomainUpperBoundViolation => {
            ExponentialAffineTransformIntegrationStatus::DomainGuardViolation
        }
        ExponentialAffineTransformGuardFailureCode::NonFiniteState
        | ExponentialAffineTransformGuardFailureCode::NonFiniteDomainMetric => {
            ExponentialAffineTransformIntegrationStatus::BlowupTerminated
        }
    }
}

fn update_optional_max(current_value: Option<f64>, candidate_value: f64) -> Option<f64> {
    match current_value {
        Some(current_value) if candidate_value <= current_value => Some(current_value),
        _ => Some(candidate_value),
    }
}

fn update_optional_min(current_value: Option<f64>, candidate_value: f64) -> Option<f64> {
    match current_value {
        Some(current_value) if candidate_value >= current_value => Some(current_value),
        _ => Some(candidate_value),
    }
}

fn evaluate_max_abs_d_component(d_vector: ExponentialAffineTransformDVector) -> Option<f64> {
    if d_vector.validate().is_err() {
        return None;
    }

    Some(
        d_vector
            .plus_plus
            .norm()
            .max(d_vector.plus_minus.norm())
            .max(d_vector.minus_plus.norm())
            .max(d_vector.minus_minus.norm()),
    )
}
