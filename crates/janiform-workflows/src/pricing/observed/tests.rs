use std::fs;
use std::time::Duration;

use chrono::{DateTime, Utc};
use janiform::calibration::market_data::MarketDataTranslationTickRecord;
use janiform::model::hawkes::HawkesEventSide;
use janiform::pricing::{PricingProgressEvent, VanillaOptionPayoffType};
use janiform_engine_pricing::VanillaOptionPricingStatus;
use serde_json::json;

use crate::progress::{
    ProgressUnitTracker, completed_tracked_progress_units, total_tracked_progress_units,
};

use super::artifacts::write_observed_artifacts_if_requested;
use super::data::prepare_selected_expiry_snapshot_specs_by_valuation_times;
use super::state::observed_tick_state_columns;
use super::surface_progress::{
    SurfaceWorkerState, surface_task_progress_totals, surface_worker_labels,
};
use super::types::{
    ObservedOptionsPricingNode, ObservedOptionsPricingScope, ObservedOptionsSnapshotResult,
    PreparedSnapshotNode,
};

#[test]
fn selected_expiry_snapshot_specs_reuse_incremental_latest_quotes() {
    let history = vec![
        snapshot_node(1, 100, 10.0, 1.0),
        snapshot_node(1, 100, 12.0, 2.0),
        snapshot_node(2, 100, 10.0, 3.0),
        snapshot_node(3, 100, 12.0, 4.0),
    ];

    let snapshot_specs =
        prepare_selected_expiry_snapshot_specs_by_valuation_times(&history, &[1, 2, 3])
            .expect("snapshot specs should be prepared");

    assert_eq!(snapshot_specs.len(), 3);
    assert_eq!(
        snapshot_specs[0].1[0].nodes[0].observed_mid_price,
        Some(1.0)
    );
    assert_eq!(
        snapshot_specs[0].1[0].nodes[1].observed_mid_price,
        Some(2.0)
    );
    assert_eq!(
        snapshot_specs[1].1[0].nodes[0].observed_mid_price,
        Some(3.0)
    );
    assert_eq!(
        snapshot_specs[1].1[0].nodes[1].observed_mid_price,
        Some(2.0)
    );
    assert_eq!(
        snapshot_specs[2].1[0].nodes[0].observed_mid_price,
        Some(3.0)
    );
    assert_eq!(
        snapshot_specs[2].1[0].nodes[1].observed_mid_price,
        Some(4.0)
    );
}

#[test]
fn observed_tick_state_columns_apply_cutoff_and_default_missing_trade_side() {
    let tick_records = vec![
        tick_record(1, 100.0, Some("buy")),
        tick_record(2, 100.5, None),
        tick_record(3, 101.0, Some("sell")),
    ];

    let columns = observed_tick_state_columns(&tick_records, 2)
        .expect("tick state columns should be prepared");

    assert_eq!(columns.timestamp_utc_nanos, vec![1, 2]);
    assert_eq!(columns.trade_prices, vec![100.0, 100.5]);
    assert_eq!(
        columns.trade_sides,
        vec![HawkesEventSide::Buy, HawkesEventSide::Buy]
    );
}

#[test]
fn surface_worker_labels_are_stable_worker_lanes() {
    assert_eq!(
        surface_worker_labels(3),
        vec![
            "worker 01".to_string(),
            "worker 02".to_string(),
            "worker 03".to_string(),
        ]
    );
}

#[test]
fn surface_global_progress_sums_real_task_progress() {
    let task_progress_totals = surface_task_progress_totals(&[100, 200, 300]);
    let mut task_progress = ProgressUnitTracker::planned_for_totals(&task_progress_totals);
    task_progress[0].record(25);
    task_progress[1].record(50);
    task_progress[2].complete();

    assert_eq!(total_tracked_progress_units(&task_progress), 603);
    assert_eq!(completed_tracked_progress_units(&task_progress), 376);
}

#[test]
fn surface_worker_transition_row_progress_does_not_reset_interval_progress() {
    let mut worker_state = SurfaceWorkerState::started(0, 1, 10, 10);
    worker_state.apply_pricing_event(&PricingProgressEvent::interval_progress(5, 10, Some(5), 10));

    worker_state.apply_pricing_event(&PricingProgressEvent::transition_row_progress(
        12,
        65,
        Some(5),
        Some(10),
    ));

    assert_eq!(worker_state.progress_percent, 45);
    assert_eq!(worker_state.interval_units, "6/10");
    assert_eq!(worker_state.row_units, "12/65");
}

