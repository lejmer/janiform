use std::collections::{BTreeMap, HashSet};

use arrow_array::RecordBatch;

use crate::execution_bridge::CalibrationHawkesDirectionalLabelDiagnostics;
use crate::market_data::preprocessing::{
    MarketDataTranslationPreprocessingRequest, MarketDataTranslationTickRecord,
    preprocess_market_data_translation_request,
};
use janiform_market_time::calendar::ExchangeCalendarRegistryLoader;

use super::coarse_data::load_coarse_data_records;
use super::column_resolution::{
    resolve_option_surface_column_mapping, resolve_option_surface_quote_input_policy,
    resolve_tick_data_column_mapping, resolve_tick_price_input_policy,
};
use super::contracts::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileFormat, MarketDataInputFileReference,
    MarketDataLoadingError, MarketDataLoadingOutput, MarketDataLoadingRequest,
    MarketDataNormalizationPolicy, MarketDataNormalizedOptionSurfaceOutput,
    MarketDataNormalizedTickOutput, MarketDataOptionQuoteInputPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataOptionSurfaceLoadingInput,
    MarketDataSessionConfiguration, MarketDataSessionResolutionPayload,
    MarketDataSourceProbePayload, MarketDataSourceSummary, MarketDataTickDataColumnMapping,
    MarketDataTickPriceInputPolicy,
};
use super::known_news::load_known_news_records;
use super::normalization::{NormalizeTickValuesInput, normalize_tick_row, normalize_tick_values};
use super::option_surface::{
    load_option_surface_records, materialize_option_surface_records_for_payload,
};
use super::progress::{
    MarketDataLoadingProgressContext, MarketDataLoadingProgressEvent,
    MarketDataLoadingProgressObserver, MarketDataLoadingProgressPhase, MarketDataLoadingSource,
    market_data_loading_source_sequence, optional_market_data_loading_source_sequence,
};
use super::rows::{
    column_index, load_rows_with_progress, load_source_columns,
    open_projected_parquet_record_batch_reader, probe_source_row_count_estimate,
    raw_value_from_arrow_array, validate_columns_exist,
};
use super::session_resolution::{
    resolve_market_data_session_configuration_from_tick_records,
    try_resolve_market_data_session_configuration_without_tick_records,
};
use super::shared::{
    CalibrationDirectionalLabelDiagnosticsPayload, LoadedTickRecord, TickLoadOutput, TradeSign,
    dedup_vec, file_format_string, normalized_float_key,
};

pub fn load_and_preprocess_market_data_translation_request(
    request: MarketDataLoadingRequest,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::disabled();

    load_and_preprocess_market_data_translation_request_with_context(request, &mut progress)
}

/// Load and preprocess market data while reporting source loading progress.
pub fn load_and_preprocess_market_data_translation_request_with_progress(
    request: MarketDataLoadingRequest,
    progress_observer: MarketDataLoadingProgressObserver<'_>,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::observed(progress_observer);

    load_and_preprocess_market_data_translation_request_with_context(request, &mut progress)
}

fn load_and_preprocess_market_data_translation_request_with_context(
    request: MarketDataLoadingRequest,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    progress.set_source_sequence_if_empty(market_data_loading_source_sequence(&request));

    let normalized_tick_output = normalize_market_data_source_with_context(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
        request.require_hawkes,
        progress,
    )?;

    let session_resolution = resolve_market_data_session_configuration(
        request.session_configuration.clone(),
        Some(&normalized_tick_output),
    )?;

    prepare_market_data_loading_from_phases_with_context(
        request,
        &normalized_tick_output,
        session_resolution,
        progress,
    )
}

pub fn probe_market_data_source(
    source: &MarketDataInputFileReference,
) -> Result<MarketDataSourceProbePayload, MarketDataLoadingError> {
    Ok(MarketDataSourceProbePayload {
        path: source.path.clone(),
        file_format: file_format_string(source.file_format).to_string(),
        provenance_label: source.provenance_label.clone(),
        available_columns: load_source_columns(source, "tick_data_source")?,
        source_row_count_estimate: probe_source_row_count_estimate(source, "tick_data_source")?,
    })
}

pub fn infer_market_data_tick_price_input_policy(
    source: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
) -> Result<Option<MarketDataTickPriceInputPolicy>, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "tick_data_source")?;

    Ok(resolve_tick_price_input_policy(
        available_columns.as_slice(),
        column_mapping,
    ))
}

pub fn resolve_market_data_tick_data_column_mapping_for_source(
    source: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataTickDataColumnMapping, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "tick_data_source")?;

    resolve_tick_data_column_mapping(
        available_columns.as_slice(),
        column_mapping,
        normalization_policy,
        &source.path,
    )
}

