use janiform::calibration::market_data::{
    MarketDataNormalizedOptionSurfaceRecord, MarketDataOptionSurfaceLoadingInput,
    MarketDataTranslationTickRecord, PriceCoordinateResolutionInput, ResolvedPriceCoordinateSet,
    normalize_market_data_source, normalize_option_surface_source, resolve_price_coordinates,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;
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
        "load observed data",
        ProgressMeasure::Unbounded,
        Some("loading observed ticks and option quotes".to_string()),
        ObservedWorkItemUpdate::none(),
    )?;

    let tick_normalization_policy = observed_tick_normalization_policy(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
    )?;

    let tick_output = normalize_market_data_source(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &tick_normalization_policy,
        true,
    )
    .map_err(|source| pricing_validation_error("tick_data_source", source.to_string()))?;

    let option_output = normalize_option_surface_source(
        &MarketDataOptionSurfaceLoadingInput {
            // Intentional copies because the calibration-runtime loader owns
            // its source and mapping contract for normalization.
            source: request.option_surface_source.clone(),
            column_mapping: request.option_surface_column_mapping.clone(),
        },
        &request.normalization_policy,
    )
    .map_err(|source| pricing_validation_error("option_surface_source", source.to_string()))?;

    emit_observed_stage_progress(
        progress_adapter,
        "load observed data",
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
