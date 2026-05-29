use chrono::{DateTime, Utc};
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, MarketState};

use super::super::contracts::{
    ReturnComponentsCloseOpenGapDetectionPolicy, ReturnComponentsDetectedCloseOpenGapCandidate,
    ReturnComponentsObservedCloseOpenBoundaryObservation,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError, ReturnComponentsStageSignalSourceWindow,
};
use super::super::{signal_partition_fit, surprise_episode_selection_fit};
use super::signals::ReturnComponentsStagePreparedCandidateGenerationInput;
use crate::shock_mark_fitting::fit_shared_news_mark_law_from_shock_samples;

pub(crate) fn prepare_candidate_generation_input(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    mut candidate_generation_input: ReturnComponentsStageCandidateGenerationInput,
) -> Result<
    ReturnComponentsStagePreparedCandidateGenerationInput,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let fitted_signal_partition_layer = signal_partition_fit::fit_signal_partition_layer(
        candidate_generation_input
            .observed_open_session_returns
            .as_slice(),
        candidate_generation_input
            .realized_known_news_return_samples
            .as_slice(),
        candidate_generation_input
            .detected_intraday_jump_candidates
            .as_slice(),
    );

    let derived_close_open_gap_candidates = derive_close_open_gap_candidates(
        exchange_code,
        calendar_registry,
        candidate_generation_input.known_news_calendar.source_window,
        candidate_generation_input
            .observed_close_open_boundary_observations
            .as_slice(),
        candidate_generation_input.close_open_gap_detection_policy,
    )?;

    let close_open_gap_detection_filter = format!(
        "derive_close_open_log_gap_candidates_abs_gt_{:.6}",
        candidate_generation_input
            .close_open_gap_detection_policy
            .material_gap_absolute_return_tolerance
    );

    let source_window_exposure = calendar_registry
        .resolve_market_state_exposure_in_utc_interval(
            exchange_code,
            candidate_generation_input
                .known_news_calendar
                .source_window
                .window_start_utc_nanos,
            candidate_generation_input
                .known_news_calendar
                .source_window
                .window_end_utc_nanos,
        )
        .map_err(|source| {
            ReturnComponentsStageCandidateGenerationInputError::SourceWindowStateExposureResolutionFailed {
                exchange_code: exchange_code.to_string(),
                window_start_utc_nanos: candidate_generation_input
                    .known_news_calendar
                    .source_window
                    .window_start_utc_nanos,
                window_end_utc_nanos: candidate_generation_input
                    .known_news_calendar
                    .source_window
                    .window_end_utc_nanos,
                error_message: source.to_string(),
            }
        })?;

    let fitted_surprise_episode_selection_layer =
        surprise_episode_selection_fit::fit_surprise_episode_selection_layer(
            candidate_generation_input
                .detected_intraday_jump_candidates
                .as_slice(),
            fitted_signal_partition_layer.continuous_return_filter_threshold,
        );

    let open_shock_returns: Vec<f64> = candidate_generation_input
        .realized_known_news_return_samples
        .iter()
        .map(|sample| sample.open_window_jump_return)
        .chain(
            fitted_surprise_episode_selection_layer
                .selected_surprise_jump_episodes
                .iter()
                .map(|episode| episode.cumulative_jump_return),
        )
        .collect();

    let gap_returns: Vec<f64> = candidate_generation_input
        .realized_known_news_return_samples
        .iter()
        .map(|sample| sample.closed_window_gap_return)
        .chain(
            derived_close_open_gap_candidates
                .iter()
                .map(|candidate| candidate.gap_return),
        )
        .collect();

    let fallback_absolute_returns: Vec<f64> = candidate_generation_input
        .observed_open_session_returns
        .iter()
        .map(|observation| observation.log_return.abs())
        .chain(
            candidate_generation_input
                .observed_close_open_boundary_observations
                .iter()
                .map(|observation| {
                    f64::abs(f64::ln(observation.open_price / observation.close_price))
                }),
        )
        .collect();

    candidate_generation_input.shared_mark_law_template =
        fit_shared_news_mark_law_from_shock_samples(
            open_shock_returns.as_slice(),
            gap_returns.as_slice(),
            fallback_absolute_returns.as_slice(),
        );

    Ok(ReturnComponentsStagePreparedCandidateGenerationInput {
        candidate_generation_input,
        derived_close_open_gap_candidates,
        close_open_gap_detection_filter,
        source_window_open_exposure_hours: duration_nanos_to_hours(
            source_window_exposure.open_window_nanos,
        ),
        source_window_closed_exposure_hours: duration_nanos_to_hours(
            source_window_exposure.closed_window_nanos,
        ),
    })
}

