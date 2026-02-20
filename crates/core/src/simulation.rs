//! Simulation-side hourly factor loading through the shared provider interface.

use std::error::Error;
use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};

use crate::calendar::ExchangeCalendarRegistry;
use crate::hawkes::{
    HawkesExcitationState, HawkesIntensityProjection, HawkesKernelParameters,
    HawkesSeasonalityIntervalError, HawkesTransitionError, HawkesTransitionStep,
};
use crate::hourly_factor_provider::{
    HourlyFactorProviderError, HourlyFactorProviderRequest, provide_hourly_factor_contract,
};
use crate::hourly_factors::HourlyFactorContractOutput;

pub const SEEDED_TICK_EVENT_RUN_CONFIG_SCHEMA_VERSION: &str =
    "simulation.seeded_tick_event.run_config.v1";
pub const SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION: &str = "simulation.seeded_tick_event.path.v1";
pub const SEEDED_TICK_EVENT_DETERMINISM_POLICY: &str =
    "stable_seed_and_run_config_with_ordered_event_path.v1";

/// Optional run settings for seeded tick-event simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeededTickSimulationRunOptions {
    /// Enable optional debug trace emission in future simulator stages.
    pub debug_trace_enabled: bool,
}

impl Default for SeededTickSimulationRunOptions {
    fn default() -> Self {
        Self {
            debug_trace_enabled: false,
        }
    }
}

/// Required model inputs for seeded tick-event simulation runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationModelInputs {
    /// Canonical Hawkes kernel parameter payload.
    pub kernel_parameters: HawkesKernelParameters,

    /// Excitation state carried into the first simulated interval.
    pub initial_excitation_state: HawkesExcitationState,

    /// Interval-aligned hourly factors (`s_h`, `nu_h`) used by the simulator.
    pub hourly_factor_contract: HourlyFactorContractOutput,
}

/// Typed public run contract for deterministic seeded tick-event simulation.
///
/// Required fields:
/// - `seed`
/// - `start_time_utc`
/// - `horizon_interval_count`
/// - `model_inputs`
///
/// Optional fields:
/// - `options`, defaults to `SeededTickSimulationRunOptions::default()`
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunConfig {
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub horizon_interval_count: usize,
    pub model_inputs: SeededTickSimulationModelInputs,
    pub options: SeededTickSimulationRunOptions,
}

impl SeededTickSimulationRunConfig {
    /// Build and validate one seeded run configuration payload.
    ///
    /// Failure modes:
    /// - invalid horizon value (`horizon_interval_count == 0`)
    /// - missing or incomplete hourly factor interval vectors
    /// - mismatched hourly factor diagnostics interval count
    /// - start time not aligned to first hourly factor interval start
    /// - requested horizon exceeds available factor intervals
    pub fn try_new(
        seed: u64,
        start_time_utc: DateTime<Utc>,
        horizon_interval_count: usize,
        model_inputs: SeededTickSimulationModelInputs,
        options: Option<SeededTickSimulationRunOptions>,
    ) -> Result<Self, SeededTickSimulationRunError> {
        let run_config = Self {
            seed,
            start_time_utc,
            horizon_interval_count,
            model_inputs,
            options: options.unwrap_or_default(),
        };

        run_config.validate()?;

        Ok(run_config)
    }

    pub fn validate(&self) -> Result<(), SeededTickSimulationRunError> {
        if self.horizon_interval_count == 0 {
            return Err(SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 });
        }

        let hourly_factor_contract = &self.model_inputs.hourly_factor_contract;
        let expected_interval_count = hourly_factor_contract.interval_indices.len();
        if expected_interval_count == 0 {
            return Err(SeededTickSimulationRunError::MissingHourlyFactorIntervals);
        }