pub fn infer_market_data_option_quote_input_policy(
    source: &MarketDataInputFileReference,
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataOptionQuoteInputPolicy, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "option_surface_input.source")?;

    resolve_option_surface_quote_input_policy(
        available_columns.as_slice(),
        column_mapping,
        normalization_policy,
        &source.path,
    )
}

pub fn resolve_market_data_option_surface_column_mapping_for_source(
    source: &MarketDataInputFileReference,
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataOptionSurfaceColumnMapping, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "option_surface_input.source")?;

    resolve_option_surface_column_mapping(
        available_columns.as_slice(),
        column_mapping,
        normalization_policy,
        &source.path,
    )
}

pub fn normalize_market_data_source(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
) -> Result<MarketDataNormalizedTickOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::disabled();

    normalize_market_data_source_with_context(
        file_reference,
        column_mapping,
        normalization_policy,
        require_hawkes,
        &mut progress,
    )
}

/// Normalize tick data while reporting source loading progress.
pub fn normalize_market_data_source_with_progress(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
    progress_observer: MarketDataLoadingProgressObserver<'_>,
) -> Result<MarketDataNormalizedTickOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::observed(progress_observer);

    normalize_market_data_source_with_context(
        file_reference,
        column_mapping,
        normalization_policy,
        require_hawkes,
        &mut progress,
    )
}

fn normalize_market_data_source_with_context(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<MarketDataNormalizedTickOutput, MarketDataLoadingError> {
    let tick_output = load_tick_records(
        file_reference,
        column_mapping,
        normalization_policy,
        require_hawkes,
        progress,
    )?;

    Ok(MarketDataNormalizedTickOutput {
        tick_records: tick_output.tick_records,
        directional_label_diagnostics: tick_output.directional_label_diagnostics,
        tick_data_summary: tick_output.summary,
        reference_price: tick_output.reference_price,
        fit_warning: tick_output.fit_warning,
    })
}

pub fn normalize_option_surface_source(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Result<MarketDataNormalizedOptionSurfaceOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::disabled();

    normalize_option_surface_source_with_context(
        option_surface_input,
        normalization_policy,
        &mut progress,
    )
}

/// Normalize option-surface data while reporting source loading progress.
pub fn normalize_option_surface_source_with_progress(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
    progress_observer: MarketDataLoadingProgressObserver<'_>,
) -> Result<MarketDataNormalizedOptionSurfaceOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::observed(progress_observer);

    normalize_option_surface_source_with_context(
        option_surface_input,
        normalization_policy,
        &mut progress,
    )
}

fn normalize_option_surface_source_with_context(
    option_surface_input: &MarketDataOptionSurfaceLoadingInput,
    normalization_policy: &MarketDataNormalizationPolicy,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<MarketDataNormalizedOptionSurfaceOutput, MarketDataLoadingError> {
    let option_surface_output =
        load_option_surface_records(option_surface_input, normalization_policy, progress)?;

    Ok(MarketDataNormalizedOptionSurfaceOutput {
        option_surface_records: materialize_option_surface_records_for_payload(
            option_surface_output.option_surface_records.as_slice(),
        ),
        option_surface_summary: option_surface_output.summary,
    })
}

pub fn resolve_market_data_session_configuration(
    session_configuration: MarketDataSessionConfiguration,
    normalized_tick_output: Option<&MarketDataNormalizedTickOutput>,
) -> Result<MarketDataSessionResolutionPayload, MarketDataLoadingError> {
    if let Some(resolved_without_tick_records) =
        try_resolve_market_data_session_configuration_without_tick_records(&session_configuration)?
    {
        return Ok(resolved_without_tick_records.session_resolution);
    }

    let normalized_tick_output =
        normalized_tick_output.ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "session_configuration".to_string(),
            message: "probe-fallback session resolution requires normalized tick records"
                .to_string(),
        })?;

    let resolved_session = resolve_market_data_session_configuration_from_tick_records(
        &session_configuration,
        normalized_tick_output.tick_records.as_slice(),
    )?;

    Ok(resolved_session.session_resolution)
}

pub fn prepare_market_data_loading_from_phases(
    request: MarketDataLoadingRequest,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
    session_resolution: MarketDataSessionResolutionPayload,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::disabled();

    prepare_market_data_loading_from_phases_with_context(
        request,
        normalized_tick_output,
        session_resolution,
        &mut progress,
    )
}

/// Complete market-data loading from pre-normalized tick data while reporting progress.
pub fn prepare_market_data_loading_from_phases_with_progress(
    request: MarketDataLoadingRequest,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
    session_resolution: MarketDataSessionResolutionPayload,
    progress_observer: MarketDataLoadingProgressObserver<'_>,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let mut progress = MarketDataLoadingProgressContext::observed(progress_observer);

    prepare_market_data_loading_from_phases_with_context(
        request,
        normalized_tick_output,
        session_resolution,
        &mut progress,
    )
}

