use super::close_open_boundaries::derive_close_open_boundary_observations;
use super::contracts::MarketDataTranslationPreprocessingError;
use super::jump_candidates::derive_intraday_jump_candidates;
use super::known_news_samples::{
    activity_lift, attributed_open_return_timestamps, derive_known_news_return_samples,
};
use super::shared::{
    BTreeMap, CalibrationMeanReversionStageObservation,
    CalibrationReturnComponentsCloseOpenGapDetectionPolicy,
    CalibrationReturnComponentsDetectedIntradayJumpCandidate,
    CalibrationReturnComponentsKnownNewsCalendarInput,
    CalibrationReturnComponentsKnownNewsReturnSample,
    CalibrationReturnComponentsObjectiveMetricWeights,
    CalibrationReturnComponentsObservedCloseOpenBoundaryObservation,
    CalibrationReturnComponentsObservedOpenSessionReturn,
    CalibrationReturnComponentsSignalSourceWindow,
    CalibrationReturnComponentsStageCandidateGenerationInput,
    CalibrationSharedNewsMarkLawConfiguration, ExchangeCalendarDefinition,
    MarketDataTranslationReturnComponentsPreprocessingSettings, OpenTickRecord,
    ResolvedKnownNewsRecord, TradeSign, Tz, fit_shared_news_mark_law_from_shock_samples,
    median_abs, round_to,
};
use janiform_calibration_artifacts::TickGridObservationModel;
use janiform_foundations::time::duration_nanos_to_hours;
use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

const TICK_GRID_OBSERVATION_POLICY: &str = "nearest_reference_price_normalized_cumulative_latent_price_rounding_preserves_sub_tick_residuals.v1";
const TICK_SIZE_INFERENCE_POLICY: &str =
    "minimum_positive_same_session_adjacent_open_price_difference_reference_price_normalized.v1";

const PROVIDED_TICK_SIZE_SOURCE: &str = "user_provided_reference_price_normalized";
const INFERRED_TICK_SIZE_SOURCE: &str = "training_data_inferred_reference_price_normalized";
const DISABLED_TICK_SIZE_SOURCE: &str = "unavailable";

pub(super) fn build_return_components_candidate_generation_input(
    input: ReturnComponentsCandidateGenerationInputBuildInput<'_>,
) -> Result<
    (
        CalibrationReturnComponentsStageCandidateGenerationInput,
        usize,
    ),
    MarketDataTranslationPreprocessingError,
> {
    let ReturnComponentsCandidateGenerationInputBuildInput {
        definition,
        timezone,
        exchange_code,
        open_price_ticks,
        open_signed_tick_count,
        open_session_sell_tick_count,
        reference_price,
        retained_known_news,
        source_window,
        _option_surface_quote_values,
        preprocessing_settings,
    } = input;

    if open_price_ticks.len() < 2 {
        return Err(MarketDataTranslationPreprocessingError::MissingOpenSessionPriceObservations);
    }

    let return_components_settings = preprocessing_settings.cloned().unwrap_or(
        MarketDataTranslationReturnComponentsPreprocessingSettings {
            intraday_jump_threshold_multiplier: 4.0,
            close_open_gap_tolerance_multiplier: 3.0,
            tick_size: None,
        },
    );

    let open_price_tick_timestamps: Vec<i64> = open_price_ticks
        .iter()
        .map(|tick| tick.timestamp_utc_nanos)
        .collect();

    let same_session_returns = same_session_open_returns(open_price_ticks);

    let observed_open_session_returns = derive_observed_open_session_returns(
        open_price_ticks,
        open_price_tick_timestamps.as_slice(),
    );

    let tick_grid_observation_model = tick_grid_observation_model(
        open_price_ticks,
        reference_price,
        return_components_settings.tick_size,
    )?;

    let close_open_boundary_observations = derive_close_open_boundary_observations(
        definition,
        timezone,
        exchange_code,
        open_price_ticks,
        open_price_tick_timestamps.as_slice(),
    )?;

    let known_news_return_samples = derive_known_news_return_samples(
        retained_known_news,
        open_price_ticks,
        close_open_boundary_observations.as_slice(),
        open_price_tick_timestamps.as_slice(),
    );

    let known_news_timestamps: Vec<i64> = retained_known_news
        .iter()
        .map(|record| record.event_timestamp_utc_nanos)
        .collect();

    let known_news_attributed_open_return_timestamps = attributed_open_return_timestamps(
        retained_known_news,
        open_price_ticks,
        open_price_tick_timestamps.as_slice(),
    );

    let intraday_jump_candidates = derive_intraday_jump_candidates(
        same_session_returns.as_slice(),
        open_price_ticks,
        open_price_tick_timestamps.as_slice(),
        known_news_attributed_open_return_timestamps.as_slice(),
        return_components_settings.intraday_jump_threshold_multiplier,
    );

    let open_tick_sell_share = if open_signed_tick_count == 0 {
        0.5
    } else {
        open_session_sell_tick_count as f64 / open_signed_tick_count as f64
    };

    let intraday_return_values: Vec<f64> = same_session_returns
        .iter()
        .map(|(_, return_value)| *return_value)
        .collect();

    let median_abs_intraday_return = median_abs(intraday_return_values.as_slice());

    let gap_tolerance =
        median_abs_intraday_return * return_components_settings.close_open_gap_tolerance_multiplier;

    let material_close_open_gap_count = close_open_boundary_observations
        .iter()
        .filter(|observation| {
            f64::abs(f64::ln(observation.open_price / observation.close_price)) > gap_tolerance
        })
        .count();

    let shared_mark_law_template = derive_shared_mark_law_template(
        known_news_return_samples.as_slice(),
        intraday_jump_candidates.as_slice(),
        close_open_boundary_observations.as_slice(),
        intraday_return_values.as_slice(),
    );

    let candidate_generation_input = CalibrationReturnComponentsStageCandidateGenerationInput {
        known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput {
            source_window,
            scheduled_event_timestamps_utc_nanos: known_news_timestamps,
        },
        realized_known_news_return_samples: known_news_return_samples,
        observed_open_session_returns,
        detected_intraday_jump_candidates: intraday_jump_candidates,
        observed_close_open_boundary_observations: close_open_boundary_observations.clone(),
        close_open_gap_detection_policy: CalibrationReturnComponentsCloseOpenGapDetectionPolicy {
            material_gap_absolute_return_tolerance: round_to(gap_tolerance, 6),
        },
        open_session_sell_share_proxy: round_to(open_tick_sell_share, 6),
        shared_mark_law_template,
        return_clock_exposure_parameters: ReturnClockExposureParameters::neutral(),
        tick_grid_observation_model,
        objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights {
            continuous_distribution_fit_weight: 1.0,
            surprise_arrival_fit_weight: 1.0,
            jump_fit_weight: 1.0,
            gap_fit_weight: 1.0,
            activity_coupling_fit_weight: 1.0,
            price_range_sanity_fit_weight: 0.1,
        },
    };

    Ok((candidate_generation_input, material_close_open_gap_count))
}

