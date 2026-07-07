use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use janiform_foundations::paths::format_path;

use crate::market_data::loading::contracts::{
    MarketDataInputFileFormat, MarketDataInputFileReference, MarketDataNormalizationPolicy,
    MarketDataNormalizedOptionSurfaceRecord, MarketDataTickDataColumnMapping,
    MarketDataTickPriceInputPolicy, OptionForwardParityWarningPolicy, PriceCoordinatePolicy,
};
use crate::market_data::preprocessing::MarketDataTranslationTickRecord;

use super::{PriceCoordinateResolutionInput, resolve_price_coordinates};

const VALUATION_TIME_NANOS: i64 = 1_000_000_000;
const EXPIRY_TIME_NANOS: i64 = 86_401_000_000_000;

#[test]
fn no_option_surface_keeps_single_coordinate_behavior() {
    let tick_source = temporary_csv_file(
        "single_ticks",
        "timestamp_utc_nanos,adjusted_trade_price\n1970-01-01T00:00:01Z,100.0\n",
    );

    let dynamics_mapping = dynamics_mapping();
    let normalization_policy = normalization_policy();

    let dynamics_tick_records = vec![dynamics_tick_record(100.0)];

    let output = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &tick_source,
        dynamics_tick_data_column_mapping: &dynamics_mapping,
        normalization_policy: &normalization_policy,
        option_surface_source: None,
        option_surface_records: None,
        dynamics_tick_records: dynamics_tick_records.as_slice(),
        valuation_time_utc_nanos: None,
        policy: PriceCoordinatePolicy::Auto,
        parity_warning_policy: &OptionForwardParityWarningPolicy::default(),
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
    })
    .expect("single-coordinate resolution should succeed");

    let resolution = output
        .resolution
        .expect("single-coordinate resolution should be reported");

    assert_eq!(PriceCoordinatePolicy::Auto, resolution.policy);
    assert_eq!("single", resolution.dynamics_fit.coordinate_kind);
    assert_eq!("single", resolution.option_valuation.coordinate_kind);
    assert_eq!(None, output.option_valuation_tick_records);
}

#[test]
fn option_surface_with_raw_adjusted_ticks_resolves_split_coordinates() {
    let tick_source = temporary_csv_file(
        "split_ticks",
        concat!(
            "timestamp_utc_nanos,adjusted_trade_price,raw_trade_price,raw_bid_price,raw_ask_price,cumulative_additive_back_adjustment,source_contract_symbol\n",
            "1970-01-01T00:00:01Z,100.0,102.0,101.75,102.25,-2.0,ESM6\n",
        ),
    );

    let option_source = temporary_csv_file("split_options", "underlying_future_symbol\nESM6\n");
    let dynamics_mapping = dynamics_mapping();
    let normalization_policy = normalization_policy();

    let dynamics_tick_records = vec![dynamics_tick_record(100.0)];

    let option_records = parity_option_records(5.0, 5.0);

    let output = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &tick_source,
        dynamics_tick_data_column_mapping: &dynamics_mapping,
        normalization_policy: &normalization_policy,
        option_surface_source: Some(&option_source),
        option_surface_records: Some(option_records.as_slice()),
        dynamics_tick_records: dynamics_tick_records.as_slice(),
        valuation_time_utc_nanos: Some(VALUATION_TIME_NANOS),
        policy: PriceCoordinatePolicy::Auto,
        parity_warning_policy: &OptionForwardParityWarningPolicy::default(),
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
    })
    .expect("split-coordinate resolution should succeed");

    let resolution = output
        .resolution
        .expect("split-coordinate resolution should be reported");

    assert_eq!("continuous", resolution.dynamics_fit.coordinate_kind);
    assert_eq!("raw_contract", resolution.option_valuation.coordinate_kind);
    assert_eq!(
        Some("ESM6".to_string()),
        resolution.option_underlier_identity
    );
    assert_eq!(
        Some("raw_trade_price".to_string()),
        resolution
            .option_valuation
            .tick_data_column_mapping
            .price_column
    );
    assert_eq!(Some(102.0), output.option_valuation_spot);
    assert_eq!(
        Some(1),
        output
            .option_valuation_tick_records
            .map(|records| records.len())
    );
    assert!(resolution.roll_adjustment_evidence.is_some());
}