fn prepare_market_data_loading_from_phases_with_context(
    request: MarketDataLoadingRequest,
    normalized_tick_output: &MarketDataNormalizedTickOutput,
    session_resolution: MarketDataSessionResolutionPayload,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    progress.set_source_sequence_if_empty(optional_market_data_loading_source_sequence(&request));

    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(
        session_resolution.resolved_session_definition_json.as_str(),
    )
    .map_err(|error| MarketDataLoadingError::InvalidValue {
        field_path: "session_resolution.resolved_session_definition_json".to_string(),
        message: format!("does not contain a valid calendar registry: {error}"),
    })?;

    let known_news_output = if let Some(known_news_input) = &request.known_news_input {
        Some(load_known_news_records(known_news_input, progress)?)
    } else {
        None
    };

    let coarse_data_output = if let Some(coarse_data_input) = &request.coarse_data_input {
        Some(load_coarse_data_records(
            coarse_data_input,
            &calendar_registry,
            &request.session_configuration.exchange_code,
            progress,
        )?)
    } else {
        None
    };

    let option_surface_output = if let Some(option_surface_input) = &request.option_surface_input {
        Some(load_option_surface_records(
            option_surface_input,
            &request.normalization_policy,
            progress,
        )?)
    } else {
        None
    };

    let (known_news_records, known_news_summary) = match known_news_output {
        Some(output) => (output.known_news_records, Some(output.summary)),
        None => (Vec::new(), None),
    };

    let (coarse_data_records, coarse_data_summary) = match coarse_data_output {
        Some(output) => (Some(output.records), Some(output.summary)),
        None => (None, None),
    };

    let (option_surface_quote_values, option_surface_summary, option_surface_records) =
        match option_surface_output {
            Some(output) => (
                output.option_surface_quote_values,
                Some(output.summary),
                Some(output.option_surface_records),
            ),
            None => (Vec::new(), None, None),
        };

    let preprocessing_request = MarketDataTranslationPreprocessingRequest {
        exchange_code: request.session_configuration.exchange_code,
        trade_events_data_reference: request.trade_events_data_reference,
        tick_records: normalized_tick_output.tick_records.clone(),
        reference_price: normalized_tick_output.reference_price,
        known_news_records,
        option_surface_quote_values,
        directional_label_diagnostics: normalized_tick_output
            .directional_label_diagnostics
            .as_ref()
            .map(|diagnostics| CalibrationHawkesDirectionalLabelDiagnostics {
                policy_id: diagnostics.policy_id.clone(),
                first_inferred_trade_sign: diagnostics.first_inferred_trade_sign.clone(),
                provided_label_count: diagnostics.provided_label_count,
                inferred_label_count: diagnostics.inferred_label_count,
                flat_move_carry_count: diagnostics.flat_move_carry_count,
                price_up_inference_count: diagnostics.price_up_inference_count,
                price_down_inference_count: diagnostics.price_down_inference_count,
            }),
        require_hawkes: request.require_hawkes,
        require_return_components: request.require_return_components,
        preprocessing_settings: request.preprocessing_settings,
    };

    if preprocessing_request.require_return_components {
        progress.emit(MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::InferringTimestampBatchingObservation,
            0,
            None,
        ))?;
    }

    let preprocessing_output =
        preprocess_market_data_translation_request(preprocessing_request, &calendar_registry)
            .map_err(MarketDataLoadingError::Preprocessing)?;

    Ok(MarketDataLoadingOutput {
        session_resolution,
        preprocessing_output,
        tick_data_summary: normalized_tick_output.tick_data_summary.clone(),
        coarse_data_summary,
        coarse_data_records,
        option_surface_summary,
        option_surface_records: option_surface_records
            .map(|records| materialize_option_surface_records_for_payload(records.as_slice())),
        known_news_summary,
        reference_price: normalized_tick_output.reference_price,
        fit_warning: normalized_tick_output.fit_warning.clone(),
    })
}

fn load_tick_records(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<TickLoadOutput, MarketDataLoadingError> {
    let available_columns = load_source_columns(file_reference, "tick_data_source")?;

    let resolved_column_mapping = resolve_tick_data_column_mapping(
        available_columns.as_slice(),
        column_mapping,
        normalization_policy,
        &file_reference.path,
    )?;

    let required_columns =
        required_tick_source_columns(&resolved_column_mapping, normalization_policy);

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.tick_data",
        &file_reference.path,
    )?;

    let (normalized_rows, source_row_count) = match file_reference.file_format {
        MarketDataInputFileFormat::Csv => load_csv_tick_rows(
            file_reference,
            &resolved_column_mapping,
            normalization_policy,
            progress,
        )?,
        MarketDataInputFileFormat::Parquet => load_parquet_tick_rows(
            file_reference,
            &resolved_column_mapping,
            normalization_policy,
            progress,
        )?,
        MarketDataInputFileFormat::Json => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "tick_data_source.file_format".to_string(),
                message: "must be one of: csv, parquet".to_string(),
            });
        }
    };

    finalize_tick_load_output(
        normalized_rows,
        source_row_count,
        file_reference,
        normalization_policy,
        require_hawkes,
        progress,
    )
}

