use std::collections::HashSet;

use crate::market_data::preprocessing::MarketDataTranslationKnownNewsRecord;

use super::contracts::{
    MarketDataKnownNewsLoadingInput, MarketDataLoadingError, MarketDataSourceSummary,
};
use super::normalization::normalize_known_news_row;
use super::rows::{load_rows, load_source_columns, validate_columns_exist};
use super::shared::{KnownNewsLoadOutput, file_format_string, required_known_news_source_columns};

pub(super) fn load_known_news_records(
    known_news_input: &MarketDataKnownNewsLoadingInput,
) -> Result<KnownNewsLoadOutput, MarketDataLoadingError> {
    let required_columns = required_known_news_source_columns(&known_news_input.column_mapping);
    let available_columns = load_source_columns(
        &known_news_input.calendar_source,
        "known_news_inputs.calendar_source",
    )?;

    validate_columns_exist(
        &available_columns,
        &required_columns,
        "column_mapping.known_news",
        &known_news_input.calendar_source.path,
    )?;

    let raw_rows = load_rows(
        &known_news_input.calendar_source,
        "known_news_inputs.calendar_source",
    )?;

    let source_row_count = raw_rows.len();

    let mut normalized_rows = Vec::with_capacity(source_row_count);
    let mut seen_keys: HashSet<(i64, Option<String>)> = HashSet::new();

    for (row_index, raw_row) in raw_rows.iter().enumerate() {
        let normalized = normalize_known_news_row(
            raw_row,
            row_index + 1,
            known_news_input,
            &known_news_input.calendar_source.path,
        )?;

        let key = (
            normalized.event_timestamp_utc_nanos,
            normalized.event_identifier.clone(),
        );

        if !seen_keys.insert(key) {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "known_news_inputs.calendar_source".to_string(),
                message: "contains duplicate normalized rows".to_string(),
            });
        }

        normalized_rows.push(normalized);
    }

    normalized_rows.sort_by_key(|row| (row.event_timestamp_utc_nanos, row.source_row_number));

    Ok(KnownNewsLoadOutput {
        known_news_records: normalized_rows
            .iter()
            .map(|row| MarketDataTranslationKnownNewsRecord {
                event_timestamp_utc_nanos: row.event_timestamp_utc_nanos,
                event_identifier: row.event_identifier.clone(),
            })
            .collect(),
        summary: MarketDataSourceSummary {
            path: known_news_input.calendar_source.path.clone(),
            file_format: file_format_string(known_news_input.calendar_source.file_format)
                .to_string(),
            provenance_label: known_news_input.calendar_source.provenance_label.clone(),
            source_row_count,
            retained_row_count: normalized_rows.len(),
            dropped_duplicate_row_count: 0,
            sort_records_by_timestamp: true,
            applied_timestamp_timezone: known_news_input.calendar_timezone.clone(),
            canonical_value_field: None,
            value_mapping_policy: None,
        },
    })
}
