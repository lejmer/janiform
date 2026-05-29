use crate::market_data::loading::contracts::{
    MarketDataInputFileReference, MarketDataLoadingError, ResolvedRollAdjustmentEvidence,
};
use crate::market_data::loading::rows::load_source_columns;

use super::source::{
    first_alias_match, load_projected_rows, normalized_source_columns, numeric_raw_value,
};

const ADDITIVE_ADJUSTMENT_ALIASES: [&str; 5] = [
    "cumulative_additive_back_adjustment",
    "additive_back_adjustment",
    "back_adjustment",
    "price_adjustment",
    "roll_adjustment",
];
// The adjusted = raw + adjustment validation only tolerates binary floating-point
// addition roundoff, scaled by the magnitude of the three operands.
const FLOAT_ADDITION_ROUNDOFF_MULTIPLIER: f64 = 64.0;

pub(super) fn roll_adjustment_evidence(
    source: &MarketDataInputFileReference,
    adjusted_price_column: Option<&str>,
    raw_price_column: Option<&str>,
) -> Result<Option<ResolvedRollAdjustmentEvidence>, MarketDataLoadingError> {
    let (Some(adjusted_price_column), Some(raw_price_column)) =
        (adjusted_price_column, raw_price_column)
    else {
        return Ok(None);
    };

    let available_columns = load_source_columns(source, "tick_data_source")?;
    let available_by_normalized = normalized_source_columns(available_columns.as_slice());

    let Some(adjustment_column) = first_alias_match(
        &available_by_normalized,
        ADDITIVE_ADJUSTMENT_ALIASES.as_slice(),
    ) else {
        return Ok(None);
    };

    // Intentional copy because the projected row reader owns requested
    // source column names while streaming.
    let projected_columns = vec![
        adjusted_price_column.to_string(),
        raw_price_column.to_string(),
        adjustment_column.clone(),
    ];

    let checked = load_projected_rows(source, projected_columns.as_slice(), Some(256))?;

    let mut checked_row_count = 0usize;
    let mut max_absolute_relation_error = 0.0f64;

    for row in checked {
        let Some(adjusted_price) = numeric_raw_value(row.get(adjusted_price_column)) else {
            continue;
        };

        let Some(raw_price) = numeric_raw_value(row.get(raw_price_column)) else {
            continue;
        };

        let Some(adjustment) = numeric_raw_value(row.get(adjustment_column.as_str())) else {
            continue;
        };

        let relation_error = (adjusted_price - (raw_price + adjustment)).abs();

        let roundoff_scale = FLOAT_ADDITION_ROUNDOFF_MULTIPLIER
            * f64::EPSILON
            * adjusted_price
                .abs()
                .max(raw_price.abs())
                .max(adjustment.abs())
                .max(1.0);
        if relation_error > roundoff_scale {
            return Err(MarketDataLoadingError::InvalidValue {
                field_path: "price_coordinate_resolution.roll_adjustment_evidence".to_string(),
                message: format!(
                    "raw and adjusted price columns did not satisfy adjusted = raw + adjustment within binary floating-point addition roundoff. adjusted_column={adjusted_price_column} raw_column={raw_price_column} adjustment_column={adjustment_column}"
                ),
            });
        }

        checked_row_count = checked_row_count.saturating_add(1);
        max_absolute_relation_error = max_absolute_relation_error.max(relation_error);
    }

    if checked_row_count == 0 {
        return Ok(None);
    }

    Ok(Some(ResolvedRollAdjustmentEvidence {
        adjusted_price_column: adjusted_price_column.to_string(),
        raw_price_column: raw_price_column.to_string(),
        adjustment_column,
        checked_row_count,
        max_absolute_relation_error,
    }))
}
