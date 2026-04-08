use chrono::{DateTime, LocalResult, NaiveDateTime, TimeZone};
use chrono_tz::Tz;

use super::contracts::{
    MarketDataKnownNewsLoadingInput, MarketDataLoadingError, MarketDataNormalizationPolicy,
    MarketDataTickDataColumnMapping, MarketDataTickPriceInputPolicy,
};
use super::shared::{
    COMMON_TIMESTAMP_FORMATS, LoadedKnownNewsRecord, LoadedTickRecord, RawRow, RawValue, TradeSign,
};

pub(super) fn normalize_tick_row(
    raw_row: &RawRow,
    source_row_number: usize,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<LoadedTickRecord, MarketDataLoadingError> {
    normalize_tick_values(
        column_mapping
            .timestamp_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .price_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .bid_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .ask_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        column_mapping
            .trade_sign_column
            .as_ref()
            .and_then(|column_name| raw_row.get(column_name)),
        source_row_number,
        column_mapping,
        normalization_policy,
        source_path,
    )
}

pub(super) fn normalize_tick_values(
    timestamp_raw_value: Option<&RawValue>,
    explicit_price_raw_value: Option<&RawValue>,
    bid_price_raw_value: Option<&RawValue>,
    ask_price_raw_value: Option<&RawValue>,
    trade_sign_raw_value: Option<&RawValue>,
    source_row_number: usize,
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<LoadedTickRecord, MarketDataLoadingError> {
    let timestamp_column = column_mapping.timestamp_column.as_deref().ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: "column_mapping.tick_data.timestamp_column".to_string(),
            message: "could not resolve a tick timestamp column".to_string(),
        }
    })?;
    let timestamp = normalize_timestamp_value(
        timestamp_raw_value,
        &format!(
            "tick_data_source.rows[{source_row_number}].{}",
            timestamp_column
        ),
        Some(normalization_policy.timestamp_timezone.as_str()),
        source_path,
    )?;

    let explicit_price = match column_mapping.price_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            explicit_price_raw_value,
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let bid_price = match column_mapping.bid_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            bid_price_raw_value,
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let ask_price = match column_mapping.ask_column.as_ref() {
        Some(column_name) => normalize_optional_numeric_value(
            ask_price_raw_value,
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
            true,
            false,
        )?,
        None => None,
    };
    let trade_sign = match column_mapping.trade_sign_column.as_ref() {
        Some(column_name) => normalize_optional_trade_sign_value(
            trade_sign_raw_value,
            &format!("tick_data_source.rows[{source_row_number}].{column_name}"),
            source_path,
        )?,
        None => None,
    };

    let price = match normalization_policy.tick_price_input_policy {
        Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn) => explicit_price,
        Some(MarketDataTickPriceInputPolicy::BidAskMidpoint) => match (bid_price, ask_price) {
            (Some(bid), Some(ask)) if ask >= bid => Some((bid + ask) / 2.0),
            _ => return Err(MarketDataLoadingError::InvalidValue {
                field_path: format!("tick_data_source.rows[{source_row_number}]"),
                message:
                    "bid_column and ask_column must both contain values to derive bid_ask_midpoint"
                        .to_string(),
            }),
        },
        None => None,
    };

    Ok(LoadedTickRecord {
        timestamp_utc_nanos: timestamp,
        price,
        trade_price: explicit_price,
        trade_sign,
        bid_price,
        ask_price,
        close_price: explicit_price,
        source_row_number,
    })
}

pub(super) fn normalize_known_news_row(
    raw_row: &RawRow,
    source_row_number: usize,
    known_news_input: &MarketDataKnownNewsLoadingInput,
    source_path: &str,
) -> Result<LoadedKnownNewsRecord, MarketDataLoadingError> {
    let timestamp = normalize_timestamp_value(
        raw_row.get(&known_news_input.column_mapping.event_timestamp_column),
        &format!(
            "known_news_inputs.calendar_source.rows[{source_row_number}].{}",
            known_news_input.column_mapping.event_timestamp_column
        ),
        known_news_input.calendar_timezone.as_deref(),
        source_path,
    )?;

    let event_identifier = match known_news_input
        .column_mapping
        .event_identifier_column
        .as_ref()
    {
        Some(column_name) => normalize_optional_text_value(
            raw_row.get(column_name),
            &format!("known_news_inputs.calendar_source.rows[{source_row_number}].{column_name}"),
            source_path,
        )?,
        None => None,
    };

    Ok(LoadedKnownNewsRecord {
        event_timestamp_utc_nanos: timestamp,
        event_identifier,
        source_row_number,
    })
}

