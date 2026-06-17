use std::collections::BTreeMap;

use janiform_calibration_artifacts::HawkesStageFittedParameters;
use janiform_market_time::calendar::OPEN_MARKET_STATE_CODE;
use janiform_model_kernels::hawkes::{HawkesEventSide, HawkesExcitationState};
use janiform_model_kernels::return_clock::{
    IntegratedSideActivityDecomposition, SideSourceActivityDecomposition,
};

use crate::stages::hawkes::{
    HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_SELL, hawkes_open_events_are_adjacent,
};
use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;
use crate::stages::seasonality::SeasonalityStageNormalizedPayload;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct OpenSessionReturnSourceActivityFit {
    pub(super) source_activities: Vec<SideSourceActivityDecomposition>,
}

pub(super) fn fit_open_session_return_source_activities(
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    event_side_codes: &[u8],
    hawkes_fitted_parameters: &HawkesStageFittedParameters,
) -> Result<OpenSessionReturnSourceActivityFit, String> {
    validate_source_fit_inputs(seasonality_payload, event_side_codes)?;

    let mut source_activities = observed_returns
        .iter()
        .map(|observation| {
            SideSourceActivityDecomposition::from_sell_share_proxy(observation.sell_share_proxy)
        })
        .collect::<Vec<_>>();

    let mut observation_positions_by_timestamp =
        observation_positions_by_timestamp(observed_returns);

    let mut previous_open_timestamp_utc_nanos = None;
    let mut state = HawkesExcitationState::from_target_sums(0.0, 0.0);

    for (event_position, ((timestamp_utc_nanos, market_state_code), event_side_code)) in
        seasonality_payload
            .timestamp_utc_nanos
            .iter()
            .copied()
            .zip(seasonality_payload.market_state_codes.iter().copied())
            .zip(event_side_codes.iter().copied())
            .enumerate()
    {
        let event_side = event_side_from_code(event_position, event_side_code)?;

        if market_state_code != OPEN_MARKET_STATE_CODE {
            continue;
        }

        let delta_exposure_hours = seasonality_payload
            .open_event_business_time_delta_exposure_hours_by_event
            .get(event_position)
            .copied()
            .ok_or_else(|| {
                format!(
                    "seasonality open exposure vector is missing event position {event_position}"
                )
            })?;

        if !delta_exposure_hours.is_finite() || delta_exposure_hours < 0.0 {
            return Err(format!(
                "seasonality open exposure at event position {event_position} must be finite and non-negative, found {delta_exposure_hours}"
            ));
        }

        let reset_state = previous_open_timestamp_utc_nanos
            .map(|previous_timestamp| {
                !hawkes_open_events_are_adjacent(previous_timestamp, timestamp_utc_nanos)
            })
            .unwrap_or(true);

        if reset_state {
            state = HawkesExcitationState::from_target_sums(0.0, 0.0);
        } else {
            state = state.apply_shared_decay_drift(
                hawkes_fitted_parameters.kernel_parameters.beta,
                delta_exposure_hours,
            );
        }

        if let Some(observation_positions) =
            observation_positions_by_timestamp.remove(&timestamp_utc_nanos)
        {
            let source_activity = source_activity_for_event(
                event_side,
                state,
                hawkes_fitted_parameters.kernel_parameters.lambda_bar,
            )?;

            for observation_position in observation_positions {
                let Some(slot) = source_activities.get_mut(observation_position) else {
                    return Err(format!(
                        "observed return source activity position {observation_position} is outside output vector"
                    ));
                };
                *slot = source_activity;
            }
        }

        state = state.apply_endogenous_event_jump(
            event_side,
            hawkes_fitted_parameters.kernel_parameters.alpha,
        );

        previous_open_timestamp_utc_nanos = Some(timestamp_utc_nanos);
    }

    Ok(OpenSessionReturnSourceActivityFit { source_activities })
}

