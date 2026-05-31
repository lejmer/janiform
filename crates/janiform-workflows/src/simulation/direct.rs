use std::path::Path;

use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunRequest, SeededTickSimulationRunConfig,
    SeededTickSimulationRunResult, SimulationProgressEvent, run_coarse_simulation_with_observer,
    run_seeded_tick_event_simulation_with_observer,
};
use janiform::time::calendar::ExchangeCalendarRegistry;

use crate::error::WorkflowError;

use super::artifacts::{
    SimulationExportSummary, SimulationOutputColumns, export_coarse_simulation,
    export_seeded_tick_simulation,
};
use super::intervals::interval_duration_nanos_from_bounds;

/// Direct simulation job accepted by workflow boundaries.
pub enum DirectSimulationJob {
    /// Seeded tick-event simulation.
    SeededTick {
        /// Validated seeded run configuration.
        run_config: Box<SeededTickSimulationRunConfig>,
        /// Export configuration.
        export: SeededTickExportSettings,
    },

    /// Coarse interval simulation.
    CoarseInterval {
        /// Validated coarse run request.
        request: Box<CoarseSimulationRunRequest>,
        /// Export configuration.
        export: CoarseIntervalExportSettings,
    },
}

/// Export settings for seeded tick simulations.
pub struct SeededTickExportSettings {
    /// Output column names.
    pub columns: SimulationOutputColumns,
    /// Reference price used to convert log prices to prices.
    pub reference_price: f64,
    /// Output row batch size for chunked artifact writers.
    pub batch_size: usize,
}

/// Export settings for coarse interval simulations.
pub struct CoarseIntervalExportSettings {
    /// Output column names.
    pub columns: SimulationOutputColumns,
    /// Calendar registry used to omit closed-market intervals from exports.
    pub calendar_registry: ExchangeCalendarRegistry,
    /// Exchange code resolved against the calendar registry.
    pub exchange_code: String,
    /// Reference price used to convert log prices to prices.
    pub reference_price: f64,
    /// Output row batch size for chunked artifact writers.
    pub batch_size: usize,
}

/// Completed direct simulation job.
pub enum DirectSimulationCompleted {
    /// Seeded tick-event result.
    SeededTick {
        /// Engine result.
        result: Box<SeededTickSimulationRunResult>,
        /// Export configuration.
        export: SeededTickExportSettings,
    },

    /// Coarse interval result.
    CoarseInterval {
        /// Engine result.
        result: Box<CoarseSimulationPath>,
        /// Export configuration.
        export: CoarseIntervalExportSettings,
    },
}

/// Common completion fields shared by direct simulations.
pub struct DirectSimulationCompletion {
    /// Simulation random seed.
    pub seed: u64,
    /// Number of simulated intervals.
    pub simulated_interval_count: usize,
    /// Fixed duration of one simulated interval, in UTC nanoseconds.
    pub simulated_interval_duration_nanos: Option<i64>,
    /// Number of generated seeded ticks when the simulation emits tick-level events.
    pub generated_tick_count: Option<usize>,
}

impl DirectSimulationJob {
    /// Run the direct simulation job with progress observation.
    pub fn run(
        self,
        observer: &mut dyn FnMut(SimulationProgressEvent),
    ) -> Result<DirectSimulationCompleted, WorkflowError> {
        match self {
            DirectSimulationJob::SeededTick { run_config, export } => {
                let result = run_seeded_tick_event_simulation_with_observer(*run_config, observer)
                    .map_err(|source| validation_error("request_json", source.to_string()))?;

                Ok(DirectSimulationCompleted::SeededTick {
                    result: Box::new(result),
                    export,
                })
            }
            DirectSimulationJob::CoarseInterval { request, export } => {
                let result = run_coarse_simulation_with_observer(*request, observer)
                    .map_err(|source| validation_error("request_json", source.to_string()))?;

                Ok(DirectSimulationCompleted::CoarseInterval {
                    result: Box::new(result),
                    export,
                })
            }
        }
    }
}

impl DirectSimulationCompleted {
    /// Export the simulation result when an output path is provided.
    pub fn export(
        &self,
        output_path: Option<&Path>,
    ) -> Result<Option<SimulationExportSummary>, WorkflowError> {
        let Some(output_path) = output_path else {
            return Ok(None);
        };

        match self {
            DirectSimulationCompleted::SeededTick { result, export } => {
                export_seeded_tick_simulation(
                    result,
                    output_path,
                    export.reference_price,
                    &export.columns,
                    export.batch_size,
                )
                .map(Some)
            }
            DirectSimulationCompleted::CoarseInterval { result, export } => {
                export_coarse_simulation(
                    result,
                    output_path,
                    export.reference_price,
                    &export.columns,
                    &export.calendar_registry,
                    export.exchange_code.as_str(),
                    export.batch_size,
                )
                .map(Some)
            }
        }
    }

    /// Return common completion fields.
    pub fn completion(&self) -> DirectSimulationCompletion {
        match self {
            DirectSimulationCompleted::SeededTick { result, .. } => DirectSimulationCompletion {
                seed: result.seed,
                simulated_interval_count: result.simulated_interval_count,
                simulated_interval_duration_nanos: seeded_result_interval_duration_nanos(result),
                generated_tick_count: Some(result.event_count()),
            },
            DirectSimulationCompleted::CoarseInterval { result, .. } => {
                DirectSimulationCompletion {
                    seed: result.seed,
                    simulated_interval_count: result.simulated_interval_count,
                    simulated_interval_duration_nanos: interval_duration_nanos_from_bounds(
                        result.start_time_utc_nanos,
                        result.end_time_utc_nanos,
                        result.simulated_interval_count,
                    ),
                    generated_tick_count: None,
                }
            }
        }
    }
}

fn seeded_result_interval_duration_nanos(result: &SeededTickSimulationRunResult) -> Option<i64> {
    interval_duration_nanos_from_bounds(
        result.start_time_utc.timestamp_nanos_opt()?,
        result.end_time_utc.timestamp_nanos_opt()?,
        result.simulated_interval_count,
    )
}

fn validation_error(field: &'static str, message: impl Into<String>) -> WorkflowError {
    WorkflowError::SimulationValidationFailed {
        field,
        message: message.into(),
    }
}
