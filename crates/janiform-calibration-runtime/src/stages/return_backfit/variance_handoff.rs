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

use super::progress::{
    RETURN_BACKFIT_STAGE_PROGRESS_UNITS, ReturnBackfitStageProgressPhase,
    ReturnBackfitStageProgressUpdate, emit_return_backfit_progress,
};
use crate::stages::hawkes::HawkesStageFittedPayload;
use crate::stages::long_variance::generated_energy::project_generated_stationary_feedback_to_budget;
use crate::stages::return_components::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageCandidateDiagnostic,
    ReturnComponentsStageCandidateGenerationInput, ReturnComponentsStageSortedSignalInputs,
    prepare_continuous_return_calibration_inputs,
    resolve_activity_calibration_context_from_hawkes_payload, return_clock_proxy,
};
use crate::surprise_profile::average_surprise_news_weekly_profile_multiplier_over_interval;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ReturnBackfitVarianceHandoffFit {
    pub(super) long_variance_clock_scale_multiplier: f64,
    pub(super) delta_handoff_multiplier: f64,
    pub(super) selected_nig_delta: f64,
    pub(super) post_handoff_nig_delta: f64,
    pub(super) selected_continuous_model_variance_scale: f64,
    pub(super) post_handoff_base_variance_scale: f64,
    pub(super) post_handoff_dynamic_variance_scale: f64,
    pub(super) observed_reference_variance_scale: f64,
    pub(super) post_handoff_nig_configuration: NigSharedPlusShiftConfiguration,
}

pub(super) struct ReturnBackfitVarianceHandoffInput<'a> {
    pub(super) hawkes_stage_payload: &'a HawkesStageFittedPayload,
    pub(super) candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    pub(super) selected_candidate: &'a ReturnComponentsStageCandidateDiagnostic,
    pub(super) nig_configuration: NigSharedPlusShiftConfiguration,
    pub(super) return_clock_exposure_parameters: ReturnClockExposureParameters,
    pub(super) long_variance_parameters: LongVarianceParameters,
    pub(super) news_process_configuration: &'a NewsProcessConfiguration,
    pub(super) surprise_deterministic_intensity_profile:
        &'a SurpriseNewsDeterministicIntensityProfile,
    pub(super) progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

struct DynamicClockReplayInput<'a> {
    hawkes_stage_payload: &'a HawkesStageFittedPayload,
    candidate_generation_input: &'a ReturnComponentsStageCandidateGenerationInput,
    news_process_configuration: &'a NewsProcessConfiguration,
    surprise_deterministic_intensity_profile: &'a SurpriseNewsDeterministicIntensityProfile,
    nig_configuration: NigSharedPlusShiftConfiguration,
    return_clock_exposure_parameters: ReturnClockExposureParameters,
    long_variance_parameters: LongVarianceParameters,
    sorted_signals: &'a ReturnComponentsStageSortedSignalInputs,
    progress_sender: Option<Sender<ReturnBackfitStageProgressUpdate>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DynamicClockReplay {
    long_variance_clock_scale_multiplier: f64,
}

#[derive(Debug, Default)]
struct ContinuousReturnClockScaleAccumulator {
    original_return_count_scale: f64,
    long_variance_return_count_scale: f64,
    return_position: usize,
}

struct ReturnBackfitVarianceScales {
    observed_reference_variance_scale: f64,
    selected_continuous_model_variance_scale: f64,
    post_handoff_base_variance_scale: f64,
    post_handoff_dynamic_variance_scale: f64,
}

