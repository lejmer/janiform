use std::collections::{BTreeMap, HashSet};

use crate::calendar::ExchangeCalendarRegistryLoader;
use crate::execution_bridge::CalibrationHawkesDirectionalLabelDiagnostics;
use crate::market_data::preprocessing::{
    MarketDataTranslationPreprocessingRequest, MarketDataTranslationTickRecord,
    preprocess_market_data_translation_request,
};

use super::contracts::{
    MarketDataDuplicateRecordPolicy, MarketDataInputFileReference, MarketDataLoadingError,
    MarketDataLoadingOutput, MarketDataLoadingRequest, MarketDataNormalizationPolicy,
    MarketDataNormalizedTickOutput, MarketDataSessionConfiguration,
    MarketDataSessionResolutionPayload, MarketDataSourceProbePayload, MarketDataSourceSummary,
    MarketDataTickDataColumnMapping, MarketDataTickPriceInputPolicy,
};
use super::known_news::load_known_news_records;
use super::normalization::normalize_tick_row;
use super::option_surface::{
    load_option_surface_records, materialize_option_surface_records_for_payload,
};
use super::rows::{
    load_rows, load_source_columns, probe_source_row_count_estimate, validate_columns_exist,
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
    let normalized_tick_output = normalize_market_data_source(
        &request.tick_data_source,
        &request.tick_data_column_mapping,
        &request.normalization_policy,
        request.require_hawkes,
    )?;

    let session_resolution = resolve_market_data_session_configuration(
        request.session_configuration.clone(),
        Some(&normalized_tick_output),
    )?;

    prepare_market_data_loading_from_phases(request, normalized_tick_output, session_resolution)
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

pub fn normalize_market_data_source(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
) -> Result<MarketDataNormalizedTickOutput, MarketDataLoadingError> {
    let tick_output = load_tick_records(
        file_reference,
        column_mapping,
        normalization_policy,
        require_hawkes,
    )?;

    Ok(MarketDataNormalizedTickOutput {
        tick_records: tick_output.tick_records,
        directional_label_diagnostics: tick_output.directional_label_diagnostics,
        tick_data_summary: tick_output.summary,
        reference_price: tick_output.reference_price,
        fit_warning: tick_output.fit_warning,
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
    normalized_tick_output: MarketDataNormalizedTickOutput,
    session_resolution: MarketDataSessionResolutionPayload,
) -> Result<MarketDataLoadingOutput, MarketDataLoadingError> {
    let calendar_registry = ExchangeCalendarRegistryLoader::load_from_json(
        session_resolution.resolved_session_definition_json.as_str(),
    )
    .map_err(|error| MarketDataLoadingError::InvalidValue {
        field_path: "session_resolution.resolved_session_definition_json".to_string(),
        message: format!("does not contain a valid calendar registry: {error}"),
    })?;

    let known_news_output = if let Some(known_news_input) = &request.known_news_input {
        Some(load_known_news_records(known_news_input)?)
    } else {
        None
    };

    let option_surface_output = if let Some(option_surface_input) = &request.option_surface_input {
        Some(load_option_surface_records(
            option_surface_input,
            &request.normalization_policy,
        )?)
    } else {
        None
    };

    let preprocessing_request = MarketDataTranslationPreprocessingRequest {
        exchange_code: request.session_configuration.exchange_code,
        trade_events_data_reference: request.trade_events_data_reference,
        tick_records: normalized_tick_output.tick_records.clone(),
        known_news_records: known_news_output
            .as_ref()
            .map(|output| output.known_news_records.clone())
            .unwrap_or_default(),
        option_surface_quote_values: option_surface_output
            .as_ref()
            .map(|output| output.option_surface_quote_values.clone())
            .unwrap_or_default(),
        directional_label_diagnostics: normalized_tick_output
            .directional_label_diagnostics
            .clone()
            .map(|diagnostics| CalibrationHawkesDirectionalLabelDiagnostics {
                policy_id: diagnostics.policy_id,
                first_inferred_trade_sign: diagnostics.first_inferred_trade_sign,
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

    let preprocessing_output =
        preprocess_market_data_translation_request(preprocessing_request, &calendar_registry)
            .map_err(MarketDataLoadingError::Preprocessing)?;

    Ok(MarketDataLoadingOutput {
        session_resolution,
        preprocessing_output,
        tick_data_summary: normalized_tick_output.tick_data_summary,
        option_surface_summary: option_surface_output
            .as_ref()
            .map(|output| output.summary.clone()),
        option_surface_records: option_surface_output.as_ref().map(|output| {
            materialize_option_surface_records_for_payload(output.option_surface_records.as_slice())
        }),
        known_news_summary: known_news_output.map(|output| output.summary),
        reference_price: normalized_tick_output.reference_price,
        fit_warning: normalized_tick_output.fit_warning,
    })
}

fn load_tick_records(
    file_reference: &MarketDataInputFileReference,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    require_hawkes: bool,
) -> Result<TickLoadOutput, MarketDataLoadingError> {
    let required_columns = required_tick_source_columns(column_mapping, normalization_policy);
    let available_columns = load_source_columns(file_reference, "tick_data_source")?;

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.tick_data",
        &file_reference.path,
    )?;

    let raw_rows = load_rows(file_reference, "tick_data_source")?;
    let source_row_count = raw_rows.len();
    let mut normalized_rows = Vec::with_capacity(source_row_count);

    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        normalized_rows.push(normalize_tick_row(
            raw_row,
            row_index + 1,
            column_mapping,
            normalization_policy,
            &file_reference.path,
        )?);
    }

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

    Ok(TickLoadOutput {
        tick_records,
        directional_label_diagnostics,
        summary: MarketDataSourceSummary {
            path: file_reference.path.clone(),
            file_format: file_format_string(file_reference.file_format).to_string(),
            provenance_label: file_reference.provenance_label.clone(),
            source_row_count,
            retained_row_count: source_row_count.saturating_sub(dropped_duplicate_row_count),
            dropped_duplicate_row_count,
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
    })
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

fn tick_record_dedup_key(
    row: &LoadedTickRecord,
) -> (
    i64,
    Option<u64>,
    Option<u64>,
    Option<TradeSign>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
) {
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
    let mut columns = vec![column_mapping.timestamp_column.clone()];

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