fn validate_source_fit_inputs(
    seasonality_payload: &SeasonalityStageNormalizedPayload,
    event_side_codes: &[u8],
) -> Result<(), String> {
    let event_count = seasonality_payload.timestamp_utc_nanos.len();

    if seasonality_payload.market_state_codes.len() != event_count {
        return Err(format!(
            "seasonality market_state_codes length {} must match timestamp length {event_count}",
            seasonality_payload.market_state_codes.len()
        ));
    }

    if seasonality_payload
        .open_event_business_time_delta_exposure_hours_by_event
        .len()
        != event_count
    {
        return Err(format!(
            "seasonality open exposure length {} must match timestamp length {event_count}",
            seasonality_payload
                .open_event_business_time_delta_exposure_hours_by_event
                .len()
        ));
    }

    if event_side_codes.len() != event_count {
        return Err(format!(
            "event_side_codes length {} must match seasonality event count {event_count}",
            event_side_codes.len()
        ));
    }

    Ok(())
}

fn observation_positions_by_timestamp(
    observed_returns: &[ReturnComponentsObservedOpenSessionReturn],
) -> BTreeMap<i64, Vec<usize>> {
    let mut positions_by_timestamp = BTreeMap::new();

    for (observation_position, observation) in observed_returns.iter().enumerate() {
        positions_by_timestamp
            .entry(observation.return_timestamp_utc_nanos)
            .or_insert_with(Vec::new)
            .push(observation_position);
    }

    positions_by_timestamp
}

fn event_side_from_code(
    event_position: usize,
    event_side_code: u8,
) -> Result<HawkesEventSide, String> {
    match event_side_code {
        HAWKES_EVENT_SIDE_CODE_BUY => Ok(HawkesEventSide::Buy),
        HAWKES_EVENT_SIDE_CODE_SELL => Ok(HawkesEventSide::Sell),
        _ => Err(format!(
            "event_side_codes[{event_position}] must be {HAWKES_EVENT_SIDE_CODE_BUY} for buy or {HAWKES_EVENT_SIDE_CODE_SELL} for sell, found {event_side_code}"
        )),
    }
}