        if hourly_factor_contract.diagnostics.interval_count != expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HourlyFactorDiagnosticsIntervalCountMismatch {
                    expected_interval_count,
                    diagnostics_interval_count: hourly_factor_contract.diagnostics.interval_count,
                },
            );
        }

        validate_hourly_factor_field_length(
            "interval_start_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_start_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "interval_end_utc_nanos",
            expected_interval_count,
            hourly_factor_contract.interval_end_utc_nanos.len(),
        )?;
        validate_hourly_factor_field_length(
            "s_h",
            expected_interval_count,
            hourly_factor_contract.s_h.len(),
        )?;
        validate_hourly_factor_field_length(
            "nu_h",
            expected_interval_count,
            hourly_factor_contract.nu_h.len(),
        )?;

        if self.horizon_interval_count > expected_interval_count {
            return Err(
                SeededTickSimulationRunError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count: expected_interval_count,
                },
            );
        }

        let start_time_utc_nanos = self.start_time_utc.timestamp_nanos_opt().ok_or(
            SeededTickSimulationRunError::StartTimeOutOfNanosecondRange {
                value: self.start_time_utc,
            },
        )?;
        let first_interval_start_utc_nanos = hourly_factor_contract.interval_start_utc_nanos[0];

        if start_time_utc_nanos != first_interval_start_utc_nanos {
            return Err(
                SeededTickSimulationRunError::StartTimeDoesNotMatchHourlyFactorCoverage {
                    start_time_utc_nanos,
                    first_interval_start_utc_nanos,
                },
            );
        }

        Ok(())
    }
}

/// Side label for one simulated buy or sell event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeededTickEventSide {
    Buy,
    Sell,
}

/// One deterministic event-path record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededTickEventRecord {
    pub event_index: u64,
    pub timestamp_utc_nanos: i64,
    pub side: SeededTickEventSide,
}

/// Deterministic seeded event-path output object.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTickSimulationRunResult {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub seed: u64,
    pub start_time_utc: DateTime<Utc>,
    pub end_time_utc: DateTime<Utc>,
    pub simulated_interval_count: usize,
    pub event_path: Vec<SeededTickEventRecord>,
    pub final_excitation_state: HawkesExcitationState,
    pub debug_trace_enabled: bool,
}

/// Structured simulator run failure with explicit validation diagnostics.
#[derive(Debug, Clone, PartialEq)]
pub enum SeededTickSimulationRunError {
    InvalidHorizonIntervalCount {
        value: usize,
    },
    MissingHourlyFactorIntervals,
    HourlyFactorFieldLengthMismatch {
        field_name: &'static str,
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    HourlyFactorDiagnosticsIntervalCountMismatch {
        expected_interval_count: usize,
        diagnostics_interval_count: usize,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    StartTimeOutOfNanosecondRange {
        value: DateTime<Utc>,
    },
    StartTimeDoesNotMatchHourlyFactorCoverage {
        start_time_utc_nanos: i64,
        first_interval_start_utc_nanos: i64,
    },
}

impl Display for SeededTickSimulationRunError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be greater than 0, found {value}"
            ),
            Self::MissingHourlyFactorIntervals => write!(
                formatter,
                "hourly_factor_contract.interval_count must be greater than 0"
            ),
            Self::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.{field_name} length must match interval_count ({expected_interval_count}), found {observed_interval_count}"
            ),
            Self::HourlyFactorDiagnosticsIntervalCountMismatch {
                expected_interval_count,
                diagnostics_interval_count,
            } => write!(
                formatter,
                "hourly_factor_contract.diagnostics.interval_count must match interval_count ({expected_interval_count}), found {diagnostics_interval_count}"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds hourly factor interval_count ({available_interval_count})"
            ),
            Self::StartTimeOutOfNanosecondRange { value } => write!(
                formatter,
                "start_time_utc cannot be represented in nanoseconds, found {value}"
            ),
            Self::StartTimeDoesNotMatchHourlyFactorCoverage {
                start_time_utc_nanos,
                first_interval_start_utc_nanos,
            } => write!(
                formatter,
                "start_time_utc nanos ({start_time_utc_nanos}) must match hourly factor first interval start ({first_interval_start_utc_nanos})"
            ),
        }
    }
}