pub(super) fn return_backfit_delta_variance_handoff(
    input: ReturnBackfitVarianceHandoffInput<'_>,
) -> Result<ReturnBackfitVarianceHandoffFit, String> {
    let ReturnBackfitVarianceHandoffInput {
        hawkes_stage_payload,
        candidate_generation_input,
        selected_candidate,
        nig_configuration,
        return_clock_exposure_parameters,
        long_variance_parameters,
        news_process_configuration,
        surprise_deterministic_intensity_profile,
        progress_sender,
    } = input;

    validate_nig_configuration_for_handoff(nig_configuration)?;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::ContinuousSamplePreparation,
        1,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("prepare continuous return sample"),
    );

    let activity_calibration_context =
        resolve_activity_calibration_context_from_hawkes_payload(hawkes_stage_payload);

    let continuous_preparation = prepare_continuous_return_calibration_inputs(
        candidate_generation_input,
        activity_calibration_context,
    )
    .map_err(|source| format!("return-backfit continuous sample preparation failed: {source}"))?;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::DynamicClockReplay,
        2,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("replay dynamic return clock"),
    );

    let dynamic_clock_replay =
        replay_dynamic_clock_for_continuous_returns(DynamicClockReplayInput {
            hawkes_stage_payload,
            candidate_generation_input,
            news_process_configuration,
            surprise_deterministic_intensity_profile,
            nig_configuration,
            return_clock_exposure_parameters,
            long_variance_parameters,
            sorted_signals: &continuous_preparation.sorted_signals,
            progress_sender: progress_sender.clone(),
        })?;

    emit_return_backfit_progress(
        progress_sender.as_ref(),
        ReturnBackfitStageProgressPhase::DeltaVarianceHandoff,
        3,
        RETURN_BACKFIT_STAGE_PROGRESS_UNITS,
        Some("apply delta variance handoff"),
    );

    let long_variance_clock_scale_multiplier =
        dynamic_clock_replay.long_variance_clock_scale_multiplier;

    let delta_handoff_multiplier =
        validate_delta_handoff_multiplier(long_variance_clock_scale_multiplier)?;

    let post_handoff_nig_configuration =
        delta_variance_handoff_nig_configuration(nig_configuration, delta_handoff_multiplier)?;

    let variance_scales = return_backfit_variance_scales(
        selected_candidate,
        delta_handoff_multiplier,
        long_variance_clock_scale_multiplier,
    )?;

    Ok(ReturnBackfitVarianceHandoffFit {
        long_variance_clock_scale_multiplier,
        delta_handoff_multiplier,
        selected_nig_delta: nig_configuration.delta,
        post_handoff_nig_delta: post_handoff_nig_configuration.delta,
        selected_continuous_model_variance_scale: variance_scales
            .selected_continuous_model_variance_scale,
        post_handoff_base_variance_scale: variance_scales.post_handoff_base_variance_scale,
        post_handoff_dynamic_variance_scale: variance_scales.post_handoff_dynamic_variance_scale,
        observed_reference_variance_scale: variance_scales.observed_reference_variance_scale,
        post_handoff_nig_configuration,
    })
}

fn validate_delta_handoff_multiplier(
    long_variance_clock_scale_multiplier: f64,
) -> Result<f64, String> {
    validate_positive_number(
        long_variance_clock_scale_multiplier,
        "long_variance_clock_scale_multiplier",
    )
    .map_err(|source| {
        format!("return-backfit long-variance clock scale multiplier is invalid: {source}")
    })?;

    let delta_handoff_multiplier = 1.0 / long_variance_clock_scale_multiplier;

    validate_positive_number(delta_handoff_multiplier, "delta_handoff_multiplier")
        .map(|()| delta_handoff_multiplier)
        .map_err(|source| format!("return-backfit delta handoff multiplier is invalid: {source}"))
}

fn delta_variance_handoff_nig_configuration(
    nig_configuration: NigSharedPlusShiftConfiguration,
    delta_handoff_multiplier: f64,
) -> Result<NigSharedPlusShiftConfiguration, String> {
    let post_handoff_delta = nig_configuration.delta * delta_handoff_multiplier;

    validate_positive_number(post_handoff_delta, "post_handoff_nig_delta")
        .map_err(|source| format!("return-backfit post-handoff NIG delta is invalid: {source}"))?;

    NigSharedPlusShiftConfiguration::try_new(
        nig_configuration.alpha,
        nig_configuration.beta_0,
        post_handoff_delta,
        nig_configuration.mu,
        nig_configuration.delta_beta,
    )
    .map_err(|source| format!("return-backfit post-handoff NIG configuration is invalid: {source}"))
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

    progress_update.aggregate_progress_percent =
        Some(super::progress::return_backfit_phase_progress_percent(
            2,
            crate::stages::progress::progress_percent(completed_units, total_units),
        ));

    progress_update.current_item_label = Some("replay dynamic return clock".to_string());

    crate::stages::progress::emit_stage_progress(progress_sender, progress_update);
}

