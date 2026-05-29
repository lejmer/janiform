use crate::market_data::preprocessing::MarketDataTranslationTickRecord;

use super::identity::{
    latest_tick_underlier_identity, unique_option_underlier_identity, validate_underlier_identity,
};
use super::parity::{parity_forward_check, validated_forward_price};
use super::roll_adjustment::roll_adjustment_evidence;
use super::source::{first_alias_match, normalized_source_columns};
use crate::market_data::loading::contracts::{
    MarketDataInputFileReference, MarketDataLoadingError, MarketDataNormalizationPolicy,
    MarketDataNormalizedOptionSurfaceRecord, MarketDataTickDataColumnMapping,
    MarketDataTickPriceInputPolicy, OptionForwardParityWarningPolicy, PriceCoordinatePolicy,
    ResolvedPriceCoordinate, ResolvedPriceCoordinateSet,
};
use crate::market_data::loading::pipeline::normalize_market_data_source;
use crate::market_data::loading::rows::load_source_columns;

const RAW_PRICE_ALIASES: [&str; 6] = [
    "raw_trade_price",
    "raw_price",
    "unadjusted_trade_price",
    "unadjusted_price",
    "source_trade_price",
    "contract_trade_price",
];
const RAW_BID_ALIASES: [&str; 5] = [
    "raw_bid_price",
    "raw_bid",
    "unadjusted_bid_price",
    "source_bid_price",
    "contract_bid_price",
];
const RAW_ASK_ALIASES: [&str; 5] = [
    "raw_ask_price",
    "raw_ask",
    "unadjusted_ask_price",
    "source_ask_price",
    "contract_ask_price",
];

/// Inputs for resolving the price coordinates used by option workflows.
pub struct PriceCoordinateResolutionInput<'a> {
    pub tick_data_source: &'a MarketDataInputFileReference,
    pub dynamics_tick_data_column_mapping: &'a MarketDataTickDataColumnMapping,
    pub normalization_policy: &'a MarketDataNormalizationPolicy,
    pub option_surface_source: Option<&'a MarketDataInputFileReference>,
    pub option_surface_records: Option<&'a [MarketDataNormalizedOptionSurfaceRecord]>,
    pub dynamics_tick_records: &'a [MarketDataTranslationTickRecord],
    pub valuation_time_utc_nanos: Option<i64>,
    pub policy: PriceCoordinatePolicy,
    pub parity_warning_policy: &'a OptionForwardParityWarningPolicy,
    pub risk_free_rate: f64,
    pub dividend_yield: f64,
}

/// Output from price-coordinate resolution.
#[derive(Debug)]
pub struct PriceCoordinateResolutionOutput {
    pub resolution: Option<ResolvedPriceCoordinateSet>,
    pub option_valuation_spot: Option<f64>,
    pub option_valuation_tick_records: Option<Vec<MarketDataTranslationTickRecord>>,
    pub warning: Option<String>,
}

