use std::sync::mpsc::Sender;

use chrono::{DateTime, Utc};
use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;
use janiform_foundations::numeric::validate_positive_number;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::hawkes::target_source_rates_from_shared_activity_loading;
use janiform_model_kernels::long_variance::{
    LongVarianceParameters, LongVarianceState, return_energy_increment_from_log_return,
};
use janiform_model_kernels::news::{
    NewsArrivalIntensityEvaluationInput, NewsProcessConfiguration,
    expected_news_activity_loading_rate,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

use super::super::progress::{ReturnBackfitStageProgressPhase, ReturnBackfitStageProgressUpdate};
use super::DynamicContinuousReturnObservation;
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::long_variance::generated_energy::project_generated_stationary_feedback_to_budget;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageSortedSignalInputs, return_clock_proxy,
};
use crate::surprise_profile::average_surprise_news_weekly_profile_multiplier_over_interval;

pub(super) struct DynamicClockReplayInput<'a> {
    pub(super) hawkes_stage_payload: &'a HawkesStageFittedPayload,
    pub(super) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(super) news_process_configuration: &'a NewsProcessConfiguration,
    pub(super) surprise_deterministic_intensity_profile:
        &'a SurpriseNewsDeterministicIntensityProfile,
    pub(super) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(super) return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub(super) long_variance_parameters: LongVarianceParameters,
    pub(super) sorted_signals: &'a ReturnComponentsStageSortedSignalInputs,
    pub(super) progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct DynamicClockReplay {
    pub(super) long_variance_clock_scale_multiplier: f64,
    pub(super) continuous_returns: Vec<DynamicContinuousReturnObservation>,
}

#[derive(Debug, Default)]
struct ContinuousReturnClockScaleAccumulator {
    original_return_count_scale: f64,
    long_variance_return_count_scale: f64,
    return_position: usize,
    continuous_returns: Vec<DynamicContinuousReturnObservation>,
}

pub(super) fn replay_dynamic_clock_for_continuous_returns(
    input: DynamicClockReplayInput<'_>,
) -> Result<DynamicClockReplay, String> {
    let DynamicClockReplayInput {
        hawkes_stage_payload,
        candidate_generation_input,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        nig_configuration,
        return_clock_exposure_parameters,
        long_variance_parameters,
        sorted_signals,
        progress_sender,
    } = input;

    let stationary_mean_reversion_level = stationary_mean_reversion_level_for_backfit(
        hawkes_stage_payload,
        candidate_generation_input,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        nig_configuration,
        return_clock_exposure_parameters,
        long_variance_parameters,
    )?;

    let mut state = LongVarianceState::unit();
    let mut continuous_clock_scale_accumulator = ContinuousReturnClockScaleAccumulator::default();

    let observed_return_count = sorted_signals.observed_open_session_returns.len();
    let mut last_emitted_replay_progress_percent = 0usize;

    for (observation_offset, observation) in sorted_signals
        .observed_open_session_returns
        .iter()
        .enumerate()
    {
        let clock_scale = state
            .baseline_clock_scale(
                long_variance_parameters,
                stationary_mean_reversion_level,
                observation.elapsed_same_session_hours,
            )
            .map_err(|source| {
                format!("return-backfit long-variance clock replay failed: {source}")
            })?;

        let original_business_time_scale =
            return_clock_proxy(*observation, return_clock_exposure_parameters, 1.0);

        let dynamic_business_time_scale =
            return_clock_proxy(*observation, return_clock_exposure_parameters, clock_scale);

        accumulate_continuous_return_clock_scale(
            &mut continuous_clock_scale_accumulator,
            sorted_signals,
            observation,
            original_business_time_scale,
            dynamic_business_time_scale,
        )?;

        state = advance_long_variance_state_for_observation(
            state,
            long_variance_parameters,
            stationary_mean_reversion_level,
            observation,
        )?;

        let completed_observation_count = observation_offset.saturating_add(1);

        let replay_progress_percent = crate::stages::progress::progress_percent(
            completed_observation_count,
            observed_return_count,
        );

        if replay_progress_percent > last_emitted_replay_progress_percent {
            emit_dynamic_clock_replay_progress(
                progress_sender.as_ref(),
                completed_observation_count,
                observed_return_count,
            );
            last_emitted_replay_progress_percent = replay_progress_percent;
        }
    }

    finish_dynamic_clock_replay(continuous_clock_scale_accumulator, sorted_signals)
}

