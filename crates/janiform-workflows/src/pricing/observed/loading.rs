use janiform::calibration::market_data::{
    MarketDataNormalizedOptionSurfaceRecord, MarketDataOptionSurfaceLoadingInput,
    MarketDataTranslationTickRecord, normalize_market_data_source, normalize_option_surface_source,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressMeasure};

use crate::error::WorkflowError;

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
        ProgressMeasure::bounded(1, 1)
            .map_err(|source| WorkflowError::OutputProgress { source })?,
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

    Ok(LoadedObservedOptionsPricingInputs {
        valuation_time_utc_nanos,
        snapshot_groups,
        option_records: option_output.option_surface_records,
        tick_records: tick_output.tick_records,
    })
}
