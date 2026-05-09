use std::path::Path;
use std::sync::mpsc;

use chrono::{DateTime, Utc};
use janiform::simulation::SimulationProgressEvent;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_foundations::randomness::{
    SimulationRandomnessSubstreamDomain, derive_simulation_substream_seed,
};
use serde_json::Value;

use super::super::artifacts::{FullSimulationPathCache, SimulationPathManifest};
use super::super::empirical::{AnalysisWindow, ReturnObservation};
use super::super::request::StylizedFactsValidationWorkflowRequest;
use super::super::workflow::StylizedFactsValidationWorkflowObserver;
use super::progress::{
    SimulationCompletedProgressInput, SimulationIntervalProgressInput,
    SimulationStartedProgressInput, emit_simulation_completed, emit_simulation_interval_progress,
    emit_simulation_plan, emit_simulation_started,
};
use crate::error::WorkflowError;
use crate::progress::ProgressUnitTracker;
use crate::simulation::{
    DEFAULT_SIMULATION_EXPORT_BATCH_SIZE, DirectSimulationCompleted, ModelArtifactSimulationKind,
    ModelArtifactSimulationWorkflowRequest, SimulationOutputColumns,
    run_simulation_from_model_artifact,
};

// Return-derived validation metrics are scale-free. The seeded tick simulator
// still requires a strictly positive export price anchor.
const RETURN_SPACE_REFERENCE_PRICE: f64 = 1.0;
// Validation consumes every generated transition and does not enable debug
// trace export, so the chunk size only satisfies the simulator request contract.
const SEEDED_TICK_DEBUG_TRACE_STRIDE: u64 = 1;
const SEEDED_TICK_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK: usize = 1024;

pub(in crate::validation) struct SimulatedValidationPaths {
    pub(in crate::validation) returns: Vec<ReturnObservation>,
    pub(in crate::validation) manifests: Vec<SimulationPathManifest>,
    pub(in crate::validation) full_paths: Vec<FullSimulationPathCache>,
}

struct SimulatedValidationPathOutput {
    path_position: usize,
    returns: Vec<ReturnObservation>,
    manifest: SimulationPathManifest,
    full_path: Option<FullSimulationPathCache>,
}

pub(in crate::validation) fn simulate_validation_paths(
    request: &StylizedFactsValidationWorkflowRequest,
    model_path: &Path,
    analysis_window: AnalysisWindow,
    runtime_interval_duration_nanos: i64,
    simulated_interval_count: usize,
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
) -> Result<SimulatedValidationPaths, WorkflowError> {
    let model_payload: Value = super::super::artifacts::load_typed_json_artifact(model_path)?;

    let generated_seeds = derived_seeds(request.seed, request.simulation_path_count);

    let worker_count = resolved_cpu_count(request.cpu_count).min(request.simulation_path_count);

    let total_interval_units =
        simulation_progress_total_units(request.simulation_path_count, simulated_interval_count)?;

    emit_simulation_plan(
        observer,
        &generated_seeds,
        worker_count,
        total_interval_units,
    );

    let path_outputs = if worker_count <= 1 {
        simulate_validation_paths_serial(
            request,
            &model_payload,
            analysis_window,
            runtime_interval_duration_nanos,
            simulated_interval_count,
            &generated_seeds,
            observer,
            worker_count,
            total_interval_units,
        )?
    } else {
        simulate_validation_paths_parallel(
            request,
            &model_payload,
            analysis_window,
            runtime_interval_duration_nanos,
            simulated_interval_count,
            &generated_seeds,
            observer,
            worker_count,
            total_interval_units,
        )?
    };

    let mut returns = Vec::new();
    let mut manifests = Vec::with_capacity(path_outputs.len());
    let mut full_paths = Vec::new();

    for output in path_outputs {
        returns.extend(output.returns);
        manifests.push(output.manifest);

        if let Some(full_path) = output.full_path {
            full_paths.push(full_path);
        }
    }

    Ok(SimulatedValidationPaths {
        returns,
        manifests,
        full_paths,
    })
}