fn emit_dynamic_clock_replay_progress(
    progress_sender: Option<&Sender<ReturnBackfitStageProgressUpdate>>,
    completed_units: usize,
    total_units: usize,
) {
    let mut progress_update = ReturnBackfitStageProgressUpdate::new(
        ReturnBackfitStageProgressPhase::DynamicClockReplay,
        completed_units,
        total_units,
    );

    progress_update.aggregate_progress_percent = Some(
        super::super::progress::return_backfit_phase_progress_percent(
            2,
            crate::stages::progress::progress_percent(completed_units, total_units),
        ),
    );

    progress_update.current_item_label = Some("replay dynamic return clock".to_string());

    crate::stages::progress::emit_stage_progress(progress_sender, progress_update);
}

fn accumulate_continuous_return_clock_scale(
    accumulator: &mut ContinuousReturnClockScaleAccumulator,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    observation: &ReturnComponentsObservedOpenSessionReturn,
    original_business_time_scale: f64,
    dynamic_business_time_scale: f64,
) -> Result<(), String> {
    let is_continuous_fit_return = sorted_signals
        .continuous_fit_returns
        .get(accumulator.return_position)
        .is_some_and(|continuous_observation| continuous_observation == observation);

    if !is_continuous_fit_return {
        return Ok(());
    }

    validate_positive_number(original_business_time_scale, "original_business_time_scale")
        .map_err(|source| {
            format!("return-backfit original continuous clock scale is invalid: {source}")
        })?;

    validate_positive_number(dynamic_business_time_scale, "dynamic_business_time_scale").map_err(
        |source| format!("return-backfit dynamic continuous clock scale is invalid: {source}"),
    )?;

    accumulator.original_return_count_scale += original_business_time_scale;
    accumulator.long_variance_return_count_scale += dynamic_business_time_scale;
    accumulator
        .continuous_returns
        .push(DynamicContinuousReturnObservation {
            observation: *observation,
            original_business_time_scale,
            dynamic_business_time_scale,
        });
    accumulator.return_position = accumulator.return_position.saturating_add(1);

    Ok(())
}

fn finish_dynamic_clock_replay(
    accumulator: ContinuousReturnClockScaleAccumulator,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
) -> Result<DynamicClockReplay, String> {
    if accumulator.return_position != sorted_signals.continuous_fit_returns.len() {
        return Err(format!(
            "return-backfit dynamic clock replay aligned {} continuous returns but expected {}",
            accumulator.return_position,
            sorted_signals.continuous_fit_returns.len()
        ));
    }

    if accumulator.original_return_count_scale <= 0.0 {
        return Err("return-backfit requires positive continuous return count scale".to_string());
    }

    let long_variance_clock_scale_multiplier =
        accumulator.long_variance_return_count_scale / accumulator.original_return_count_scale;

    validate_positive_number(
        long_variance_clock_scale_multiplier,
        "long_variance_clock_scale_multiplier",
    )
    .map_err(|source| {
        format!("return-backfit long-variance clock scale multiplier is invalid: {source}")
    })?;

    Ok(DynamicClockReplay {
        long_variance_clock_scale_multiplier,
        continuous_returns: accumulator.continuous_returns,
    })
}