pub(super) struct ReturnComponentsCandidateGenerationInputBuildInput<'a> {
    pub(super) definition: &'a ExchangeCalendarDefinition,
    pub(super) timezone: Tz,
    pub(super) exchange_code: &'a str,
    pub(super) open_price_ticks: &'a [OpenTickRecord],
    pub(super) open_signed_tick_count: usize,
    pub(super) open_session_sell_tick_count: usize,
    pub(super) reference_price: Option<f64>,
    pub(super) retained_known_news: &'a [ResolvedKnownNewsRecord],
    pub(super) source_window: CalibrationReturnComponentsSignalSourceWindow,
    pub(super) _option_surface_quote_values: &'a [f64],
    pub(super) preprocessing_settings:
        Option<&'a MarketDataTranslationReturnComponentsPreprocessingSettings>,
}

pub(super) fn derive_mean_reversion_open_session_grid_inputs(
    open_price_ticks: &[OpenTickRecord],
) -> (Vec<CalibrationMeanReversionStageObservation>, Vec<f64>) {
    let observations = open_price_ticks
        .iter()
        .map(|tick| CalibrationMeanReversionStageObservation {
            observation_timestamp_utc_nanos: tick.timestamp_utc_nanos,
            observed_log_price: tick.price.ln(),
        })
        .collect();

    let mut unique_session_duration_hours: BTreeMap<(i64, i64), f64> = BTreeMap::new();

    for tick in open_price_ticks {
        let session_duration_nanos = tick.session_close_utc_nanos - tick.session_open_utc_nanos;
        if session_duration_nanos <= 0 {
            continue;
        }

        unique_session_duration_hours
            .entry((tick.session_open_utc_nanos, tick.session_close_utc_nanos))
            .or_insert(duration_nanos_to_hours(session_duration_nanos));
    }

    (
        observations,
        unique_session_duration_hours.into_values().collect(),
    )
}

pub(super) fn derive_observed_open_session_returns(
    open_price_ticks: &[OpenTickRecord],
    open_price_tick_timestamps: &[i64],
) -> Vec<CalibrationReturnComponentsObservedOpenSessionReturn> {
    open_price_ticks
        .iter()
        .zip(open_price_ticks.iter().skip(1))
        .filter(|(previous_tick, current_tick)| {
            previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
                && previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
                && current_tick.timestamp_utc_nanos > previous_tick.timestamp_utc_nanos
        })
        .map(
            |(previous_tick, current_tick)| CalibrationReturnComponentsObservedOpenSessionReturn {
                return_timestamp_utc_nanos: current_tick.timestamp_utc_nanos,
                elapsed_same_session_hours: duration_nanos_to_hours(
                    current_tick.timestamp_utc_nanos - previous_tick.timestamp_utc_nanos,
                ),
                log_return: f64::ln(current_tick.price / previous_tick.price),
                post_shock_activity_lift: activity_lift(
                    current_tick.timestamp_utc_nanos,
                    open_price_ticks,
                    open_price_tick_timestamps,
                ),
                sell_share_proxy: sell_share_proxy_for_open_return(current_tick.trade_sign),
            },
        )
        .collect()
}