#[test]
fn invalid_raw_adjusted_relation_is_rejected() {
    let tick_source = temporary_csv_file(
        "bad_adjustment_ticks",
        concat!(
            "timestamp_utc_nanos,adjusted_trade_price,raw_trade_price,cumulative_additive_back_adjustment,source_contract_symbol\n",
            "1970-01-01T00:00:01Z,100.0,102.0,-1.0,ESM6\n",
        ),
    );

    let option_source =
        temporary_csv_file("bad_adjustment_options", "underlying_future_symbol\nESM6\n");

    let dynamics_mapping = dynamics_mapping();
    let normalization_policy = normalization_policy();

    let dynamics_tick_records = vec![dynamics_tick_record(100.0)];

    let option_records = parity_option_records(5.0, 5.0);

    let error = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &tick_source,
        dynamics_tick_data_column_mapping: &dynamics_mapping,
        normalization_policy: &normalization_policy,
        option_surface_source: Some(&option_source),
        option_surface_records: Some(option_records.as_slice()),
        dynamics_tick_records: dynamics_tick_records.as_slice(),
        valuation_time_utc_nanos: Some(VALUATION_TIME_NANOS),
        policy: PriceCoordinatePolicy::ContinuousFitRawOptionAnchor,
        parity_warning_policy: &OptionForwardParityWarningPolicy::default(),
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
    })
    .expect_err("invalid adjusted/raw relation should be rejected");

    assert!(error.to_string().contains("adjusted = raw + adjustment"));
}

#[test]
fn parity_mismatch_emits_warning() {
    let tick_source = temporary_csv_file(
        "parity_warning_ticks",
        concat!(
            "timestamp_utc_nanos,adjusted_trade_price,raw_trade_price,cumulative_additive_back_adjustment,source_contract_symbol\n",
            "1970-01-01T00:00:01Z,100.0,102.0,-2.0,ESM6\n",
        ),
    );

    let option_source =
        temporary_csv_file("parity_warning_options", "underlying_future_symbol\nESM6\n");

    let dynamics_mapping = dynamics_mapping();
    let normalization_policy = normalization_policy();

    let dynamics_tick_records = vec![dynamics_tick_record(100.0)];

    let option_records = parity_option_records(5.0, 5.0);

    let output = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &tick_source,
        dynamics_tick_data_column_mapping: &dynamics_mapping,
        normalization_policy: &normalization_policy,
        option_surface_source: Some(&option_source),
        option_surface_records: Some(option_records.as_slice()),
        dynamics_tick_records: dynamics_tick_records.as_slice(),
        valuation_time_utc_nanos: Some(VALUATION_TIME_NANOS),
        policy: PriceCoordinatePolicy::Auto,
        parity_warning_policy: &OptionForwardParityWarningPolicy::default(),
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
    })
    .expect("parity warning resolution should succeed");

    let parity_check = output
        .resolution
        .and_then(|resolution| resolution.parity_forward_check)
        .expect("parity check should be reported");

    assert!(output.warning.is_some());
    assert_eq!("warning", parity_check.status);
    assert_eq!(Some(2.0), parity_check.absolute_delta);
}