fn advance_long_variance_state_for_observation(
    state: LongVarianceState,
    long_variance_parameters: LongVarianceParameters,
    stationary_mean_reversion_level: f64,
    observation: &ReturnComponentsObservedOpenSessionReturn,
) -> Result<LongVarianceState, String> {
    let drifted_state = state
        .apply_mean_reversion_drift(
            long_variance_parameters,
            stationary_mean_reversion_level,
            observation.elapsed_same_session_hours,
        )
        .map_err(|source| format!("return-backfit long-variance state replay failed: {source}"))?;

    let return_energy_increment = return_energy_increment_from_log_return(observation.log_return)
        .map_err(|source| {
        format!("return-backfit long-variance return-energy input is invalid: {source}")
    })?;

    let energy_adjusted_state = drifted_state
        .apply_return_energy_jump(long_variance_parameters, return_energy_increment)
        .map_err(|source| {
            format!("return-backfit long-variance return-energy replay failed: {source}")
        })?;

    let sell_share = observation.sell_share_proxy.clamp(0.0, 1.0);

    let event_loading = long_variance_parameters.buy_event_loading * (1.0 - sell_share)
        + long_variance_parameters.sell_event_loading * sell_share;

    LongVarianceState::try_new(energy_adjusted_state.level + event_loading).map_err(|source| {
        format!("return-backfit long-variance event jump replay failed: {source}")
    })
}

fn stationary_mean_reversion_level_for_backfit(
    hawkes_stage_payload: &HawkesStageFittedPayload,
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    news_process_configuration: &NewsProcessConfiguration,
    surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
    nig_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    long_variance_parameters: LongVarianceParameters,
) -> Result<f64, String> {
    let source_window = candidate_generation_input.known_news_calendar.source_window;

    let source_window_duration_hours = duration_nanos_to_hours(
        source_window
            .window_end_utc_nanos
            .saturating_sub(source_window.window_start_utc_nanos),
    );
    if !source_window_duration_hours.is_finite() || source_window_duration_hours <= 0.0 {
        return Err("return-backfit requires a positive source-window duration".to_string());
    }

    let surprise_time_multiplier = average_surprise_news_weekly_profile_multiplier_over_interval(
        DateTime::<Utc>::from_timestamp_nanos(source_window.window_start_utc_nanos),
        DateTime::<Utc>::from_timestamp_nanos(source_window.window_end_utc_nanos),
        surprise_deterministic_intensity_profile,
    )
    .map_err(|source| format!("return-backfit surprise intensity profile invalid: {source}"))?;

    let news_intensities = news_process_configuration
        .arrivals
        .evaluate_with_deterministic_time_multiplier(NewsArrivalIntensityEvaluationInput {
            interval_start_utc_nanos: source_window.window_start_utc_nanos,
            interval_end_utc_nanos: source_window.window_end_utc_nanos,
            surprise_deterministic_time_multiplier: surprise_time_multiplier,
            surprise_open_window_share: 1.0,
            surprise_closed_window_share: 0.0,
        })
        .map_err(|source| format!("return-backfit news intensity evaluation failed: {source}"))?;

    let news_activity_loading_rate = expected_news_activity_loading_rate(
        news_intensities.known_component_intensity,
        news_intensities.surprise_component_intensity,
        news_process_configuration.known_component_mark_scales,
        news_process_configuration.surprise_component_mark_scales,
        news_process_configuration
            .shared_mark_law
            .gamma_severity_marks,
    )
    .map_err(|source| {
        format!("return-backfit news activity-loading rate evaluation failed: {source}")
    })?;

    let exogenous_source_rates = target_source_rates_from_shared_activity_loading(
        hawkes_stage_payload.fitted_parameters.kernel_parameters.rho,
        news_activity_loading_rate,
    )
    .ok_or_else(|| "return-backfit news target source-rate evaluation failed".to_string())?;

    let long_variance_parameters = project_generated_stationary_feedback_to_budget(
        long_variance_parameters,
        hawkes_stage_payload.fitted_parameters.kernel_parameters,
        nig_configuration,
        return_clock_exposure_parameters,
    )?;

    long_variance_parameters
        .stationary_mean_reversion_level_for_hawkes_kernel(
            hawkes_stage_payload.fitted_parameters.kernel_parameters,
            exogenous_source_rates,
        )
        .map_err(|source| {
            format!("return-backfit long-variance stationary level evaluation failed: {source}")
        })
}