pub(super) fn sell_share_proxy_for_open_return(trade_sign: Option<TradeSign>) -> f64 {
    match trade_sign {
        Some(TradeSign::Buy) => 0.0,
        Some(TradeSign::Sell) => 1.0,
        None => 0.5,
    }
}

pub(super) fn same_session_open_returns(open_price_ticks: &[OpenTickRecord]) -> Vec<(i64, f64)> {
    let mut same_session_returns: Vec<(i64, f64)> = Vec::new();

    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos != current_tick.session_close_utc_nanos
            || previous_tick.session_open_utc_nanos != current_tick.session_open_utc_nanos
            || current_tick.timestamp_utc_nanos <= previous_tick.timestamp_utc_nanos
        {
            continue;
        }

        same_session_returns.push((
            current_tick.timestamp_utc_nanos,
            f64::ln(current_tick.price / previous_tick.price),
        ));
    }

    same_session_returns
}

fn tick_grid_observation_model(
    open_price_ticks: &[OpenTickRecord],
    reference_price: Option<f64>,
    provided_tick_size: Option<f64>,
) -> Result<TickGridObservationModel, MarketDataTranslationPreprocessingError> {
    let (tick_size, tick_size_source) = match provided_tick_size {
        Some(tick_size) => (
            Some(reference_price_normalized_tick_size(
                tick_size,
                reference_price,
            )?),
            PROVIDED_TICK_SIZE_SOURCE,
        ),
        None => match infer_tick_size_from_open_prices(open_price_ticks) {
            Some(tick_size) => (
                Some(reference_price_normalized_tick_size(
                    tick_size,
                    reference_price,
                )?),
                INFERRED_TICK_SIZE_SOURCE,
            ),
            None => (None, DISABLED_TICK_SIZE_SOURCE),
        },
    };

    Ok(TickGridObservationModel {
        tick_size,
        tick_size_source: tick_size_source.to_string(),
        inference_policy: TICK_SIZE_INFERENCE_POLICY.to_string(),
        observation_policy: TICK_GRID_OBSERVATION_POLICY.to_string(),
    })
}

fn reference_price_normalized_tick_size(
    tick_size: f64,
    reference_price: Option<f64>,
) -> Result<f64, MarketDataTranslationPreprocessingError> {
    let Some(reference_price) = reference_price
        .filter(|reference_price| reference_price.is_finite() && *reference_price > 0.0)
    else {
        return Err(
            MarketDataTranslationPreprocessingError::InvalidTickGridReferencePrice {
                reference_price,
            },
        );
    };

    Ok(tick_size / reference_price)
}

pub(super) fn infer_tick_size_from_open_prices(open_price_ticks: &[OpenTickRecord]) -> Option<f64> {
    open_price_ticks
        .iter()
        .zip(open_price_ticks.iter().skip(1))
        .filter(|(previous_tick, current_tick)| {
            previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
                && previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
        })
        .filter_map(|(previous_tick, current_tick)| {
            if !previous_tick.price.is_finite()
                || previous_tick.price <= 0.0
                || !current_tick.price.is_finite()
                || current_tick.price <= 0.0
            {
                return None;
            }

            let price_difference = (current_tick.price - previous_tick.price).abs();
            (price_difference.is_finite() && price_difference > 0.0).then_some(price_difference)
        })
        .min_by(f64::total_cmp)
}

pub(super) fn derive_shared_mark_law_template(
    known_news_return_samples: &[CalibrationReturnComponentsKnownNewsReturnSample],
    intraday_jump_candidates: &[CalibrationReturnComponentsDetectedIntradayJumpCandidate],
    close_open_boundary_observations: &[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation],
    intraday_return_values: &[f64],
) -> CalibrationSharedNewsMarkLawConfiguration {
    let open_shock_returns: Vec<f64> = known_news_return_samples
        .iter()
        .map(|sample| sample.open_window_jump_return)
        .chain(
            intraday_jump_candidates
                .iter()
                .map(|candidate| candidate.jump_return),
        )
        .collect();

    let gap_returns: Vec<f64> = known_news_return_samples
        .iter()
        .map(|sample| sample.closed_window_gap_return)
        .chain(
            close_open_boundary_observations
                .iter()
                .map(|observation| f64::ln(observation.open_price / observation.close_price)),
        )
        .collect();

    let fallback_absolute_returns: Vec<f64> =
        intraday_return_values
            .iter()
            .copied()
            .map(f64::abs)
            .chain(close_open_boundary_observations.iter().map(|observation| {
                f64::abs(f64::ln(observation.open_price / observation.close_price))
            }))
            .collect();

    let fitted_mark_law = fit_shared_news_mark_law_from_shock_samples(
        open_shock_returns.as_slice(),
        gap_returns.as_slice(),
        fallback_absolute_returns.as_slice(),
    );

    CalibrationSharedNewsMarkLawConfiguration {
        gamma_severity_marks: fitted_mark_law.gamma_severity_marks,
    }
}