#[test]
fn unsupported_multiple_option_underliers_are_rejected() {
    let tick_source = temporary_csv_file(
        "multi_underlier_ticks",
        concat!(
            "timestamp_utc_nanos,adjusted_trade_price,raw_trade_price,cumulative_additive_back_adjustment,source_contract_symbol\n",
            "1970-01-01T00:00:01Z,100.0,102.0,-2.0,ESM6\n",
        ),
    );

    let option_source = temporary_csv_file(
        "multi_underlier_options",
        "underlying_future_symbol\nESM6\nNQM6\n",
    );

    let dynamics_mapping = dynamics_mapping();
    let normalization_policy = normalization_policy();

    let dynamics_tick_records = vec![dynamics_tick_record(100.0)];

    let option_records = parity_option_records(5.0, 5.0);

    let error = resolve_price_coordinates(PriceCoordinateResolutionInput {
        tick_data_source: &tick_source,
        dynamics_tick_data_column_mapping: &dynamics_mapping,
        normalization_policy: &normalization_policy,
        option_surface_source: Some(&option_source),
        option_surface_records: Some(option_records.as_slice()),
        dynamics_tick_records: dynamics_tick_records.as_slice(),
        valuation_time_utc_nanos: Some(VALUATION_TIME_NANOS),
        policy: PriceCoordinatePolicy::Auto,
        parity_warning_policy: &OptionForwardParityWarningPolicy::default(),
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
    })
    .expect_err("multiple unsupported underliers should fail");

    assert!(
        error
            .to_string()
            .contains("contains multiple underlier identities")
    );
}

fn dynamics_mapping() -> MarketDataTickDataColumnMapping {
    MarketDataTickDataColumnMapping {
        timestamp_column: Some("timestamp_utc_nanos".to_string()),
        price_column: Some("adjusted_trade_price".to_string()),
        trade_sign_column: None,
        bid_column: None,
        ask_column: None,
    }
}

fn normalization_policy() -> MarketDataNormalizationPolicy {
    MarketDataNormalizationPolicy {
        timestamp_timezone: "UTC".to_string(),
        sort_records_by_timestamp: true,
        duplicate_record_policy: None,
        tick_price_input_policy: Some(MarketDataTickPriceInputPolicy::ExplicitPriceColumn),
        option_quote_input_policy: None,
    }
}

fn dynamics_tick_record(price: f64) -> MarketDataTranslationTickRecord {
    MarketDataTranslationTickRecord {
        timestamp_utc_nanos: VALUATION_TIME_NANOS,
        price: Some(price),
        trade_sign: None,
        source_row_number: 1,
    }
}

fn parity_option_records(
    call_mid_price: f64,
    put_mid_price: f64,
) -> Vec<MarketDataNormalizedOptionSurfaceRecord> {
    vec![
        MarketDataNormalizedOptionSurfaceRecord {
            quote_timestamp_utc_nanos: VALUATION_TIME_NANOS,
            expiry_timestamp_utc_nanos: EXPIRY_TIME_NANOS,
            strike: 100.0,
            option_type: "call".to_string(),
            quote_value: call_mid_price,
            mid_price: Some(call_mid_price),
            bid_price: Some(call_mid_price - 0.25),
            ask_price: Some(call_mid_price + 0.25),
            implied_volatility: None,
            source_row_number: 1,
        },
        MarketDataNormalizedOptionSurfaceRecord {
            quote_timestamp_utc_nanos: VALUATION_TIME_NANOS,
            expiry_timestamp_utc_nanos: EXPIRY_TIME_NANOS,
            strike: 100.0,
            option_type: "put".to_string(),
            quote_value: put_mid_price,
            mid_price: Some(put_mid_price),
            bid_price: Some(put_mid_price - 0.25),
            ask_price: Some(put_mid_price + 0.25),
            implied_volatility: None,
            source_row_number: 2,
        },
    ]
}

fn temporary_csv_file(name: &str, contents: &str) -> MarketDataInputFileReference {
    let path = temporary_path(name);
    fs::write(&path, contents).expect("test CSV should be writable");

    MarketDataInputFileReference {
        path: format_path(&path),
        file_format: MarketDataInputFileFormat::Csv,
        provenance_label: name.to_string(),
    }
}

fn temporary_path(name: &str) -> PathBuf {
    let timestamp_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "janiform_price_coordinate_{name}_{}_{}.csv",
        std::process::id(),
        timestamp_nanos,
    ))
}