impl Error for SeededTickSimulationRunError {}

/// Run deterministic seeded tick-event simulation.
///
/// Current stage behavior:
/// - validates run configuration
/// - returns deterministic output metadata
/// - returns an empty event path while scheduler work is in progress
pub fn run_seeded_tick_event_simulation(
    run_config: SeededTickSimulationRunConfig,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    run_config.validate()?;

    let horizon_end_utc_nanos = run_config
        .model_inputs
        .hourly_factor_contract
        .interval_end_utc_nanos[run_config.horizon_interval_count - 1];

    Ok(SeededTickSimulationRunResult {
        schema_version: SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
        determinism_policy: SEEDED_TICK_EVENT_DETERMINISM_POLICY,
        seed: run_config.seed,
        start_time_utc: run_config.start_time_utc,
        end_time_utc: DateTime::<Utc>::from_timestamp_nanos(horizon_end_utc_nanos),
        simulated_interval_count: run_config.horizon_interval_count,
        event_path: Vec::new(),
        final_excitation_state: run_config.model_inputs.initial_excitation_state,
        debug_trace_enabled: run_config.options.debug_trace_enabled,
    })
}

fn validate_hourly_factor_field_length(
    field_name: &'static str,
    expected_interval_count: usize,
    observed_interval_count: usize,
) -> Result<(), SeededTickSimulationRunError> {
    if observed_interval_count != expected_interval_count {
        return Err(
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name,
                expected_interval_count,
                observed_interval_count,
            },
        );
    }

    Ok(())
}

/// Resolve interval-aligned hourly factors for the simulation path.
pub fn load_hourly_factor_contract_for_simulation(
    calendar_registry: &ExchangeCalendarRegistry,
    provider_request: HourlyFactorProviderRequest<'_>,
) -> Result<HourlyFactorContractOutput, HourlyFactorProviderError> {
    provide_hourly_factor_contract(calendar_registry, provider_request)
}

