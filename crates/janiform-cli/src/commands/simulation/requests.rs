use std::path::Path;

use chrono::{DateTime, Utc};
use janiform::model::factors::interval_factor_grid::{
    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD, INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
    INTERVAL_FACTOR_GRID_SCHEMA_VERSION, INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY, IntervalFactorGrid,
    IntervalFactorGridDiagnostics, IntervalFactorGridMetadata, IntervalFactorGridPayload,
    load_interval_factor_grid,
};
use janiform::model::hawkes::{HawkesExcitationState, HawkesKernelParameters};
use janiform::model::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform::model::market_pressure::{MarketPressureParameters, MarketPressureState};
use janiform::model::news::NewsProcessConfiguration;
use janiform::model::nig::NigSharedPlusShiftConfiguration;
use janiform::model::return_clock::ReturnClockExposureParameters;
use janiform::model::sufficient_statistics::SufficientStatisticsIntervalFactors;
use janiform::model::timestamp_batching_surface::TimestampBatchingSurface;
use janiform::simulation::{
    CoarseSimulationModelInputs, CoarseSimulationRunRequest, MeanReversionBoundaryState,
    MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
};
use janiform_foundations::time::parse_utc_datetime;
use serde::Deserialize;

use crate::commands::shared::artifacts::{
    load_calendar_registry_artifact, load_typed_json_artifact,
};
use crate::error::CliError;

const SIMULATION_ARTIFACT_NAME: &str = "simulation request";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CoarseSimulationRequestDocument {
    seed: u64,
    start_time_utc_nanos: i64,
    horizon_interval_count: usize,
    interval_delta: f64,
    model_inputs: CoarseSimulationModelInputs,
    interval_factor_grid: IntervalFactorGridPayload,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationRequestDocument {
    seed: u64,
    start_time_utc: String,
    horizon_interval_count: usize,
    kernel_parameters: HawkesKernelParameters,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    news_process_configuration: NewsProcessConfiguration,
    initial_excitation_state: HawkesExcitationState,
    mean_reversion_fitted_parameters: MeanReversionFittedParameters,
    mean_reversion_runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    initial_mean_reversion_boundary_state: MeanReversionBoundaryState,
    market_pressure_parameters: MarketPressureParameters,
    initial_market_pressure_state: MarketPressureState,
    interval_factor_grid_summary: SeededTickIntervalFactorGridSummary,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    long_variance_parameters: LongVarianceParameters,
    initial_long_variance_state: LongVarianceState,
    observed_price_tick_size: Option<f64>,
    timestamp_batching_surface: TimestampBatchingSurface,
    #[serde(default)]
    debug_trace_enabled: bool,
    #[serde(default = "default_sample_every_nth_transition")]
    sample_every_nth_transition: u64,
    #[serde(default = "default_max_records_per_chunk")]
    max_records_per_chunk: usize,
    #[serde(default)]
    state_snapshot_capture: SeededTickEventStateSnapshotCapture,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickIntervalFactorGridSummary {
    schema_version: String,
    exchange_code: String,
    interval_count: usize,
    interval_duration_nanos: i64,
    interval_start_utc_nanos: Vec<i64>,
    interval_end_utc_nanos: Vec<i64>,
    seasonality_factor: Vec<f64>,
    news_intensity: Vec<f64>,
}

pub(crate) fn load_coarse_simulation_request(
    path: &Path,
) -> Result<CoarseSimulationRunRequest, CliError> {
    let document: CoarseSimulationRequestDocument =
        load_typed_json_artifact(path, SIMULATION_ARTIFACT_NAME)?;

    let interval_factor_grid =
        load_interval_factor_grid(document.interval_factor_grid).map_err(|source| {
            CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message: source.to_string(),
            }
        })?;

    let interval_factor_grids = SufficientStatisticsIntervalFactors::try_new(
        interval_factor_grid.seasonality_factor,
        interval_factor_grid.known_news_intensity,
        interval_factor_grid.surprise_news_intensity,
        Some(document.interval_delta),
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--request-json",
        message: source.to_string(),
    })?;

    CoarseSimulationRunRequest::try_new(
        document.seed,
        document.start_time_utc_nanos,
        document.horizon_interval_count,
        document.interval_delta,
        document.model_inputs,
        interval_factor_grids,
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--request-json",
        message: source.to_string(),
    })
}

