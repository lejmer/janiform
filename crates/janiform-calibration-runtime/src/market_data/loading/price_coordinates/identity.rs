use std::collections::BTreeSet;

use crate::market_data::loading::contracts::{
    MarketDataInputFileReference, MarketDataLoadingError,
};
use crate::market_data::loading::rows::load_source_columns;

use super::source::{
    first_alias_match, integer_raw_value, load_projected_rows, normalized_source_columns,
    text_raw_value,
};

const OPTION_UNDERLIER_ALIASES: [&str; 6] = [
    "underlying_future_symbol",
    "underlying_symbol",
    "underlying",
    "underlier",
    "underlying_instrument_id",
    "underlier_symbol",
];
const TICK_UNDERLIER_ALIASES: [&str; 6] = [
    "source_contract_symbol",
    "active_contract_symbol",
    "contract_symbol",
    "instrument_symbol",
    "symbol",
    "raw_symbol",
];

pub(super) fn unique_option_underlier_identity(
    source: &MarketDataInputFileReference,
) -> Result<Option<String>, MarketDataLoadingError> {
    unique_text_value_for_aliases(source, OPTION_UNDERLIER_ALIASES.as_slice())
}

pub(super) fn latest_tick_underlier_identity(
    source: &MarketDataInputFileReference,
    valuation_time_utc_nanos: i64,
    timestamp_column: Option<&str>,
) -> Result<Option<String>, MarketDataLoadingError> {
    let Some(timestamp_column) = timestamp_column else {
        return Ok(None);
    };

    let available_columns = load_source_columns(source, "tick_data_source")?;
    let available_by_normalized = normalized_source_columns(available_columns.as_slice());

    let Some(identity_column) =
        first_alias_match(&available_by_normalized, TICK_UNDERLIER_ALIASES.as_slice())
    else {
        return Ok(None);
    };

    // Intentional copies because the projected row reader owns requested
    // source column names while streaming.
    let projected_columns = [timestamp_column.to_string(), identity_column.clone()];
    let rows = load_projected_rows(source, &projected_columns, None)?;

    let mut latest = None::<(i64, String)>;

    for row in rows {
        let Some(timestamp) = integer_raw_value(row.get(timestamp_column)) else {
            continue;
        };

        if timestamp > valuation_time_utc_nanos {
            continue;
        }

        let Some(identity) = text_raw_value(row.get(identity_column.as_str())) else {
            continue;
        };

        if latest
            .as_ref()
            .is_none_or(|(latest_timestamp, _)| timestamp >= *latest_timestamp)
        {
            latest = Some((timestamp, identity));
        }
    }

    Ok(latest.map(|(_, identity)| identity))
}

pub(super) fn validate_underlier_identity(
    option_underlier_identity: Option<&str>,
    tick_underlier_identity: Option<&str>,
) -> Result<(), MarketDataLoadingError> {
    let (Some(option_identity), Some(tick_identity)) =
        (option_underlier_identity, tick_underlier_identity)
    else {
        return Ok(());
    };

    if option_identity == tick_identity {
        return Ok(());
    }

    Err(MarketDataLoadingError::InvalidValue {
        field_path: "price_coordinate_resolution.underlier_identity".to_string(),
        message: format!(
            "option underlier identity {option_identity} does not match tick contract identity {tick_identity}"
        ),
    })
}

fn unique_text_value_for_aliases(
    source: &MarketDataInputFileReference,
    aliases: &[&str],
) -> Result<Option<String>, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "source")?;
    let available_by_normalized = normalized_source_columns(available_columns.as_slice());

    let Some(column) = first_alias_match(&available_by_normalized, aliases) else {
        return Ok(None);
    };

    let rows = load_projected_rows(source, &[column], None)?;

    let values = rows
        .iter()
        .filter_map(|row| text_raw_value(row.values().next()))
        .collect::<BTreeSet<_>>();

    match values.len() {
        0 => Ok(None),
        1 => Ok(values.into_iter().next()),
        _ => Err(MarketDataLoadingError::InvalidValue {
            field_path: "price_coordinate_resolution.option_underlier_identity".to_string(),
            message: "option surface contains multiple underlier identities in one workflow input"
                .to_string(),
        }),
    }
}