fn load_csv_tick_rows(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<(Vec<LoadedTickRecord>, usize), MarketDataLoadingError> {
    let raw_rows = load_rows_with_progress(
        file_reference,
        "tick_data_source",
        progress,
        MarketDataLoadingSource::TickData,
    )?;

    let source_row_count = raw_rows.len();

    let mut normalized_rows = Vec::with_capacity(source_row_count);

    progress.emit(MarketDataLoadingProgressEvent::new(
        MarketDataLoadingSource::TickData,
        MarketDataLoadingProgressPhase::NormalizingRecords,
        0,
        Some(source_row_count),
    ))?;

    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        normalized_rows.push(normalize_tick_row(
            raw_row,
            row_index + 1,
            column_mapping,
            normalization_policy,
            &file_reference.path,
        )?);

        progress.emit(MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::NormalizingRecords,
            row_index + 1,
            Some(source_row_count),
        ))?;
    }

    Ok((normalized_rows, source_row_count))
}

fn load_parquet_tick_rows(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<(Vec<LoadedTickRecord>, usize), MarketDataLoadingError> {
    let projected_columns = required_tick_source_columns(column_mapping, normalization_policy);

    let source_row_count = probe_source_row_count_estimate(file_reference, "tick_data_source")?
        .ok_or_else(|| MarketDataLoadingError::Parquet {
            field_path: "tick_data_source.path".to_string(),
            message: "parquet source row-count metadata is unavailable".to_string(),
        })?;

    let reader = open_projected_parquet_record_batch_reader(
        file_reference,
        "tick_data_source",
        projected_columns.as_slice(),
    )?;

    let mut normalized_rows = Vec::with_capacity(source_row_count);
    let mut source_row_number: usize = 0;

    progress.emit(MarketDataLoadingProgressEvent::new(
        MarketDataLoadingSource::TickData,
        MarketDataLoadingProgressPhase::ReadingRecords,
        0,
        Some(source_row_count),
    ))?;

    for batch_result in reader {
        let batch = batch_result.map_err(|error| MarketDataLoadingError::Parquet {
            field_path: "tick_data_source.path".to_string(),
            message: error.to_string(),
        })?;

        let projected_columns =
            ProjectedTickColumns::new(&batch, column_mapping, &file_reference.path)?;

        for row_index in 0..batch.num_rows() {
            source_row_number = source_row_number.saturating_add(1);

            let timestamp_raw_value = projected_columns.required_value(
                &batch,
                projected_columns.timestamp_index,
                row_index,
                "column_mapping.tick_data.timestamp_column",
                &file_reference.path,
            )?;

            let explicit_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.explicit_price_index,
                row_index,
                "column_mapping.tick_data.price_column",
                &file_reference.path,
            )?;

            let bid_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.bid_price_index,
                row_index,
                "column_mapping.tick_data.bid_column",
                &file_reference.path,
            )?;

            let ask_price_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.ask_price_index,
                row_index,
                "column_mapping.tick_data.ask_column",
                &file_reference.path,
            )?;

            let trade_sign_raw_value = projected_columns.optional_value(
                &batch,
                projected_columns.trade_sign_index,
                row_index,
                "column_mapping.tick_data.trade_sign_column",
                &file_reference.path,
            )?;

            normalized_rows.push(normalize_tick_values(NormalizeTickValuesInput {
                timestamp_raw_value: Some(&timestamp_raw_value),
                explicit_price_raw_value: explicit_price_raw_value.as_ref(),
                bid_price_raw_value: bid_price_raw_value.as_ref(),
                ask_price_raw_value: ask_price_raw_value.as_ref(),
                trade_sign_raw_value: trade_sign_raw_value.as_ref(),
                source_row_number,
                column_mapping,
                normalization_policy,
                source_path: &file_reference.path,
            })?);
        }

        progress.emit(MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::ReadingRecords,
            source_row_number,
            Some(source_row_count),
        ))?;
    }

    Ok((normalized_rows, source_row_count))
}