fn source_activity_for_event(
    event_side: HawkesEventSide,
    state: HawkesExcitationState,
    lambda_bar: f64,
) -> Result<SideSourceActivityDecomposition, String> {
    let target_excitation = match event_side {
        HawkesEventSide::Buy => state.target_plus_sum(),
        HawkesEventSide::Sell => state.target_minus_sum(),
    };

    if !lambda_bar.is_finite() || lambda_bar <= 0.0 {
        return Err(format!(
            "Hawkes lambda_bar must be finite and positive for source responsibilities, found {lambda_bar}"
        ));
    }

    if !target_excitation.is_finite() || target_excitation < 0.0 {
        return Err(format!(
            "Hawkes target excitation must be finite and non-negative for source responsibilities, found {target_excitation}"
        ));
    }

    let target_intensity = lambda_bar + target_excitation;
    if !target_intensity.is_finite() || target_intensity <= 0.0 {
        return Err(format!(
            "Hawkes target intensity must be finite and positive for source responsibilities, found {target_intensity}"
        ));
    }

    let baseline_activity = lambda_bar / target_intensity;
    let endogenous_activity = target_excitation / target_intensity;

    match event_side {
        HawkesEventSide::Buy => Ok(SideSourceActivityDecomposition::new(
            IntegratedSideActivityDecomposition::new(baseline_activity, endogenous_activity),
            IntegratedSideActivityDecomposition::new(0.0, 0.0),
        )),
        HawkesEventSide::Sell => Ok(SideSourceActivityDecomposition::new(
            IntegratedSideActivityDecomposition::new(0.0, 0.0),
            IntegratedSideActivityDecomposition::new(baseline_activity, endogenous_activity),
        )),
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::HawkesStageFittedParameters;
    use janiform_market_time::business_time_clock::{
        FOURIER_BUSINESS_TIME_CLOCK_BASIS, FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM,
        FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION,
        FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY,
        FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY, FourierBusinessTimeClockModel,
        zero_order_harmonic_series_by_axis,
    };
    use janiform_market_time::calendar::OPEN_MARKET_STATE_CODE;
    use janiform_model_kernels::hawkes::{
        HawkesKernelLoadings, HawkesKernelParameters, HawkesStructuredAlphaParameterization,
        HawkesStructuredRhoParameterization,
    };
    use janiform_model_kernels::return_clock::{
        IntegratedSideActivityDecomposition, SideSourceActivityDecomposition,
    };

    use super::fit_open_session_return_source_activities;
    use crate::stages::hawkes::HAWKES_EVENT_SIDE_CODE_BUY;
    use crate::stages::return_components::ReturnComponentsObservedOpenSessionReturn;
    use crate::stages::seasonality::SeasonalityStageNormalizedPayload;

    #[test]
    fn source_activity_fit_uses_current_hawkes_source_responsibility() {
        let fit = fit_open_session_return_source_activities(
            &[
                observed_return(0),
                observed_return(janiform_foundations::time::NANOS_PER_SECOND),
            ],
            &seasonality_payload(),
            &[HAWKES_EVENT_SIDE_CODE_BUY, HAWKES_EVENT_SIDE_CODE_BUY],
            &hawkes_parameters(),
        )
        .expect("source responsibility fit should succeed");

        assert_eq!(
            fit.source_activities,
            vec![
                SideSourceActivityDecomposition::new(
                    IntegratedSideActivityDecomposition::new(1.0, 0.0),
                    IntegratedSideActivityDecomposition::new(0.0, 0.0),
                ),
                SideSourceActivityDecomposition::new(
                    IntegratedSideActivityDecomposition::new(0.5, 0.5),
                    IntegratedSideActivityDecomposition::new(0.0, 0.0),
                ),
            ]
        );
    }

    fn observed_return(
        return_timestamp_utc_nanos: i64,
    ) -> ReturnComponentsObservedOpenSessionReturn {
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos,
            elapsed_same_session_hours: 1.0,
            log_return: 0.0,
            post_shock_activity_lift: 1.0,
            sell_share_proxy: 0.5,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.5),
        }
    }

    fn seasonality_payload() -> SeasonalityStageNormalizedPayload {
        SeasonalityStageNormalizedPayload {
            schema_version: "seasonality.normalized.test.v1",
            dataset_id: "dataset".to_string(),
            exchange_code: "XCME".to_string(),
            trade_events_data_reference: "memory://trade-events".to_string(),
            timestamp_event_input_schema_version: "timestamp-input.v1".to_string(),
            timestamp_event_preprocessed_schema_version: "timestamp-preprocessed.v1".to_string(),
            clock_model_schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION
                .to_string(),
            source_record_positions: vec![0, 1],
            timestamp_utc_nanos: vec![0, janiform_foundations::time::NANOS_PER_SECOND],
            exchange_local_minute_of_week: vec![0, 0],
            market_state_codes: vec![OPEN_MARKET_STATE_CODE, OPEN_MARKET_STATE_CODE],
            business_time_rate_by_event: vec![1.0, 1.0],
            open_event_business_time_delta_exposure_hours_by_event: vec![0.0, 0.0],
            clock_model: flat_clock_model(),
        }
    }

    fn flat_clock_model() -> FourierBusinessTimeClockModel {
        FourierBusinessTimeClockModel {
            schema_version: FOURIER_BUSINESS_TIME_CLOCK_MODEL_SCHEMA_VERSION.to_string(),
            coordinate_system: FOURIER_BUSINESS_TIME_CLOCK_COORDINATE_SYSTEM.to_string(),
            coefficient_basis: FOURIER_BUSINESS_TIME_CLOCK_BASIS.to_string(),
            normalization_policy: FOURIER_BUSINESS_TIME_CLOCK_NORMALIZATION_POLICY.to_string(),
            selection_policy: FOURIER_BUSINESS_TIME_CLOCK_SELECTION_POLICY.to_string(),
            intercept: 0.0,
            harmonic_series: zero_order_harmonic_series_by_axis(),
            selected_objective_value: 0.0,
            selected_negative_log_likelihood: 0.0,
            evaluated_candidate_count: 1,
            fit_open_minute_count: 1,
            fit_open_event_count: 2,
            fit_open_exposure_seconds: 60.0,
        }
    }

    fn hawkes_parameters() -> HawkesStageFittedParameters {
        let alpha = HawkesKernelLoadings {
            plus_plus: 1.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 1.0,
        };

        let rho = HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        };

        HawkesStageFittedParameters {
            kernel_parameters: HawkesKernelParameters::try_new(1.0, 4.0, alpha, rho)
                .expect("test Hawkes parameters should validate"),
            structured_alpha: HawkesStructuredAlphaParameterization::new(1.0, 0.0, 0.0, 0.0),
            structured_rho: HawkesStructuredRhoParameterization::new(0.0, 0.0, 0.0, 0.0),
            self_dominance_policy: None,
            stability_numeric_tolerance: 0.0,
        }
    }
}
