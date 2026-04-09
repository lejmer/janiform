use std::collections::HashMap;

use super::contracts::{
    MarketDataLoadingError, MarketDataNormalizationPolicy, MarketDataOptionQuoteInputPolicy,
    MarketDataOptionSurfaceColumnMapping, MarketDataTickDataColumnMapping,
    MarketDataTickPriceInputPolicy,
};

const TICK_TIMESTAMP_ALIASES: [&str; 16] = [
    "timestamp",
    "timestamp_utc",
    "event_timestamp_utc",
    "event_timestamp",
    "time",
    "timestamp_microseconds",
    "time_microseconds",
    "ts_microseconds",
    "ts_utc",
    "ts",
    "trade_timestamp",
    "trade_time",
    "exchange_timestamp",
    "datetime",
    "quote_timestamp",
    "ts_recv",
];
const TICK_PRICE_ALIASES: [&str; 18] = [
    "price",
    "midprice",
    "mid_price",
    "midpoint",
    "mid_point",
    "mark_price",
    "mark",
    "trade_price",
    "tradeprice",
    "last_price",
    "last_trade_price",
    "traded_price",
    "close",
    "close_price",
    "closeprice",
    "last_close",
    "px",
    "value",
];
const TICK_TRADE_SIGN_ALIASES: [&str; 10] = [
    "trade_sign",
    "tradesign",
    "trade_direction",
    "tradedirection",
    "trade_dir",
    "tradedir",
    "side",
    "trade_side",
    "aggressor_side",
    "direction",
];
const TICK_BID_ALIASES: [&str; 6] = [
    "bid",
    "bid_price",
    "bidprice",
    "best_bid",
    "bid_px",
    "bestbid",
];
const TICK_ASK_ALIASES: [&str; 6] = [
    "ask",
    "ask_price",
    "askprice",
    "best_ask",
    "ask_px",
    "bestask",
];

const OPTION_QUOTE_TIMESTAMP_ALIASES: [&str; 10] = [
    "quote_timestamp",
    "quote_timestamp_utc",
    "timestamp",
    "timestamp_utc",
    "quote_time",
    "quote_datetime",
    "event_timestamp_utc",
    "event_timestamp",
    "ts",
    "ts_recv",
];
const OPTION_EXPIRY_TIMESTAMP_ALIASES: [&str; 10] = [
    "expiry_timestamp",
    "expiry_timestamp_utc",
    "expiry_utc",
    "expiration_timestamp",
    "expiration_timestamp_utc",
    "expiration_utc",
    "maturity_timestamp",
    "maturity_utc",
    "expiry",
    "expiration",
];
const OPTION_STRIKE_ALIASES: [&str; 6] = [
    "strike",
    "strike_price",
    "strikeprice",
    "exercise_price",
    "exerciseprice",
    "k",
];
const OPTION_TYPE_ALIASES: [&str; 7] = [
    "option_type",
    "option_kind",
    "payoff_type",
    "right",
    "call_put",
    "put_call",
    "cp",
];
const OPTION_MID_PRICE_ALIASES: [&str; 6] = [
    "mid_price",
    "midprice",
    "mid",
    "midpoint",
    "mid_point",
    "mark_price",
];
const OPTION_BID_ALIASES: [&str; 6] = [
    "bid",
    "bid_price",
    "bidprice",
    "best_bid",
    "bid_px",
    "bestbid",
];
const OPTION_ASK_ALIASES: [&str; 6] = [
    "ask",
    "ask_price",
    "askprice",
    "best_ask",
    "ask_px",
    "bestask",
];
const OPTION_IMPLIED_VOLATILITY_ALIASES: [&str; 7] = [
    "implied_volatility",
    "impliedvolatility",
    "iv",
    "ivol",
    "mark_iv",
    "mid_iv",
    "volatility",
];