fn finalize_tick_load_output(
    normalized_rows: Vec<LoadedTickRecord>,
    source_row_count: usize,
    file_reference: &MarketDataInputFileReference,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
    progress: &mut MarketDataLoadingProgressContext<'_>,
) -> Result<TickLoadOutput, MarketDataLoadingError> {
    let (mut deduplicated_rows, dropped_duplicate_row_count) = deduplicate_tick_rows(
        normalized_rows,
        normalization_policy.duplicate_record_policy,
    );

    if normalization_policy.sort_records_by_timestamp {
        deduplicated_rows.sort_by_key(|row| (row.timestamp_utc_nanos, row.source_row_number));
    }

    let fit_warning = detect_interval_data_warning(&deduplicated_rows);
    let reference_price = deduplicated_rows.last().and_then(|row| row.price);

    let (resolved_rows, directional_label_diagnostics) =
        resolve_trade_signs(deduplicated_rows, require_hawkes, &file_reference.path)?;

    let tick_records = resolved_rows
        .into_iter()
        .map(|row| MarketDataTranslationTickRecord {
            timestamp_utc_nanos: row.timestamp_utc_nanos,
            price: row.price,
            trade_sign: row.trade_sign.map(trade_sign_string),
            source_row_number: row.source_row_number,
        })
        .collect::<Vec<_>>();

    let output = TickLoadOutput {
        tick_records,
        directional_label_diagnostics,
        summary: MarketDataSourceSummary {
            path: file_reference.path.clone(),
            file_format: file_format_string(file_reference.file_format).to_string(),
            provenance_label: file_reference.provenance_label.clone(),
            source_row_count,
            retained_row_count: source_row_count.saturating_sub(dropped_duplicate_row_count),
            dropped_duplicate_row_count,
            dropped_invalid_row_count: 0,
            sort_records_by_timestamp: normalization_policy.sort_records_by_timestamp,
            applied_timestamp_timezone: Some(normalization_policy.timestamp_timezone.clone()),
            canonical_value_field: normalization_policy
                .tick_price_input_policy
                .map(|_| "price".to_string()),
            value_mapping_policy: normalization_policy
                .tick_price_input_policy
                .map(tick_price_policy_string),
        },
        reference_price,
        fit_warning,
    };

    progress.emit(MarketDataLoadingProgressEvent::new(
        MarketDataLoadingSource::TickData,
        MarketDataLoadingProgressPhase::Completed,
        source_row_count,
        Some(source_row_count),
    ))?;

    Ok(output)
}

struct ProjectedTickColumns {
    timestamp_index: usize,
    explicit_price_index: Option<usize>,
    bid_price_index: Option<usize>,
    ask_price_index: Option<usize>,
    trade_sign_index: Option<usize>,
}

impl ProjectedTickColumns {
    fn new(
        batch: &RecordBatch,
        column_mapping: &MarketDataTickDataColumnMapping,
        source_path: &str,
    ) -> Result<Self, MarketDataLoadingError> {
        let timestamp_column = column_mapping.timestamp_column.as_deref().ok_or_else(|| {
            MarketDataLoadingError::InvalidValue {
                field_path: "column_mapping.tick_data.timestamp_column".to_string(),
                message: "could not resolve a tick timestamp column".to_string(),
            }
        })?;

        Ok(Self {
            timestamp_index: column_index(
                batch,
                timestamp_column,
                "column_mapping.tick_data.timestamp_column",
                source_path,
            )?,
            explicit_price_index: column_mapping
                .price_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.tick_data.price_column",
                        source_path,
                    )
                })
                .transpose()?,
            bid_price_index: column_mapping
                .bid_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.tick_data.bid_column",
                        source_path,
                    )
                })
                .transpose()?,
            ask_price_index: column_mapping
                .ask_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.tick_data.ask_column",
                        source_path,
                    )
                })
                .transpose()?,
            trade_sign_index: column_mapping
                .trade_sign_column
                .as_ref()
                .map(|column_name| {
                    column_index(
                        batch,
                        column_name,
                        "column_mapping.tick_data.trade_sign_column",
                        source_path,
                    )
                })
                .transpose()?,
        })
    }

    fn required_value(
        &self,
        batch: &RecordBatch,
        column_index: usize,
        row_index: usize,
        field_path: &str,
        source_path: &str,
    ) -> Result<super::shared::RawValue, MarketDataLoadingError> {
        raw_value_from_arrow_array(
            batch.column(column_index).as_ref(),
            row_index,
            field_path,
            source_path,
        )
    }

    fn optional_value(
        &self,
        batch: &RecordBatch,
        column_index: Option<usize>,
        row_index: usize,
        field_path: &str,
        source_path: &str,
    ) -> Result<Option<super::shared::RawValue>, MarketDataLoadingError> {
        column_index
            .map(|index| {
                raw_value_from_arrow_array(
                    batch.column(index).as_ref(),
                    row_index,
                    field_path,
                    source_path,
                )
            })
            .transpose()
    }
}