pub(crate) fn load_seeded_tick_simulation_request(
    request_path: &Path,
    calendar_registry_path: &Path,
) -> Result<SeededTickSimulationRunConfig, CliError> {
    let document: SeededTickSimulationRequestDocument =
        load_typed_json_artifact(request_path, SIMULATION_ARTIFACT_NAME)?;

    let calendar_registry = load_calendar_registry_artifact(calendar_registry_path)?;
    let start_time_utc = parse_start_time_utc(&document.start_time_utc)?;
    let interval_factor_grid = document.interval_factor_grid_summary.into_contract()?;

    let run_options = SeededTickSimulationRunOptions {
        debug_trace_enabled: document.debug_trace_enabled,
        debug_trace_options: SeededTickSimulationDebugTraceOptions {
            sample_every_nth_transition: document.sample_every_nth_transition,
            max_records_per_chunk: document.max_records_per_chunk,
        },
        state_snapshot_capture: document.state_snapshot_capture,
    };

    SeededTickSimulationRunConfig::try_new(
        document.seed,
        start_time_utc,
        document.horizon_interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters: document.kernel_parameters,
            nig_shared_plus_shift_configuration: document.nig_shared_plus_shift_configuration,
            news_process_configuration: document.news_process_configuration,
            initial_excitation_state: document.initial_excitation_state,
            mean_reversion_fitted_parameters: document.mean_reversion_fitted_parameters,
            mean_reversion_runtime_interval_anchors: document
                .mean_reversion_runtime_interval_anchors,
            initial_mean_reversion_boundary_state: document.initial_mean_reversion_boundary_state,
            market_pressure_parameters: document.market_pressure_parameters,
            initial_market_pressure_state: document.initial_market_pressure_state,
            interval_factor_grid,
            return_clock_exposure_parameters: document.return_clock_exposure_parameters,
            long_variance_parameters: document.long_variance_parameters,
            initial_long_variance_state: document.initial_long_variance_state,
            observed_price_tick_size: document.observed_price_tick_size,
            timestamp_batching_surface: document.timestamp_batching_surface,
            calendar_registry,
        },
        Some(run_options),
    )
    .map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--request-json",
        message: source.to_string(),
    })
}

impl SeededTickIntervalFactorGridSummary {
    fn into_contract(self) -> Result<IntervalFactorGrid, CliError> {
        self.validate()?;

        let interval_count = self.interval_count;

        let interval_count_u64 =
            u64::try_from(interval_count).map_err(|source| CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message: format!(
                    "interval_factor_grid_summary.interval_count is too large: {source}"
                ),
            })?;

        let first_interval_start_utc_nanos = self.interval_start_utc_nanos[0];
        let last_interval_end_utc_nanos = self.interval_end_utc_nanos[interval_count - 1];