pub(super) fn normalize_timestamp_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_timezone: Option<&str>,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let raw_value = raw_value.ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("is required and cannot be null. source_path={source_path}"),
    })?;

    let parsed = match raw_value {
        RawValue::Null => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("is required and cannot be null. source_path={source_path}"),
            });
        }
        RawValue::Text(value) => {
            parse_timestamp_text(value, field_path, source_timezone, source_path)?
        }
        RawValue::Integer(value) => parse_unix_epoch_value(*value as f64),
        RawValue::Float(value) => parse_unix_epoch_value(*value),
        RawValue::Boolean => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!(
                    "must be a supported timestamp string, Unix epoch value, or a parquet timestamp column. source_path={source_path}"
                ),
            });
        }
    };

    Ok(parsed)
}

pub(super) fn parse_timestamp_text(
    raw_value: &str,
    field_path: &str,
    source_timezone: Option<&str>,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let stripped = raw_value.trim();
    if stripped.is_empty() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("is required and cannot be blank. source_path={source_path}"),
        });
    }

    if let Some(parsed_numeric) = try_parse_numeric_timestamp_text(stripped) {
        return Ok(parsed_numeric);
    }

    let normalized_value = stripped.replace(" UTC", "+00:00").replace(" GMT", "+00:00");

    let rfc3339_candidate = if normalized_value.ends_with('Z') {
        format!("{}+00:00", &normalized_value[..normalized_value.len() - 1])
    } else {
        normalized_value.clone()
    };

    if let Ok(parsed) = DateTime::parse_from_rfc3339(&rfc3339_candidate) {
        return Ok(parsed.timestamp_nanos_opt().unwrap_or(i64::MAX));
    }

    for format in COMMON_TIMESTAMP_FORMATS {
        if let Ok(parsed) = DateTime::parse_from_str(&normalized_value, format) {
            return Ok(parsed.timestamp_nanos_opt().unwrap_or(i64::MAX));
        }

        if let Ok(parsed) = NaiveDateTime::parse_from_str(&normalized_value, format) {
            let timezone = source_timezone.ok_or_else(|| MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!("contains a naive timestamp. Provide a timezone-aware value or configure the source timezone explicitly. source_path={source_path}"),
            })?;

            return localize_naive_datetime(parsed, timezone, field_path, source_path);
        }
    }

    Err(MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!(
            "must be a supported timestamp string, Unix epoch value, or a parquet timestamp column. source_path={source_path}"
        ),
    })
}

pub(super) fn localize_naive_datetime(
    parsed_value: NaiveDateTime,
    source_timezone: &str,
    field_path: &str,
    source_path: &str,
) -> Result<i64, MarketDataLoadingError> {
    let timezone = resolve_source_timezone(source_timezone, field_path, source_path)?;
    match timezone.from_local_datetime(&parsed_value) {
        LocalResult::Single(value) => {
            value
                .timestamp_nanos_opt()
                .ok_or_else(|| MarketDataLoadingError::InvalidValue {
                    field_path: field_path.to_string(),
                    message: format!(
                        "timestamp value is outside the supported range. source_path={source_path}"
                    ),
                })
        }
        LocalResult::Ambiguous(_, _) => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!(
                "is ambiguous in the configured source timezone due to a daylight-saving transition. Provide explicit UTC offsets in the file. source_path={source_path}"
            ),
        }),
        LocalResult::None => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!(
                "falls into a nonexistent local-time gap. Provide explicit UTC offsets or normalize the source file. source_path={source_path}"
            ),
        }),
    }
}

pub(super) fn resolve_source_timezone(
    source_timezone: &str,
    field_path: &str,
    source_path: &str,
) -> Result<Tz, MarketDataLoadingError> {
    let normalized = source_timezone.trim();
    if matches!(
        normalized.to_ascii_uppercase().as_str(),
        "UTC" | "Z" | "GMT" | "UT"
    ) {
        return Ok(chrono_tz::UTC);
    }

    normalized
        .parse::<Tz>()
        .map_err(|_| MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!(
                "references unknown timezone '{source_timezone}'. source_path={source_path}"
            ),
        })
}

pub(super) fn try_parse_numeric_timestamp_text(raw_value: &str) -> Option<i64> {
    let unsigned = raw_value.trim_start_matches(['+', '-']);
    if unsigned.chars().all(|character| character.is_ascii_digit()) {
        let digit_count = unsigned.len();
        if !matches!(digit_count, 9 | 10 | 12 | 13 | 15 | 16 | 18 | 19) {
            return None;
        }
    }

    if let Ok(parsed_integer) = raw_value.parse::<i64>() {
        return Some(parse_unix_epoch_value(parsed_integer as f64));
    }
    if let Ok(parsed_float) = raw_value.parse::<f64>() {
        return Some(parse_unix_epoch_value(parsed_float));
    }

    None
}