fn deduplicate_tick_rows(
    rows: Vec<LoadedTickRecord>,
    duplicate_record_policy: Option<MarketDataDuplicateRecordPolicy>,
) -> (Vec<LoadedTickRecord>, usize) {
    let Some(policy) = duplicate_record_policy else {
        return (rows, 0);
    };

    match policy {
        MarketDataDuplicateRecordPolicy::KeepFirst => {
            let source_count = rows.len();

            let mut seen = HashSet::new();
            let mut retained = Vec::new();

            for row in rows {
                let key = tick_record_dedup_key(&row);
                if seen.insert(key) {
                    retained.push(row);
                }
            }

            let dropped_duplicate_row_count = source_count.saturating_sub(retained.len());

            (retained, dropped_duplicate_row_count)
        }
        MarketDataDuplicateRecordPolicy::KeepLast => {
            let source_count = rows.len();

            let mut last_index_by_key = BTreeMap::new();

            for (index, row) in rows.iter().enumerate() {
                last_index_by_key.insert(tick_record_dedup_key(row), index);
            }

            let retained = rows
                .into_iter()
                .enumerate()
                .filter_map(|(index, row)| {
                    let key = tick_record_dedup_key(&row);
                    (last_index_by_key.get(&key).copied() == Some(index)).then_some(row)
                })
                .collect::<Vec<_>>();

            let dropped_duplicate_row_count = source_count.saturating_sub(retained.len());

            (retained, dropped_duplicate_row_count)
        }
    }
}

type TickRecordDedupKey = (
    i64,
    Option<u64>,
    Option<u64>,
    Option<TradeSign>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
);

fn tick_record_dedup_key(row: &LoadedTickRecord) -> TickRecordDedupKey {
    (
        row.timestamp_utc_nanos,
        normalized_float_key(row.price),
        normalized_float_key(row.trade_price),
        row.trade_sign,
        normalized_float_key(row.bid_price),
        normalized_float_key(row.ask_price),
        normalized_float_key(row.close_price),
    )
}

fn resolve_trade_signs(
    rows: Vec<LoadedTickRecord>,
    require_hawkes: bool,
    source_path: &str,
) -> Result<
    (
        Vec<LoadedTickRecord>,
        Option<CalibrationDirectionalLabelDiagnosticsPayload>,
    ),
    MarketDataLoadingError,
> {
    if !require_hawkes {
        return Ok((rows, None));
    }

    if rows.is_empty() {
        return Ok((
            rows,
            Some(CalibrationDirectionalLabelDiagnosticsPayload {
                policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
                first_inferred_trade_sign: "buy".to_string(),
                provided_label_count: 0,
                inferred_label_count: 0,
                flat_move_carry_count: 0,
                price_up_inference_count: 0,
                price_down_inference_count: 0,
            }),
        ));
    }

    let mut previous_price: Option<f64> = None;
    let mut previous_sign = TradeSign::Buy;
    let mut provided_label_count = 0;
    let mut inferred_label_count = 0;
    let mut flat_move_carry_count = 0;
    let mut price_up_inference_count = 0;
    let mut price_down_inference_count = 0;

    let mut resolved_rows = Vec::with_capacity(rows.len());

    for mut row in rows {
        let resolved_sign = match row.trade_sign {
            Some(sign) => {
                provided_label_count += 1;
                previous_sign = sign;

                Some(sign)
            }
            None => {
                inferred_label_count += 1;

                let price = row.price.ok_or_else(|| MarketDataLoadingError::InvalidValue {
                    field_path: "column_mapping.tick_data.trade_sign_column".to_string(),
                    message: format!("trade sign inference requires a canonical tick price for every retained trade event. source_path={source_path}"),
                })?;

                let inferred_sign = match previous_price {
                    None => TradeSign::Buy,
                    Some(previous) if price > previous => {
                        price_up_inference_count += 1;
                        TradeSign::Buy
                    }
                    Some(previous) if price < previous => {
                        price_down_inference_count += 1;
                        TradeSign::Sell
                    }
                    Some(_) => {
                        flat_move_carry_count += 1;
                        previous_sign
                    }
                };

                previous_sign = inferred_sign;

                Some(inferred_sign)
            }
        };

        previous_price = row.price.or(previous_price);
        row.trade_sign = resolved_sign;

        resolved_rows.push(row);
    }

    Ok((
        resolved_rows,
        Some(CalibrationDirectionalLabelDiagnosticsPayload {
            policy_id: "tick_price_change_with_flat_move_carryover.v1".to_string(),
            first_inferred_trade_sign: "buy".to_string(),
            provided_label_count,
            inferred_label_count,
            flat_move_carry_count,
            price_up_inference_count,
            price_down_inference_count,
        }),
    ))
}