        if self.schema_version != INTERVAL_FACTOR_GRID_SCHEMA_VERSION {
            return Err(CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message: format!(
                    "interval_factor_grid_summary.schema_version must be {INTERVAL_FACTOR_GRID_SCHEMA_VERSION}"
                ),
            });
        }

        Ok(IntervalFactorGrid {
            schema_version: INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
            exchange_code: self.exchange_code,
            exchange_timezone_name: "UTC".to_string(),
            request_start_utc_nanos: first_interval_start_utc_nanos,
            request_end_utc_nanos: last_interval_end_utc_nanos,
            interval_duration_nanos: self.interval_duration_nanos,
            interval_indices: (0..interval_count_u64).collect(),
            interval_start_utc_nanos: self.interval_start_utc_nanos,
            interval_end_utc_nanos: self.interval_end_utc_nanos,
            interval_start_exchange_local: Vec::new(),
            interval_end_exchange_local: Vec::new(),
            seasonality_factor: self.seasonality_factor,
            known_news_intensity: vec![0.0; interval_count],
            // The direct seeded summary has combined news intensity only.
            // Store it as surprise intensity and as the combined vector used by the engine.
            surprise_news_intensity: self.news_intensity.clone(),
            news_intensity: self.news_intensity,
            metadata: IntervalFactorGridMetadata {
                seasonality_materialization_method:
                    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD.to_string(),
                news_intensity_materialization_method:
                    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD.to_string(),
                interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
                interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
                timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
                spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
                fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
            },
            diagnostics: IntervalFactorGridDiagnostics {
                interval_count,
                interval_duration_nanos: self.interval_duration_nanos,
                first_interval_start_utc_nanos,
                last_interval_end_utc_nanos,
                skipped_exchange_local_interval_count: 0,
                repeated_exchange_local_interval_count: 0,
            },
        })
    }

    fn validate(&self) -> Result<(), CliError> {
        if self.interval_count == 0 {
            return Err(CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message: "interval_factor_grid_summary.interval_count must be strictly positive"
                    .to_string(),
            });
        }

        if self.interval_duration_nanos <= 0 {
            return Err(CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message:
                    "interval_factor_grid_summary.interval_duration_nanos must be strictly positive"
                        .to_string(),
            });
        }

        let field_lengths = [
            (
                "interval_factor_grid_summary.interval_start_utc_nanos",
                self.interval_start_utc_nanos.len(),
            ),
            (
                "interval_factor_grid_summary.interval_end_utc_nanos",
                self.interval_end_utc_nanos.len(),
            ),
            (
                "interval_factor_grid_summary.seasonality_factor",
                self.seasonality_factor.len(),
            ),
            (
                "interval_factor_grid_summary.news_intensity",
                self.news_intensity.len(),
            ),
        ];

        for (field, length) in field_lengths {
            if length != self.interval_count {
                return Err(CliError::InvalidCommandArgument {
                    argument_name: "--request-json",
                    message: format!("{field} must contain one entry per interval"),
                });
            }
        }

        for (interval_index, (interval_start, interval_end)) in self
            .interval_start_utc_nanos
            .iter()
            .zip(self.interval_end_utc_nanos.iter())
            .enumerate()
        {
            let observed_width =
                interval_end
                    .checked_sub(*interval_start)
                    .ok_or_else(|| CliError::InvalidCommandArgument {
                        argument_name: "--request-json",
                        message: format!(
                            "interval_factor_grid_summary interval {interval_index} must have increasing bounds"
                        ),
                    })?;

            if observed_width != self.interval_duration_nanos {
                return Err(CliError::InvalidCommandArgument {
                    argument_name: "--request-json",
                    message: format!(
                        "interval_factor_grid_summary interval {interval_index} must have width {} nanos",
                        self.interval_duration_nanos
                    ),
                });
            }
        }

        for (field, values) in [
            (
                "interval_factor_grid_summary.seasonality_factor",
                self.seasonality_factor.as_slice(),
            ),
            (
                "interval_factor_grid_summary.news_intensity",
                self.news_intensity.as_slice(),
            ),
        ] {
            for (interval_index, value) in values.iter().copied().enumerate() {
                if !value.is_finite() || value < 0.0 {
                    return Err(CliError::InvalidCommandArgument {
                        argument_name: "--request-json",
                        message: format!(
                            "{field}[{interval_index}] must be finite and non-negative"
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

fn parse_start_time_utc(value: &str) -> Result<DateTime<Utc>, CliError> {
    parse_utc_datetime(value).map_err(|source| CliError::InvalidCommandArgument {
        argument_name: "--request-json",
        message: format!("start_time_utc must be an RFC 3339 timestamp: {source}"),
    })
}

fn default_sample_every_nth_transition() -> u64 {
    SeededTickSimulationDebugTraceOptions::default().sample_every_nth_transition
}

fn default_max_records_per_chunk() -> usize {
    SeededTickSimulationDebugTraceOptions::default().max_records_per_chunk
}