#[test]
fn surface_worker_stays_running_until_task_done() {
    let mut worker_state = SurfaceWorkerState::started(0, 1, 10, 10);
    worker_state.apply_pricing_event(&PricingProgressEvent::interval_progress(
        10,
        10,
        Some(9),
        10,
    ));

    assert_eq!(worker_state.progress_percent, 90);
    assert_eq!(worker_state.phase, "pricing intervals");

    worker_state.complete();

    assert_eq!(worker_state.progress_percent, 100);
    assert_eq!(worker_state.phase, "done");
}

#[test]
fn observed_primary_artifact_matches_native_result_shape() {
    let valuation_time_utc = DateTime::<Utc>::from_timestamp_nanos(1_000_000_000);
    let output_directory = std::env::temp_dir().join(format!(
        "janiform-observed-pricing-artifact-test-{}",
        std::process::id()
    ));
    let output_path = output_directory.join("observed_pricing.json");
    let node = observed_pricing_node(valuation_time_utc);
    let snapshot_result = ObservedOptionsSnapshotResult {
        valuation_time_utc,
        spot: 100.0,
        nodes: vec![node.clone()],
    };

    let paths = write_observed_artifacts_if_requested(
        Some(&output_path),
        "observed-options-test",
        ObservedOptionsPricingScope::Surface,
        valuation_time_utc,
        &[node],
        &[snapshot_result],
        Duration::from_secs(2),
    )
    .expect("observed artifact should be writable")
    .expect("artifact paths should be returned");
    let primary_payload: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(&paths.primary_output_path)
            .expect("primary observed artifact should be readable"),
    )
    .expect("primary observed artifact should be valid JSON");

    fs::remove_dir_all(&output_directory).expect("test artifact directory should be removable");

    assert_eq!(
        primary_payload["schema_version"],
        json!("janiform.observed_options_pricing_result.v1")
    );
    assert_eq!(primary_payload["run_id"], json!("observed-options-test"));
    assert_eq!(primary_payload["scope"], json!("surface"));
    assert_eq!(
        primary_payload["valuation_time_utc"],
        json!(valuation_time_utc.to_rfc3339())
    );
    assert_eq!(primary_payload["nodes"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        primary_payload["snapshot_results"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(primary_payload["artifact_paths"], json!(null));
    assert_eq!(primary_payload["execution_duration_seconds"], json!(2.0));
}

fn snapshot_node(
    quote_timestamp_utc_nanos: i64,
    expiry_timestamp_utc_nanos: i64,
    strike: f64,
    observed_mid_price: f64,
) -> PreparedSnapshotNode {
    PreparedSnapshotNode {
        quote_timestamp_utc_nanos,
        expiry_timestamp_utc_nanos,
        strike,
        option_type: VanillaOptionPayoffType::Call,
        observed_bid_price: None,
        observed_ask_price: None,
        observed_mid_price: Some(observed_mid_price),
    }
}

fn observed_pricing_node(valuation_time_utc: DateTime<Utc>) -> ObservedOptionsPricingNode {
    ObservedOptionsPricingNode {
        valuation_time_utc,
        quote_timestamp_utc: valuation_time_utc,
        expiry_timestamp_utc: DateTime::<Utc>::from_timestamp_nanos(3_600_000_000_000),
        strike: 100.0,
        option_type: VanillaOptionPayoffType::Call,
        risk_free_rate: 0.0,
        dividend_yield: 0.0,
        maturity_interval: 1.0,
        observed_bid_price: Some(1.0),
        observed_ask_price: Some(1.2),
        observed_mid_price: Some(1.1),
        model_price: Some(1.05),
        model_implied_volatility: Some(0.2),
        implied_volatility_inversion_status: None,
        pricing_status: VanillaOptionPricingStatus::Succeeded,
        pricing_failure_stage: None,
        pricing_failure_reason: None,
        forward_price: Some(100.0),
        discount_factor: Some(1.0),
        boundary_state_norm: Some(0.0),
    }
}

fn tick_record(
    timestamp_utc_nanos: i64,
    price: f64,
    trade_sign: Option<&str>,
) -> MarketDataTranslationTickRecord {
    MarketDataTranslationTickRecord {
        timestamp_utc_nanos,
        price: Some(price),
        trade_sign: trade_sign.map(str::to_string),
        source_row_number: 0,
    }
}