fn detect_interval_data_warning(rows: &[LoadedTickRecord]) -> Option<String> {
    if rows.len() < 4 {
        return None;
    }

    let mut deltas_in_seconds = Vec::new();

    for pair in rows.windows(2) {
        let delta_seconds =
            (pair[1].timestamp_utc_nanos - pair[0].timestamp_utc_nanos) / 1_000_000_000;
        if delta_seconds > 0 {
            deltas_in_seconds.push(delta_seconds);
        }
    }

    if deltas_in_seconds.len() < 3 {
        return None;
    }

    let aligned_to_minute_count = deltas_in_seconds
        .iter()
        .filter(|delta| **delta % 60 == 0)
        .count();

    let mut counts = BTreeMap::new();

    for delta in &deltas_in_seconds {
        *counts.entry(*delta).or_insert(0_usize) += 1;
    }

    let (dominant_delta_seconds, dominant_count) =
        counts.into_iter().max_by_key(|(_, count)| *count)?;

    let dominant_share = dominant_count as f64 / deltas_in_seconds.len() as f64;

    if dominant_delta_seconds < 60 {
        return None;
    }

    if aligned_to_minute_count as f64 / (deltas_in_seconds.len() as f64) < 0.8 {
        return None;
    }

    if dominant_share < 0.6 {
        return None;
    }

    Some(format!(
        "The supplied fit input appears to be interval data rather than tick data. Janiform detected a dominant spacing of {}, so fitting the model on this data will likely produce an invalid fit.",
        format_interval_warning_delta(dominant_delta_seconds)
    ))
}

fn format_interval_warning_delta(delta_seconds: i64) -> String {
    if delta_seconds % 86_400 == 0 {
        return format!("{}D", delta_seconds / 86_400);
    }

    if delta_seconds % 3_600 == 0 {
        return format!("{}h", delta_seconds / 3_600);
    }

    if delta_seconds % 60 == 0 {
        return format!("{}min", delta_seconds / 60);
    }

    format!("{delta_seconds}s")
}

fn required_tick_source_columns(
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
) -> Vec<String> {
    let mut columns = Vec::new();

    if let Some(column) = &column_mapping.timestamp_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.price_column {
        columns.push(column.clone());
    }

    if let Some(column) = &column_mapping.trade_sign_column {
        columns.push(column.clone());
    }

    if matches!(
        normalization_policy.tick_price_input_policy,
        Some(MarketDataTickPriceInputPolicy::BidAskMidpoint)
    ) {
        if let Some(column) = &column_mapping.bid_column {
            columns.push(column.clone());
        }
        if let Some(column) = &column_mapping.ask_column {
            columns.push(column.clone());
        }
    }

    dedup_vec(columns)
}

fn trade_sign_string(trade_sign: TradeSign) -> String {
    match trade_sign {
        TradeSign::Buy => "buy".to_string(),
        TradeSign::Sell => "sell".to_string(),
    }
}