pub(super) fn parse_unix_epoch_value(raw_value: f64) -> i64 {
    let absolute = raw_value.abs();
    if absolute >= 100_000_000_000_000_000.0 {
        return ((raw_value / 1_000_000_000.0) * 1_000_000_000.0).round() as i64;
    }
    if absolute >= 100_000_000_000_000.0 {
        return ((raw_value / 1_000_000.0) * 1_000_000_000.0).round() as i64;
    }
    if absolute >= 100_000_000_000.0 {
        return ((raw_value / 1_000.0) * 1_000_000_000.0).round() as i64;
    }

    (raw_value * 1_000_000_000.0).round() as i64
}

pub(super) fn normalize_optional_numeric_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
    allow_zero: bool,
    allow_negative: bool,
) -> Result<Option<f64>, MarketDataLoadingError> {
    let Some(raw_value) = raw_value else {
        return Ok(None);
    };

    let numeric_value = match raw_value {
        RawValue::Null => return Ok(None),
        RawValue::Text(value) => {
            let stripped = value.trim();
            if stripped.is_empty() {
                return Ok(None);
            }

            stripped
                .parse::<f64>()
                .map_err(|_| MarketDataLoadingError::InvalidValue {
                    field_path: field_path.to_string(),
                    message: format!(
                        "must be a finite numeric value when provided. source_path={source_path}"
                    ),
                })?
        }
        RawValue::Integer(value) => *value as f64,
        RawValue::Float(value) => *value,
        RawValue::Boolean => {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: field_path.to_string(),
                message: format!(
                    "must be a finite numeric value when provided. source_path={source_path}"
                ),
            });
        }
    };

    if !numeric_value.is_finite() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!(
                "must be a finite numeric value when provided. source_path={source_path}"
            ),
        });
    }
    if !allow_negative && numeric_value < 0.0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be non-negative. source_path={source_path}"),
        });
    }
    if !allow_zero && numeric_value <= 0.0 {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be strictly positive. source_path={source_path}"),
        });
    }

    Ok(Some(numeric_value))
}

pub(super) fn normalize_optional_text_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<Option<String>, MarketDataLoadingError> {
    let Some(raw_value) = raw_value else {
        return Ok(None);
    };

    match raw_value {
        RawValue::Null => Ok(None),
        RawValue::Text(value) => {
            let stripped = value.trim();
            if stripped.is_empty() {
                Ok(None)
            } else {
                Ok(Some(stripped.to_string()))
            }
        }
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be a string when provided. source_path={source_path}"),
        }),
    }
}

pub(super) fn normalize_optional_trade_sign_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<Option<TradeSign>, MarketDataLoadingError> {
    let Some(text) = normalize_optional_text_value(raw_value, field_path, source_path)? else {
        return Ok(None);
    };

    match text.to_ascii_lowercase().as_str() {
        "buy" => Ok(Some(TradeSign::Buy)),
        "sell" => Ok(Some(TradeSign::Sell)),
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!(
                "must be either 'buy' or 'sell' when provided. source_path={source_path}"
            ),
        }),
    }
}

pub(super) fn normalize_required_numeric_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
    allow_zero: bool,
    allow_negative: bool,
) -> Result<f64, MarketDataLoadingError> {
    normalize_optional_numeric_value(
        raw_value,
        field_path,
        source_path,
        allow_zero,
        allow_negative,
    )?
    .ok_or_else(|| MarketDataLoadingError::InvalidValue {
        field_path: field_path.to_string(),
        message: format!("is required and cannot be blank. source_path={source_path}"),
    })
}

pub(super) fn normalize_required_text_value(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<String, MarketDataLoadingError> {
    normalize_optional_text_value(raw_value, field_path, source_path)?.ok_or_else(|| {
        MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("is required and cannot be blank. source_path={source_path}"),
        }
    })
}

pub(super) fn normalize_option_type(
    raw_value: Option<&RawValue>,
    field_path: &str,
    source_path: &str,
) -> Result<String, MarketDataLoadingError> {
    let text_value =
        normalize_required_text_value(raw_value, field_path, source_path)?.to_ascii_lowercase();
    match text_value.as_str() {
        "call" | "c" => Ok("call".to_string()),
        "put" | "p" => Ok("put".to_string()),
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: field_path.to_string(),
            message: format!("must be one of: call, put, c, p. source_path={source_path}"),
        }),
    }
}