#[allow(clippy::too_many_arguments)]
fn simulate_validation_paths_serial(
    request: &StylizedFactsValidationWorkflowRequest,
    model_payload: &Value,
    analysis_window: AnalysisWindow,
    runtime_interval_duration_nanos: i64,
    simulated_interval_count: usize,
    generated_seeds: &[u64],
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    worker_count: usize,
    total_interval_units: usize,
) -> Result<Vec<SimulatedValidationPathOutput>, WorkflowError> {
    let mut outputs = Vec::with_capacity(generated_seeds.len());
    let mut completed_interval_units = 0usize;

    for (path_position, seed) in generated_seeds.iter().copied().enumerate() {
        emit_simulation_started(
            observer,
            SimulationStartedProgressInput {
                path_position,
                seed,
                path_total_intervals: simulated_interval_count,
                total_path_count: generated_seeds.len(),
                completed_units: completed_interval_units,
                total_units: total_interval_units,
                worker_count,
            },
        );

        let mut path_progress_units = ProgressUnitTracker::new(simulated_interval_count);

        let output = {
            let mut progress_observer = |event: SimulationProgressEvent| {
                let completed_delta = path_progress_units.record(event.completed_units);
                if completed_delta == 0 {
                    return;
                }

                completed_interval_units += completed_delta;

                emit_simulation_interval_progress(
                    observer,
                    SimulationIntervalProgressInput {
                        path_position,
                        seed,
                        path_completed_intervals: path_progress_units.completed_units(),
                        path_total_intervals: simulated_interval_count,
                        completed_units: completed_interval_units,
                        total_units: total_interval_units,
                        worker_count,
                    },
                );
            };

            simulate_validation_path(
                request,
                // Intentional copy because the simulator request owns its model payload.
                model_payload.clone(),
                analysis_window,
                runtime_interval_duration_nanos,
                simulated_interval_count,
                path_position,
                seed,
                &mut progress_observer,
            )?
        };

        completed_interval_units += path_progress_units.complete();

        emit_simulation_completed(
            observer,
            SimulationCompletedProgressInput {
                manifest: &output.manifest,
                path_total_intervals: simulated_interval_count,
                completed_units: completed_interval_units,
                total_units: total_interval_units,
                completed_path_count: outputs.len() + 1,
                total_path_count: generated_seeds.len(),
                worker_count,
            },
        );

        outputs.push(output);
    }

    Ok(outputs)
}