pub(crate) fn derive_close_open_gap_candidates(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    source_window: ReturnComponentsStageSignalSourceWindow,
    observed_close_open_boundary_observations: &[ReturnComponentsObservedCloseOpenBoundaryObservation],
    detection_policy: ReturnComponentsCloseOpenGapDetectionPolicy,
) -> Result<
    Vec<ReturnComponentsDetectedCloseOpenGapCandidate>,
    ReturnComponentsStageCandidateGenerationInputError,
> {
    let mut derived_close_open_gap_candidates = Vec::new();

    for observed_boundary in observed_close_open_boundary_observations {
        observed_boundary.validate(source_window)?;

        validate_close_open_boundary_transition(
            exchange_code,
            calendar_registry,
            observed_boundary.close_timestamp_utc_nanos,
            observed_boundary.open_timestamp_utc_nanos,
        )?;

        let gap_return = (observed_boundary.open_price / observed_boundary.close_price).ln();
        if gap_return.abs() <= detection_policy.material_gap_absolute_return_tolerance {
            continue;
        }

        let derived_candidate = ReturnComponentsDetectedCloseOpenGapCandidate {
            close_timestamp_utc_nanos: observed_boundary.close_timestamp_utc_nanos,
            open_timestamp_utc_nanos: observed_boundary.open_timestamp_utc_nanos,
            gap_return,
            post_shock_activity_lift: observed_boundary.post_shock_activity_lift,
        };

        derived_candidate.validate(source_window)?;
        derived_close_open_gap_candidates.push(derived_candidate);
    }

    Ok(derived_close_open_gap_candidates)
}

fn validate_close_open_boundary_transition(
    exchange_code: &str,
    calendar_registry: &ExchangeCalendarRegistry,
    close_timestamp_utc_nanos: i64,
    open_timestamp_utc_nanos: i64,
) -> Result<(), ReturnComponentsStageCandidateGenerationInputError> {
    let close_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        close_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let close_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, close_timestamp_utc_nanos)?;

    if close_previous_state != MarketState::Open || close_boundary_state != MarketState::Closed {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidCloseBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                close_timestamp_utc_nanos,
                previous_state: close_previous_state,
                boundary_state: close_boundary_state,
            },
        );
    }

    let open_previous_state = resolve_market_state_at_nanos(
        calendar_registry,
        exchange_code,
        open_timestamp_utc_nanos.saturating_sub(1),
    )?;
    let open_boundary_state =
        resolve_market_state_at_nanos(calendar_registry, exchange_code, open_timestamp_utc_nanos)?;

    if open_previous_state != MarketState::Closed || open_boundary_state != MarketState::Open {
        return Err(
            ReturnComponentsStageCandidateGenerationInputError::InvalidOpenBoundaryTransition {
                exchange_code: exchange_code.to_string(),
                open_timestamp_utc_nanos,
                previous_state: open_previous_state,
                boundary_state: open_boundary_state,
            },
        );
    }

    Ok(())
}

fn resolve_market_state_at_nanos(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    timestamp_utc_nanos: i64,
) -> Result<MarketState, ReturnComponentsStageCandidateGenerationInputError> {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);

    calendar_registry
        .resolve_market_state_at_utc(exchange_code, timestamp_utc)
        .map_err(|source: janiform_market_time::calendar::ExchangeCalendarResolveError| {
            ReturnComponentsStageCandidateGenerationInputError::CloseOpenBoundaryStateResolutionFailed {
                exchange_code: exchange_code.to_string(),
                timestamp_utc_nanos,
                error_message: source.to_string(),
            }
        })
}