pub(super) fn resolve_tick_data_column_mapping(
    available_columns: &[String],
    column_mapping: &MarketDataTickDataColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<MarketDataTickDataColumnMapping, MarketDataLoadingError> {
    let available_by_normalized = normalized_source_columns(available_columns);
    let timestamp_column = Some(resolve_required_column(
        column_mapping.timestamp_column.as_deref(),
        &available_by_normalized,
        TICK_TIMESTAMP_ALIASES.as_slice(),
        "column_mapping.tick_data.timestamp_column",
        source_path,
    )?);
    let trade_sign_column = resolve_optional_column(
        column_mapping.trade_sign_column.as_deref(),
        &available_by_normalized,
        TICK_TRADE_SIGN_ALIASES.as_slice(),
    );

    let (price_column, bid_column, ask_column) = match normalization_policy.tick_price_input_policy
    {
        Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn) => (
            Some(resolve_required_column(
                column_mapping.price_column.as_deref(),
                &available_by_normalized,
                TICK_PRICE_ALIASES.as_slice(),
                "column_mapping.tick_data.price_column",
                source_path,
            )?),
            None,
            None,
        ),
        Some(MarketDataTickPriceInputPolicy::BidAskMidpoint) => (
            None,
            Some(resolve_required_column(
                column_mapping.bid_column.as_deref(),
                &available_by_normalized,
                TICK_BID_ALIASES.as_slice(),
                "column_mapping.tick_data.bid_column",
                source_path,
            )?),
            Some(resolve_required_column(
                column_mapping.ask_column.as_deref(),
                &available_by_normalized,
                TICK_ASK_ALIASES.as_slice(),
                "column_mapping.tick_data.ask_column",
                source_path,
            )?),
        ),
        None => (None, None, None),
    };

    Ok(MarketDataTickDataColumnMapping {
        timestamp_column,
        price_column,
        trade_sign_column,
        bid_column,
        ask_column,
    })
}

