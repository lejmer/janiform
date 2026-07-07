use std::sync::Arc;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::market_pressure::{
    MarketPressureParameters, signed_balance_exposure_scale_from_side_exposures,
};
use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};
use janiform_work_scheduler::{StaticWorkPoolInput, run_static_work_pool};

use super::errors::MarketPressureFitError;
use super::hidden_state::{HiddenSideBalanceFit, fit_hidden_side_balance_model};
use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
use crate::stages::parallelism::{
    ContiguousWorkChunk, bounded_stage_worker_count, build_stage_thread_pool,
    contiguous_work_chunks,
};
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn,
    ReturnComponentsStageMarketPressureSelectionDiagnostic,
    average_negative_log_likelihood_resolution,
};

pub(crate) struct MarketPressureFitInput<'a> {
    pub(crate) observed_open_session_returns: &'a [ReturnComponentsObservedOpenSessionReturn],
    pub(crate) coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    pub(crate) coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    pub(crate) source_window_open_exposure_hours: f64,
    pub(crate) solver_settings: &'a CalibrationOptimizerSettings,
    pub(crate) progress_observer: Option<MarketPressureFitProgressObserver<'a>>,
}

pub(crate) struct MarketPressureNigFitInput<'a> {
    pub(crate) observed_open_session_returns: &'a [ReturnComponentsObservedOpenSessionReturn],
    pub(crate) coarse_data_records: Option<&'a [MarketDataNormalizedCoarseDataRecord]>,
    pub(crate) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(crate) source_window_open_exposure_hours: f64,
    pub(crate) solver_settings: &'a CalibrationOptimizerSettings,
    pub(crate) progress_observer: Option<MarketPressureFitProgressObserver<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MarketPressureFitOutput {
    pub(crate) parameters: MarketPressureParameters,
    pub(crate) runtime_state: Arc<MarketPressureRuntimeState>,
    pub(crate) selection_diagnostics: Vec<ReturnComponentsStageMarketPressureSelectionDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarketPressureFitProgress {
    pub(crate) completed_units: usize,
    pub(crate) total_units: usize,
    pub(crate) current_item_label: &'static str,
}

pub(crate) type MarketPressureFitProgressObserver<'a> =
    &'a mut (dyn FnMut(MarketPressureFitProgress) + Send);

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MarketPressureCoarseReturnLaw {
    pub(crate) configuration: NigSharedPlusShiftConfiguration,
    pub(crate) driver_parameters: NigBuySellDriverParameters,
    pub(crate) log_density_resolution: f64,
}

#[derive(Clone, PartialEq)]
pub(crate) struct MarketPressureRuntimeState {
    pub(super) observations: Vec<MarketPressureSideBalanceObservation>,
    pub(super) coarse_return_record_count: usize,
    pub(super) coarse_return_observations: Vec<MarketPressureCoarseReturnObservation>,
    pub(super) coarse_return_pressure_evidence_available: bool,
    pub(super) open_event_rate_per_hour: f64,
    pub(super) hidden_fit: HiddenSideBalanceFit,
}

impl std::fmt::Debug for MarketPressureRuntimeState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MarketPressureRuntimeState")
            .field("observation_count", &self.observations.len())
            .field(
                "coarse_return_record_count",
                &self.coarse_return_record_count,
            )
            .field(
                "coarse_return_observation_count",
                &self.coarse_return_observations.len(),
            )
            .field(
                "coarse_return_pressure_evidence_available",
                &self.coarse_return_pressure_evidence_available,
            )
            .field("open_event_rate_per_hour", &self.open_event_rate_per_hour)
            .field(
                "pressure_state_count",
                &self.hidden_fit.support_levels.len(),
            )
            .finish()
    }
}

impl MarketPressureRuntimeState {
    pub(crate) fn covers_coarse_return_record_count(
        &self,
        coarse_return_record_count: usize,
    ) -> bool {
        if coarse_return_record_count == 0 {
            return true;
        }

        if self.coarse_return_record_count != coarse_return_record_count {
            return false;
        }

        self.coarse_return_pressure_evidence_available || self.coarse_return_observations.is_empty()
    }

    pub(crate) fn can_refit_coarse_return_pressure_for_record_count(
        &self,
        coarse_return_record_count: usize,
    ) -> bool {
        coarse_return_record_count > 0
            && self.coarse_return_record_count == coarse_return_record_count
            && !self.coarse_return_observations.is_empty()
    }
}

struct SideBalanceObservationChunk {
    observations: Vec<MarketPressureSideBalanceObservation>,
    timestamps_are_ordered: bool,
}

struct SideBalanceObservationBuildOutput {
    start_position: usize,
    result: Result<SideBalanceObservationChunk, MarketPressureFitError>,
}

struct MarketPressureObservationFitInput<'a> {
    observations: &'a [MarketPressureSideBalanceObservation],
    coarse_return_observations: &'a [MarketPressureCoarseReturnObservation],
    coarse_return_record_count: usize,
    coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
    coarse_return_pressure_evidence_available: bool,
    open_event_rate_per_hour: f64,
    solver_settings: &'a CalibrationOptimizerSettings,
}

impl MarketPressureCoarseReturnLaw {
    pub(crate) fn from_nig_configuration(
        configuration: NigSharedPlusShiftConfiguration,
        coarse_return_observation_count: usize,
    ) -> Result<Option<Self>, MarketPressureFitError> {
        if coarse_return_observation_count == 0 {
            return Ok(None);
        }

        let driver_parameters = configuration.derive_driver_parameters().map_err(|source| {
            MarketPressureFitError::InvalidCoarseReturnLaw {
                error_message: source.to_string(),
            }
        })?;

        let log_density_resolution =
            average_negative_log_likelihood_resolution(coarse_return_observation_count);
        if !log_density_resolution.is_finite() || log_density_resolution <= 0.0 {
            return Err(MarketPressureFitError::InvalidCoarseReturnLaw {
                error_message: format!("invalid log-density resolution {log_density_resolution}"),
            });
        }

        Ok(Some(Self {
            configuration,
            driver_parameters,
            log_density_resolution,
        }))
    }
}