pub fn resolve_price_coordinates(
    input: PriceCoordinateResolutionInput<'_>,
) -> Result<PriceCoordinateResolutionOutput, MarketDataLoadingError> {
    input.parity_warning_policy.validate()?;

    let Some(option_surface_source) = input.option_surface_source else {
        return single_coordinate_output(&input, None, None);
    };

    let Some(option_surface_records) = input.option_surface_records else {
        return single_coordinate_output(&input, None, None);
    };

    let Some(valuation_time_utc_nanos) = input.valuation_time_utc_nanos else {
        return single_coordinate_output(&input, None, None);
    };

    let raw_mapping = infer_raw_option_tick_mapping(
        input.tick_data_source,
        input.dynamics_tick_data_column_mapping,
    )?;

    let resolved = match (input.policy, raw_mapping) {
        (PriceCoordinatePolicy::Single, _) => {
            return single_coordinate_output(
                &input,
                option_surface_records
                    .first()
                    .map(|_| option_surface_source),
                Some(valuation_time_utc_nanos),
            );
        }
        (PriceCoordinatePolicy::ContinuousFitRawOptionAnchor, None) => {
            return Err(MarketDataLoadingError::MissingColumn {
                field_path: "price_coordinate_policy".to_string(),
                message: "continuous_fit_raw_option_anchor requires raw tick price columns"
                    .to_string(),
            });
        }
        (PriceCoordinatePolicy::Auto, None) => {
            return single_coordinate_output(
                &input,
                option_surface_records
                    .first()
                    .map(|_| option_surface_source),
                Some(valuation_time_utc_nanos),
            );
        }
        (_, Some(raw_mapping)) => raw_mapping,
    };

    let option_underlier_identity = unique_option_underlier_identity(option_surface_source)?;

    let tick_underlier_identity = latest_tick_underlier_identity(
        input.tick_data_source,
        valuation_time_utc_nanos,
        input
            .dynamics_tick_data_column_mapping
            .timestamp_column
            .as_deref(),
    )?;

    validate_underlier_identity(
        option_underlier_identity.as_deref(),
        tick_underlier_identity.as_deref(),
    )?;

    let raw_tick_output = normalize_market_data_source(
        input.tick_data_source,
        &resolved,
        // Intentional copy because raw coordinate normalization reuses the
        // caller policy with only the price input mode overridden.
        &MarketDataNormalizationPolicy {
            tick_price_input_policy: Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn),
            ..input.normalization_policy.clone()
        },
        true,
    )?;

    let option_valuation_spot = last_positive_price_at_or_before(
        raw_tick_output.tick_records.as_slice(),
        valuation_time_utc_nanos,
        "raw option valuation coordinate",
    )?;

    let model_forward_price = validated_forward_price(
        option_valuation_spot,
        input.risk_free_rate,
        input.dividend_yield,
        0.0,
    )?;

    let parity_forward_check = parity_forward_check(
        option_surface_records,
        valuation_time_utc_nanos,
        model_forward_price,
        input.risk_free_rate,
        input.dividend_yield,
        input.parity_warning_policy.relative_threshold,
    )?;

    // Intentional copy because the warning is returned both as a workflow
    // warning and inside the persisted parity diagnostic.
    let warning = parity_forward_check
        .as_ref()
        .and_then(|check| check.warning.clone());

    let resolution = ResolvedPriceCoordinateSet {
        policy: input.policy,
        dynamics_fit: resolved_coordinate(
            "dynamics_fit",
            "continuous",
            input.dynamics_tick_data_column_mapping,
            input.normalization_policy.tick_price_input_policy,
        ),
        option_valuation: resolved_coordinate(
            "option_valuation",
            "raw_contract",
            &resolved,
            Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn),
        ),
        option_underlier_identity,
        tick_underlier_identity,
        roll_adjustment_evidence: roll_adjustment_evidence(
            input.tick_data_source,
            input
                .dynamics_tick_data_column_mapping
                .price_column
                .as_deref(),
            resolved.price_column.as_deref(),
        )?,
        parity_forward_check,
    };

    Ok(PriceCoordinateResolutionOutput {
        resolution: Some(resolution),
        option_valuation_spot: Some(option_valuation_spot),
        option_valuation_tick_records: Some(raw_tick_output.tick_records),
        warning,
    })
}