pub(super) fn resolve_option_surface_column_mapping(
    available_columns: &[String],
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<MarketDataOptionSurfaceColumnMapping, MarketDataLoadingError> {
    let available_by_normalized = normalized_source_columns(available_columns);
    let quote_timestamp_column = Some(resolve_required_column(
        column_mapping.quote_timestamp_column.as_deref(),
        &available_by_normalized,
        OPTION_QUOTE_TIMESTAMP_ALIASES.as_slice(),
        "column_mapping.option_surface.quote_timestamp_column",
        source_path,
    )?);
    let expiry_timestamp_column = Some(resolve_required_column(
        column_mapping.expiry_timestamp_column.as_deref(),
        &available_by_normalized,
        OPTION_EXPIRY_TIMESTAMP_ALIASES.as_slice(),
        "column_mapping.option_surface.expiry_timestamp_column",
        source_path,
    )?);
    let strike_column = Some(resolve_required_column(
        column_mapping.strike_column.as_deref(),
        &available_by_normalized,
        OPTION_STRIKE_ALIASES.as_slice(),
        "column_mapping.option_surface.strike_column",
        source_path,
    )?);
    let option_type_column = Some(resolve_required_column(
        column_mapping.option_type_column.as_deref(),
        &available_by_normalized,
        OPTION_TYPE_ALIASES.as_slice(),
        "column_mapping.option_surface.option_type_column",
        source_path,
    )?);

    let (mid_price_column, bid_column, ask_column, implied_volatility_column) =
        match normalization_policy.option_quote_input_policy {
            Some(MarketDataOptionQuoteInputPolicy::MidPriceColumn) => (
                Some(resolve_required_column(
                    column_mapping.mid_price_column.as_deref(),
                    &available_by_normalized,
                    OPTION_MID_PRICE_ALIASES.as_slice(),
                    "column_mapping.option_surface.mid_price_column",
                    source_path,
                )?),
                Some(resolve_required_column(
                    column_mapping.bid_column.as_deref(),
                    &available_by_normalized,
                    OPTION_BID_ALIASES.as_slice(),
                    "column_mapping.option_surface.bid_column",
                    source_path,
                )?),
                Some(resolve_required_column(
                    column_mapping.ask_column.as_deref(),
                    &available_by_normalized,
                    OPTION_ASK_ALIASES.as_slice(),
                    "column_mapping.option_surface.ask_column",
                    source_path,
                )?),
                None,
            ),
            Some(MarketDataOptionQuoteInputPolicy::BidAskMidpoint) => (
                None,
                Some(resolve_required_column(
                    column_mapping.bid_column.as_deref(),
                    &available_by_normalized,
                    OPTION_BID_ALIASES.as_slice(),
                    "column_mapping.option_surface.bid_column",
                    source_path,
                )?),
                Some(resolve_required_column(
                    column_mapping.ask_column.as_deref(),
                    &available_by_normalized,
                    OPTION_ASK_ALIASES.as_slice(),
                    "column_mapping.option_surface.ask_column",
                    source_path,
                )?),
                None,
            ),
            Some(MarketDataOptionQuoteInputPolicy::ImpliedVolatilityColumn) => (
                None,
                None,
                None,
                Some(resolve_required_column(
                    column_mapping.implied_volatility_column.as_deref(),
                    &available_by_normalized,
                    OPTION_IMPLIED_VOLATILITY_ALIASES.as_slice(),
                    "column_mapping.option_surface.implied_volatility_column",
                    source_path,
                )?),
            ),
            None => (None, None, None, None),
        };

    Ok(MarketDataOptionSurfaceColumnMapping {
        quote_timestamp_column,
        expiry_timestamp_column,
        strike_column,
        option_type_column,
        mid_price_column,
        bid_column,
        ask_column,
        implied_volatility_column,
    })
}

pub(super) fn resolve_option_surface_quote_input_policy(
    available_columns: &[String],
    column_mapping: &MarketDataOptionSurfaceColumnMapping,
    normalization_policy: &MarketDataNormalizationPolicy,
    source_path: &str,
) -> Result<MarketDataOptionQuoteInputPolicy, MarketDataLoadingError> {
    if let Some(policy) = normalization_policy.option_quote_input_policy {
        return Ok(policy);
    }

    let available_by_normalized = normalized_source_columns(available_columns);
    let bid_column = resolve_optional_column(
        column_mapping.bid_column.as_deref(),
        &available_by_normalized,
        OPTION_BID_ALIASES.as_slice(),
    );
    let ask_column = resolve_optional_column(
        column_mapping.ask_column.as_deref(),
        &available_by_normalized,
        OPTION_ASK_ALIASES.as_slice(),
    );

    if bid_column.is_some() && ask_column.is_some() {
        return Ok(MarketDataOptionQuoteInputPolicy::BidAskMidpoint);
    }

    let mid_price_column = resolve_optional_column(
        column_mapping.mid_price_column.as_deref(),
        &available_by_normalized,
        OPTION_MID_PRICE_ALIASES.as_slice(),
    );
    if mid_price_column.is_some() {
        return Err(MarketDataLoadingError::MissingColumn {
            field_path: "normalization_policy.option_quote_input_policy".to_string(),
            message: format!(
                "could not infer an option quote-input policy for `{source_path}`. v1 joint option calibration requires bid and ask quotes when mid-price columns are present. Provide bid and ask columns or set explicit column overrides."
            ),
        });
    }

    let implied_volatility_column = resolve_optional_column(
        column_mapping.implied_volatility_column.as_deref(),
        &available_by_normalized,
        OPTION_IMPLIED_VOLATILITY_ALIASES.as_slice(),
    );
    if implied_volatility_column.is_some() {
        return Err(MarketDataLoadingError::InvalidValue {
            field_path: "normalization_policy.option_quote_input_policy".to_string(),
            message: "implied_volatility_column is not a valid joint option calibration target in v1; price-space targets require bid and ask quotes".to_string(),
        });
    }

    Err(MarketDataLoadingError::MissingColumn {
        field_path: "normalization_policy.option_quote_input_policy".to_string(),
        message: format!(
            "could not infer an option quote-input policy for `{source_path}`. Common bid and ask aliases were not found. Set explicit option quote columns or an explicit option quote-input policy."
        ),
    })
}

fn resolve_required_column(
    explicit_column: Option<&str>,
    available_by_normalized: &HashMap<String, String>,
    aliases: &[&str],
    field_path: &str,
    source_path: &str,
) -> Result<String, MarketDataLoadingError> {
    if let Some(column_name) = explicit_column {
        return Ok(column_name.to_string());
    }

    resolve_optional_column(explicit_column, available_by_normalized, aliases).ok_or_else(|| {
        MarketDataLoadingError::MissingColumn {
            field_path: field_path.to_string(),
            message: format!(
                "could not resolve a source column from common aliases [{}]. source_path={source_path}",
                aliases.join(", ")
            ),
        }
    })
}

fn resolve_optional_column(
    explicit_column: Option<&str>,
    available_by_normalized: &HashMap<String, String>,
    aliases: &[&str],
) -> Option<String> {
    explicit_column
        .map(ToOwned::to_owned)
        .or_else(|| first_alias_match(available_by_normalized, aliases))
}

fn first_alias_match(
    available_by_normalized: &HashMap<String, String>,
    aliases: &[&str],
) -> Option<String> {
    aliases.iter().find_map(|alias| {
        available_by_normalized
            .get(&normalize_identifier(alias))
            .cloned()
    })
}

fn normalized_source_columns(available_columns: &[String]) -> HashMap<String, String> {
    let mut normalized = HashMap::new();
    for column_name in available_columns {
        normalized
            .entry(normalize_identifier(column_name))
            .or_insert_with(|| column_name.clone());
    }

    normalized
}

fn normalize_identifier(identifier: &str) -> String {
    identifier
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        MarketDataNormalizationPolicy, MarketDataOptionQuoteInputPolicy,
        MarketDataOptionSurfaceColumnMapping, MarketDataTickDataColumnMapping,
        MarketDataTickPriceInputPolicy, resolve_option_surface_column_mapping,
        resolve_option_surface_quote_input_policy, resolve_tick_data_column_mapping,
    };

    #[test]
    fn resolve_tick_mapping_uses_common_aliases_for_missing_columns() {
        let available_columns = vec![
            "ts_recv".to_string(),
            "trade_price".to_string(),
            "side".to_string(),
        ];

        let resolved = resolve_tick_data_column_mapping(
            available_columns.as_slice(),
            &MarketDataTickDataColumnMapping {
                timestamp_column: None,
                price_column: None,
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
            "/tmp/ticks.parquet",
        )
        .expect("tick mapping should resolve");

        assert_eq!(resolved.timestamp_column.as_deref(), Some("ts_recv"));
        assert_eq!(resolved.price_column.as_deref(), Some("trade_price"));
        assert_eq!(resolved.trade_sign_column.as_deref(), Some("side"));
    }

    #[test]
    fn resolve_option_surface_mapping_uses_common_aliases_for_missing_columns() {
        let available_columns = vec![
            "quote_time".to_string(),
            "expiry_utc".to_string(),
            "strike_price".to_string(),
            "right".to_string(),
            "best_bid".to_string(),
            "best_ask".to_string(),
        ];

        let resolved = resolve_option_surface_column_mapping(
            available_columns.as_slice(),
            &MarketDataOptionSurfaceColumnMapping {
                quote_timestamp_column: None,
                expiry_timestamp_column: None,
                strike_column: None,
                option_type_column: None,
                mid_price_column: None,
                bid_column: None,
                ask_column: None,
                implied_volatility_column: None,
            },
            &MarketDataNormalizationPolicy {
                timestamp_timezone: "UTC".to_string(),
                sort_records_by_timestamp: true,
                duplicate_record_policy: None,
                tick_price_input_policy: None,
                option_quote_input_policy: Some(MarketDataOptionQuoteInputPolicy::BidAskMidpoint),
            },
            "/tmp/options.parquet",
        )
        .expect("option surface mapping should resolve");

        assert_eq!(
            resolved.quote_timestamp_column.as_deref(),
            Some("quote_time")
        );
        assert_eq!(
            resolved.expiry_timestamp_column.as_deref(),
            Some("expiry_utc")
        );
        assert_eq!(resolved.strike_column.as_deref(), Some("strike_price"));
        assert_eq!(resolved.option_type_column.as_deref(), Some("right"));
        assert_eq!(resolved.bid_column.as_deref(), Some("best_bid"));
        assert_eq!(resolved.ask_column.as_deref(), Some("best_ask"));
    }

    #[test]
    fn resolve_option_surface_quote_input_policy_infers_bid_ask_midpoint() {
        let available_columns = vec![
            "quote_time".to_string(),
            "expiry_utc".to_string(),
            "strike_price".to_string(),
            "right".to_string(),
            "best_bid".to_string(),
            "best_ask".to_string(),
        ];

        let resolved = resolve_option_surface_quote_input_policy(
            available_columns.as_slice(),
            &MarketDataOptionSurfaceColumnMapping {
                quote_timestamp_column: None,
                expiry_timestamp_column: None,
                strike_column: None,
                option_type_column: None,
                mid_price_column: None,
                bid_column: None,
                ask_column: None,
                implied_volatility_column: None,
            },
            &MarketDataNormalizationPolicy {
                timestamp_timezone: "UTC".to_string(),
                sort_records_by_timestamp: true,
                duplicate_record_policy: None,
                tick_price_input_policy: None,
                option_quote_input_policy: None,
            },
            "/tmp/options.parquet",
        )
        .expect("option quote-input policy should resolve");

        assert_eq!(resolved, MarketDataOptionQuoteInputPolicy::BidAskMidpoint);
    }
}
