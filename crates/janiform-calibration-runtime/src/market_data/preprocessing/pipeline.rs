use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::contracts::{
    MarketDataTranslationCalibrationError, MarketDataTranslationCalibrationOutput,
    MarketDataTranslationCalibrationRequest,
};
use super::hawkes_inputs::{
    HawkesCandidateInputsBuildInput, build_hawkes_candidate_inputs, build_hawkes_event_side_codes,
};
use super::known_news_samples::resolve_known_news_records;
use super::return_components_inputs::{
    ReturnComponentsCandidateGenerationInputBuildInput,
    build_return_components_candidate_generation_input,
    derive_mean_reversion_open_session_grid_inputs,
    derive_observation_models_open_session_timestamp_transitions,
};
use super::seasonality_input::build_seasonality_stage_input;
use super::session_windows::required_definition_and_timezone;
use super::tick_resolution::preprocess_tick_records;
use crate::execution_bridge::CalibrationObservationModelsStageExecutionInput;

pub fn calibrate_market_data_translation_request(
    request: MarketDataTranslationCalibrationRequest,
    calendar_registry: &ExchangeCalendarRegistry,
) -> Result<MarketDataTranslationCalibrationOutput, MarketDataTranslationCalibrationError> {
    let (definition, timezone) =
        required_definition_and_timezone(calendar_registry, request.exchange_code.as_str())?;

    let tick_summary = preprocess_tick_records(
        request.tick_records.as_slice(),
        definition,
        timezone,
        request.exchange_code.as_str(),
    )?;

    let retained_known_news = resolve_known_news_records(
        request.known_news_records.as_slice(),
        definition,
        timezone,
        tick_summary.source_window.clone(),
    )?;

    let seasonality_stage_input = build_seasonality_stage_input(
        tick_summary.timestamp_event_records.clone(),
        retained_known_news.retained_records.as_slice(),
        request.trade_events_data_reference,
        request
            .calibration_settings
            .as_ref()
            .and_then(|settings| settings.seasonality.as_ref()),
    );

    let (hawkes_candidate_inputs, hawkes_event_side_codes) = if request.require_hawkes {
        (
            Some(build_hawkes_candidate_inputs(
                HawkesCandidateInputsBuildInput {
                    resolved_ticks: tick_summary.resolved_ticks.as_slice(),
                    open_price_ticks: tick_summary.open_price_ticks.as_slice(),
                    retained_known_news: retained_known_news.retained_records.as_slice(),
                    session_open_tick_count: tick_summary.session_open_tick_count,
                    session_closed_tick_count: tick_summary.session_closed_tick_count,
                    open_session_buy_tick_count: tick_summary.open_session_buy_tick_count,
                    open_session_sell_tick_count: tick_summary.open_session_sell_tick_count,
                    directional_label_diagnostics: request.directional_label_diagnostics,
                },
            )?),
            Some(build_hawkes_event_side_codes(
                tick_summary.resolved_ticks.as_slice(),
            )?),
        )
    } else {
        (None, None)
    };

    let (
        return_components_candidate_generation_input,
        observation_models_stage_input,
        mean_reversion_open_session_price_observations,
        mean_reversion_observed_session_duration_hours,
        material_close_open_gap_count,
    ) = if request.require_return_components {
        let output = build_return_components_candidate_generation_input(
            ReturnComponentsCandidateGenerationInputBuildInput {
                definition,
                timezone,
                exchange_code: request.exchange_code.as_str(),
                open_price_ticks: tick_summary.open_price_ticks.as_slice(),
                open_signed_tick_count: tick_summary.open_signed_tick_count,
                open_session_sell_tick_count: tick_summary.open_session_sell_tick_count,
                reference_price: request.reference_price,
                retained_known_news: retained_known_news.retained_records.as_slice(),
                source_window: tick_summary.source_window.clone(),
                _option_surface_quote_values: request.option_surface_quote_values.as_slice(),
                calibration_settings: request
                    .calibration_settings
                    .as_ref()
                    .and_then(|settings| settings.return_components.as_ref()),
            },
        )?;

        let mean_reversion_grid_inputs = derive_mean_reversion_open_session_grid_inputs(
            tick_summary.open_price_ticks.as_slice(),
        );
        let observation_models_stage_input = CalibrationObservationModelsStageExecutionInput {
            open_session_timestamp_transitions:
                derive_observation_models_open_session_timestamp_transitions(
                    tick_summary.open_price_ticks.as_slice(),
                ),
        };

        (
            Some(output.0),
            Some(observation_models_stage_input),
            Some(mean_reversion_grid_inputs.0),
            Some(mean_reversion_grid_inputs.1),
            output.1,
        )
    } else {
        (None, None, None, None, 0)
    };

    Ok(MarketDataTranslationCalibrationOutput {
        seasonality_stage_input,
        hawkes_candidate_inputs,
        hawkes_event_side_codes,
        observation_models_stage_input,
        return_components_candidate_generation_input,
        mean_reversion_open_session_price_observations,
        mean_reversion_observed_session_duration_hours,
        session_open_tick_count: tick_summary.session_open_tick_count,
        session_closed_tick_count: tick_summary.session_closed_tick_count,
        known_news_event_count: retained_known_news.retained_records.len(),
        known_news_events_outside_source_window_count: retained_known_news
            .outside_source_window_count,
        known_news_open_event_count: retained_known_news.open_event_count,
        known_news_closed_event_count: retained_known_news.closed_event_count,
        material_close_open_gap_count,
    })
}