fn tick_price_policy_string(policy: MarketDataTickPriceInputPolicy) -> String {
    match policy {
        MarketDataTickPriceInputPolicy::ExplicitPriceColumn => "explicit_price_column".to_string(),
        MarketDataTickPriceInputPolicy::BidAskMidpoint => "bid_ask_midpoint".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    use arrow_array::{
        ArrayRef, Decimal128Array, Float64Array, StringArray, TimestampNanosecondArray,
    };
    use arrow_schema::{DataType, Field, Schema, TimeUnit};

    use super::{
        MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataLoadingProgressEvent,
        MarketDataLoadingProgressPhase, MarketDataLoadingSource, MarketDataNormalizationPolicy,
        MarketDataNormalizedTickOutput, MarketDataTickDataColumnMapping,
        MarketDataTickPriceInputPolicy, normalize_market_data_source,
        normalize_market_data_source_with_progress,
    };
    use crate::market_data::loading::test_support::{unique_test_path, write_parquet_record_batch};

    fn normalize_projected_parquet_tick_source(
        parquet_path: &Path,
    ) -> MarketDataNormalizedTickOutput {
        normalize_market_data_source(
            &MarketDataInputFileReference {
                path: parquet_path.to_string_lossy().into_owned(),
                file_format: MarketDataInputFileFormat::Parquet,
                provenance_label: "ticks".to_string(),
            },
            &MarketDataTickDataColumnMapping {
                timestamp_column: Some("event_time".to_string()),
                price_column: Some("trade_price".to_string()),
                trade_sign_column: Some("trade_side".to_string()),
                bid_column: None,
                ask_column: None,
            },
            &MarketDataNormalizationPolicy {
                timestamp_timezone: "UTC".to_string(),
                sort_records_by_timestamp: true,
                duplicate_record_policy: None,
                tick_price_input_policy: Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn),
                option_quote_input_policy: None,
            },
            false,
        )
        .expect("projected parquet tick normalization should succeed")
    }

    #[test]
    fn normalize_market_data_source_reads_projected_parquet_tick_columns() {
        let parquet_path = unique_test_path("tick-source.parquet");

        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "event_time",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                false,
            ),
            Field::new("trade_price", DataType::Float64, false),
            Field::new("trade_side", DataType::Utf8, false),
            Field::new("ignored_label", DataType::Utf8, false),
        ]));

        write_parquet_record_batch(
            parquet_path.as_path(),
            schema,
            vec![
                Arc::new(TimestampNanosecondArray::from(vec![
                    1_711_957_600_000_000_000_i64,
                    1_711_957_601_000_000_000_i64,
                ])) as ArrayRef,
                Arc::new(Float64Array::from(vec![10.0_f64, 11.0_f64])) as ArrayRef,
                Arc::new(StringArray::from(vec!["buy", "sell"])) as ArrayRef,
                Arc::new(StringArray::from(vec!["first", "second"])) as ArrayRef,
            ],
        );

        let normalized = normalize_projected_parquet_tick_source(parquet_path.as_path());

        assert_eq!(normalized.tick_records.len(), 2);
        assert_eq!(normalized.tick_records[0].price, Some(10.0));
        assert_eq!(
            normalized.tick_records[1].trade_sign.as_deref(),
            Some("sell")
        );
        assert_eq!(normalized.tick_data_summary.source_row_count, 2);
        assert_eq!(normalized.tick_data_summary.retained_row_count, 2);
        assert_eq!(normalized.reference_price, Some(11.0));
    }

    #[test]
    fn normalize_market_data_source_reads_projected_decimal_parquet_prices() {
        let parquet_path = unique_test_path("decimal-tick-source.parquet");

        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "event_time",
                DataType::Timestamp(TimeUnit::Nanosecond, None),
                false,
            ),
            Field::new("trade_price", DataType::Decimal128(6, 5), false),
            Field::new("trade_side", DataType::Utf8, false),
        ]));

        write_parquet_record_batch(
            parquet_path.as_path(),
            schema,
            vec![
                Arc::new(TimestampNanosecondArray::from(vec![
                    1_711_957_600_000_000_000_i64,
                    1_711_957_601_000_000_000_i64,
                ])) as ArrayRef,
                Arc::new(
                    Decimal128Array::from_iter_values([109_125_i128, 109_250_i128])
                        .with_precision_and_scale(6, 5)
                        .expect("test decimal scale must match schema"),
                ) as ArrayRef,
                Arc::new(StringArray::from(vec!["buy", "sell"])) as ArrayRef,
            ],
        );

        let normalized = normalize_projected_parquet_tick_source(parquet_path.as_path());

        assert_eq!(normalized.tick_records.len(), 2);
        assert_eq!(normalized.tick_records[0].price, Some(1.09125));
        assert_eq!(normalized.tick_records[1].price, Some(1.0925));
        assert_eq!(normalized.reference_price, Some(1.0925));
    }

    #[test]
    fn normalize_market_data_source_with_progress_emits_tick_loading_events() {
        let csv_path = unique_test_path("tick-source.csv");

        fs::write(
            csv_path.as_path(),
            "event_time,trade_price\n2024-04-01T12:00:00Z,10.0\n2024-04-01T12:00:01Z,11.0\n",
        )
        .expect("test CSV source should be writable");

        let mut observed_events = Vec::<MarketDataLoadingProgressEvent>::new();
        let mut observer = |event| {
            observed_events.push(event);
            Ok(())
        };

        normalize_market_data_source_with_progress(
            &MarketDataInputFileReference {
                path: csv_path.to_string_lossy().into_owned(),
                file_format: MarketDataInputFileFormat::Csv,
                provenance_label: "ticks".to_string(),
            },
            &MarketDataTickDataColumnMapping {
                timestamp_column: Some("event_time".to_string()),
                price_column: Some("trade_price".to_string()),
                trade_sign_column: None,
                bid_column: None,
                ask_column: None,
            },
            &MarketDataNormalizationPolicy {
                timestamp_timezone: "UTC".to_string(),
                sort_records_by_timestamp: true,
                duplicate_record_policy: None,
                tick_price_input_policy: Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn),
                option_quote_input_policy: None,
            },
            false,
            &mut observer,
        )
        .expect("CSV tick normalization should succeed");

        fs::remove_file(csv_path).expect("test CSV source should be removable");

        assert!(
            observed_events.contains(&MarketDataLoadingProgressEvent::new(
                MarketDataLoadingSource::TickData,
                MarketDataLoadingProgressPhase::ReadingRecords,
                2,
                None,
            ))
        );
        assert!(
            observed_events.contains(&MarketDataLoadingProgressEvent::new(
                MarketDataLoadingSource::TickData,
                MarketDataLoadingProgressPhase::NormalizingRecords,
                2,
                Some(2),
            ))
        );
        assert_eq!(
            observed_events.last(),
            Some(&MarketDataLoadingProgressEvent::new(
                MarketDataLoadingSource::TickData,
                MarketDataLoadingProgressPhase::Completed,
                2,
                Some(2),
            ))
        );
    }
}