pub(crate) fn fit_market_pressure(
    mut input: MarketPressureFitInput<'_>,
) -> Result<MarketPressureFitOutput, MarketPressureFitError> {
    if input.observed_open_session_returns.is_empty() {
        return Err(MarketPressureFitError::EmptyObservedOpenSessionReturns);
    }

    let observations = build_market_pressure_side_balance_observations(
        input.observed_open_session_returns,
        input.solver_settings,
        &mut input.progress_observer,
    )?;

    emit_market_pressure_fit_progress(
        &mut input.progress_observer,
        0,
        1,
        "prepare coarse-return pressure evidence",
    );

    let coarse_data_records = input.coarse_data_records.unwrap_or_default();
    let coarse_return_record_count = coarse_data_records.len();
    let coarse_return_observations =
        build_market_pressure_coarse_return_observations(coarse_data_records);

    let coarse_return_law = input
        .coarse_return_law
        .filter(|_| !coarse_return_observations.is_empty());
    let coarse_return_pressure_evidence_available = coarse_return_law.is_some();

    let open_event_rate_per_hour = open_event_rate_per_hour(
        observations.as_slice(),
        input.source_window_open_exposure_hours,
    );

    let output = fit_market_pressure_from_observations(
        MarketPressureObservationFitInput {
            observations: &observations,
            coarse_return_observations: coarse_return_observations.as_slice(),
            coarse_return_record_count,
            coarse_return_law,
            coarse_return_pressure_evidence_available,
            open_event_rate_per_hour,
            solver_settings: input.solver_settings,
        },
        &mut input.progress_observer,
    )?;

    Ok(output)
}

pub(crate) fn fit_market_pressure_from_nig_configuration(
    input: MarketPressureNigFitInput<'_>,
) -> Result<MarketPressureFitOutput, MarketPressureFitError> {
    let coarse_return_law = MarketPressureCoarseReturnLaw::from_nig_configuration(
        input.nig_configuration,
        input.coarse_data_records.map_or(0, |records| records.len()),
    )?;

    fit_market_pressure(MarketPressureFitInput {
        observed_open_session_returns: input.observed_open_session_returns,
        coarse_data_records: input.coarse_data_records,
        coarse_return_law,
        source_window_open_exposure_hours: input.source_window_open_exposure_hours,
        solver_settings: input.solver_settings,
        progress_observer: input.progress_observer,
    })
}

fn fit_market_pressure_from_observations(
    input: MarketPressureObservationFitInput<'_>,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<MarketPressureFitOutput, MarketPressureFitError> {
    let hidden_fit = fit_hidden_side_balance_model(
        input.observations,
        input.coarse_return_observations,
        input.coarse_return_law,
        input.open_event_rate_per_hour,
        input.solver_settings,
        progress_observer,
    )?;

    let runtime_state = Arc::new(MarketPressureRuntimeState {
        observations: input.observations.to_vec(),
        coarse_return_record_count: input.coarse_return_record_count,
        coarse_return_observations: input.coarse_return_observations.to_vec(),
        coarse_return_pressure_evidence_available: input.coarse_return_pressure_evidence_available,
        open_event_rate_per_hour: input.open_event_rate_per_hour,
        hidden_fit,
    });

    let parameters = market_pressure_parameters_from_runtime_state(&runtime_state)?;

    // The fit output owns the stage-artifact diagnostics alongside the reusable runtime state.
    let selection_diagnostics = runtime_state.hidden_fit.selection_diagnostics.clone();

    Ok(MarketPressureFitOutput {
        parameters,
        runtime_state,
        selection_diagnostics,
    })
}

pub(crate) fn market_pressure_parameters_from_runtime_state(
    runtime_state: &MarketPressureRuntimeState,
) -> Result<MarketPressureParameters, MarketPressureFitError> {
    let support_levels = runtime_state.hidden_fit.support_levels.as_slice();
    if support_levels.len() == 1 && support_levels[0] == 0.0 {
        return Ok(MarketPressureParameters::no_anchor_impact());
    }

    let anchor_drift_per_signed_balance = estimate_anchor_drift_per_signed_balance(
        runtime_state.observations.as_slice(),
        support_levels,
        &runtime_state.hidden_fit.state_probabilities,
        runtime_state.coarse_return_observations.as_slice(),
        &runtime_state.hidden_fit.coarse_state_probabilities,
        runtime_state.open_event_rate_per_hour,
    )?;

    let transition_rates = runtime_state.hidden_fit.transition_rates.clone();

    MarketPressureParameters::try_new(
        support_levels.to_vec(),
        transition_rates,
        anchor_drift_per_signed_balance,
    )
    .map_err(|source| MarketPressureFitError::InvalidParameters { source })
}

pub(crate) fn emit_market_pressure_fit_progress(
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
    completed_units: usize,
    total_units: usize,
    current_item_label: &'static str,
) {
    let Some(progress_observer) = progress_observer.as_mut() else {
        return;
    };

    progress_observer(MarketPressureFitProgress {
        completed_units,
        total_units,
        current_item_label,
    });
}

