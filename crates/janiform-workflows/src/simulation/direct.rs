use std::path::Path;

use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunError, CoarseSimulationRunRequest,
    SeededTickSimulationRunConfig, SeededTickSimulationRunError, SeededTickSimulationRunResult,
    SimulationProgressEvent, run_coarse_simulation_with_observer,
    run_seeded_tick_event_simulation_with_observer,
};
use janiform::time::calendar::ExchangeCalendarRegistry;
use janiform_foundations::cancellation::CancellationToken;

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
    },

    /// Coarse interval simulation.
    CoarseInterval {
        /// Validated coarse run request.
        request: Box<CoarseSimulationRunRequest>,
    },
}

/// Export plan for direct simulation outputs.
pub enum DirectSimulationExportPlan {
    /// Seeded tick-event export settings.
    SeededTick(SeededTickExportSettings),
    /// Coarse interval export settings.
    CoarseInterval(CoarseIntervalExportSettings),
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
    },

    /// Coarse interval result.
    CoarseInterval {
        /// Engine result.
        result: Box<CoarseSimulationPath>,
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
        cancellation_token: CancellationToken,
        observer: &mut dyn FnMut(SimulationProgressEvent),
    ) -> Result<DirectSimulationCompleted, WorkflowError> {
        match self {
            DirectSimulationJob::SeededTick { run_config } => {
                let result = run_seeded_tick_event_simulation_with_observer(
                    *run_config,
                    cancellation_token,
                    observer,
                )
                .map_err(seed_tick_error_to_workflow_error)?;

                Ok(DirectSimulationCompleted::SeededTick {
                    result: Box::new(result),
                })
            }
            DirectSimulationJob::CoarseInterval { request } => {
                let result =
                    run_coarse_simulation_with_observer(*request, cancellation_token, observer)
                        .map_err(coarse_error_to_workflow_error)?;

                Ok(DirectSimulationCompleted::CoarseInterval {
                    result: Box::new(result),
                })
            }
        }
    }
}

fn seed_tick_error_to_workflow_error(source: SeededTickSimulationRunError) -> WorkflowError {
    match source {
        SeededTickSimulationRunError::Cancelled { source } => WorkflowError::Cancelled { source },
        source => validation_error("request_json", source.to_string()),
    }
}

fn coarse_error_to_workflow_error(source: CoarseSimulationRunError) -> WorkflowError {
    match source {
        CoarseSimulationRunError::Cancelled { source } => WorkflowError::Cancelled { source },
        source => validation_error("request_json", source.to_string()),
    }
}

impl DirectSimulationCompleted {
    /// Export the simulation result when an output path is provided.
    pub fn export(
        &self,
        output_path: Option<&Path>,
        export_plan: Option<&DirectSimulationExportPlan>,
    ) -> Result<Option<SimulationExportSummary>, WorkflowError> {
        let Some(output_path) = output_path else {
            return Ok(None);
        };

        let Some(export_plan) = export_plan else {
            return Err(validation_error(
                "export_plan",
                "output path requires simulation export settings",
            ));
        };

        match (self, export_plan) {
            (
                DirectSimulationCompleted::SeededTick { result },
                DirectSimulationExportPlan::SeededTick(export),
            ) => export_seeded_tick_simulation(
                result,
                output_path,
                export.reference_price,
                &export.columns,
                export.batch_size,
            )
            .map(Some),
            (
                DirectSimulationCompleted::CoarseInterval { result },
                DirectSimulationExportPlan::CoarseInterval(export),
            ) => export_coarse_simulation(
                result,
                output_path,
                export.reference_price,
                &export.columns,
                &export.calendar_registry,
                export.exchange_code.as_str(),
                export.batch_size,
            )
            .map(Some),
            _ => Err(validation_error(
                "export_plan",
                "simulation export settings do not match the completed simulation kind",
            )),
        }
    }

    /// Return common completion fields.
    pub fn completion(&self) -> DirectSimulationCompletion {
        match self {
            DirectSimulationCompleted::SeededTick { result } => DirectSimulationCompletion {
                seed: result.seed,
                simulated_interval_count: result.simulated_interval_count,
                simulated_interval_duration_nanos: seeded_result_interval_duration_nanos(result),
                generated_tick_count: Some(result.event_count()),
            },
            DirectSimulationCompleted::CoarseInterval { result } => DirectSimulationCompletion {
                seed: result.seed,
                simulated_interval_count: result.simulated_interval_count,
                simulated_interval_duration_nanos: interval_duration_nanos_from_bounds(
                    result.start_time_utc_nanos,
                    result.end_time_utc_nanos,
                    result.simulated_interval_count,
                ),
                generated_tick_count: None,
            },
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