fn replay_dynamic_clock_for_continuous_returns(
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
        );

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

    if continuous_clock_scale_accumulator.return_position
        != sorted_signals.continuous_fit_returns.len()
    {
        return Err(format!(
            "return-backfit dynamic clock replay aligned {} continuous returns but expected {}",
            continuous_clock_scale_accumulator.return_position,
            sorted_signals.continuous_fit_returns.len()
        ));
    }

    if continuous_clock_scale_accumulator.original_return_count_scale <= 0.0 {
        return Err("return-backfit requires positive continuous return count scale".to_string());
    }

    let long_variance_clock_scale_multiplier = continuous_clock_scale_accumulator
        .long_variance_return_count_scale
        / continuous_clock_scale_accumulator.original_return_count_scale;

    validate_positive_number(
        long_variance_clock_scale_multiplier,
        "long_variance_clock_scale_multiplier",
    )
    .map_err(|source| {
        format!("return-backfit long-variance clock scale multiplier is invalid: {source}")
    })?;

    Ok(DynamicClockReplay {
        long_variance_clock_scale_multiplier,
    })
}

fn accumulate_continuous_return_clock_scale(
    accumulator: &mut ContinuousReturnClockScaleAccumulator,
    sorted_signals: &ReturnComponentsStageSortedSignalInputs,
    observation: &ReturnComponentsObservedOpenSessionReturn,
    original_business_time_scale: f64,
    dynamic_business_time_scale: f64,
) {
    let is_continuous_fit_return = sorted_signals
        .continuous_fit_returns
        .get(accumulator.return_position)
        .is_some_and(|continuous_observation| continuous_observation == observation);

    if !is_continuous_fit_return {
        return;
    }

    accumulator.original_return_count_scale += original_business_time_scale;
    accumulator.long_variance_return_count_scale += dynamic_business_time_scale;
    accumulator.return_position = accumulator.return_position.saturating_add(1);
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

fn validated_return_backfit_variance_scale(
    value: f64,
    field_name: &'static str,
) -> Result<f64, String> {
    validate_positive_number(value, field_name)
        .map(|()| value)
        .map_err(|source| format!("return-backfit {field_name} is invalid: {source}"))
}

fn return_backfit_variance_scales(
    selected_candidate: &ReturnComponentsStageCandidateDiagnostic,
    delta_handoff_multiplier: f64,
    long_variance_clock_scale_multiplier: f64,
) -> Result<ReturnBackfitVarianceScales, String> {
    let observed_reference_variance_scale = validated_return_backfit_variance_scale(
        selected_candidate
            .return_distribution_diagnostic
            .observed_return_volatility
            .powi(2),
        "observed_reference_variance_scale",
    )?;

    let selected_continuous_model_variance_scale = validated_return_backfit_variance_scale(
        selected_candidate
            .return_distribution_diagnostic
            .fitted_return_volatility
            .powi(2),
        "selected_continuous_model_variance_scale",
    )?;

    let post_handoff_base_variance_scale = validated_return_backfit_variance_scale(
        selected_continuous_model_variance_scale * delta_handoff_multiplier,
        "post_handoff_base_variance_scale",
    )?;

    let post_handoff_dynamic_variance_scale = validated_return_backfit_variance_scale(
        post_handoff_base_variance_scale * long_variance_clock_scale_multiplier,
        "post_handoff_dynamic_variance_scale",
    )?;

    Ok(ReturnBackfitVarianceScales {
        observed_reference_variance_scale,
        selected_continuous_model_variance_scale,
        post_handoff_base_variance_scale,
        post_handoff_dynamic_variance_scale,
    })
}

fn validate_nig_configuration_for_handoff(
    nig_configuration: NigSharedPlusShiftConfiguration,
) -> Result<(), String> {
    nig_configuration
        .derive_driver_parameters()
        .map(|_| ())
        .map_err(|source| format!("selected NIG configuration is invalid: {source}"))
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

#[cfg(test)]
mod tests {
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    use super::{delta_variance_handoff_nig_configuration, validate_delta_handoff_multiplier};

    #[test]
    fn delta_handoff_multiplier_offsets_dynamic_clock_scale() {
        assert_eq!(validate_delta_handoff_multiplier(2.0), Ok(0.5));
    }

    #[test]
    fn delta_handoff_preserves_nig_shape_parameters() {
        let configuration =
            NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
                .expect("fixture NIG parameters should be valid");

        let post_handoff = delta_variance_handoff_nig_configuration(configuration, 0.25)
            .expect("delta handoff should preserve NIG parameter validity");

        assert_eq!(post_handoff.alpha, configuration.alpha);
        assert_eq!(post_handoff.beta_0, configuration.beta_0);
        assert_eq!(post_handoff.delta, 5.0e-5);
        assert_eq!(post_handoff.mu, configuration.mu);
        assert_eq!(post_handoff.delta_beta, configuration.delta_beta);
    }
}