fn build_market_pressure_side_balance_observations(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Result<Vec<MarketPressureSideBalanceObservation>, MarketPressureFitError> {
    let worker_count = bounded_stage_worker_count(
        solver_settings.cpu_count,
        observed_open_session_returns.len(),
    );

    let jobs = contiguous_work_chunks(observed_open_session_returns.len(), worker_count);

    emit_market_pressure_fit_progress(
        progress_observer,
        0,
        jobs.len().max(1),
        "build side-balance observations",
    );

    let chunks = if worker_count <= 1 {
        build_side_balance_observation_chunks_sequential(
            observed_open_session_returns,
            jobs,
            progress_observer,
        )
    } else {
        build_side_balance_observation_chunks_parallel(
            observed_open_session_returns,
            jobs,
            worker_count,
            solver_settings,
            progress_observer,
        )
    };

    assemble_side_balance_observation_chunks(chunks)
}

fn build_side_balance_observation_chunks_sequential(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    jobs: Vec<ContiguousWorkChunk>,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Vec<SideBalanceObservationBuildOutput> {
    let total_units = jobs.len().max(1);
    let mut outputs = Vec::with_capacity(jobs.len());

    for job in jobs {
        outputs.push(build_side_balance_observation_chunk(
            observed_open_session_returns,
            job,
        ));

        emit_market_pressure_fit_progress(
            progress_observer,
            outputs.len(),
            total_units,
            "build side-balance observations",
        );
    }

    outputs
}

fn build_side_balance_observation_chunks_parallel(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    jobs: Vec<ContiguousWorkChunk>,
    worker_count: usize,
    solver_settings: &CalibrationOptimizerSettings,
    progress_observer: &mut Option<MarketPressureFitProgressObserver<'_>>,
) -> Vec<SideBalanceObservationBuildOutput> {
    let total_units = jobs.len().max(1);
    let thread_pool = build_stage_thread_pool(solver_settings.cpu_count);
    let mut outputs = Vec::with_capacity(jobs.len());

    run_static_work_pool(
        thread_pool.as_deref(),
        StaticWorkPoolInput::new(
            &mut outputs,
            CancellationToken::uncancelled(),
            jobs,
            worker_count,
            |job: ContiguousWorkChunk, _cancellation_token| {
                build_side_balance_observation_chunk(observed_open_session_returns, job)
            },
            |outputs: &mut Vec<SideBalanceObservationBuildOutput>, output, _status| {
                outputs.push(output);

                emit_market_pressure_fit_progress(
                    progress_observer,
                    outputs.len(),
                    total_units,
                    "build side-balance observations",
                );
            },
        ),
    );

    outputs
}

fn build_side_balance_observation_chunk(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    job: ContiguousWorkChunk,
) -> SideBalanceObservationBuildOutput {
    let result = build_side_balance_observation_chunk_result(observed_open_session_returns, job);

    SideBalanceObservationBuildOutput {
        start_position: job.start_position,
        result,
    }
}

fn build_side_balance_observation_chunk_result(
    observed_open_session_returns: &[ReturnComponentsObservedOpenSessionReturn],
    job: ContiguousWorkChunk,
) -> Result<SideBalanceObservationChunk, MarketPressureFitError> {
    let mut observations = Vec::with_capacity(job.end_position.saturating_sub(job.start_position));
    let mut previous_timestamp_utc_nanos = None;
    let mut timestamps_are_ordered = true;

    for observed_return in &observed_open_session_returns[job.start_position..job.end_position] {
        let observation = market_pressure_side_balance_observation(*observed_return)?;

        if let Some(previous_timestamp_utc_nanos) = previous_timestamp_utc_nanos
            && observation.return_timestamp_utc_nanos < previous_timestamp_utc_nanos
        {
            timestamps_are_ordered = false;
        }

        previous_timestamp_utc_nanos = Some(observation.return_timestamp_utc_nanos);
        observations.push(observation);
    }

    Ok(SideBalanceObservationChunk {
        observations,
        timestamps_are_ordered,
    })
}

fn assemble_side_balance_observation_chunks(
    mut chunks: Vec<SideBalanceObservationBuildOutput>,
) -> Result<Vec<MarketPressureSideBalanceObservation>, MarketPressureFitError> {
    chunks.sort_by_key(|chunk| chunk.start_position);

    let observation_count = chunks
        .iter()
        .filter_map(|chunk| chunk.result.as_ref().ok())
        .map(|chunk| chunk.observations.len())
        .sum::<usize>();

    let mut observations = Vec::with_capacity(observation_count);
    let mut previous_timestamp_utc_nanos = None;
    let mut timestamps_are_ordered = true;

    for chunk_output in chunks {
        let chunk = chunk_output.result?;

        timestamps_are_ordered &= chunk.timestamps_are_ordered;

        for observation in chunk.observations {
            if let Some(previous_timestamp_utc_nanos) = previous_timestamp_utc_nanos
                && observation.return_timestamp_utc_nanos < previous_timestamp_utc_nanos
            {
                timestamps_are_ordered = false;
            }

            previous_timestamp_utc_nanos = Some(observation.return_timestamp_utc_nanos);
            observations.push(observation);
        }
    }

    if !timestamps_are_ordered {
        observations.sort_by_key(|observation| observation.return_timestamp_utc_nanos);
    }

    Ok(observations)
}

fn build_market_pressure_coarse_return_observations(
    coarse_data_records: &[MarketDataNormalizedCoarseDataRecord],
) -> Vec<MarketPressureCoarseReturnObservation> {
    let mut observations = Vec::with_capacity(coarse_data_records.len());
    let mut previous_timestamp_utc_nanos = None;
    let mut timestamps_are_ordered = true;

    for observation in coarse_data_records
        .iter()
        .filter_map(market_pressure_coarse_return_observation)
    {
        if let Some(previous_timestamp_utc_nanos) = previous_timestamp_utc_nanos
            && observation.interval_end_utc_nanos < previous_timestamp_utc_nanos
        {
            timestamps_are_ordered = false;
        }

        previous_timestamp_utc_nanos = Some(observation.interval_end_utc_nanos);
        observations.push(observation);
    }

    if !timestamps_are_ordered {
        observations.sort_by_key(|observation| observation.interval_end_utc_nanos);
    }

    observations
}

fn market_pressure_coarse_return_observation(
    record: &MarketDataNormalizedCoarseDataRecord,
) -> Option<MarketPressureCoarseReturnObservation> {
    let log_return = record
        .close_to_close_log_return
        .filter(|log_return| log_return.is_finite())?;

    let interval_duration_nanos = record
        .interval_end_utc_nanos
        .checked_sub(record.interval_start_utc_nanos)?;
    if interval_duration_nanos <= 0 {
        return None;
    }

    let elapsed_hours = duration_nanos_to_hours(interval_duration_nanos);
    if !elapsed_hours.is_finite() || elapsed_hours <= 0.0 {
        return None;
    }

    Some(MarketPressureCoarseReturnObservation {
        interval_end_utc_nanos: record.interval_end_utc_nanos,
        elapsed_hours,
        log_return,
    })
}

fn open_event_rate_per_hour(
    observations: &[MarketPressureSideBalanceObservation],
    source_window_open_exposure_hours: f64,
) -> f64 {
    if !source_window_open_exposure_hours.is_finite() || source_window_open_exposure_hours <= 0.0 {
        return 0.0;
    }

    let total_side_exposure = observations
        .iter()
        .map(|observation| observation.signed_balance_exposure_scale)
        .sum::<f64>();

    if !total_side_exposure.is_finite() || total_side_exposure <= 0.0 {
        return 0.0;
    }

    total_side_exposure / source_window_open_exposure_hours
}

fn market_pressure_side_balance_observation(
    observed_return: ReturnComponentsObservedOpenSessionReturn,
) -> Result<MarketPressureSideBalanceObservation, MarketPressureFitError> {
    if !observed_return.elapsed_same_session_hours.is_finite()
        || observed_return.elapsed_same_session_hours <= 0.0
    {
        return Err(MarketPressureFitError::InvalidElapsedSameSessionHours {
            return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
            value: observed_return.elapsed_same_session_hours,
        });
    }

    if !observed_return.log_return.is_finite() {
        return Err(MarketPressureFitError::InvalidLogReturn {
            return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
            value: observed_return.log_return,
        });
    }

    if !observed_return.sell_share_proxy.is_finite()
        || !(0.0..=1.0).contains(&observed_return.sell_share_proxy)
    {
        return Err(MarketPressureFitError::InvalidSellShareProxy {
            return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
            value: observed_return.sell_share_proxy,
        });
    }

    observed_return
        .source_activity
        .validate()
        .map_err(|source| MarketPressureFitError::InvalidSourceActivity {
            return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
            source,
        })?;

    let buy_side_exposure = observed_return
        .source_activity
        .buy_activity
        .baseline_activity;

    let sell_side_exposure = observed_return
        .source_activity
        .sell_activity
        .baseline_activity;

    let signed_balance_exposure_scale =
        signed_balance_exposure_scale_from_side_exposures(buy_side_exposure, sell_side_exposure)
            .map_err(
                |source| MarketPressureFitError::InvalidSignedBalanceExposureScale {
                    return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
                    source,
                },
            )?;

    let side_imbalance = side_imbalance_from_side_exposures(
        observed_return.return_timestamp_utc_nanos,
        buy_side_exposure,
        sell_side_exposure,
        signed_balance_exposure_scale,
    )?;

    Ok(MarketPressureSideBalanceObservation {
        return_timestamp_utc_nanos: observed_return.return_timestamp_utc_nanos,
        elapsed_same_session_hours: observed_return.elapsed_same_session_hours,
        log_return: observed_return.log_return,
        side_imbalance,
        signed_balance_exposure_scale,
    })
}

fn side_imbalance_from_side_exposures(
    return_timestamp_utc_nanos: i64,
    buy_side_exposure: f64,
    sell_side_exposure: f64,
    signed_balance_exposure_scale: f64,
) -> Result<f64, MarketPressureFitError> {
    let side_imbalance = if signed_balance_exposure_scale == 0.0 {
        0.0
    } else {
        (buy_side_exposure - sell_side_exposure) / signed_balance_exposure_scale
    };

    if !side_imbalance.is_finite() || !(-1.0..=1.0).contains(&side_imbalance) {
        return Err(MarketPressureFitError::InvalidSideImbalance {
            return_timestamp_utc_nanos,
            value: side_imbalance,
        });
    }

    Ok(side_imbalance)
}

fn estimate_anchor_drift_per_signed_balance(
    observations: &[MarketPressureSideBalanceObservation],
    support_levels: &[f64],
    state_probabilities: &[Vec<f64>],
    coarse_return_observations: &[MarketPressureCoarseReturnObservation],
    coarse_state_probabilities: &[Vec<f64>],
    open_event_rate_per_hour: f64,
) -> Result<f64, MarketPressureFitError> {
    let mut numerator = 0.0;
    let mut denominator = 0.0;

    for (observation, probabilities) in observations.iter().zip(state_probabilities.iter()) {
        if probabilities.len() != support_levels.len() {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }

        let expected_side_imbalance = probabilities
            .iter()
            .zip(support_levels.iter())
            .map(|(probability, side_imbalance_level)| probability * side_imbalance_level)
            .sum::<f64>();

        let signed_balance_exposure =
            expected_side_imbalance * observation.signed_balance_exposure_scale;
        if !signed_balance_exposure.is_finite() {
            return Err(MarketPressureFitError::InvalidSignedBalanceExposure {
                return_timestamp_utc_nanos: observation.return_timestamp_utc_nanos,
                value: signed_balance_exposure,
            });
        }

        numerator += signed_balance_exposure * observation.log_return;
        denominator += signed_balance_exposure * signed_balance_exposure;
    }

    for (observation, probabilities) in coarse_return_observations
        .iter()
        .zip(coarse_state_probabilities.iter())
    {
        if probabilities.len() != support_levels.len() {
            return Err(MarketPressureFitError::InvalidStateAssignments);
        }

        let expected_side_imbalance = probabilities
            .iter()
            .zip(support_levels.iter())
            .map(|(probability, side_imbalance_level)| probability * side_imbalance_level)
            .sum::<f64>();

        let signed_balance_exposure =
            expected_side_imbalance * open_event_rate_per_hour * observation.elapsed_hours;
        if !signed_balance_exposure.is_finite() {
            return Err(MarketPressureFitError::InvalidSignedBalanceExposure {
                return_timestamp_utc_nanos: observation.interval_end_utc_nanos,
                value: signed_balance_exposure,
            });
        }

        numerator += signed_balance_exposure * observation.log_return;
        denominator += signed_balance_exposure * signed_balance_exposure;
    }

    if denominator == 0.0 {
        return Ok(0.0);
    }

    let anchor_drift_per_signed_balance = numerator / denominator;
    if !anchor_drift_per_signed_balance.is_finite() {
        return Err(MarketPressureFitError::InvalidAnchorDriftPerSignedBalance {
            value: anchor_drift_per_signed_balance,
        });
    }

    Ok(anchor_drift_per_signed_balance)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MarketPressureSideBalanceObservation {
    pub(super) return_timestamp_utc_nanos: i64,
    pub(super) elapsed_same_session_hours: f64,
    pub(super) log_return: f64,
    pub(super) side_imbalance: f64,
    pub(super) signed_balance_exposure_scale: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MarketPressureCoarseReturnObservation {
    pub(super) interval_end_utc_nanos: i64,
    pub(super) elapsed_hours: f64,
    pub(super) log_return: f64,
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationOptimizerSettings;
    use janiform_foundations::time::NANOS_PER_HOUR;
    use janiform_model_kernels::market_pressure::{
        MarketPressureParameters, MarketPressureTransitionRates,
    };
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
    use janiform_model_kernels::return_clock::{
        IntegratedSideActivityDecomposition, SideSourceActivityDecomposition,
    };

    use super::{
        MarketPressureCoarseReturnLaw, MarketPressureCoarseReturnObservation,
        MarketPressureFitError, MarketPressureFitInput, MarketPressureFitOutput,
        MarketPressureFitProgress, MarketPressureObservationFitInput,
        MarketPressureSideBalanceObservation, fit_market_pressure,
        fit_market_pressure_from_observations, market_pressure_side_balance_observation,
    };
    use crate::market_data::loading::MarketDataNormalizedCoarseDataRecord;
    use crate::market_pressure::{
        MarketPressureRuntimeNigFitInput,
        fit_market_pressure_from_runtime_state_and_nig_configuration,
    };
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;

    #[test]
    fn alternating_event_sides_fit_neutral_pressure() {
        let observations = alternating_side_observations(100);

        let parameters =
            fit_market_pressure_parameters_from_test_observations(&observations, &[], None, 1.0)
                .expect("alternating side observations are valid");

        assert_eq!(parameters.side_imbalance_levels, vec![0.0]);
        assert_eq!(
            parameters.transition_rates,
            MarketPressureTransitionRates::single_state()
        );
        assert_eq!(parameters.anchor_drift_per_signed_balance, 0.0);
    }

    #[test]
    fn single_state_fit_reports_side_balance_and_coarse_return_selection_diagnostics() {
        let observations = vec![test_observation(0, 0.0, 0.0)];

        let output =
            fit_market_pressure_output_from_test_observations(&observations, &[], None, 1.0)
                .expect("alternating side observations are valid");

        assert!(output.selection_diagnostics.iter().any(|diagnostic| {
            diagnostic.evidence_source == "side_balance"
                && diagnostic.decision == "skipped"
                && diagnostic.reason == "no_split_candidate"
                && diagnostic.current_state_count == 1
                && diagnostic.candidate_state_count == 1
        }));
        assert!(output.selection_diagnostics.iter().any(|diagnostic| {
            diagnostic.evidence_source == "coarse_return"
                && diagnostic.decision == "skipped"
                && diagnostic.reason == "missing_coarse_return_law"
        }));
    }

    #[test]
    fn persistent_side_blocks_fit_latent_side_balance_split() {
        let observations = persistent_side_block_observations();

        let parameters =
            fit_market_pressure_parameters_from_test_observations(&observations, &[], None, 1.0)
                .expect("persistent side observations are valid");

        assert_eq!(parameters.side_imbalance_levels.len(), 2);
        assert!(parameters.side_imbalance_levels[0] < 0.0);
        assert!(parameters.side_imbalance_levels[1] > 0.0);
        assert_eq!(parameters.transition_rates.upward_transition_rates.len(), 1);
        assert_eq!(
            parameters.transition_rates.downward_transition_rates.len(),
            1
        );
    }

    #[test]
    fn persistent_side_blocks_can_accept_multiple_latent_side_balance_splits() {
        let observations = persistent_three_side_block_observations();

        let parameters =
            fit_market_pressure_parameters_from_test_observations(&observations, &[], None, 1.0)
                .expect("persistent side observations are valid");

        assert!(parameters.side_imbalance_levels.len() >= 3);
        assert!(parameters.side_imbalance_levels[0] < -0.5);
        assert!(
            parameters
                .side_imbalance_levels
                .iter()
                .any(|level| level.abs() < 0.1)
        );
        assert!(
            parameters
                .side_imbalance_levels
                .last()
                .is_some_and(|level| *level > 0.5)
        );
    }

    #[test]
    fn pressure_side_evidence_uses_baseline_source_activity() {
        let observation = market_pressure_side_balance_observation(observed_return(
            1,
            0.0,
            0.0,
            SideSourceActivityDecomposition::new(
                IntegratedSideActivityDecomposition::new(0.25, 0.75),
                IntegratedSideActivityDecomposition::zero(),
            ),
        ))
        .expect("source activity is valid");

        assert_eq!(
            observation,
            MarketPressureSideBalanceObservation {
                return_timestamp_utc_nanos: 1,
                elapsed_same_session_hours: 0.001,
                log_return: 0.0,
                side_imbalance: 1.0,
                signed_balance_exposure_scale: 0.25,
            }
        );
    }

    #[test]
    fn endogenous_only_side_blocks_do_not_fit_market_pressure() {
        let mut observed_returns = Vec::new();

        for observation_position in 0..40 {
            observed_returns.push(observed_return(
                observation_position,
                0.0,
                0.001,
                SideSourceActivityDecomposition::new(
                    IntegratedSideActivityDecomposition::new(0.0, 1.0),
                    IntegratedSideActivityDecomposition::zero(),
                ),
            ));
        }

        for observation_position in 40..80 {
            observed_returns.push(observed_return(
                observation_position,
                1.0,
                -0.001,
                SideSourceActivityDecomposition::new(
                    IntegratedSideActivityDecomposition::zero(),
                    IntegratedSideActivityDecomposition::new(0.0, 1.0),
                ),
            ));
        }

        let parameters = fit_market_pressure(MarketPressureFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: None,
            coarse_return_law: None,
            source_window_open_exposure_hours: 1.0,
            solver_settings: &test_solver_settings(),
            progress_observer: None,
        })
        .expect("endogenous-only observations are valid")
        .parameters;

        assert_eq!(parameters.side_imbalance_levels, vec![0.0]);
        assert_eq!(
            parameters.transition_rates,
            MarketPressureTransitionRates::single_state()
        );
        assert_eq!(parameters.anchor_drift_per_signed_balance, 0.0);
    }

    #[test]
    fn coarse_returns_fit_transition_rates_when_side_evidence_has_one_state() {
        let observations = vec![test_observation(0, 0.25, 0.0)];
        let coarse_return_observations = coarse_return_block_observations();

        let output = fit_market_pressure_output_from_test_observations(
            &observations,
            coarse_return_observations.as_slice(),
            test_coarse_return_law(coarse_return_observations.len()),
            1.0,
        )
        .expect("coarse transition evidence is valid");

        let parameters = &output.parameters;

        assert_eq!(parameters.side_imbalance_levels, vec![-0.25, 0.25]);
        assert_eq!(parameters.transition_rates.upward_transition_rates.len(), 1);
        assert_eq!(
            parameters.transition_rates.downward_transition_rates.len(),
            1
        );
        assert!(
            parameters
                .transition_rates
                .upward_transition_rates
                .iter()
                .all(|transition_rate| transition_rate.is_finite() && *transition_rate > 0.0)
        );
        assert!(
            parameters
                .transition_rates
                .downward_transition_rates
                .iter()
                .all(|transition_rate| transition_rate.is_finite() && *transition_rate > 0.0)
        );
        assert!(parameters.anchor_drift_per_signed_balance > 0.0);
        assert!(output.selection_diagnostics.iter().any(|diagnostic| {
            diagnostic.evidence_source == "coarse_return"
                && diagnostic.decision == "accepted"
                && diagnostic.reason == "penalized_score_improved"
                && diagnostic.current_state_count == 1
                && diagnostic.candidate_state_count == 2
                && diagnostic
                    .score_improvement
                    .is_some_and(|improvement| improvement > 0.0)
        }));
    }

    #[test]
    fn balanced_coarse_returns_fit_transition_rates_when_side_mean_is_neutral() {
        let observations = alternating_half_side_observations(100);
        let coarse_return_observations = balanced_coarse_return_block_observations();

        let parameters = fit_market_pressure_parameters_from_test_observations(
            &observations,
            coarse_return_observations.as_slice(),
            test_coarse_return_law(coarse_return_observations.len()),
            1.0,
        )
        .expect("balanced coarse transition evidence is valid");

        assert_eq!(parameters.side_imbalance_levels, vec![-0.5, 0.5]);
        assert_eq!(parameters.transition_rates.upward_transition_rates.len(), 1);
        assert_eq!(
            parameters.transition_rates.downward_transition_rates.len(),
            1
        );
    }

    #[test]
    fn cached_nig_refit_reuses_side_balance_fit_for_coarse_pressure() {
        let observed_returns = vec![observed_return(0, 0.25, 0.0, side_activity(0.003, 0.001))];
        let source_window_open_exposure_hours = 0.003 + 0.001;
        let coarse_data_records = balanced_coarse_return_data_records();

        let side_only_output = fit_market_pressure(MarketPressureFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: Some(coarse_data_records.as_slice()),
            coarse_return_law: None,
            source_window_open_exposure_hours,
            solver_settings: &test_solver_settings(),
            progress_observer: None,
        })
        .expect("side-only pressure fit should succeed");

        let mut progress_updates = Vec::<MarketPressureFitProgress>::new();
        let mut progress_observer = |progress| progress_updates.push(progress);

        let output = fit_market_pressure_from_runtime_state_and_nig_configuration(
            MarketPressureRuntimeNigFitInput {
                runtime_state: &side_only_output.runtime_state,
                nig_configuration: test_nig_configuration(),
                solver_settings: &test_solver_settings(),
                progress_observer: Some(&mut progress_observer),
            },
        )
        .expect("cached coarse pressure refit should succeed");

        assert_eq!(output.parameters.side_imbalance_levels, vec![-0.5, 0.5]);
        assert!(
            output
                .runtime_state
                .covers_coarse_return_record_count(coarse_data_records.len())
        );
        assert!(!progress_updates.iter().any(|progress| {
            progress.current_item_label == "fit ordered side-balance HMM"
                || progress.current_item_label == "fit ordered side-balance HMM start candidates"
        }));
        assert!(
            progress_updates.iter().any(|progress| {
                progress.current_item_label == "fit coarse-return pressure HMM"
            })
        );
    }

    #[test]
    fn market_pressure_fit_reports_ordered_side_balance_progress() {
        let observed_returns = persistent_side_block_observed_returns();
        let mut progress_updates = Vec::<MarketPressureFitProgress>::new();
        let mut progress_observer = |progress| progress_updates.push(progress);

        let parameters = fit_market_pressure(MarketPressureFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: None,
            coarse_return_law: None,
            source_window_open_exposure_hours: 1.0,
            solver_settings: &test_solver_settings(),
            progress_observer: Some(&mut progress_observer),
        })
        .expect("persistent side observations are valid")
        .parameters;

        assert_eq!(parameters.side_imbalance_levels.len(), 2);
        assert!(progress_updates.iter().any(|progress| {
            progress.current_item_label == "fit ordered side-balance HMM"
                && progress.total_units > test_solver_settings().max_iterations + 1
                && progress.completed_units > 0
        }));
    }

    #[test]
    fn market_pressure_parallel_start_fit_reports_streaming_progress() {
        let observed_returns = persistent_side_block_observed_returns();
        let solver_settings = test_solver_settings_with_cpu_count(2);
        let mut progress_updates = Vec::<MarketPressureFitProgress>::new();
        let mut progress_observer = |progress| progress_updates.push(progress);

        let parameters = fit_market_pressure(MarketPressureFitInput {
            observed_open_session_returns: observed_returns.as_slice(),
            coarse_data_records: None,
            coarse_return_law: None,
            source_window_open_exposure_hours: 1.0,
            solver_settings: &solver_settings,
            progress_observer: Some(&mut progress_observer),
        })
        .expect("persistent side observations are valid")
        .parameters;

        assert_eq!(parameters.side_imbalance_levels.len(), 2);
        assert!(progress_updates.iter().any(|progress| {
            progress.current_item_label == "fit ordered side-balance HMM start candidates"
                && progress.total_units > solver_settings.max_iterations + 1
                && progress.completed_units > 0
        }));
    }

    fn persistent_side_block_observed_returns() -> Vec<ReturnComponentsObservedOpenSessionReturn> {
        let mut observed_returns = Vec::new();

        for observation_position in 0..40 {
            observed_returns.push(observed_return(
                observation_position,
                0.0,
                0.001,
                side_activity(1.0, 0.0),
            ));
        }

        for observation_position in 40..80 {
            observed_returns.push(observed_return(
                observation_position,
                1.0,
                -0.001,
                side_activity(0.0, 1.0),
            ));
        }

        observed_returns
    }

    fn alternating_side_observations(
        observation_count: usize,
    ) -> Vec<MarketPressureSideBalanceObservation> {
        (0..observation_count)
            .map(|observation_position| {
                let side_imbalance = if observation_position % 2 == 0 {
                    1.0
                } else {
                    -1.0
                };

                test_observation(observation_position, side_imbalance, 0.0)
            })
            .collect()
    }

    fn alternating_half_side_observations(
        observation_count: usize,
    ) -> Vec<MarketPressureSideBalanceObservation> {
        (0..observation_count)
            .map(|observation_position| {
                let side_imbalance = if observation_position % 2 == 0 {
                    0.5
                } else {
                    -0.5
                };

                test_observation(observation_position, side_imbalance, 0.0)
            })
            .collect()
    }

    fn persistent_side_block_observations() -> Vec<MarketPressureSideBalanceObservation> {
        let mut observations = Vec::new();

        for observation_position in 0..40 {
            observations.push(test_observation(observation_position, 1.0, 0.001));
        }

        for observation_position in 40..80 {
            observations.push(test_observation(observation_position, -1.0, -0.001));
        }

        observations
    }

    fn persistent_three_side_block_observations() -> Vec<MarketPressureSideBalanceObservation> {
        let block_specifications = [(-1.0, -0.001), (0.0, 0.0), (1.0, 0.001)];
        let mut observations = Vec::new();

        for (side_imbalance, log_return) in block_specifications {
            for _ in 0..120 {
                observations.push(test_observation(
                    observations.len(),
                    side_imbalance,
                    log_return,
                ));
            }
        }

        observations
    }

    fn coarse_return_block_observations() -> Vec<MarketPressureCoarseReturnObservation> {
        let block_specifications = [(10, 1.5e-4), (10, -1.5e-4), (10, 1.5e-4)];
        let mut observations = Vec::new();

        for (block_length, log_return) in block_specifications {
            for _ in 0..block_length {
                observations.push(test_coarse_return_observation(
                    observations.len() + 1,
                    1.0,
                    log_return,
                ));
            }
        }

        observations
    }

    fn balanced_coarse_return_block_observations() -> Vec<MarketPressureCoarseReturnObservation> {
        let block_specifications = [(24, 3.0e-4), (24, -3.0e-4), (24, 3.0e-4), (24, -3.0e-4)];
        let mut observations = Vec::new();

        for (block_length, log_return) in block_specifications {
            for _ in 0..block_length {
                observations.push(test_coarse_return_observation(
                    observations.len() + 1,
                    1.0,
                    log_return,
                ));
            }
        }

        observations
    }

    fn balanced_coarse_return_data_records() -> Vec<MarketDataNormalizedCoarseDataRecord> {
        balanced_coarse_return_block_observations()
            .into_iter()
            .map(|observation| {
                let interval_end_utc_nanos = observation.interval_end_utc_nanos * NANOS_PER_HOUR;
                test_coarse_data_record(interval_end_utc_nanos, observation.log_return)
            })
            .collect()
    }

    fn fit_market_pressure_parameters_from_test_observations(
        observations: &[MarketPressureSideBalanceObservation],
        coarse_return_observations: &[MarketPressureCoarseReturnObservation],
        coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
        open_event_rate_per_hour: f64,
    ) -> Result<MarketPressureParameters, MarketPressureFitError> {
        fit_market_pressure_output_from_test_observations(
            observations,
            coarse_return_observations,
            coarse_return_law,
            open_event_rate_per_hour,
        )
        .map(|output| output.parameters)
    }

    fn fit_market_pressure_output_from_test_observations(
        observations: &[MarketPressureSideBalanceObservation],
        coarse_return_observations: &[MarketPressureCoarseReturnObservation],
        coarse_return_law: Option<MarketPressureCoarseReturnLaw>,
        open_event_rate_per_hour: f64,
    ) -> Result<MarketPressureFitOutput, MarketPressureFitError> {
        let mut progress_observer = None;
        let coarse_return_pressure_evidence_available =
            coarse_return_law.is_some() && !coarse_return_observations.is_empty();

        fit_market_pressure_from_observations(
            MarketPressureObservationFitInput {
                observations,
                coarse_return_observations,
                coarse_return_record_count: coarse_return_observations.len(),
                coarse_return_law,
                coarse_return_pressure_evidence_available,
                open_event_rate_per_hour,
                solver_settings: &test_solver_settings(),
            },
            &mut progress_observer,
        )
    }

    fn test_observation(
        observation_position: usize,
        side_imbalance: f64,
        log_return: f64,
    ) -> MarketPressureSideBalanceObservation {
        let return_timestamp_utc_nanos =
            i64::try_from(observation_position).expect("test observation position fits in i64");

        MarketPressureSideBalanceObservation {
            return_timestamp_utc_nanos,
            elapsed_same_session_hours: 0.001,
            log_return,
            side_imbalance,
            signed_balance_exposure_scale: 1.0,
        }
    }

    fn test_coarse_return_observation(
        observation_position: usize,
        elapsed_hours: f64,
        log_return: f64,
    ) -> MarketPressureCoarseReturnObservation {
        let interval_end_utc_nanos =
            i64::try_from(observation_position).expect("test observation position fits in i64");

        MarketPressureCoarseReturnObservation {
            interval_end_utc_nanos,
            elapsed_hours,
            log_return,
        }
    }

    fn test_coarse_data_record(
        interval_end_utc_nanos: i64,
        log_return: f64,
    ) -> MarketDataNormalizedCoarseDataRecord {
        MarketDataNormalizedCoarseDataRecord {
            interval_start_utc_nanos: interval_end_utc_nanos - NANOS_PER_HOUR,
            interval_end_utc_nanos,
            observation_timestamp_utc_nanos: interval_end_utc_nanos,
            close_price: 1.0,
            open_price: None,
            high_price: None,
            low_price: None,
            volume: None,
            close_to_close_log_return: Some(log_return),
            open_to_close_log_return: None,
            close_to_open_gap_log_return: None,
            high_low_log_range: None,
            source_row_number: usize::try_from(interval_end_utc_nanos / NANOS_PER_HOUR)
                .expect("test coarse interval position fits in usize"),
        }
    }

    fn observed_return(
        observation_position: usize,
        sell_share_proxy: f64,
        log_return: f64,
        source_activity: SideSourceActivityDecomposition,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        let return_timestamp_utc_nanos =
            i64::try_from(observation_position).expect("test observation position fits in i64");

        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos,
            elapsed_same_session_hours: 0.001,
            log_return,
            post_shock_activity_lift: 1.0,
            sell_share_proxy,
            source_activity,
        }
    }

    fn side_activity(
        buy_baseline_activity: f64,
        sell_baseline_activity: f64,
    ) -> SideSourceActivityDecomposition {
        SideSourceActivityDecomposition::new(
            IntegratedSideActivityDecomposition::new(buy_baseline_activity, 0.0),
            IntegratedSideActivityDecomposition::new(sell_baseline_activity, 0.0),
        )
    }

    fn test_solver_settings() -> CalibrationOptimizerSettings {
        test_solver_settings_with_cpu_count(1)
    }

    fn test_solver_settings_with_cpu_count(cpu_count: usize) -> CalibrationOptimizerSettings {
        CalibrationOptimizerSettings::try_new(
            "market-pressure-test",
            64,
            cpu_count,
            1.0e-10,
            1.0e-10,
            0.0,
        )
        .expect("test solver settings are valid")
    }

    fn test_coarse_return_law(observation_count: usize) -> Option<MarketPressureCoarseReturnLaw> {
        MarketPressureCoarseReturnLaw::from_nig_configuration(
            test_nig_configuration(),
            observation_count,
        )
        .expect("test coarse return law is valid")
    }

    fn test_nig_configuration() -> NigSharedPlusShiftConfiguration {
        NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
            .expect("test NIG configuration is valid")
    }
}
