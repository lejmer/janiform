use std::path::Path;

use janiform::calendar::ExchangeCalendarRegistry;
use janiform::simulation::{
    CoarseSimulationPath, CoarseSimulationRunRequest, SeededTickSimulationRunConfig,
    SeededTickSimulationRunResult, SimulationProgressEvent, run_coarse_simulation_with_observer,
    run_seeded_tick_event_simulation_with_observer,
};

use super::artifacts::{
    SimulationExportSummary, SimulationOutputColumns, export_coarse_simulation,
    export_seeded_tick_simulation,
};
use crate::error::CliError;

pub(super) enum DirectSimulationJob {
    SeededTick {
        run_config: SeededTickSimulationRunConfig,
        export: SeededTickExportSettings,
    },
    CoarseInterval {
        request: CoarseSimulationRunRequest,
        export: CoarseIntervalExportSettings,
    },
}

pub(super) struct SeededTickExportSettings {
    pub(super) columns: SimulationOutputColumns,
    pub(super) reference_price: f64,
    pub(super) batch_size: usize,
}

pub(super) struct CoarseIntervalExportSettings {
    pub(super) columns: SimulationOutputColumns,
    pub(super) calendar_registry: ExchangeCalendarRegistry,
    pub(super) exchange_code: String,
    pub(super) reference_price: f64,
    pub(super) batch_size: usize,
}

pub(super) enum DirectSimulationCompleted {
    SeededTick {
        result: SeededTickSimulationRunResult,
        export: SeededTickExportSettings,
    },
    CoarseInterval {
        result: CoarseSimulationPath,
        export: CoarseIntervalExportSettings,
    },
}

pub(super) struct DirectSimulationCompletion {
    pub(super) seed: u64,
    pub(super) simulated_interval_count: usize,
}

impl DirectSimulationJob {
    pub(super) fn run(
        self,
        observer: &mut dyn FnMut(SimulationProgressEvent),
    ) -> Result<DirectSimulationCompleted, CliError> {
        match self {
            DirectSimulationJob::SeededTick { run_config, export } => {
                let result = run_seeded_tick_event_simulation_with_observer(run_config, observer)
                    .map_err(|source| CliError::InvalidCommandArgument {
                    argument_name: "--request-json",
                    message: source.to_string(),
                })?;

                Ok(DirectSimulationCompleted::SeededTick { result, export })
            }
            DirectSimulationJob::CoarseInterval { request, export } => {
                let result =
                    run_coarse_simulation_with_observer(request, observer).map_err(|source| {
                        CliError::InvalidCommandArgument {
                            argument_name: "--request-json",
                            message: source.to_string(),
                        }
                    })?;

                Ok(DirectSimulationCompleted::CoarseInterval { result, export })
            }
        }
    }
}

impl DirectSimulationCompleted {
    pub(super) fn export(
        &self,
        output_path: Option<&Path>,
    ) -> Result<Option<SimulationExportSummary>, CliError> {
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

    pub(super) fn completion(&self) -> DirectSimulationCompletion {
        match self {
            DirectSimulationCompleted::SeededTick { result, .. } => DirectSimulationCompletion {
                seed: result.seed,
                simulated_interval_count: result.simulated_interval_count,
            },
            DirectSimulationCompleted::CoarseInterval { result, .. } => {
                DirectSimulationCompletion {
                    seed: result.seed,
                    simulated_interval_count: result.simulated_interval_count,
                }
            }
        }
    }
}
