mod support;

use std::collections::BTreeSet;
use std::fs;

use chrono::{DateTime, Utc};
use janiform::calendar::ExchangeCalendarRegistry;
use janiform::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, build_hourly_factor_contract,
};
use janiform::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
    NewsComponentMarkScaleConfiguration, NewsProcessConfiguration, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration,
};
use janiform::nig::NigSharedPlusShiftConfiguration;
use janiform::simulation::{
    SeededTickEventSide, SeededTickSimulationModelInputs, SeededTickSimulationRunConfig,
    run_seeded_tick_event_simulation,
};
use serde::Deserialize;

use support::validation_fixture::{
    fixtures_root_path, load_calendar_regression_fixture_from_manifest,
};

const SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SCHEMA_VERSION: &str =
    "simulation.seeded_tick.parity_fixture.v1";
const SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SET_ID: &str =
    "seeded_tick_simulation_parity_fixture_v1";
const SEEDED_TICK_SIMULATION_CASE_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
const SEEDED_TICK_SIMULATION_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
const SEEDED_TICK_SIMULATION_EVENT_ORDERING_POLICY: &str = "timestamp_then_endogenous_before_exogenous_then_buy_before_sell_then_known_before_surprise_then_generation_order.v1";
const ONE_HOUR_NANOS: i64 = 3_600_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureDeterminismMetadata {
    fixture_loading_order: String,
    event_ordering_policy: String,
    content_hash_scope: String,
    content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureCanonicalMetadata {
    required_tags: Vec<String>,
    stable_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureKernelLoadings {
    plus_plus: f64,
    plus_minus: f64,
    minus_plus: f64,
    minus_minus: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureKernelParameters {
    lambda_bar: f64,
    beta: f64,
    alpha: SeededTickSimulationParityFixtureKernelLoadings,
    rho: SeededTickSimulationParityFixtureKernelLoadings,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureInitialExcitationState {
    y_plus_plus: f64,
    y_plus_minus: f64,
    y_minus_plus: f64,
    y_minus_minus: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureCase {
    fixture_id: String,
    seed: u64,
    exchange_code: String,
    range_start_utc: String,
    range_end_utc: String,
    horizon_interval_count: usize,
    kernel_parameters: SeededTickSimulationParityFixtureKernelParameters,
    initial_excitation_state: SeededTickSimulationParityFixtureInitialExcitationState,
    seasonality_hourly_factors: Vec<f64>,
    news_intensity_hourly_factors: Vec<f64>,
    expected_timestamps_utc_nanos: Vec<i64>,
    expected_side_sequence: Vec<String>,
    expected_total_event_count: usize,
    expected_interval_event_counts: Vec<usize>,
    expected_interval_buy_counts: Vec<usize>,
    expected_interval_sell_counts: Vec<usize>,
    expected_min_positive_timestamp_gap_nanos: i64,
    expected_has_non_grid_arrival: bool,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeededTickSimulationParityFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    fixture_set_version: String,
    determinism_metadata: SeededTickSimulationParityFixtureDeterminismMetadata,
    canonical_regression_metadata: SeededTickSimulationParityFixtureCanonicalMetadata,
    cases: Vec<SeededTickSimulationParityFixtureCase>,
}

#[test]
fn seeded_tick_simulation_parity_fixture_matches_rust_core() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");
    let fixture_document = load_seeded_tick_simulation_parity_fixture()
        .expect("seeded tick simulation parity fixture should load");

    for fixture_case in &fixture_document.cases {
        let run_result = run_fixture_case(&calendar_registry, fixture_case)
            .expect("fixture case run should complete");
        let mismatch_diagnostic = first_mismatch_diagnostic(&run_result, fixture_case);
        assert!(
            mismatch_diagnostic.is_none(),
            "{}",
            mismatch_diagnostic.unwrap_or_default()
        );
    }
}

#[test]
fn seeded_tick_simulation_parity_fixture_covers_short_and_medium_horizons() {
    let fixture_document = load_seeded_tick_simulation_parity_fixture()
        .expect("seeded tick simulation parity fixture should load");
    let observed_tags: BTreeSet<String> = fixture_document
        .cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter().cloned())
        .collect();

    assert!(observed_tags.contains("short_horizon"));
    assert!(observed_tags.contains("medium_horizon"));
}

#[test]
fn seeded_tick_simulation_parity_mismatch_diagnostics_are_actionable() {
    let loaded_calendar_fixture = load_calendar_regression_fixture_from_manifest()
        .expect("calendar fixture should load from shared manifest");
    let calendar_registry = ExchangeCalendarRegistry::from_artifact(
        loaded_calendar_fixture.fixture_document.calendar_registry,
    )
    .expect("calendar registry fixture should load");
    let fixture_document = load_seeded_tick_simulation_parity_fixture()
        .expect("seeded tick simulation parity fixture should load");
    let fixture_case = fixture_document
        .cases
        .first()
        .expect("fixture should include at least one case")
        .clone();
    let run_result = run_fixture_case(&calendar_registry, &fixture_case)
        .expect("fixture case run should complete");
    let projected_event_path = run_result.projected_event_path();
    let mut mutated_case = fixture_case.clone();
    let observed_first_side = projected_event_path
        .first()
        .map(|event_record| match event_record.side {
            SeededTickEventSide::Buy => "buy",
            SeededTickEventSide::Sell => "sell",
        })
        .expect("fixture case should include at least one event");
    mutated_case.expected_side_sequence[0] = if observed_first_side == "buy" {
        "sell".to_string()
    } else {
        "buy".to_string()
    };

    let mismatch_diagnostic = first_mismatch_diagnostic(&run_result, &mutated_case)
        .expect("mutated case should produce a mismatch diagnostic");

    assert!(mismatch_diagnostic.contains("fixture_id="));
    assert!(mismatch_diagnostic.contains("side_sequence mismatch at event_index=0"));
}

fn load_seeded_tick_simulation_parity_fixture()
-> Result<SeededTickSimulationParityFixtureDocument, String> {
    let fixture_path = fixtures_root_path()
        .join("simulation")
        .join("seeded_tick_simulation_parity_fixture.json");
    let fixture_payload = fs::read_to_string(&fixture_path).map_err(|source| {
        format!(
            "failed to read seeded tick simulation parity fixture `{}`: {source}",
            fixture_path.display()
        )
    })?;
    let fixture_document =
        serde_json::from_str::<SeededTickSimulationParityFixtureDocument>(&fixture_payload)
            .map_err(|source| {
                format!(
                    "failed to parse seeded tick simulation parity fixture `{}`: {source}",
                    fixture_path.display()
                )
            })?;

    validate_seeded_tick_simulation_parity_fixture(&fixture_document)?;

    Ok(fixture_document)
}

fn validate_seeded_tick_simulation_parity_fixture(
    fixture_document: &SeededTickSimulationParityFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported seeded tick simulation parity schema version: {}",
            fixture_document.schema_version
        ));
    }
    if fixture_document.fixture_set_id != SEEDED_TICK_SIMULATION_PARITY_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected seeded tick simulation parity fixture set id: {}",
            fixture_document.fixture_set_id
        ));
    }
    if fixture_document.determinism_metadata.fixture_loading_order
        != SEEDED_TICK_SIMULATION_CASE_ORDER_POLICY
    {
        return Err(format!(
            "unexpected seeded tick simulation parity case order policy: {}",
            fixture_document.determinism_metadata.fixture_loading_order
        ));
    }
    if fixture_document.determinism_metadata.event_ordering_policy
        != SEEDED_TICK_SIMULATION_EVENT_ORDERING_POLICY
    {
        return Err(format!(
            "unexpected seeded tick simulation parity event ordering policy: {}",
            fixture_document.determinism_metadata.event_ordering_policy
        ));
    }
    if fixture_document.determinism_metadata.content_hash_scope
        != SEEDED_TICK_SIMULATION_CONTENT_HASH_SCOPE_POLICY
    {
        return Err(format!(
            "unexpected seeded tick simulation parity content hash scope policy: {}",
            fixture_document.determinism_metadata.content_hash_scope
        ));
    }
    if fixture_document.determinism_metadata.content_hash_algorithm
        != support::validation_fixture::CONTENT_HASH_ALGORITHM_POLICY
    {
        return Err(format!(
            "unexpected seeded tick simulation parity content hash algorithm: {}",
            fixture_document.determinism_metadata.content_hash_algorithm
        ));
    }
    if fixture_document.cases.is_empty() {
        return Err(
            "seeded tick simulation parity fixture must include at least one case".to_string(),
        );
    }

    let mut observed_fixture_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_tags: BTreeSet<String> = BTreeSet::new();
    for fixture_case in &fixture_document.cases {
        if fixture_case.horizon_interval_count == 0 {
            return Err(format!(
                "fixture case {} has horizon_interval_count=0",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.seasonality_hourly_factors.len() != fixture_case.horizon_interval_count {
            return Err(format!(
                "fixture case {} seasonality_hourly_factors length must match horizon_interval_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.news_intensity_hourly_factors.len() != fixture_case.horizon_interval_count {
            return Err(format!(
                "fixture case {} news_intensity_hourly_factors length must match horizon_interval_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.expected_timestamps_utc_nanos.len()
            != fixture_case.expected_total_event_count
        {
            return Err(format!(
                "fixture case {} expected_total_event_count must match expected_timestamps_utc_nanos length",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.expected_side_sequence.len() != fixture_case.expected_total_event_count {
            return Err(format!(
                "fixture case {} expected_total_event_count must match expected_side_sequence length",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.expected_interval_event_counts.len() != fixture_case.horizon_interval_count
        {
            return Err(format!(
                "fixture case {} expected_interval_event_counts length must match horizon_interval_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.expected_interval_buy_counts.len() != fixture_case.horizon_interval_count {
            return Err(format!(
                "fixture case {} expected_interval_buy_counts length must match horizon_interval_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case.expected_interval_sell_counts.len() != fixture_case.horizon_interval_count {
            return Err(format!(
                "fixture case {} expected_interval_sell_counts length must match horizon_interval_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case
            .expected_interval_event_counts
            .iter()
            .sum::<usize>()
            != fixture_case.expected_total_event_count
        {
            return Err(format!(
                "fixture case {} expected_interval_event_counts sum must match expected_total_event_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case
            .expected_interval_buy_counts
            .iter()
            .sum::<usize>()
            + fixture_case
                .expected_interval_sell_counts
                .iter()
                .sum::<usize>()
            != fixture_case.expected_total_event_count
        {
            return Err(format!(
                "fixture case {} expected_interval_buy_counts plus expected_interval_sell_counts must match expected_total_event_count",
                fixture_case.fixture_id
            ));
        }
        if fixture_case
            .expected_side_sequence
            .iter()
            .any(|side| side != "buy" && side != "sell")
        {
            return Err(format!(
                "fixture case {} expected_side_sequence must only include buy or sell",
                fixture_case.fixture_id
            ));
        }
        if fixture_case
            .expected_timestamps_utc_nanos
            .windows(2)
            .any(|pair| pair[1] < pair[0])
        {
            return Err(format!(
                "fixture case {} expected_timestamps_utc_nanos must be non-decreasing",
                fixture_case.fixture_id
            ));
        }
        if !observed_fixture_ids.insert(fixture_case.fixture_id.clone()) {
            return Err(format!(
                "duplicate seeded tick simulation parity fixture_id: {}",
                fixture_case.fixture_id
            ));
        }
        observed_tags.extend(fixture_case.tags.iter().cloned());
    }

    let required_tags: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .cloned()
        .collect();
    if !required_tags.is_subset(&observed_tags) {
        return Err(
            "seeded tick simulation parity canonical required_tags must be present in case tags"
                .to_string(),
        );
    }

    let stable_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids
        .iter()
        .cloned()
        .collect();
    if stable_fixture_ids != observed_fixture_ids {
        return Err(
            "seeded tick simulation parity canonical stable_fixture_ids must match observed case ids"
                .to_string(),
        );
    }

    Ok(())
}

fn run_fixture_case(
    calendar_registry: &ExchangeCalendarRegistry,
    fixture_case: &SeededTickSimulationParityFixtureCase,
) -> Result<janiform::simulation::SeededTickSimulationRunResult, String> {
    let range_start_utc = parse_utc_timestamp(&fixture_case.range_start_utc)?;
    let range_end_utc = parse_utc_timestamp(&fixture_case.range_end_utc)?;

    let hourly_factor_contract = build_hourly_factor_contract(
        calendar_registry,
        &fixture_case.exchange_code,
        range_start_utc,
        range_end_utc,
        ONE_HOUR_NANOS,
        &fixture_case.seasonality_hourly_factors,
        &fixture_case.news_intensity_hourly_factors,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
        DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD,
    )
    .map_err(|source| format!("failed to build hourly factor contract: {source}"))?;

    let kernel_parameters = HawkesKernelParameters::try_new(
        fixture_case.kernel_parameters.lambda_bar,
        fixture_case.kernel_parameters.beta,
        to_kernel_loadings(&fixture_case.kernel_parameters.alpha),
        to_kernel_loadings(&fixture_case.kernel_parameters.rho),
    )
    .map_err(|source| format!("invalid kernel parameters in fixture case: {source}"))?;

    let run_config = SeededTickSimulationRunConfig::try_new(
        fixture_case.seed,
        range_start_utc,
        fixture_case.horizon_interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters,
            initial_excitation_state: HawkesExcitationState {
                y_plus_plus: fixture_case.initial_excitation_state.y_plus_plus,
                y_plus_minus: fixture_case.initial_excitation_state.y_plus_minus,
                y_minus_plus: fixture_case.initial_excitation_state.y_minus_plus,
                y_minus_minus: fixture_case.initial_excitation_state.y_minus_minus,
            },
            hourly_factor_contract,
            calendar_registry: calendar_registry.clone(),
            nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
            news_process_configuration: build_news_process_configuration(),
        },
        None,
    )
    .map_err(|source| format!("invalid seeded simulation run config: {source}"))?;

    run_seeded_tick_event_simulation(run_config)
        .map_err(|source| format!("seeded simulation run failed: {source}"))
}

fn first_mismatch_diagnostic(
    run_result: &janiform::simulation::SeededTickSimulationRunResult,
    fixture_case: &SeededTickSimulationParityFixtureCase,
) -> Option<String> {
    let projected_event_path = run_result.projected_event_path();
    let observed_timestamps: Vec<i64> = projected_event_path
        .iter()
        .map(|event_record| event_record.timestamp_utc_nanos)
        .collect();
    let observed_sides: Vec<String> = projected_event_path
        .iter()
        .map(|event_record| map_side_to_label(event_record.side).to_string())
        .collect();

    if observed_timestamps.len() != fixture_case.expected_total_event_count {
        return Some(format!(
            "fixture_id={} total_event_count mismatch expected={} observed={}",
            fixture_case.fixture_id,
            fixture_case.expected_total_event_count,
            observed_timestamps.len()
        ));
    }

    if observed_timestamps != fixture_case.expected_timestamps_utc_nanos {
        for (event_index, (observed_timestamp, expected_timestamp)) in observed_timestamps
            .iter()
            .zip(fixture_case.expected_timestamps_utc_nanos.iter())
            .enumerate()
        {
            if observed_timestamp != expected_timestamp {
                return Some(format!(
                    "fixture_id={} timestamp mismatch at event_index={} expected={} observed={}",
                    fixture_case.fixture_id, event_index, expected_timestamp, observed_timestamp
                ));
            }
        }
        return Some(format!(
            "fixture_id={} timestamp vector length mismatch after comparison",
            fixture_case.fixture_id
        ));
    }

    if observed_sides != fixture_case.expected_side_sequence {
        for (event_index, (observed_side, expected_side)) in observed_sides
            .iter()
            .zip(fixture_case.expected_side_sequence.iter())
            .enumerate()
        {
            if observed_side != expected_side {
                return Some(format!(
                    "fixture_id={} side_sequence mismatch at event_index={} expected={} observed={}",
                    fixture_case.fixture_id, event_index, expected_side, observed_side
                ));
            }
        }
        return Some(format!(
            "fixture_id={} side sequence length mismatch after comparison",
            fixture_case.fixture_id
        ));
    }

    let (interval_event_counts, interval_buy_counts, interval_sell_counts) =
        match summarize_counts_by_interval(
            &observed_timestamps,
            &observed_sides,
            &fixture_case.range_start_utc,
            fixture_case.horizon_interval_count,
        ) {
            Ok(summary) => summary,
            Err(source) => {
                return Some(format!(
                    "fixture_id={} failed to summarize interval counts: {source}",
                    fixture_case.fixture_id
                ));
            }
        };

    if interval_event_counts != fixture_case.expected_interval_event_counts {
        return Some(format!(
            "fixture_id={} interval_event_counts mismatch expected={:?} observed={:?}",
            fixture_case.fixture_id,
            fixture_case.expected_interval_event_counts,
            interval_event_counts
        ));
    }
    if interval_buy_counts != fixture_case.expected_interval_buy_counts {
        return Some(format!(
            "fixture_id={} interval_buy_counts mismatch expected={:?} observed={:?}",
            fixture_case.fixture_id, fixture_case.expected_interval_buy_counts, interval_buy_counts
        ));
    }
    if interval_sell_counts != fixture_case.expected_interval_sell_counts {
        return Some(format!(
            "fixture_id={} interval_sell_counts mismatch expected={:?} observed={:?}",
            fixture_case.fixture_id,
            fixture_case.expected_interval_sell_counts,
            interval_sell_counts
        ));
    }

    let min_positive_gap_nanos = minimum_positive_gap_nanos(&observed_timestamps);
    if min_positive_gap_nanos != fixture_case.expected_min_positive_timestamp_gap_nanos {
        return Some(format!(
            "fixture_id={} minimum positive timestamp gap mismatch expected={} observed={}",
            fixture_case.fixture_id,
            fixture_case.expected_min_positive_timestamp_gap_nanos,
            min_positive_gap_nanos
        ));
    }

    let has_non_grid_arrival = match has_non_grid_arrival(
        &observed_timestamps,
        &fixture_case.range_start_utc,
        fixture_case.horizon_interval_count,
    ) {
        Ok(value) => value,
        Err(source) => {
            return Some(format!(
                "fixture_id={} failed to evaluate non-grid arrival coverage: {source}",
                fixture_case.fixture_id
            ));
        }
    };
    if has_non_grid_arrival != fixture_case.expected_has_non_grid_arrival {
        return Some(format!(
            "fixture_id={} non-grid arrival mismatch expected={} observed={}",
            fixture_case.fixture_id,
            fixture_case.expected_has_non_grid_arrival,
            has_non_grid_arrival
        ));
    }

    None
}

fn summarize_counts_by_interval(
    observed_timestamps: &[i64],
    observed_sides: &[String],
    range_start_utc: &str,
    horizon_interval_count: usize,
) -> Result<(Vec<usize>, Vec<usize>, Vec<usize>), String> {
    let mut interval_event_counts = vec![0usize; horizon_interval_count];
    let mut interval_buy_counts = vec![0usize; horizon_interval_count];
    let mut interval_sell_counts = vec![0usize; horizon_interval_count];
    let range_start_utc_nanos = parse_utc_timestamp(range_start_utc)?
        .timestamp_nanos_opt()
        .ok_or_else(|| "range_start_utc cannot be represented in nanoseconds".to_string())?;

    for (timestamp_utc_nanos, side_label) in observed_timestamps.iter().zip(observed_sides.iter()) {
        let interval_offset = *timestamp_utc_nanos - range_start_utc_nanos;
        let interval_index = interval_offset / ONE_HOUR_NANOS;
        if interval_index < 0 || interval_index >= horizon_interval_count as i64 {
            return Err(format!(
                "timestamp {} is outside configured interval range",
                timestamp_utc_nanos
            ));
        }

        let interval_index = interval_index as usize;
        interval_event_counts[interval_index] += 1;
        match side_label.as_str() {
            "buy" => interval_buy_counts[interval_index] += 1,
            "sell" => interval_sell_counts[interval_index] += 1,
            _ => {
                return Err(format!(
                    "unsupported side label `{side_label}` in observed side sequence"
                ));
            }
        }
    }

    Ok((
        interval_event_counts,
        interval_buy_counts,
        interval_sell_counts,
    ))
}

fn minimum_positive_gap_nanos(observed_timestamps: &[i64]) -> i64 {
    observed_timestamps
        .windows(2)
        .filter_map(|pair| {
            let gap = pair[1] - pair[0];
            (gap > 0).then_some(gap)
        })
        .min()
        .unwrap_or(0)
}

fn has_non_grid_arrival(
    observed_timestamps: &[i64],
    range_start_utc: &str,
    horizon_interval_count: usize,
) -> Result<bool, String> {
    let range_start_utc_nanos = parse_utc_timestamp(range_start_utc)?
        .timestamp_nanos_opt()
        .ok_or_else(|| "range_start_utc cannot be represented in nanoseconds".to_string())?;
    let interval_boundaries: BTreeSet<i64> = (0..=horizon_interval_count)
        .map(|interval_index| range_start_utc_nanos + interval_index as i64 * ONE_HOUR_NANOS)
        .collect();

    Ok(observed_timestamps
        .iter()
        .any(|timestamp_utc_nanos| !interval_boundaries.contains(timestamp_utc_nanos)))
}

fn to_kernel_loadings(
    fixture_loadings: &SeededTickSimulationParityFixtureKernelLoadings,
) -> HawkesKernelLoadings {
    HawkesKernelLoadings {
        plus_plus: fixture_loadings.plus_plus,
        plus_minus: fixture_loadings.plus_minus,
        minus_plus: fixture_loadings.minus_plus,
        minus_minus: fixture_loadings.minus_minus,
    }
}

fn map_side_to_label(side: SeededTickEventSide) -> &'static str {
    match side {
        SeededTickEventSide::Buy => "buy",
        SeededTickEventSide::Sell => "sell",
    }
}

fn valid_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration {
    NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("nig configuration should construct")
}

fn build_news_process_configuration() -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: Vec::new(),
            },
            surprise: SurpriseNewsArrivalConfiguration {
                baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma severity configuration should construct"),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.4, 1.3, 1.1,
            )
            .expect("news base shock configuration should construct"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.6,
            return_jump_scale: 1.2,
        },
    )
    .expect("news process configuration should construct")
}

fn parse_utc_timestamp(raw_timestamp: &str) -> Result<DateTime<Utc>, String> {
    let normalized_timestamp = raw_timestamp
        .trim()
        .strip_suffix('Z')
        .map(|timestamp_without_z| format!("{timestamp_without_z}+00:00"))
        .unwrap_or_else(|| raw_timestamp.trim().to_string());

    DateTime::parse_from_rfc3339(&normalized_timestamp)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|source| format!("failed to parse UTC timestamp `{raw_timestamp}`: {source}"))
}