/// Project state to interval intensities for the fine-scale simulation path.
pub fn project_hawkes_intensities_for_simulation_interval(
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

/// Apply one simulation-path Hawkes state transition using canonical call order.
///
/// Call order is delegated to `HawkesExcitationState::apply_transition_step`:
/// drift, then optional endogenous jump, then optional exogenous jump.
pub fn evolve_hawkes_state_for_simulation_step(
    state: HawkesExcitationState,
    kernel_parameters: HawkesKernelParameters,
    transition_step: HawkesTransitionStep,
) -> Result<HawkesExcitationState, HawkesTransitionError> {
    state.apply_transition_step(kernel_parameters, transition_step)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use crate::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
    use crate::hourly_factors::{
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
        HOURLY_FACTOR_FALL_BACK_POLICY, HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
        HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY, HOURLY_FACTOR_SPRING_FORWARD_POLICY,
        HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY, HourlyFactorContractDiagnostics,
        HourlyFactorContractOutput, HourlyFactorMaterializationMetadata,
    };
    use crate::simulation::{
        SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
        SeededTickSimulationModelInputs, SeededTickSimulationRunConfig,
        SeededTickSimulationRunError, SeededTickSimulationRunOptions,
        run_seeded_tick_event_simulation,
    };

    const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

    #[test]
    fn seeded_simulation_run_executes_with_valid_config() {
        let run_config = SeededTickSimulationRunConfig::try_new(
            9_101,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            2,
            valid_model_inputs(),
            Some(SeededTickSimulationRunOptions {
                debug_trace_enabled: true,
            }),
        )
        .expect("valid run config should initialize");

        let first_result = run_seeded_tick_event_simulation(run_config.clone())
            .expect("valid run should produce deterministic output");
        let second_result = run_seeded_tick_event_simulation(run_config)
            .expect("same run config should produce deterministic output");

        assert_eq!(first_result, second_result);
        assert_eq!(
            first_result.schema_version,
            SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
        );
        assert_eq!(
            first_result.determinism_policy,
            SEEDED_TICK_EVENT_DETERMINISM_POLICY
        );
        assert_eq!(first_result.seed, 9_101);
        assert_eq!(first_result.simulated_interval_count, 2);
        assert!(first_result.debug_trace_enabled);
        assert!(first_result.event_path.is_empty());
    }

    #[test]
    fn seeded_simulation_run_rejects_zero_horizon() {
        let error = SeededTickSimulationRunConfig::try_new(
            42,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            0,
            valid_model_inputs(),
            None,
        )
        .expect_err("zero horizon must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 }
        ));
    }

    #[test]
    fn seeded_simulation_run_rejects_incomplete_hourly_factor_payload() {
        let mut model_inputs = valid_model_inputs();
        model_inputs.hourly_factor_contract.s_h.pop();

        let error = SeededTickSimulationRunConfig::try_new(
            77,
            DateTime::<Utc>::from_timestamp_nanos(1_704_067_200_000_000_000),
            1,
            model_inputs,
            None,
        )
        .expect_err("incomplete hourly factor vectors must fail");

        assert!(matches!(
            error,
            SeededTickSimulationRunError::HourlyFactorFieldLengthMismatch {
                field_name: "s_h",
                expected_interval_count: 2,
                observed_interval_count: 1,
            }
        ));
    }

    fn valid_model_inputs() -> SeededTickSimulationModelInputs {
        let start_utc_nanos = 1_704_067_200_000_000_000;
        let interval_start_utc_nanos = vec![start_utc_nanos, start_utc_nanos + ONE_HOUR_NANOS];
        let interval_end_utc_nanos = vec![
            start_utc_nanos + ONE_HOUR_NANOS,
            start_utc_nanos + 2 * ONE_HOUR_NANOS,
        ];

        SeededTickSimulationModelInputs {
            kernel_parameters: HawkesKernelParameters::try_new(
                0.8,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.3,
                    plus_minus: 0.15,
                    minus_plus: 0.1,
                    minus_minus: 0.25,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.05,
                    plus_minus: 0.02,
                    minus_plus: 0.03,
                    minus_minus: 0.06,
                },
            )
            .expect("kernel parameter fixture should be valid"),
            initial_excitation_state: crate::hawkes::HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.15,
                y_minus_minus: 0.3,
            },
            hourly_factor_contract: HourlyFactorContractOutput {
                schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
                exchange_code: "XNYS".to_string(),
                exchange_timezone_name: "America/New_York".to_string(),
                request_start_utc_nanos: start_utc_nanos,
                request_end_utc_nanos: start_utc_nanos + 2 * ONE_HOUR_NANOS,
                interval_indices: vec![0, 1],
                interval_start_utc_nanos: interval_start_utc_nanos.clone(),
                interval_end_utc_nanos: interval_end_utc_nanos.clone(),
                interval_start_exchange_local: vec![
                    "2024-01-22T09:00:00-05:00".to_string(),
                    "2024-01-22T10:00:00-05:00".to_string(),
                ],
                interval_end_exchange_local: vec![
                    "2024-01-22T10:00:00-05:00".to_string(),
                    "2024-01-22T11:00:00-05:00".to_string(),
                ],
                s_h: vec![0.95, 1.05],
                nu_h: vec![0.02, 0.03],
                metadata: HourlyFactorMaterializationMetadata {
                    seasonality_materialization_method:
                        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD.to_string(),
                    news_intensity_materialization_method:
                        DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                    interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                    interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                    timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                    spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                    fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
                },
                diagnostics: HourlyFactorContractDiagnostics {
                    interval_count: 2,
                    first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                    last_interval_end_utc_nanos: interval_end_utc_nanos[1],
                    skipped_local_hour_interval_count: 0,
                    repeated_local_hour_interval_count: 0,
                },
            },
        }
    }
}
