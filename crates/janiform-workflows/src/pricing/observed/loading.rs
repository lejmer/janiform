use janiform::calibration::market_data::{
    MarketDataLoadingProgressError, MarketDataNormalizedOptionSurfaceRecord,
    MarketDataOptionSurfaceLoadingInput, MarketDataTranslationTickRecord,
    PriceCoordinateResolutionInput, ResolvedPriceCoordinateSet, normalize_market_data_source,
    normalize_market_data_source_with_progress, normalize_option_surface_source,
    normalize_option_surface_source_with_progress, resolve_price_coordinates,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;
use crate::market_data_progress::{
    MarketDataLoadingProgressGate, market_data_loading_progress_measure,
    market_data_loading_progress_signal, should_emit_market_data_loading_progress,
};
use crate::progress::bounded_progress_measure;

use super::super::validation::pricing_validation_error;
use super::data::{prepare_snapshot_groups, resolve_effective_valuation_time_utc_nanos};
use super::normalization::observed_tick_normalization_policy;
use super::progress::{
    ObservedWorkItemUpdate, complete_observed_pricing_stage, emit_observed_stage_progress,
    start_observed_pricing_stage,
};
use super::types::{
    LOAD_OBSERVED_DATA_STAGE, ObservedOptionsPricingRequest, PreparedSnapshotGroup,
};

pub(super) struct LoadedObservedOptionsPricingInputs {
    pub(super) valuation_time_utc_nanos: i64,
    pub(super) snapshot_groups: Vec<PreparedSnapshotGroup>,
    pub(super) option_records: Vec<MarketDataNormalizedOptionSurfaceRecord>,
    pub(super) tick_records: Vec<MarketDataTranslationTickRecord>,
    pub(super) price_coordinate_resolution: Option<ResolvedPriceCoordinateSet>,
    pub(super) warnings: Vec<String>,
    pub(super) option_valuation_spot: Option<f64>,
    pub(super) option_valuation_tick_records: Option<Vec<MarketDataTranslationTickRecord>>,
}

pub(super) fn load_observed_pricing_inputs(
    request: &ObservedOptionsPricingRequest,
    stage_count: usize,
    progress_adapter: &mut Option<&mut ProgressAdapter>,
) -> Result<LoadedObservedOptionsPricingInputs, WorkflowError> {
    let load_stage_started_at =
        start_observed_pricing_stage(progress_adapter, 1, stage_count, LOAD_OBSERVED_DATA_STAGE)?;

    emit_observed_stage_progress(
        progress_adapter,
        LOAD_OBSERVED_DATA_STAGE,
        ProgressMeasure::Unbounded,
        Some("loading observed ticks and option quotes".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    let tick_normalization_policy = observed_tick_normalization_policy(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
    )?;

    let mut progress_gate = MarketDataLoadingProgressGate::default();

    let tick_output = if progress_adapter.is_some() {
        let mut loading_progress = |event| {
            emit_loading_progress_for_observed_stage(progress_adapter, &mut progress_gate, event)
        };

        normalize_market_data_source_with_progress(
            &request.tick_data_source,
            &request.tick_data_column_mapping,
            &tick_normalization_policy,
            true,
            &mut loading_progress,
        )
    } else {
        normalize_market_data_source(
            &request.tick_data_source,
            &request.tick_data_column_mapping,
            &tick_normalization_policy,
            true,
        )
    }
    .map_err(|source| pricing_validation_error("tick_data_source", source.to_string()))?;

    let option_surface_input = MarketDataOptionSurfaceLoadingInput {
        // Intentional copies because the calibration-runtime loader owns
        // its source and mapping contract for normalization.
        source: request.option_surface_source.clone(),
        column_mapping: request.option_surface_column_mapping.clone(),
    };

    let option_output = if progress_adapter.is_some() {
        let mut loading_progress = |event| {
            emit_loading_progress_for_observed_stage(progress_adapter, &mut progress_gate, event)
        };

        normalize_option_surface_source_with_progress(
            &option_surface_input,
            &request.normalization_policy,
            &mut loading_progress,
        )
    } else {
        normalize_option_surface_source(&option_surface_input, &request.normalization_policy)
    }
    .map_err(|source| pricing_validation_error("option_surface_source", source.to_string()))?;

    emit_observed_stage_progress(
        progress_adapter,
        LOAD_OBSERVED_DATA_STAGE,
        bounded_progress_measure(1, 1)?,
        Some(format!(
            "loaded {} ticks and {} option quotes",
            tick_output.tick_records.len(),
            option_output.option_surface_records.len()
        )),
        ObservedWorkItemUpdate::none(),
    )?;

    complete_observed_pricing_stage(
        progress_adapter,
        LOAD_OBSERVED_DATA_STAGE,
        load_stage_started_at,
    )?;

    let valuation_time_utc_nanos = resolve_effective_valuation_time_utc_nanos(
        request.valuation_time_utc,
        option_output.option_surface_records.as_slice(),
    )?;

    let snapshot_groups = prepare_snapshot_groups(
        option_output.option_surface_records.as_slice(),
        valuation_time_utc_nanos,
        request.option_type,
    )?;

    let price_coordinate_output = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &request.tick_data_source,
        dynamics_tick_data_column_mapping: &request.tick_data_column_mapping,
        normalization_policy: &tick_normalization_policy,
        option_surface_source: Some(&request.option_surface_source),
        option_surface_records: Some(option_output.option_surface_records.as_slice()),
        dynamics_tick_records: tick_output.tick_records.as_slice(),
        valuation_time_utc_nanos: Some(valuation_time_utc_nanos),
        policy: request.price_coordinate_policy,
        parity_warning_policy: &request.option_forward_parity_warning_policy,
        risk_free_rate: request.risk_free_rate,
        dividend_yield: request.dividend_yield,
    })
    .map_err(|source| {
        pricing_validation_error("price_coordinate_resolution", source.to_string())
    })?;

    Ok(LoadedObservedOptionsPricingInputs {
        valuation_time_utc_nanos,
        snapshot_groups,
        option_records: option_output.option_surface_records,
        tick_records: tick_output.tick_records,
        price_coordinate_resolution: price_coordinate_output.resolution,
        warnings: price_coordinate_output.warning.into_iter().collect(),
        option_valuation_spot: price_coordinate_output.option_valuation_spot,
        option_valuation_tick_records: price_coordinate_output.option_valuation_tick_records,
    })
}

fn emit_loading_progress_for_observed_stage(
    progress_adapter: &mut Option<&mut ProgressAdapter>,
    progress_gate: &mut MarketDataLoadingProgressGate,
    event: janiform::calibration::market_data::MarketDataLoadingProgressEvent,
) -> Result<(), MarketDataLoadingProgressError> {
    if !should_emit_market_data_loading_progress(progress_gate, &event) {
        return Ok(());
    }

    emit_observed_stage_progress(
        progress_adapter,
        LOAD_OBSERVED_DATA_STAGE,
        market_data_loading_progress_measure(&event)
            .map_err(|source| MarketDataLoadingProgressError::observer(source.to_string()))?,
        Some(market_data_loading_progress_signal(&event)),
        ObservedWorkItemUpdate::none(),
    )
    .map_err(|source| MarketDataLoadingProgressError::observer(source.to_string()))
}