fn single_coordinate_output(
    input: &PriceCoordinateResolutionInput<'_>,
    option_surface_source: Option<&MarketDataInputFileReference>,
    valuation_time_utc_nanos: Option<i64>,
) -> Result<PriceCoordinateResolutionOutput, MarketDataLoadingError> {
    let option_valuation_spot = match valuation_time_utc_nanos {
        Some(time) => Some(last_positive_price_at_or_before(
            input.dynamics_tick_records,
            time,
            "single coordinate",
        )?),
        None => None,
    };

    let option_underlier_identity = match option_surface_source {
        Some(source) => unique_option_underlier_identity(source)?,
        None => None,
    };

    let tick_underlier_identity = match valuation_time_utc_nanos {
        Some(time) => latest_tick_underlier_identity(
            input.tick_data_source,
            time,
            input
                .dynamics_tick_data_column_mapping
                .timestamp_column
                .as_deref(),
        )?,
        None => None,
    };

    validate_underlier_identity(
        option_underlier_identity.as_deref(),
        tick_underlier_identity.as_deref(),
    )?;

    Ok(PriceCoordinateResolutionOutput {
        resolution: Some(ResolvedPriceCoordinateSet {
            policy: input.policy,
            dynamics_fit: resolved_coordinate(
                "dynamics_fit",
                "single",
                input.dynamics_tick_data_column_mapping,
                input.normalization_policy.tick_price_input_policy,
            ),
            option_valuation: resolved_coordinate(
                "option_valuation",
                "single",
                input.dynamics_tick_data_column_mapping,
                input.normalization_policy.tick_price_input_policy,
            ),
            option_underlier_identity,
            tick_underlier_identity,
            roll_adjustment_evidence: None,
            parity_forward_check: None,
        }),
        option_valuation_spot,
        option_valuation_tick_records: None,
        warning: None,
    })
}

fn infer_raw_option_tick_mapping(
    source: &MarketDataInputFileReference,
    dynamics_mapping: &MarketDataTickDataColumnMapping,
) -> Result<Option<MarketDataTickDataColumnMapping>, MarketDataLoadingError> {
    let available_columns = load_source_columns(source, "tick_data_source")?;
    let available_by_normalized = normalized_source_columns(available_columns.as_slice());

    let raw_price_column =
        first_alias_match(&available_by_normalized, RAW_PRICE_ALIASES.as_slice());

    let Some(raw_price_column) = raw_price_column else {
        return Ok(None);
    };

    let raw_bid_column = first_alias_match(&available_by_normalized, RAW_BID_ALIASES.as_slice());
    let raw_ask_column = first_alias_match(&available_by_normalized, RAW_ASK_ALIASES.as_slice());

    // Intentional copies because the resolved raw mapping owns selected column
    // names independently of the dynamics mapping.
    Ok(Some(MarketDataTickDataColumnMapping {
        timestamp_column: dynamics_mapping.timestamp_column.clone(),
        price_column: Some(raw_price_column),
        trade_sign_column: dynamics_mapping.trade_sign_column.clone(),
        bid_column: raw_bid_column,
        ask_column: raw_ask_column,
    }))
}

fn resolved_coordinate(
    role: &str,
    kind: &str,
    mapping: &MarketDataTickDataColumnMapping,
    policy: Option<MarketDataTickPriceInputPolicy>,
) -> ResolvedPriceCoordinate {
    ResolvedPriceCoordinate {
        coordinate_role: role.to_string(),
        coordinate_kind: kind.to_string(),
        // Intentional copy because the persisted resolution owns its column mapping.
        tick_data_column_mapping: mapping.clone(),
        tick_price_input_policy: policy,
    }
}

fn last_positive_price_at_or_before(
    tick_records: &[MarketDataTranslationTickRecord],
    valuation_time_utc_nanos: i64,
    coordinate_label: &str,
) -> Result<f64, MarketDataLoadingError> {
    tick_records
        .iter()
        .take_while(|record| record.timestamp_utc_nanos <= valuation_time_utc_nanos)
        .filter_map(|record| record.price)
        .filter(|price| price.is_finite() && *price > 0.0)
        .last()
        .ok_or_else(|| MarketDataLoadingError::InvalidValue {
            field_path: "price_coordinate_resolution.option_valuation_spot".to_string(),
            message: format!(
                "could not resolve a finite positive {coordinate_label} price at or before valuation time"
            ),
        })
}