#[allow(clippy::too_many_arguments)]
fn simulate_validation_paths_parallel(
    request: &StylizedFactsValidationWorkflowRequest,
    model_payload: &Value,
    analysis_window: AnalysisWindow,
    runtime_interval_duration_nanos: i64,
    simulated_interval_count: usize,
    generated_seeds: &[u64],
    observer: &mut StylizedFactsValidationWorkflowObserver<'_>,
    worker_count: usize,
    total_interval_units: usize,
) -> Result<Vec<SimulatedValidationPathOutput>, WorkflowError> {
    let active_initial_count = worker_count.min(generated_seeds.len());

    let mut path_started = (0..generated_seeds.len())
        .map(|path_position| path_position < active_initial_count)
        .collect::<Vec<_>>();

    for (path_position, seed) in generated_seeds
        .iter()
        .copied()
        .enumerate()
        .take(active_initial_count)
    {
        emit_simulation_started(
            observer,
            SimulationStartedProgressInput {
                path_position,
                seed,
                path_total_intervals: simulated_interval_count,
                total_path_count: generated_seeds.len(),
                completed_units: 0,
                total_units: total_interval_units,
                worker_count,
            },
        );
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .build()
        .map_err(|source| WorkflowError::ValidationInputFailed {
            field: "--cpu-count",
            message: source.to_string(),
        })?;

    let (sender, receiver) = mpsc::channel::<SimulationPathMessage>();

    for (path_position, seed) in generated_seeds.iter().copied().enumerate() {
        let sender = sender.clone();

        // Intentional copy because each simulation request owns its model payload.
        let model_payload = model_payload.clone();

        // Intentional copy because each parallel path needs an owned workflow request.
        let request = request.clone();

        pool.spawn(move || {
            let progress_sender = sender.clone();

            let mut progress_observer = move |event: SimulationProgressEvent| {
                let _ = progress_sender.send(SimulationPathMessage::Progress {
                    path_position,
                    seed,
                    event,
                });
            };

            let result = simulate_validation_path(
                &request,
                model_payload,
                analysis_window,
                runtime_interval_duration_nanos,
                simulated_interval_count,
                path_position,
                seed,
                &mut progress_observer,
            );

            let _ = sender.send(SimulationPathMessage::Completed(result));
        });
    }

    drop(sender);

    let mut completed_count = 0usize;
    let mut completed_interval_units = 0usize;
    let mut next_started_position = active_initial_count;

    let mut path_progress_units = (0..generated_seeds.len())
        .map(|_| ProgressUnitTracker::new(simulated_interval_count))
        .collect::<Vec<_>>();

    let mut outputs = (0..generated_seeds.len()).map(|_| None).collect::<Vec<_>>();

    for message in receiver {
        let output = match message {
            SimulationPathMessage::Progress {
                path_position,
                seed,
                event,
            } => {
                let Some(path_started_flag) = path_started.get_mut(path_position) else {
                    return Err(WorkflowError::ValidationInputFailed {
                        field: "validate stylized-facts",
                        message: "simulation progress referenced an unknown path".to_string(),
                    });
                };

                if !*path_started_flag {
                    emit_simulation_started(
                        observer,
                        SimulationStartedProgressInput {
                            path_position,
                            seed,
                            path_total_intervals: simulated_interval_count,
                            total_path_count: generated_seeds.len(),
                            completed_units: completed_interval_units,
                            total_units: total_interval_units,
                            worker_count,
                        },
                    );

                    *path_started_flag = true;
                }

                let Some(path_progress_units) = path_progress_units.get_mut(path_position) else {
                    return Err(WorkflowError::ValidationInputFailed {
                        field: "validate stylized-facts",
                        message: "simulation progress referenced an unknown path".to_string(),
                    });
                };

                let completed_delta = path_progress_units.record(event.completed_units);
                if completed_delta == 0 {
                    continue;
                }

                completed_interval_units += completed_delta;

                emit_simulation_interval_progress(
                    observer,
                    SimulationIntervalProgressInput {
                        path_position,
                        seed,
                        path_completed_intervals: path_progress_units.completed_units(),
                        path_total_intervals: simulated_interval_count,
                        completed_units: completed_interval_units,
                        total_units: total_interval_units,
                        worker_count,
                    },
                );

                continue;
            }
            SimulationPathMessage::Completed(result) => result?,
        };

        completed_count += 1;

        let path_position = output.path_position;

        let Some(path_started_flag) = path_started.get_mut(path_position) else {
            return Err(WorkflowError::ValidationInputFailed {
                field: "validate stylized-facts",
                message: "simulation result referenced an unknown path".to_string(),
            });
        };

        *path_started_flag = true;

        let Some(path_progress_units) = path_progress_units.get_mut(path_position) else {
            return Err(WorkflowError::ValidationInputFailed {
                field: "validate stylized-facts",
                message: "simulation result referenced an unknown path".to_string(),
            });
        };

        completed_interval_units += path_progress_units.complete();

        emit_simulation_completed(
            observer,
            SimulationCompletedProgressInput {
                manifest: &output.manifest,
                path_total_intervals: simulated_interval_count,
                completed_units: completed_interval_units,
                total_units: total_interval_units,
                completed_path_count: completed_count,
                total_path_count: generated_seeds.len(),
                worker_count,
            },
        );

        while next_started_position < generated_seeds.len() && path_started[next_started_position] {
            next_started_position += 1;
        }

        if let Some(seed) = generated_seeds.get(next_started_position).copied() {
            emit_simulation_started(
                observer,
                SimulationStartedProgressInput {
                    path_position: next_started_position,
                    seed,
                    path_total_intervals: simulated_interval_count,
                    total_path_count: generated_seeds.len(),
                    completed_units: completed_interval_units,
                    total_units: total_interval_units,
                    worker_count,
                },
            );

            path_started[next_started_position] = true;
            next_started_position += 1;
        }

        outputs[path_position] = Some(output);
    }

    outputs
        .into_iter()
        .map(|output| {
            output.ok_or_else(|| WorkflowError::ValidationInputFailed {
                field: "validate stylized-facts",
                message: "simulation did not return every path".to_string(),
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn simulate_validation_path(
    request: &StylizedFactsValidationWorkflowRequest,
    model_payload: Value,
    analysis_window: AnalysisWindow,
    runtime_interval_duration_nanos: i64,
    simulated_interval_count: usize,
    path_position: usize,
    seed: u64,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SimulatedValidationPathOutput, WorkflowError> {
    let simulation_output = run_simulation_from_model_artifact(
        ModelArtifactSimulationWorkflowRequest {
            model_payload,
            run_id: Some(format!("stylized_facts_validation_path_{path_position}")),
            scope_label: Some("stylized facts validation".to_string()),
            request_path: None,
            simulation_kind: ModelArtifactSimulationKind::SeededTick,
            seed,
            start_time_utc: DateTime::<Utc>::from_timestamp_nanos(analysis_window.start_utc_nanos),
            horizon_interval_count: simulated_interval_count,
            interval_duration_nanos: runtime_interval_duration_nanos,
            output_path: None,
            output_columns: SimulationOutputColumns {
                timestamp_column: "timestamp".to_string(),
                price_column: "price".to_string(),
                side_column: Some("side".to_string()),
            },
            reference_price: RETURN_SPACE_REFERENCE_PRICE,
            export_batch_size: DEFAULT_SIMULATION_EXPORT_BATCH_SIZE,
            continuation_data: None,
            initial_excitation_state: None,
            initial_mean_reversion_boundary_state: None,
            debug_trace_enabled: false,
            sample_every_nth_transition: SEEDED_TICK_DEBUG_TRACE_STRIDE,
            max_records_per_chunk: SEEDED_TICK_DEBUG_TRACE_MAX_RECORDS_PER_CHUNK,
            state_snapshot_capture: Default::default(),
        },
        observer,
    )?;

    let DirectSimulationCompleted::SeededTick { result, .. } = simulation_output.completed else {
        return Err(WorkflowError::ValidationInputFailed {
            field: "--model-path",
            message: "validation expected seeded tick simulation output".to_string(),
        });
    };

    let transition_count = result.transition_count();
    let event_count = result.event_count();

    let mut returns = Vec::new();

    for record in &result.fine_path {
        if !record.core_total_return_increment.is_finite() {
            continue;
        }

        let timestamp_utc_nanos = concatenated_path_timestamp(
            record.timestamp_utc_nanos,
            path_position,
            simulated_interval_count,
            runtime_interval_duration_nanos,
        )?;

        returns.push(ReturnObservation {
            timestamp_utc_nanos,
            return_increment: record.core_total_return_increment,
        });
    }

    let manifest = SimulationPathManifest {
        path_position,
        seed,
        transition_count,
        event_count,
    };

    let full_path = (request.cache
        == Some(super::super::output::StylizedFactsValidationCacheLevel::Full))
    .then_some(FullSimulationPathCache {
        path_position,
        seed,
        fine_path: result.fine_path,
    });

    Ok(SimulatedValidationPathOutput {
        path_position,
        returns,
        manifest,
        full_path,
    })
}

enum SimulationPathMessage {
    Progress {
        path_position: usize,
        seed: u64,
        event: SimulationProgressEvent,
    },
    Completed(Result<SimulatedValidationPathOutput, WorkflowError>),
}

fn simulation_progress_total_units(
    path_count: usize,
    interval_count: usize,
) -> Result<usize, WorkflowError> {
    path_count
        .checked_mul(interval_count)
        .ok_or_else(|| WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "simulation progress unit count does not fit in usize".to_string(),
        })
}

pub(in crate::validation) fn derived_seeds(seed: u64, path_count: usize) -> Vec<u64> {
    (0..path_count)
        .map(|path_position| {
            derive_simulation_substream_seed(
                seed,
                SimulationRandomnessSubstreamDomain::StylizedFactsValidationSimulationPath,
                path_position as u64,
            )
        })
        .collect()
}

fn concatenated_path_timestamp(
    timestamp_utc_nanos: i64,
    path_position: usize,
    path_interval_count: usize,
    interval_duration_nanos: i64,
) -> Result<i64, WorkflowError> {
    let offset =
        i128::try_from(path_position).map_err(|_| WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "simulation path position does not fit in i128".to_string(),
        })? * i128::try_from(path_interval_count).map_err(|_| {
            WorkflowError::ValidationInputFailed {
                field: "validate stylized-facts",
                message: "simulation interval count does not fit in i128".to_string(),
            }
        })? * i128::from(interval_duration_nanos);

    i64::try_from(i128::from(timestamp_utc_nanos) + offset).map_err(|_| {
        WorkflowError::ValidationInputFailed {
            field: "validate stylized-facts",
            message: "concatenated simulation timestamp does not fit in i64".to_string(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::concatenated_path_timestamp;

    #[test]
    fn concatenated_path_timestamp_offsets_later_paths_by_full_path_span() {
        let timestamp = concatenated_path_timestamp(1_000, 2, 10, 5).expect("timestamp should fit");

        assert_eq!(timestamp, 1_100);
    }
}
