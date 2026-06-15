use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors, SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY,
    SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY, SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_DETERMINISM_POLICY, SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY,
    SEEDED_TICK_EVENT_ORDERING_POLICY, SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY,
    SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY,
    SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION, SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY,
    SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION,
    SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY, SeededFinePathTransitionKind,
    SeededTickEventDebugTraceTransitionKind, SeededTickEventSide,
    SeededTickEventStateSnapshotCapture, SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationNewsEventMarketState,
    SeededTickSimulationRunConfig, SeededTickSimulationRunError, SeededTickSimulationRunOptions,
    SeededTickSimulationRunResult, SimulationProgressEvent, SimulationProgressPhase,
    run_seeded_tick_event_simulation as run_seeded_tick_event_simulation_core,
    run_seeded_tick_event_simulation_with_observer as run_seeded_tick_event_simulation_with_observer_core,
    run_seeded_tick_event_simulation_with_randomness_factory as run_seeded_tick_event_simulation_with_randomness_factory_core,
    sample_nig_trading_time_increment, sample_nig_trading_time_increment_with_edge_mode,
};
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::randomness::{
    SimulationRandomnessFactory, SimulationRandomnessStream, SimulationRandomnessSubstreamDomain,
    SplitMix64SimulationRandomnessStream, derive_simulation_substream_seed,
};

fn run_seeded_tick_event_simulation(
    run_config: SeededTickSimulationRunConfig,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    run_seeded_tick_event_simulation_core(run_config, CancellationToken::uncancelled())
}

fn run_seeded_tick_event_simulation_with_observer(
    run_config: SeededTickSimulationRunConfig,
    observer: &mut dyn FnMut(SimulationProgressEvent),
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError> {
    run_seeded_tick_event_simulation_with_observer_core(
        run_config,
        CancellationToken::uncancelled(),
        observer,
    )
}

fn run_seeded_tick_event_simulation_with_randomness_factory<F>(
    run_config: SeededTickSimulationRunConfig,
    randomness_factory: &F,
) -> Result<SeededTickSimulationRunResult, SeededTickSimulationRunError>
where
    F: SimulationRandomnessFactory,
{
    run_seeded_tick_event_simulation_with_randomness_factory_core(
        run_config,
        CancellationToken::uncancelled(),
        randomness_factory,
    )
}
use janiform_foundations::time::NANOS_PER_HOUR;
use janiform_market_time::calendar::{
    ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader, MarketState,
};
use janiform_market_time::interval_factor_grids::{
    DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD,
    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD, INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
    INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY, INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
    INTERVAL_FACTOR_GRID_SCHEMA_VERSION, INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
    INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY, IntervalFactorGrid,
    IntervalFactorGridDiagnostics, IntervalFactorGridMetadata,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters, HawkesTransitionStep,
};
use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalComponent,
    NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
    NewsReturnJumpRoutingTarget, SharedNewsAccumulatorState, SharedNewsMarkLawConfiguration,
    SurpriseNewsArrivalConfiguration, dispatch_shared_news_events_with_resolved_market_states,
};
use janiform_model_kernels::nig::{
    NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY, NigSharedPlusShiftConfiguration,
};
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
};

const SEEDED_TEST_START_UTC_NANOS: i64 = 1_705_932_000_000_000_000;
const EVENT_TAPE_RECORD_FIELDS_WITHOUT_SNAPSHOT: [&str; 9] = [
    "anchor_value",
    "core_total_return_increment",
    "deviation_from_anchor",
    "event_index",
    "latent_event_time_utc_nanos",
    "observed_log_price",
    "observed_timestamp_utc_nanos",
    "pressure_state",
    "side",
];

#[test]
fn trading_time_assembler_buy_only_edge_mode_keeps_sell_and_gap_components_zero() {
    let driver_parameters = valid_nig_driver_parameters();

    let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.7,
        open_jump: 0.0,
        closed_gap: -0.2,
    })
    .expect("buy-only decomposition should be valid");

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_101);

    let contributions = sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        decomposition,
        ReturnDecompositionEdgeMode::BuyOnly,
        &mut randomness_stream,
    )
    .expect("buy-only assembly should sample successfully");

    assert_eq!(
        contributions.policy,
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
    );
    assert_float_close(contributions.sell_side_increment, 0.0);
    assert_float_close(
        contributions.combined_increment,
        contributions.buy_side_increment,
    );
}

#[test]
fn trading_time_assembler_sell_only_edge_mode_applies_minus_sign_to_combined_increment() {
    let driver_parameters = valid_nig_driver_parameters();

    let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.6,
        a_minus: 0.9,
        open_jump: 0.0,
        closed_gap: -0.1,
    })
    .expect("sell-only decomposition should be valid");

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_102);

    let contributions = sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        decomposition,
        ReturnDecompositionEdgeMode::SellOnly,
        &mut randomness_stream,
    )
    .expect("sell-only assembly should sample successfully");

    assert_eq!(
        contributions.policy,
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
    );
    assert_float_close(contributions.buy_side_increment, 0.0);
    assert_float_close(
        contributions.combined_increment,
        -contributions.sell_side_increment,
    );
}

#[test]
fn trading_time_assembler_gap_only_edge_mode_zeroes_trading_components() {
    let driver_parameters = valid_nig_driver_parameters();

    let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.8,
        open_jump: 0.0,
        closed_gap: -0.3,
    })
    .expect("gap-only decomposition should be valid");

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(42_104);

    let contributions = sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        decomposition,
        ReturnDecompositionEdgeMode::ClosedGapOnly,
        &mut randomness_stream,
    )
    .expect("gap-only assembly should sample successfully");

    assert_eq!(
        contributions.policy,
        NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
    );
    assert_float_close(contributions.buy_side_increment, 0.0);
    assert_float_close(contributions.sell_side_increment, 0.0);
    assert_float_close(contributions.combined_increment, 0.0);
}

#[test]
fn trading_time_assembler_is_seed_deterministic() {
    let driver_parameters = valid_nig_driver_parameters();

    let decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.8,
        a_minus: 0.5,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("mixed decomposition should be valid");

    let mut first_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);
    let mut second_stream = SplitMix64SimulationRandomnessStream::from_seed(42_103);

    let first_contributions =
        sample_nig_trading_time_increment(driver_parameters, decomposition, &mut first_stream)
            .expect("first seeded assembly should sample successfully");

    let second_contributions =
        sample_nig_trading_time_increment(driver_parameters, decomposition, &mut second_stream)
            .expect("second seeded assembly should sample successfully");

    assert_eq!(first_contributions, second_contributions);
}

fn valid_nig_driver_parameters() -> janiform_model_kernels::nig::NigBuySellDriverParameters {
    valid_nig_shared_plus_shift_configuration()
        .derive_driver_parameters()
        .expect("test shared-plus-shift payload should derive drivers")
}

fn valid_nig_shared_plus_shift_configuration() -> NigSharedPlusShiftConfiguration {
    NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("test shared-plus-shift payload should construct")
}

fn build_news_process_configuration(
    known_news_timestamps_utc_nanos: Vec<i64>,
) -> NewsProcessConfiguration {
    NewsProcessConfiguration::try_new(
        NewsArrivalConfiguration {
            known: KnownNewsArrivalConfiguration {
                scheduled_event_timestamps_utc_nanos: known_news_timestamps_utc_nanos,
            },
            surprise: SurpriseNewsArrivalConfiguration {
                open_baseline_intensity: 0.0,
                closed_baseline_intensity: 0.0,
            },
        },
        SharedNewsMarkLawConfiguration {
            gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.5)
                .expect("news gamma severity configuration should construct"),
        },
        default_news_component_mark_scales(),
        default_news_component_mark_scales(),
    )
    .expect("news process configuration should construct")
}

fn default_news_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.6,
        return_jump_drift: 0.0,
        return_jump_volatility: 1.2,
    }
}

fn sample_expected_shared_news_marks(
    randomness_stream: &mut dyn SimulationRandomnessStream,
) -> (f64, f64) {
    let sampled_mark_payload = build_news_process_configuration(Vec::new())
        .default_mark_pipeline()
        .sample_gamma_normal_marks(default_news_component_mark_scales(), randomness_stream)
        .expect("shared news mark sampling should succeed for test configuration");

    (
        sampled_mark_payload.shared_severity_mark,
        sampled_mark_payload.shared_return_shock_mark,
    )
}

fn expected_gamma_normal_return_jump(
    mark_scales: NewsComponentMarkScaleConfiguration,
    shared_severity_mark: f64,
    shared_return_shock_mark: f64,
) -> f64 {
    mark_scales.return_jump_drift * shared_severity_mark
        + mark_scales.return_jump_volatility
            * shared_severity_mark.sqrt()
            * shared_return_shock_mark
}

fn set_known_news_timestamps(
    model_inputs: &mut SeededTickSimulationModelInputs,
    known_news_timestamps_utc_nanos: Vec<i64>,
) {
    model_inputs
        .news_process_configuration
        .arrivals
        .known
        .scheduled_event_timestamps_utc_nanos = known_news_timestamps_utc_nanos;
}

#[test]
fn seeded_simulation_run_executes_with_valid_config() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        9_101,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("valid run config should initialize");

    let first_result = run_seeded_tick_event_simulation(run_config.clone())
        .expect("valid run should produce deterministic output");

    let second_result = run_seeded_tick_event_simulation(run_config)
        .expect("same run config should produce deterministic output");

    assert_eq!(first_result, second_result);
    assert_eq!(
        first_result.schema_version,
        SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
    );
    assert_eq!(
        first_result.determinism_policy,
        SEEDED_TICK_EVENT_DETERMINISM_POLICY
    );
    assert_eq!(
        first_result.event_ordering_policy,
        SEEDED_TICK_EVENT_ORDERING_POLICY
    );
    assert_eq!(
        first_result.randomness_stream_policy,
        SEEDED_TICK_EVENT_RANDOMNESS_STREAM_POLICY
    );
    assert_eq!(
        first_result.randomness_substream_derivation_policy,
        SEEDED_TICK_EVENT_RANDOMNESS_SUBSTREAM_DERIVATION_POLICY
    );
    assert_eq!(first_result.seed, 9_101);
    assert_eq!(first_result.simulated_interval_count, 2);
    assert!(first_result.debug_trace_enabled);
    assert!(first_result.event_count() > 0);
    assert!(is_non_decreasing_timestamp_path(&first_result));
}

#[test]
fn seeded_simulation_progress_observer_reports_exact_interval_completion() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        9_101,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("seeded simulation config should build");

    let mut observed_progress_events: Vec<SimulationProgressEvent> = Vec::new();

    let run_result = run_seeded_tick_event_simulation_with_observer(run_config, &mut |event| {
        observed_progress_events.push(event);
    })
    .expect("seeded simulation with observer should succeed");

    assert_eq!(run_result.simulated_interval_count, 2);

    let observed_interval_progress = observed_progress_events
        .iter()
        .map(|event| {
            (
                event.phase,
                event.completed_units,
                event.total_units,
                event.interval_duration_nanos,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        observed_interval_progress,
        vec![
            (
                SimulationProgressPhase::SeededTickIntervals,
                0,
                2,
                NANOS_PER_HOUR,
            ),
            (
                SimulationProgressPhase::SeededTickIntervals,
                1,
                2,
                NANOS_PER_HOUR,
            ),
            (
                SimulationProgressPhase::SeededTickIntervals,
                2,
                2,
                NANOS_PER_HOUR,
            ),
        ]
    );
    assert_eq!(
        observed_progress_events
            .first()
            .and_then(|event| event.generated_tick_count),
        Some(0)
    );
    assert_eq!(
        observed_progress_events
            .last()
            .and_then(|event| event.generated_tick_count),
        u64::try_from(run_result.event_count()).ok()
    );
}

#[test]
fn seeded_simulation_run_diverges_with_different_seeds() {
    let first_run_config = SeededTickSimulationRunConfig::try_new(
        9_101,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        None,
    )
    .expect("first run config should initialize");

    let second_run_config = SeededTickSimulationRunConfig::try_new(
        9_102,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        None,
    )
    .expect("second run config should initialize");

    let first_result = run_seeded_tick_event_simulation(first_run_config)
        .expect("first deterministic run should complete");

    let second_result = run_seeded_tick_event_simulation(second_run_config)
        .expect("second deterministic run should complete");

    assert_ne!(
        first_result.projected_event_path(),
        second_result.projected_event_path()
    );
}

#[test]
fn seeded_simulation_run_is_independent_from_debug_trace_flag() {
    let run_config_without_debug = SeededTickSimulationRunConfig::try_new(
        8_001,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("run config without debug trace should initialize");

    let run_config_with_debug = SeededTickSimulationRunConfig::try_new(
        8_001,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("run config with debug trace should initialize");

    let result_without_debug = run_seeded_tick_event_simulation(run_config_without_debug)
        .expect("run without debug trace should complete");

    let result_with_debug = run_seeded_tick_event_simulation(run_config_with_debug)
        .expect("run with debug trace should complete");

    assert_eq!(
        result_without_debug.projected_event_path(),
        result_with_debug.projected_event_path()
    );
    assert_eq!(
        result_without_debug.final_excitation_state,
        result_with_debug.final_excitation_state
    );
    assert!(!result_without_debug.debug_trace_enabled);
    assert!(result_with_debug.debug_trace_enabled);
    assert!(result_without_debug.debug_trace_output.is_none());
    assert!(
        result_with_debug
            .debug_trace_output
            .as_ref()
            .is_some_and(|debug_trace_output| debug_trace_output.captured_transition_count >= 2)
    );
}

#[test]
fn seeded_simulation_run_rejects_invalid_debug_trace_sampling_stride() {
    let error = SeededTickSimulationRunConfig::try_new(
        8_011,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(0.8, 0.8, 0.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 0,
                max_records_per_chunk: 128,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect_err("debug trace with zero sampling stride must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::InvalidDebugTraceSamplingStride { value: 0 }
    ));
}

#[test]
fn seeded_simulation_run_rejects_invalid_debug_trace_chunk_size() {
    let error = SeededTickSimulationRunConfig::try_new(
        8_012,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(0.8, 0.8, 0.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 0,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect_err("debug trace with zero chunk size must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::InvalidDebugTraceChunkSize { value: 0 }
    ));
}

#[test]
fn seeded_simulation_debug_trace_schema_exposes_required_fields() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        8_013,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 1.0, 0.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 1,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("debug trace schema run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.25],
        sell_timestamp_draws: vec![0.5],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("debug trace schema run should complete");

    let serialized_debug_trace = run_result
        .serialize_debug_trace_json()
        .expect("debug trace serialization should succeed")
        .expect("debug trace payload should exist when debug mode is enabled");

    let debug_trace_payload: Value =
        serde_json::from_str(&serialized_debug_trace).expect("debug trace JSON should parse");

    assert_eq!(
        debug_trace_payload["schema_version"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_SCHEMA_VERSION
    );
    assert_eq!(
        debug_trace_payload["metadata"]["schema_version"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_METADATA_SCHEMA_VERSION
    );
    assert_eq!(
        debug_trace_payload["metadata"]["trace_record_schema_version"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_RECORD_SCHEMA_VERSION
    );
    assert_eq!(
        debug_trace_payload["metadata"]["trace_chunk_schema_version"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNK_SCHEMA_VERSION
    );
    assert_eq!(
        debug_trace_payload["metadata"]["news_market_state_routing_policy"],
        SEEDED_TICK_EVENT_NEWS_MARKET_STATE_ROUTING_POLICY
    );
    assert_eq!(
        debug_trace_payload["metadata"]["sampling_policy"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_SAMPLING_POLICY
    );
    assert_eq!(
        debug_trace_payload["metadata"]["chunking_policy"],
        SEEDED_TICK_EVENT_DEBUG_TRACE_CHUNKING_POLICY
    );
    assert_eq!(
        debug_trace_payload["metadata"]["sample_every_nth_transition"],
        1
    );
    assert_eq!(debug_trace_payload["metadata"]["max_records_per_chunk"], 1);
    assert_eq!(
        debug_trace_payload["metadata"]["interval_boundary_drift_always_captured"],
        true
    );

    let trace_chunks = debug_trace_payload["trace_chunks"]
        .as_array()
        .expect("trace_chunks should serialize as an array");

    assert!(!trace_chunks.is_empty());

    let first_chunk = trace_chunks[0]
        .as_object()
        .expect("trace chunk should serialize as an object");

    for required_chunk_field in [
        "schema_version",
        "chunk_index",
        "start_transition_index",
        "end_transition_index",
        "records",
    ] {
        assert!(first_chunk.contains_key(required_chunk_field));
    }

    let first_record = first_chunk["records"][0]
        .as_object()
        .expect("trace record should serialize as an object");

    for required_record_field in [
        "transition_index",
        "interval_index",
        "previous_latent_transition_time_utc_nanos",
        "observed_timestamp_utc_nanos",
        "elapsed_hours_since_previous_latent_update",
        "transition_kind",
        "cumulative_news_activity_coupling_j_t_increment",
        "cumulative_open_window_return_jump_m_t_increment",
        "cumulative_closed_window_gap_g_t_increment",
        "seasonality_factor",
        "interval_news_intensity",
        "state_before_transition",
        "state_after_transition",
    ] {
        assert!(first_record.contains_key(required_record_field));
    }
}

#[test]
fn seeded_simulation_debug_trace_captures_transition_ordering_and_carryover_context() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;

    let run_config = SeededTickSimulationRunConfig::try_new(
        8_014,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        1,
        one_interval_model_inputs(1.0, 1.0, 1.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 8,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("debug trace correctness run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0, 0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.99, 0.0],
        sell_timestamp_draws: vec![0.0, 0.0],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![0.3],
        news_severity_draws: vec![0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.25, 0.5],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("debug trace correctness run should complete");

    let debug_trace_output = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist");

    let trace_records: Vec<_> = debug_trace_output
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .collect();

    assert!(trace_records.len() >= 3);
    assert_eq!(
        trace_records[0].transition_kind,
        SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent
    );
    assert_eq!(
        trace_records
            .last()
            .expect("trace record set should include a boundary drift transition")
            .transition_kind,
        SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
    );

    let expected_interval_end_utc_nanos = start_utc_nanos + NANOS_PER_HOUR;

    assert_eq!(
        trace_records
            .last()
            .expect("trace record set should include interval-end drift")
            .observed_timestamp_utc_nanos,
        expected_interval_end_utc_nanos
    );

    assert_eq!(trace_records[0].generation_order, Some(0));
    assert_eq!(trace_records[0].event_index, Some(0));
    assert_eq!(
        trace_records
            .last()
            .expect("trace record set should include interval-end drift")
            .event_index,
        None
    );

    let exogenous_news_record = trace_records
        .iter()
        .find(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .expect("trace should include one exogenous news transition");

    let mut expected_severity_stream = ScriptedRandomnessStream::new(vec![0.5, 0.25, 0.5]);

    let (expected_shared_severity_mark, _) =
        sample_expected_shared_news_marks(&mut expected_severity_stream);

    let expected_activity_coupling_mark = default_news_component_mark_scales()
        .activity_coupling_scale
        * expected_shared_severity_mark;

    assert_float_close(
        exogenous_news_record
            .exogenous_news_activity_coupling_mark
            .expect("exogenous transition should include news activity-coupling mark"),
        expected_activity_coupling_mark,
    );
    assert_eq!(
        exogenous_news_record.exogenous_news_arrival_component,
        Some(NewsArrivalComponent::Surprise)
    );
    assert_eq!(
        exogenous_news_record.exogenous_news_market_state,
        Some(SeededTickSimulationNewsEventMarketState::Open)
    );
    assert_eq!(
        exogenous_news_record.exogenous_news_return_jump_routing_target,
        Some(NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM)
    );
    assert!(
        trace_records[0]
            .exogenous_news_activity_coupling_mark
            .is_none()
    );

    for trace_window in trace_records.windows(2) {
        assert_state_snapshot_close(
            trace_window[0].state_after_transition,
            trace_window[1].state_before_transition,
        );
    }

    let projected_event_path = run_result.projected_event_path();

    assert_eq!(projected_event_path.len(), 1);
    assert_eq!(projected_event_path[0].event_index, 0);
    assert_eq!(projected_event_path[0].side, SeededTickEventSide::Buy);
}

#[test]
fn seeded_simulation_suppresses_endogenous_events_while_market_is_closed() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        8_014,
        DateTime::<Utc>::from_timestamp_nanos(1_705_928_400_000_000_000),
        1,
        one_interval_closed_market_model_inputs(),
        None,
    )
    .expect("closed-market run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.1, 0.2, 0.3],
        sell_timestamp_draws: vec![0.0, 0.0, 0.0],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("closed-market run should complete");

    assert_eq!(run_result.event_count(), 0);
}

#[test]
fn seeded_simulation_hides_closed_boundary_drift_from_public_fine_path() {
    let start_utc_nanos = 1_705_924_800_000_000_000;

    let mut model_inputs = two_interval_zero_event_carryover_model_inputs();

    model_inputs.interval_factor_grid.request_start_utc_nanos = start_utc_nanos;
    model_inputs.interval_factor_grid.request_end_utc_nanos = start_utc_nanos + 2 * NANOS_PER_HOUR;

    model_inputs.interval_factor_grid.interval_start_utc_nanos =
        vec![start_utc_nanos, start_utc_nanos + NANOS_PER_HOUR];

    model_inputs.interval_factor_grid.interval_end_utc_nanos = vec![
        start_utc_nanos + NANOS_PER_HOUR,
        start_utc_nanos + 2 * NANOS_PER_HOUR,
    ];

    model_inputs
        .interval_factor_grid
        .interval_start_exchange_local = vec![
        "2024-01-22T07:00:00-05:00".to_string(),
        "2024-01-22T08:00:00-05:00".to_string(),
    ];

    model_inputs
        .interval_factor_grid
        .interval_end_exchange_local = vec![
        "2024-01-22T08:00:00-05:00".to_string(),
        "2024-01-22T09:00:00-05:00".to_string(),
    ];

    model_inputs
        .interval_factor_grid
        .diagnostics
        .first_interval_start_utc_nanos = start_utc_nanos;

    model_inputs
        .interval_factor_grid
        .diagnostics
        .last_interval_end_utc_nanos = start_utc_nanos + 2 * NANOS_PER_HOUR;

    let run_config = SeededTickSimulationRunConfig::try_new(
        8_115,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        model_inputs,
        None,
    )
    .expect("two-interval closed-window run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0, 0.0],
        sell_count_draws: vec![0.0, 0.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0, 0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("two-interval closed-window run should complete");

    assert_eq!(run_result.event_count(), 0);
    assert_eq!(run_result.public_transition_count(), 1);

    let public_fine_path =
        run_result.public_fine_path_chunk(0, run_result.public_transition_count());
    assert_eq!(public_fine_path.len(), 1);
    assert_eq!(
        public_fine_path[0].transition_kind,
        SeededFinePathTransitionKind::IntervalBoundaryDrift
    );
    assert_eq!(
        public_fine_path[0].market_state,
        SeededTickSimulationNewsEventMarketState::Open
    );
    assert_eq!(
        public_fine_path[0].observed_timestamp_utc_nanos,
        start_utc_nanos + 2 * NANOS_PER_HOUR
    );
}

#[test]
fn exogenous_interval_news_scheduler_routes_surprise_news_into_closed_market_gap_channel() {
    let closed_interval_start_utc_nanos = 1_705_928_400_000_000_000;

    let mut model_inputs = one_interval_model_inputs(0.0, 0.0, 0.0);

    override_interval_factor_grid(
        &mut model_inputs,
        &[closed_interval_start_utc_nanos],
        &[closed_interval_start_utc_nanos + NANOS_PER_HOUR],
        &[1.0],
        &[1.0],
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        8_016,
        DateTime::<Utc>::from_timestamp_nanos(closed_interval_start_utc_nanos),
        1,
        model_inputs,
        None,
    )
    .expect("closed-market surprise-news run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![1.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![0.25],
        news_severity_draws: vec![0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.25, 0.5],
    });

    let event_inputs = super::runtime::build_interval_news_event_inputs(
        super::runtime::IntervalNewsEventInputBuildInput {
            interval_index: 0,
            interval_start_utc_nanos: closed_interval_start_utc_nanos,
            interval_duration_nanos: NANOS_PER_HOUR,
            interval_duration_in_hours: 1.0,
            interval_news_intensity: 1.0,
            run_config: &run_config,
            interval_known_news_event_timestamps_utc_nanos: &[],
        },
        &scripted_factory,
    )
    .expect("closed-market surprise-news input generation should complete");

    assert_eq!(event_inputs.len(), 1);
    assert_eq!(
        event_inputs[0].news_arrival_component,
        NewsArrivalComponent::Surprise
    );

    let dispatch_output = dispatch_shared_news_events_with_resolved_market_states(
        &run_config.model_inputs.calendar_registry,
        run_config
            .model_inputs
            .interval_factor_grid
            .exchange_code
            .as_str(),
        SharedNewsAccumulatorState::new(0.0, 0.0, 0.0),
        event_inputs.as_slice(),
    )
    .expect("closed-market surprise-news dispatch should complete");

    assert_eq!(dispatch_output.records.len(), 1);
    assert_eq!(dispatch_output.records[0].market_state, MarketState::Closed);
    assert_eq!(
        dispatch_output.records[0].return_jump_routing_target,
        NewsReturnJumpRoutingTarget::ClosedWindowGapG
    );
    assert_float_close(dispatch_output.records[0].m_t_increment, 0.0);
    assert_ne!(dispatch_output.records[0].g_t_increment, 0.0);
    assert!(dispatch_output.records[0].j_t_increment > 0.0);
}

#[test]
fn seeded_simulation_debug_trace_sampling_and_chunking_controls_limit_payload_size() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        8_015,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(3.0, 3.0, 0.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 2,
                max_records_per_chunk: 2,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("debug trace sampling and chunking run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.1, 0.2, 0.3],
        sell_timestamp_draws: vec![0.4, 0.5, 0.6],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("debug trace sampling and chunking run should complete");

    let debug_trace_output = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist");

    assert!(debug_trace_output.captured_transition_count >= 2);
    assert!(!debug_trace_output.trace_chunks.is_empty());
    assert!(
        debug_trace_output
            .trace_chunks
            .iter()
            .all(|trace_chunk| trace_chunk.records.len() <= 2)
    );

    let sampled_transition_indices: Vec<u64> = debug_trace_output
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .map(|trace_record| trace_record.transition_index)
        .collect();

    assert!(
        sampled_transition_indices
            .windows(2)
            .all(|window| window[0] < window[1])
    );
    assert!(
        debug_trace_output
            .trace_chunks
            .iter()
            .flat_map(|trace_chunk| trace_chunk.records.iter())
            .any(|trace_record| {
                trace_record.transition_kind
                    == SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift
            })
    );
}

#[test]
fn seeded_simulation_run_accepts_injected_randomness_factory() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        5_003,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        None,
    )
    .expect("run config should initialize");

    let fixture_factory = FixtureRandomnessFactory;

    let first_result = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config.clone(),
        &fixture_factory,
    )
    .expect("injected fixture randomness run should complete");

    let second_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &fixture_factory)
            .expect("second injected fixture randomness run should complete");

    assert_eq!(
        first_result.projected_event_path(),
        second_result.projected_event_path()
    );
    assert!(is_non_decreasing_timestamp_path(&first_result));
}

#[test]
fn seeded_simulation_run_rejects_zero_horizon() {
    let error = SeededTickSimulationRunConfig::try_new(
        42,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        0,
        valid_model_inputs(),
        None,
    )
    .expect_err("zero horizon must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::InvalidHorizonIntervalCount { value: 0 }
    ));
}

#[test]
fn seeded_simulation_run_rejects_incomplete_interval_factor_grid_payload() {
    let mut model_inputs = valid_model_inputs();

    model_inputs.interval_factor_grid.seasonality_factor.pop();

    let error = SeededTickSimulationRunConfig::try_new(
        77,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        model_inputs,
        None,
    )
    .expect_err("incomplete interval factor grid vectors must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::IntervalFactorGridFieldLengthMismatch {
            field_name: "seasonality_factor",
            expected_interval_count: 2,
            observed_interval_count: 1,
        }
    ));
}

#[test]
fn seeded_simulation_run_rejects_non_consecutive_interval_factor_grid_coverage() {
    let mut model_inputs = valid_model_inputs();

    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;

    model_inputs.interval_factor_grid.interval_start_utc_nanos[1] += NANOS_PER_HOUR;
    model_inputs.interval_factor_grid.interval_end_utc_nanos[1] += NANOS_PER_HOUR;
    model_inputs.interval_factor_grid.request_end_utc_nanos += NANOS_PER_HOUR;

    model_inputs
        .interval_factor_grid
        .diagnostics
        .last_interval_end_utc_nanos += NANOS_PER_HOUR;

    let error = SeededTickSimulationRunConfig::try_new(
        88,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        model_inputs,
        None,
    )
    .expect_err("non-consecutive interval coverage must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::NonConsecutiveIntervalFactorGridCoverage {
            previous_interval_index: 0,
            previous_interval_end_utc_nanos,
            current_interval_index: 1,
            current_interval_start_utc_nanos,
        } if previous_interval_end_utc_nanos == start_utc_nanos + NANOS_PER_HOUR
            && current_interval_start_utc_nanos == start_utc_nanos + 2 * NANOS_PER_HOUR
    ));
}

#[test]
fn seeded_simulation_run_accepts_seasonality_scaled_subcritical_hawkes_inputs() {
    let mut model_inputs = valid_model_inputs();

    model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        0.8,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.9,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.9,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("supercritical guard fixture should be valid in kernel domain");

    model_inputs.interval_factor_grid.seasonality_factor = vec![1.2, 1.2];

    SeededTickSimulationRunConfig::try_new(
        99,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        model_inputs,
        None,
    )
    .expect("raw-subcritical Hawkes inputs should remain valid when seasonality scales baseline");
}

#[test]
fn seeded_simulation_preserves_full_state_carryover_across_hour_boundaries() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;

    let mut one_hour_model_inputs = one_interval_model_inputs(0.0, 0.0, 0.0);

    let two_hour_model_inputs = two_interval_zero_event_carryover_model_inputs();
    let kernel_parameters = two_hour_model_inputs.kernel_parameters;

    one_hour_model_inputs.kernel_parameters = kernel_parameters;
    one_hour_model_inputs.initial_excitation_state = two_hour_model_inputs.initial_excitation_state;

    one_hour_model_inputs
        .interval_factor_grid
        .seasonality_factor = vec![0.0];
    one_hour_model_inputs.interval_factor_grid.news_intensity = vec![0.0];

    one_hour_model_inputs
        .interval_factor_grid
        .known_news_intensity = vec![0.0];

    one_hour_model_inputs
        .interval_factor_grid
        .surprise_news_intensity = vec![0.0];

    let one_hour_config = SeededTickSimulationRunConfig::try_new(
        10_052,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        1,
        one_hour_model_inputs,
        None,
    )
    .expect("one-hour carryover run config should initialize");

    let two_hour_config = SeededTickSimulationRunConfig::try_new(
        10_052,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        two_hour_model_inputs,
        None,
    )
    .expect("two-hour carryover run config should initialize");

    let one_hour_result = run_seeded_tick_event_simulation(one_hour_config)
        .expect("one-hour carryover run should complete");

    let two_hour_result = run_seeded_tick_event_simulation(two_hour_config)
        .expect("two-hour carryover run should complete");

    let expected_two_hour_final_state = super::evolve_hawkes_state(
        one_hour_result.final_excitation_state,
        kernel_parameters,
        HawkesTransitionStep::new(1.0, None, None),
    )
    .expect("deterministic carryover drift step should succeed");

    assert_eq!(one_hour_result.event_count(), 0);
    assert_eq!(two_hour_result.event_count(), 0);
    assert_float_close(
        two_hour_result.final_excitation_state.y_plus_plus,
        expected_two_hour_final_state.y_plus_plus,
    );
    assert_float_close(
        two_hour_result.final_excitation_state.y_plus_minus,
        expected_two_hour_final_state.y_plus_minus,
    );
    assert_float_close(
        two_hour_result.final_excitation_state.y_minus_plus,
        expected_two_hour_final_state.y_minus_plus,
    );
    assert_float_close(
        two_hour_result.final_excitation_state.y_minus_minus,
        expected_two_hour_final_state.y_minus_minus,
    );
}

#[test]
fn seeded_simulation_factor_changes_apply_on_hour_boundaries_only() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let boundary_utc_nanos = start_utc_nanos + NANOS_PER_HOUR;

    let baseline_run_config = SeededTickSimulationRunConfig::try_new(
        10_053,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        two_interval_constant_intensity_model_inputs(1.0),
        None,
    )
    .expect("baseline boundary-factor run config should initialize");

    let muted_second_hour_run_config = SeededTickSimulationRunConfig::try_new(
        10_053,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        two_interval_constant_intensity_model_inputs(0.0),
        None,
    )
    .expect("muted boundary-factor run config should initialize");

    let baseline_result = run_seeded_tick_event_simulation(baseline_run_config)
        .expect("baseline boundary-factor run should complete");

    let muted_second_hour_result = run_seeded_tick_event_simulation(muted_second_hour_run_config)
        .expect("muted boundary-factor run should complete");

    let baseline_event_path = baseline_result.projected_event_path();
    let muted_second_hour_event_path = muted_second_hour_result.projected_event_path();

    assert!(baseline_event_path.len() >= muted_second_hour_event_path.len());
    assert!(
        muted_second_hour_event_path
            .iter()
            .all(|event_record| event_record.observed_timestamp_utc_nanos < boundary_utc_nanos)
    );
    assert_eq!(
        &baseline_event_path[..muted_second_hour_event_path.len()],
        muted_second_hour_event_path.as_slice()
    );
}

#[test]
fn seeded_simulation_mean_reversion_decay_matches_pure_exponential_reference() {
    let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    model_inputs.interval_factor_grid.seasonality_factor = vec![0.0];
    model_inputs.interval_factor_grid.news_intensity = vec![0.0];

    configure_mean_reversion_runtime_inputs(&mut model_inputs, 1.0, 0.0, &[0.0], 1.5);

    let run_config = SeededTickSimulationRunConfig::try_new(
        10_053,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        model_inputs,
        None,
    )
    .expect("mean-reversion decay run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("mean-reversion decay run should complete");

    assert_eq!(run_result.fine_path.len(), 1);

    let boundary_record = &run_result.fine_path[0];
    let expected_deviation = (-1.0_f64).exp() * 1.5;

    assert_float_close(boundary_record.core_total_return_increment, 0.0);
    assert_float_close(boundary_record.anchor_value, 0.0);
    assert_float_close(boundary_record.deviation_from_anchor, expected_deviation);
    assert_float_close(boundary_record.observed_log_price, expected_deviation);
}

#[test]
fn seeded_simulation_positive_kappa_pulls_constant_anchor_path_toward_anchor() {
    let mut zero_kappa_model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    zero_kappa_model_inputs
        .interval_factor_grid
        .seasonality_factor = vec![0.0];

    zero_kappa_model_inputs.interval_factor_grid.news_intensity = vec![0.0];

    configure_mean_reversion_runtime_inputs(&mut zero_kappa_model_inputs, 0.0, 2.0, &[2.0], 3.0);

    let mut positive_kappa_model_inputs = zero_kappa_model_inputs.clone();

    configure_mean_reversion_runtime_inputs(
        &mut positive_kappa_model_inputs,
        1.0,
        2.0,
        &[2.0],
        3.0,
    );

    let zero_kappa_config = SeededTickSimulationRunConfig::try_new(
        10_054,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        zero_kappa_model_inputs,
        None,
    )
    .expect("zero-kappa constant-anchor run config should initialize");

    let positive_kappa_config = SeededTickSimulationRunConfig::try_new(
        10_054,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        positive_kappa_model_inputs,
        None,
    )
    .expect("positive-kappa constant-anchor run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let zero_kappa_result = run_seeded_tick_event_simulation_with_randomness_factory(
        zero_kappa_config,
        &scripted_factory,
    )
    .expect("zero-kappa constant-anchor run should complete");

    let positive_kappa_result = run_seeded_tick_event_simulation_with_randomness_factory(
        positive_kappa_config,
        &scripted_factory,
    )
    .expect("positive-kappa constant-anchor run should complete");

    let zero_kappa_observed_log_price = zero_kappa_result.fine_path[0].observed_log_price;
    let positive_kappa_observed_log_price = positive_kappa_result.fine_path[0].observed_log_price;

    assert_float_close(zero_kappa_observed_log_price, 3.0);
    assert!(positive_kappa_observed_log_price < zero_kappa_observed_log_price);
    assert!(positive_kappa_observed_log_price > 2.0);
}

#[test]
fn seeded_simulation_does_not_decay_mean_reversion_during_closed_interval() {
    let mut model_inputs = one_interval_closed_market_model_inputs();

    model_inputs.interval_factor_grid.seasonality_factor = vec![0.0];
    model_inputs.interval_factor_grid.news_intensity = vec![0.0];

    configure_mean_reversion_runtime_inputs(&mut model_inputs, 1.0, 2.0, &[2.0], 3.0);

    let run_config = SeededTickSimulationRunConfig::try_new(
        10_054,
        DateTime::<Utc>::from_timestamp_nanos(1_705_928_400_000_000_000),
        1,
        model_inputs,
        None,
    )
    .expect("closed-interval mean-reversion run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("closed-interval mean-reversion run should complete");

    let public_fine_path =
        run_result.public_fine_path_chunk(0, run_result.public_transition_count());

    assert_eq!(public_fine_path.len(), 1);
    assert_float_close(public_fine_path[0].observed_log_price, 3.0);
    assert_float_close(public_fine_path[0].deviation_from_anchor, 1.0);
}

#[test]
fn seeded_simulation_kappa_zero_with_flat_anchor_matches_nested_core_path_semantics() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        10_055,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        None,
    )
    .expect("nested-equivalence run config should initialize");

    let run_result = run_seeded_tick_event_simulation(run_config)
        .expect("nested-equivalence run should complete");

    let mut expected_cumulative_core_log_return = 0.0;

    for fine_path_record in &run_result.fine_path {
        expected_cumulative_core_log_return += fine_path_record.core_total_return_increment;
        assert_float_close(
            fine_path_record.cumulative_core_log_return,
            expected_cumulative_core_log_return,
        );
        assert_float_close(
            fine_path_record.observed_log_price,
            fine_path_record.cumulative_core_log_return,
        );
        assert_float_close(fine_path_record.anchor_value, 0.0);
        assert_float_close(
            fine_path_record.deviation_from_anchor,
            fine_path_record.cumulative_core_log_return,
        );
    }

    assert_float_close(
        run_result.cumulative_core_log_return,
        expected_cumulative_core_log_return,
    );
}

#[test]
fn seeded_simulation_anchor_reset_applies_exactly_once_per_interval_boundary() {
    let mut model_inputs = valid_model_inputs();

    model_inputs.initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    model_inputs.interval_factor_grid.seasonality_factor = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.news_intensity = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.known_news_intensity = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.surprise_news_intensity = vec![0.0, 0.0];

    configure_mean_reversion_runtime_inputs(&mut model_inputs, 0.0, 0.0, &[2.0, 2.0], 1.0);

    let run_config = SeededTickSimulationRunConfig::try_new(
        10_056,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        model_inputs,
        None,
    )
    .expect("anchor-reset run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("anchor-reset run should complete");

    assert_eq!(run_result.fine_path.len(), 2);

    let first_boundary_record = &run_result.fine_path[0];
    let second_boundary_record = &run_result.fine_path[1];

    assert_float_close(first_boundary_record.observed_log_price, 1.0);
    assert_float_close(first_boundary_record.anchor_value, 2.0);
    assert_float_close(first_boundary_record.deviation_from_anchor, -1.0);

    assert_float_close(second_boundary_record.observed_log_price, 1.0);
    assert_float_close(second_boundary_record.anchor_value, 2.0);
    assert_float_close(second_boundary_record.deviation_from_anchor, -1.0);
}

#[test]
fn seeded_simulation_event_time_feedback_changes_same_interval_arrival_dynamics_relative_to_no_feedback_baseline()
 {
    let mut no_feedback_model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    no_feedback_model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        0.5,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("no-feedback kernel fixture should be valid");

    let mut feedback_model_inputs = no_feedback_model_inputs.clone();

    feedback_model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        0.5,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.8,
            plus_minus: 0.0,
            minus_plus: 0.6,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("feedback kernel fixture should be valid");

    let no_feedback_run_config = SeededTickSimulationRunConfig::try_new(
        1_007,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        no_feedback_model_inputs,
        None,
    )
    .expect("no-feedback run config should initialize");

    let feedback_run_config = SeededTickSimulationRunConfig::try_new(
        1_007,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        feedback_model_inputs,
        None,
    )
    .expect("feedback run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0, 0.0, 0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.95, 0.35, 0.0],
        sell_timestamp_draws: vec![0.0, 0.0, 0.0],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let no_feedback_result = run_seeded_tick_event_simulation_with_randomness_factory(
        no_feedback_run_config,
        &scripted_factory,
    )
    .expect("no-feedback run should complete");

    let feedback_result = run_seeded_tick_event_simulation_with_randomness_factory(
        feedback_run_config,
        &scripted_factory,
    )
    .expect("feedback run should complete");

    let no_feedback_event_path = no_feedback_result.projected_event_path();
    let feedback_event_path = feedback_result.projected_event_path();

    assert_eq!(no_feedback_event_path.len(), 1);
    assert!(feedback_event_path.len() >= 2);
    assert!(
        feedback_event_path[1].observed_timestamp_utc_nanos
            > feedback_event_path[0].observed_timestamp_utc_nanos
    );
}

#[test]
fn seeded_simulation_regresses_if_endogenous_scheduling_reverts_to_static_count_uniform_placement()
{
    let run_config = SeededTickSimulationRunConfig::try_new(
        2_008,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(2.0, 2.0, 0.0),
        None,
    )
    .expect("regression-guard run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.25, 0.25],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.9, 0.9],
        sell_timestamp_draws: vec![0.0, 0.0],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("regression-guard run should complete");

    let interval_start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;

    let legacy_uniform_timestamps = vec![
        interval_start_utc_nanos + NANOS_PER_HOUR / 4,
        interval_start_utc_nanos + 3 * NANOS_PER_HOUR / 4,
    ];

    let observed_timestamps: Vec<i64> = run_result
        .projected_event_path()
        .iter()
        .map(|event_record| event_record.observed_timestamp_utc_nanos)
        .collect();

    assert_eq!(observed_timestamps.len(), 2);
    assert_ne!(observed_timestamps, legacy_uniform_timestamps);
    assert_eq!(
        run_result.event_ordering_policy,
        SEEDED_TICK_EVENT_ORDERING_POLICY
    );
}

#[test]
fn seeded_simulation_generates_fitted_same_timestamp_batch_continuations() {
    let mut model_inputs = one_interval_model_inputs(2.0, 2.0, 0.0);

    model_inputs.timestamp_batching_observation_model =
        janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel::fixed_bucket_floor(
            1_000,
            "unit_test_same_timestamp_continuation",
            1,
            1,
        )
        .expect("timestamp batching fixture should validate");

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_008,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        model_inputs,
        None,
    )
    .expect("timestamp batching run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.25, 0.25],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.9, 0.9],
        sell_timestamp_draws: vec![0.0, 0.0],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("timestamp batching run should complete");

    let event_path = run_result.projected_event_path();

    assert_eq!(event_path.len(), 2);
    assert!(event_path[1].latent_event_time_utc_nanos > event_path[0].latent_event_time_utc_nanos);
    assert_eq!(
        event_path[1].observed_timestamp_utc_nanos,
        event_path[0].observed_timestamp_utc_nanos
    );
    assert_eq!(
        run_result
            .timing_diagnostics
            .observed_timestamp_interarrivals
            .zero_interarrival_count,
        1
    );
}

#[test]
fn seeded_simulation_fine_path_interval_aggregation_matches_coarse_endogenous_trace_diagnostics() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_009,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        two_interval_constant_intensity_model_inputs(1.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("fine-aggregation integration run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0; 16],
        sell_count_draws: vec![0.0; 4],
        buy_timestamp_draws: vec![0.95, 0.95, 0.0],
        sell_timestamp_draws: vec![0.0; 16],
        news_count_draws: vec![1.0, 1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("fine-aggregation integration run should complete");

    let mut aggregated_fine_buy_counts = [0_usize; 2];
    let mut aggregated_fine_sell_counts = [0_usize; 2];

    for event_record in run_result.projected_event_path() {
        let interval_index = ((event_record.observed_timestamp_utc_nanos - start_utc_nanos)
            .div_euclid(NANOS_PER_HOUR)) as usize;

        match event_record.side {
            SeededTickEventSide::Buy => {
                aggregated_fine_buy_counts[interval_index] =
                    aggregated_fine_buy_counts[interval_index].saturating_add(1);
            }
            SeededTickEventSide::Sell => {
                aggregated_fine_sell_counts[interval_index] =
                    aggregated_fine_sell_counts[interval_index].saturating_add(1);
            }
        }
    }

    let mut coarse_trace_buy_counts = [0_usize; 2];
    let mut coarse_trace_sell_counts = [0_usize; 2];

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .collect::<Vec<_>>();

    for trace_record in trace_records {
        match trace_record.transition_kind {
            SeededTickEventDebugTraceTransitionKind::EndogenousBuyEvent => {
                coarse_trace_buy_counts[trace_record.interval_index] =
                    coarse_trace_buy_counts[trace_record.interval_index].saturating_add(1);
            }
            SeededTickEventDebugTraceTransitionKind::EndogenousSellEvent => {
                coarse_trace_sell_counts[trace_record.interval_index] =
                    coarse_trace_sell_counts[trace_record.interval_index].saturating_add(1);
            }
            SeededTickEventDebugTraceTransitionKind::ExogenousNews
            | SeededTickEventDebugTraceTransitionKind::IntervalBoundaryDrift => {}
        }
    }

    assert_eq!(aggregated_fine_buy_counts, coarse_trace_buy_counts);
    assert_eq!(aggregated_fine_sell_counts, coarse_trace_sell_counts);
}

#[test]
fn seeded_simulation_scheduler_applies_known_before_surprise_tie_ordering_policy() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let tied_timestamp_utc_nanos = start_utc_nanos + ((NANOS_PER_HOUR as f64 * 0.3).floor() as i64);

    let mut model_inputs = one_interval_model_inputs(1.0, 1.0, 2.0);

    set_known_news_timestamps(&mut model_inputs, vec![tied_timestamp_utc_nanos]);

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_009,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        1,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("known-surprise tie-ordering run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.3],
        sell_timestamp_draws: vec![0.3],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![0.3],
        news_severity_draws: vec![0.5, 0.25, 0.5, 0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.75, 0.2, 0.25, 0.3],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("known-surprise tie-ordering run should complete");

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .collect::<Vec<_>>();

    let exogenous_records = trace_records
        .iter()
        .filter(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .collect::<Vec<_>>();

    assert_eq!(exogenous_records.len(), 2);
    assert_eq!(
        exogenous_records[0].exogenous_news_arrival_component,
        Some(NewsArrivalComponent::Known)
    );
    assert_eq!(
        exogenous_records[1].exogenous_news_arrival_component,
        Some(NewsArrivalComponent::Surprise)
    );
    assert_eq!(
        exogenous_records[0].observed_timestamp_utc_nanos,
        exogenous_records[1].observed_timestamp_utc_nanos
    );
    assert_eq!(
        exogenous_records[0].observed_timestamp_utc_nanos,
        tied_timestamp_utc_nanos
    );
}

#[test]
fn interval_news_arrival_decomposition_scales_known_component_by_interval_duration() {
    let decomposition = super::runtime::derive_interval_news_arrival_decomposition(1.75, 2.0, 3);

    assert_float_close(decomposition.known_component_intensity, 1.5);
    assert_float_close(decomposition.surprise_component_intensity, 0.25);
}

#[test]
fn seeded_simulation_surprise_sampling_is_invariant_to_known_timestamp_profile_when_known_count_is_fixed()
 {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let interval_end_utc_nanos = start_utc_nanos + 2 * NANOS_PER_HOUR;

    let known_timestamp_profiles: Vec<Vec<i64>> = vec![
        vec![
            start_utc_nanos + NANOS_PER_HOUR / 10,
            start_utc_nanos + 3 * NANOS_PER_HOUR / 2,
        ],
        vec![
            start_utc_nanos + NANOS_PER_HOUR / 4,
            start_utc_nanos + 7 * NANOS_PER_HOUR / 4,
        ],
        vec![
            start_utc_nanos + NANOS_PER_HOUR / 2,
            start_utc_nanos + 9 * NANOS_PER_HOUR / 5,
        ],
    ];

    for seed in 9_300_u64..9_316_u64 {
        let mut expected_surprise_timestamps: Option<Vec<i64>> = None;

        for known_timestamps in &known_timestamp_profiles {
            let mut model_inputs = one_interval_model_inputs(0.0, 0.0, 1.75);

            override_interval_factor_grid(
                &mut model_inputs,
                &[start_utc_nanos],
                &[interval_end_utc_nanos],
                &[1.0],
                &[1.75],
            );

            set_known_news_timestamps(&mut model_inputs, known_timestamps.clone());

            let run_config = SeededTickSimulationRunConfig::try_new(
                seed,
                DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
                1,
                model_inputs,
                Some(SeededTickSimulationRunOptions {
                    debug_trace_enabled: true,
                    debug_trace_options: SeededTickSimulationDebugTraceOptions {
                        sample_every_nth_transition: 1,
                        max_records_per_chunk: 16,
                    },
                    state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
                }),
            )
            .expect("profile-invariance run config should initialize");

            let run_result = run_seeded_tick_event_simulation(run_config)
                .expect("profile-invariance run should complete");

            let trace_records = run_result
                .debug_trace_output
                .as_ref()
                .expect("debug trace output should exist")
                .trace_chunks
                .iter()
                .flat_map(|trace_chunk| trace_chunk.records.iter())
                .collect::<Vec<_>>();

            let observed_known_timestamps: Vec<i64> = trace_records
                .iter()
                .filter(|trace_record| {
                    trace_record.transition_kind
                        == SeededTickEventDebugTraceTransitionKind::ExogenousNews
                        && trace_record.exogenous_news_arrival_component
                            == Some(NewsArrivalComponent::Known)
                })
                .map(|trace_record| trace_record.observed_timestamp_utc_nanos)
                .collect();

            let observed_surprise_timestamps: Vec<i64> = trace_records
                .iter()
                .filter(|trace_record| {
                    trace_record.transition_kind
                        == SeededTickEventDebugTraceTransitionKind::ExogenousNews
                        && trace_record.exogenous_news_arrival_component
                            == Some(NewsArrivalComponent::Surprise)
                })
                .map(|trace_record| trace_record.observed_timestamp_utc_nanos)
                .collect();

            assert_eq!(observed_known_timestamps, *known_timestamps);

            if let Some(reference_surprise_timestamps) = &expected_surprise_timestamps {
                assert_eq!(observed_surprise_timestamps, *reference_surprise_timestamps);
            } else {
                expected_surprise_timestamps = Some(observed_surprise_timestamps);
            }
        }
    }
}

#[test]
fn seeded_simulation_news_component_counts_follow_known_plus_surprise_decomposition_for_controlled_fixture()
 {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let second_interval_start_utc_nanos = start_utc_nanos + NANOS_PER_HOUR;
    let second_interval_end_utc_nanos = start_utc_nanos + 2 * NANOS_PER_HOUR;

    let mut model_inputs = one_interval_model_inputs(0.0, 0.0, 0.0);

    override_interval_factor_grid(
        &mut model_inputs,
        &[start_utc_nanos, second_interval_start_utc_nanos],
        &[
            second_interval_start_utc_nanos,
            second_interval_end_utc_nanos,
        ],
        &[1.0, 1.0],
        &[2.75, 1.4],
    );

    set_known_news_timestamps(
        &mut model_inputs,
        vec![
            start_utc_nanos + NANOS_PER_HOUR / 10,
            start_utc_nanos + 3 * NANOS_PER_HOUR / 5,
            second_interval_start_utc_nanos + NANOS_PER_HOUR / 3,
        ],
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        9_317,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("decomposition-fixture run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![],
        sell_count_draws: vec![],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.2, 0.1],
        news_timestamp_draws: vec![0.3, 0.6],
        news_severity_draws: vec![0.5; 64],
        news_return_jump_draws: vec![0.5; 64],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("decomposition-fixture run should complete");

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .filter(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .collect::<Vec<_>>();

    assert_eq!(trace_records.len(), 5);

    let known_count = trace_records
        .iter()
        .filter(|trace_record| {
            trace_record.exogenous_news_arrival_component == Some(NewsArrivalComponent::Known)
        })
        .count();

    let surprise_count = trace_records
        .iter()
        .filter(|trace_record| {
            trace_record.exogenous_news_arrival_component == Some(NewsArrivalComponent::Surprise)
        })
        .count();

    assert_eq!(known_count, 3);
    assert_eq!(surprise_count, 2);

    let surprise_interval_zero_count = trace_records
        .iter()
        .filter(|trace_record| {
            trace_record.interval_index == 0
                && trace_record.exogenous_news_arrival_component
                    == Some(NewsArrivalComponent::Surprise)
        })
        .count();

    let surprise_interval_one_count = trace_records
        .iter()
        .filter(|trace_record| {
            trace_record.interval_index == 1
                && trace_record.exogenous_news_arrival_component
                    == Some(NewsArrivalComponent::Surprise)
        })
        .count();

    assert_eq!(surprise_interval_zero_count, 1);
    assert_eq!(surprise_interval_one_count, 1);

    for trace_record in trace_records {
        let expected_interval_news_intensity = [2.75, 1.4][trace_record.interval_index];

        assert_float_close(
            trace_record.interval_news_intensity,
            expected_interval_news_intensity,
        );
    }
}

#[test]
fn seeded_simulation_known_open_news_timestamp_updates_m_t_not_g_t() {
    let interval_start_utc_nanos = timestamp_utc_nanos("2024-01-22T15:00:00Z");
    let known_open_timestamp_utc_nanos = interval_start_utc_nanos + NANOS_PER_HOUR / 4;

    let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    override_interval_factor_grid(
        &mut model_inputs,
        &[interval_start_utc_nanos],
        &[interval_start_utc_nanos + NANOS_PER_HOUR],
        &[0.0],
        &[0.0],
    );

    set_known_news_timestamps(&mut model_inputs, vec![known_open_timestamp_utc_nanos]);

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_010,
        DateTime::<Utc>::from_timestamp_nanos(interval_start_utc_nanos),
        1,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("open-window news routing run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![1.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.25, 0.5],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("open-window news routing run should complete");

    let mut expected_mark_stream = ScriptedRandomnessStream::new(vec![0.5, 0.25, 0.5, 0.25, 0.5]);

    let (expected_shared_severity_mark, expected_shared_return_jump_mark) =
        sample_expected_shared_news_marks(&mut expected_mark_stream);

    let expected_mark_scales = default_news_component_mark_scales();

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .filter(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .collect::<Vec<_>>();

    assert_eq!(trace_records.len(), 1);
    assert_eq!(
        trace_records[0].exogenous_news_market_state,
        Some(SeededTickSimulationNewsEventMarketState::Open)
    );
    assert_eq!(
        trace_records[0].exogenous_news_return_jump_routing_target,
        Some(NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM)
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_open_window_return_jump_m_t_increment,
        expected_gamma_normal_return_jump(
            expected_mark_scales,
            expected_shared_severity_mark,
            expected_shared_return_jump_mark,
        ),
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_activity_coupling_j_t_increment,
        expected_mark_scales.activity_coupling_scale * expected_shared_severity_mark,
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_closed_window_gap_g_t_increment,
        0.0,
    );
}

#[test]
fn seeded_simulation_known_closed_news_timestamp_updates_g_t_not_m_t() {
    let interval_start_utc_nanos = timestamp_utc_nanos("2024-12-25T15:00:00Z");
    let known_closed_timestamp_utc_nanos = interval_start_utc_nanos + NANOS_PER_HOUR / 4;

    let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    override_interval_factor_grid(
        &mut model_inputs,
        &[interval_start_utc_nanos],
        &[interval_start_utc_nanos + NANOS_PER_HOUR],
        &[1.0],
        &[0.0],
    );

    set_known_news_timestamps(&mut model_inputs, vec![known_closed_timestamp_utc_nanos]);

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_011,
        DateTime::<Utc>::from_timestamp_nanos(interval_start_utc_nanos),
        1,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("closed-window news routing run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![1.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.25, 0.5],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("closed-window news routing run should complete");

    let mut expected_mark_stream = ScriptedRandomnessStream::new(vec![0.5, 0.25, 0.5, 0.25, 0.5]);

    let (expected_shared_severity_mark, expected_shared_return_jump_mark) =
        sample_expected_shared_news_marks(&mut expected_mark_stream);

    let expected_mark_scales = default_news_component_mark_scales();

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .filter(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .collect::<Vec<_>>();

    assert_eq!(trace_records.len(), 1);
    assert_eq!(
        trace_records[0].exogenous_news_market_state,
        Some(SeededTickSimulationNewsEventMarketState::Closed)
    );
    assert_eq!(
        trace_records[0].exogenous_news_return_jump_routing_target,
        Some(NewsReturnJumpRoutingTarget::ClosedWindowGapG)
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_open_window_return_jump_m_t_increment,
        0.0,
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_closed_window_gap_g_t_increment,
        expected_gamma_normal_return_jump(
            expected_mark_scales,
            expected_shared_severity_mark,
            expected_shared_return_jump_mark,
        ),
    );
    assert_float_close(
        run_result
            .news_routing_totals
            .cumulative_activity_coupling_j_t_increment,
        expected_mark_scales.activity_coupling_scale * expected_shared_severity_mark,
    );
}

#[test]
fn seeded_simulation_calendar_resolution_is_source_of_truth_for_early_close_boundary() {
    let interval_start_utc_nanos = timestamp_utc_nanos("2024-11-29T17:00:00Z");
    let open_window_timestamp_utc_nanos = interval_start_utc_nanos + NANOS_PER_HOUR / 2;
    let early_close_boundary_timestamp_utc_nanos = interval_start_utc_nanos + NANOS_PER_HOUR;

    let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    override_interval_factor_grid(
        &mut model_inputs,
        &[
            interval_start_utc_nanos,
            interval_start_utc_nanos + NANOS_PER_HOUR,
        ],
        &[
            interval_start_utc_nanos + NANOS_PER_HOUR,
            interval_start_utc_nanos + 2 * NANOS_PER_HOUR,
        ],
        &[0.0, 1.0],
        &[0.0, 0.0],
    );

    set_known_news_timestamps(
        &mut model_inputs,
        vec![
            open_window_timestamp_utc_nanos,
            early_close_boundary_timestamp_utc_nanos,
        ],
    );

    let run_config = SeededTickSimulationRunConfig::try_new(
        2_012,
        DateTime::<Utc>::from_timestamp_nanos(interval_start_utc_nanos),
        2,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: true,
            debug_trace_options: SeededTickSimulationDebugTraceOptions {
                sample_every_nth_transition: 1,
                max_records_per_chunk: 16,
            },
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::Disabled,
        }),
    )
    .expect("early-close boundary routing run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![1.0, 1.0],
        sell_count_draws: vec![1.0, 1.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![1.0, 1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![0.5, 0.25, 0.5, 0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.75, 0.5, 0.75, 0.5],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("early-close boundary routing run should complete");

    let trace_records = run_result
        .debug_trace_output
        .as_ref()
        .expect("debug trace output should exist")
        .trace_chunks
        .iter()
        .flat_map(|trace_chunk| trace_chunk.records.iter())
        .filter(|trace_record| {
            trace_record.transition_kind == SeededTickEventDebugTraceTransitionKind::ExogenousNews
        })
        .collect::<Vec<_>>();

    assert_eq!(trace_records.len(), 2);
    assert_eq!(
        trace_records[0].observed_timestamp_utc_nanos,
        open_window_timestamp_utc_nanos
    );
    assert_eq!(
        trace_records[0].exogenous_news_market_state,
        Some(SeededTickSimulationNewsEventMarketState::Open)
    );
    assert_eq!(
        trace_records[0].exogenous_news_return_jump_routing_target,
        Some(NewsReturnJumpRoutingTarget::OpenWindowDirectJumpM)
    );
    assert_eq!(
        trace_records[1].observed_timestamp_utc_nanos,
        early_close_boundary_timestamp_utc_nanos
    );
    assert_eq!(
        trace_records[1].exogenous_news_market_state,
        Some(SeededTickSimulationNewsEventMarketState::Closed)
    );
    assert_eq!(
        trace_records[1].exogenous_news_return_jump_routing_target,
        Some(NewsReturnJumpRoutingTarget::ClosedWindowGapG)
    );
    assert_ne!(
        run_result
            .news_routing_totals
            .cumulative_open_window_return_jump_m_t_increment,
        0.0
    );
    assert_ne!(
        run_result
            .news_routing_totals
            .cumulative_closed_window_gap_g_t_increment,
        0.0
    );
}

#[test]
fn seeded_simulation_rejects_invalid_generated_news_return_jump_mark() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        2_011,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(0.5, 0.5, 1.0),
        None,
    )
    .expect("invalid return-jump mark run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![1.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![0.3],
        news_severity_draws: vec![0.5, 0.25, 0.5],
        news_return_jump_draws: vec![0.75, f64::NAN],
    });

    let error =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect_err("invalid generated return-jump mark must fail");

    assert!(matches!(
        error,
        SeededTickSimulationRunError::NewsMarkSamplingFailure {
            interval_index: 0,
            ..
        }
    ));
}

#[test]
fn seeded_exogenous_mark_sampling_matches_shared_gamma_normal_reference_contract_for_fixed_seed() {
    let root_seed = 98_771_u64;
    let interval_index = 3_u64;

    let mark_scales = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 1.35,
        return_jump_drift: 0.0,
        return_jump_volatility: 0.85,
    };

    let news_mark_substream_seed = derive_simulation_substream_seed(
        root_seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsMarks,
        interval_index,
    );

    let mut observed_mark_stream =
        SplitMix64SimulationRandomnessStream::from_seed(news_mark_substream_seed);

    let observed_event_input = super::runtime::build_timestamped_news_event_input(
        &build_news_process_configuration(Vec::new()).default_mark_pipeline(),
        NewsArrivalComponent::Surprise,
        SEEDED_TEST_START_UTC_NANOS,
        0,
        mark_scales,
        &mut observed_mark_stream,
        interval_index as usize,
    )
    .expect("seeded exogenous mark sampling should succeed for valid fixed-seed setup");

    let mut expected_mark_stream =
        SplitMix64SimulationRandomnessStream::from_seed(news_mark_substream_seed);

    let (expected_shared_severity_mark, expected_shared_return_jump_mark) =
        sample_expected_shared_news_marks(&mut expected_mark_stream);

    assert_float_close(
        observed_event_input.shared_severity_mark,
        mark_scales.activity_coupling_scale * expected_shared_severity_mark,
    );
    assert_float_close(
        observed_event_input.coupled_return_jump_mark,
        expected_gamma_normal_return_jump(
            mark_scales,
            expected_shared_severity_mark,
            expected_shared_return_jump_mark,
        ),
    );
}

#[test]
fn seeded_exogenous_mark_sampling_preserves_shared_gamma_normal_sign_and_scale_statistics() {
    let root_seed = 98_772_u64;
    let interval_index = 4_u64;
    let sample_count = 8_192_u64;

    let mark_scales = NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 1.1,
        return_jump_drift: 0.0,
        return_jump_volatility: 0.9,
    };

    let news_mark_substream_seed = derive_simulation_substream_seed(
        root_seed,
        SimulationRandomnessSubstreamDomain::ExogenousNewsMarks,
        interval_index,
    );

    let mut observed_mark_stream =
        SplitMix64SimulationRandomnessStream::from_seed(news_mark_substream_seed);

    let mut expected_mark_stream =
        SplitMix64SimulationRandomnessStream::from_seed(news_mark_substream_seed);

    let mark_pipeline = build_news_process_configuration(Vec::new()).default_mark_pipeline();

    let mut observed_activity_sum = 0.0;
    let mut expected_activity_sum = 0.0;
    let mut observed_return_abs_sum = 0.0;
    let mut expected_return_abs_sum = 0.0;
    let mut observed_positive_return_count = 0_u64;
    let mut observed_negative_return_count = 0_u64;
    let mut expected_positive_return_count = 0_u64;

    for _sample_index in 0..sample_count {
        let observed_event_input = super::runtime::build_timestamped_news_event_input(
            &mark_pipeline,
            NewsArrivalComponent::Surprise,
            SEEDED_TEST_START_UTC_NANOS,
            0,
            mark_scales,
            &mut observed_mark_stream,
            interval_index as usize,
        )
        .expect("seeded exogenous mark sampling should succeed for valid setup");

        let observed_activity_coupling_mark = observed_event_input.shared_severity_mark;
        let observed_return_jump_mark = observed_event_input.coupled_return_jump_mark;

        let (expected_shared_severity_mark, expected_shared_return_jump_mark) =
            sample_expected_shared_news_marks(&mut expected_mark_stream);

        let expected_activity_coupling_mark =
            mark_scales.activity_coupling_scale * expected_shared_severity_mark;

        let expected_return_jump_mark = expected_gamma_normal_return_jump(
            mark_scales,
            expected_shared_severity_mark,
            expected_shared_return_jump_mark,
        );

        assert!(observed_activity_coupling_mark.is_finite());
        assert!(observed_activity_coupling_mark >= 0.0);
        assert!(observed_return_jump_mark.is_finite());

        observed_activity_sum += observed_activity_coupling_mark;
        expected_activity_sum += expected_activity_coupling_mark;
        observed_return_abs_sum += observed_return_jump_mark.abs();
        expected_return_abs_sum += expected_return_jump_mark.abs();

        if observed_return_jump_mark > 0.0 {
            observed_positive_return_count = observed_positive_return_count.saturating_add(1);
        } else if observed_return_jump_mark < 0.0 {
            observed_negative_return_count = observed_negative_return_count.saturating_add(1);
        }

        if expected_return_jump_mark > 0.0 {
            expected_positive_return_count = expected_positive_return_count.saturating_add(1);
        }
    }

    assert!(
        observed_positive_return_count > 0,
        "seeded return jumps should contain positive marks under centered Gamma-normal configuration"
    );
    assert!(
        observed_negative_return_count > 0,
        "seeded return jumps should contain negative marks under centered Gamma-normal configuration"
    );

    let observed_activity_mean = observed_activity_sum / sample_count as f64;
    let expected_activity_mean = expected_activity_sum / sample_count as f64;
    let observed_return_abs_mean = observed_return_abs_sum / sample_count as f64;
    let expected_return_abs_mean = expected_return_abs_sum / sample_count as f64;
    let observed_positive_frequency = observed_positive_return_count as f64 / sample_count as f64;
    let expected_positive_frequency = expected_positive_return_count as f64 / sample_count as f64;

    assert_float_close(observed_activity_mean, expected_activity_mean);
    assert_float_close(observed_return_abs_mean, expected_return_abs_mean);
    assert_float_close(observed_positive_frequency, expected_positive_frequency);
}

#[test]
fn seeded_simulation_boundary_adjacent_event_times_are_not_snapped_to_hour_edges() {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let boundary_utc_nanos = start_utc_nanos + NANOS_PER_HOUR;

    let run_config = SeededTickSimulationRunConfig::try_new(
        10_054,
        DateTime::<Utc>::from_timestamp_nanos(start_utc_nanos),
        2,
        two_interval_boundary_adjacent_model_inputs(),
        None,
    )
    .expect("boundary-adjacent run config should initialize");

    let run_result = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config,
        &BoundaryAdjacentRandomnessFactory,
    )
    .expect("boundary-adjacent run should complete");

    let projected_event_path = run_result.projected_event_path();

    assert_eq!(projected_event_path.len(), 2);

    let first_timestamp_utc_nanos = projected_event_path[0].observed_timestamp_utc_nanos;
    let second_timestamp_utc_nanos = projected_event_path[1].observed_timestamp_utc_nanos;

    assert!(first_timestamp_utc_nanos < boundary_utc_nanos);
    assert!(second_timestamp_utc_nanos > boundary_utc_nanos);
    assert_ne!(first_timestamp_utc_nanos, boundary_utc_nanos);
    assert_ne!(second_timestamp_utc_nanos, boundary_utc_nanos);
}

#[test]
fn seeded_simulation_state_snapshot_captures_post_event_transition_boundary() {
    let mut model_inputs = one_interval_model_inputs(0.5, 0.5, 0.0);

    model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        0.5,
        1.5,
        HawkesKernelLoadings {
            plus_plus: 0.4,
            plus_minus: 0.0,
            minus_plus: 0.2,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("snapshot boundary kernel fixture should be valid");

    model_inputs.initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    let run_config = SeededTickSimulationRunConfig::try_new(
        7_047,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        model_inputs,
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
        }),
    )
    .expect("snapshot boundary run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![0.99],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("snapshot boundary run should complete");

    let projected_event_path = run_result.projected_event_path();

    assert_eq!(projected_event_path.len(), 1);
    assert_eq!(
        run_result.event_tape_metadata.state_snapshot_capture,
        SeededTickEventStateSnapshotCapture::PostEventTransition
    );
    assert_eq!(
        run_result
            .event_tape_metadata
            .state_snapshot_capture_point_policy,
        Some(SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY)
    );

    let snapshot = projected_event_path[0]
        .state_snapshot
        .expect("snapshot capture must populate state_snapshot");

    assert_float_close(snapshot.y_plus_plus, 0.4);
    assert_float_close(snapshot.y_plus_minus, 0.0);
    assert_float_close(snapshot.y_minus_plus, 0.2);
    assert_float_close(snapshot.y_minus_minus, 0.0);
    assert_float_close(snapshot.deseasonalized_lambda_plus, 0.9);
    assert_float_close(snapshot.deseasonalized_lambda_minus, 0.7);
}

#[test]
fn seeded_simulation_fine_path_schema_exposes_required_priced_fields() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        8_047,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 0.5, 0.0),
        None,
    )
    .expect("fine-path schema run config should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![0.25],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result =
        run_seeded_tick_event_simulation_with_randomness_factory(run_config, &scripted_factory)
            .expect("fine-path schema run should complete");

    let serialized_fine_path = run_result
        .serialize_fine_path_json()
        .expect("fine path serialization should succeed");

    let fine_path_payload: Value =
        serde_json::from_str(&serialized_fine_path).expect("fine path JSON should parse");

    let fine_path_records = fine_path_payload["fine_path"]
        .as_array()
        .expect("fine_path must serialize as an array");

    assert_eq!(fine_path_records.len(), 2);

    let first_record = fine_path_records[0]
        .as_object()
        .expect("fine-path record must serialize as an object");

    for required_field in [
        "transition_index",
        "observed_timestamp_utc_nanos",
        "transition_kind",
        "market_state",
        "return_components",
        "core_total_return_increment",
        "observed_log_price",
        "anchor_value",
        "pressure_state",
        "deviation_from_anchor",
        "cumulative_core_log_return",
    ] {
        assert!(first_record.contains_key(required_field));
    }

    let return_components = first_record["return_components"]
        .as_object()
        .expect("return_components must serialize as an object");

    for required_component_field in [
        "buy_side_return",
        "sell_side_return",
        "trading_time_return",
        "open_jump_return",
        "closed_gap_return",
        "total_return",
    ] {
        assert!(return_components.contains_key(required_component_field));
    }
}

#[test]
fn seeded_simulation_event_tape_schema_exposes_required_and_optional_fields() {
    let run_config_without_snapshot = SeededTickSimulationRunConfig::try_new(
        8_048,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 0.5, 0.0),
        None,
    )
    .expect("schema run config without snapshot should initialize");

    let scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![1.0],
        buy_timestamp_draws: vec![0.25],
        sell_timestamp_draws: vec![],
        news_count_draws: vec![0.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let run_result_without_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config_without_snapshot,
        &scripted_factory,
    )
    .expect("schema run without snapshot should complete");

    let serialized_without_snapshot = run_result_without_snapshot
        .serialize_event_tape_json()
        .expect("event tape serialization without snapshot should succeed");

    let payload_without_snapshot: Value = serde_json::from_str(&serialized_without_snapshot)
        .expect("serialized event tape payload should parse");

    assert_eq!(
        payload_without_snapshot["schema_version"],
        SEEDED_TICK_EVENT_PATH_SCHEMA_VERSION
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["schema_version"],
        SEEDED_TICK_EVENT_TAPE_METADATA_SCHEMA_VERSION
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["event_record_schema_version"],
        SEEDED_TICK_EVENT_RECORD_SCHEMA_VERSION
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["state_snapshot_schema_version"],
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_SCHEMA_VERSION
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["timestamp_representation_policy"],
        SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["side_encoding_policy"],
        SEEDED_TICK_EVENT_SIDE_ENCODING_POLICY
    );
    assert_eq!(
        payload_without_snapshot["metadata"]["state_snapshot_capture"],
        "disabled"
    );
    assert!(
        payload_without_snapshot["metadata"]
            .get("state_snapshot_capture_point_policy")
            .is_none()
    );

    let serialized_events_without_snapshot = payload_without_snapshot["event_tape"]
        .as_array()
        .expect("event_tape must serialize as an array");

    assert_eq!(serialized_events_without_snapshot.len(), 1);

    assert_eq!(
        sorted_json_object_field_names(&serialized_events_without_snapshot[0]),
        EVENT_TAPE_RECORD_FIELDS_WITHOUT_SNAPSHOT
    );

    let run_config_with_snapshot = SeededTickSimulationRunConfig::try_new(
        8_048,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 0.5, 0.0),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
        }),
    )
    .expect("schema run config with snapshot should initialize");

    let run_result_with_snapshot = run_seeded_tick_event_simulation_with_randomness_factory(
        run_config_with_snapshot,
        &scripted_factory,
    )
    .expect("schema run with snapshot should complete");

    let serialized_with_snapshot = run_result_with_snapshot
        .serialize_event_tape_json()
        .expect("event tape serialization with snapshot should succeed");

    let payload_with_snapshot: Value = serde_json::from_str(&serialized_with_snapshot)
        .expect("serialized event tape payload should parse");

    assert_eq!(
        payload_with_snapshot["metadata"]["state_snapshot_capture"],
        "post_event_transition"
    );
    assert_eq!(
        payload_with_snapshot["metadata"]["state_snapshot_capture_point_policy"],
        SEEDED_TICK_EVENT_STATE_SNAPSHOT_CAPTURE_POINT_POLICY
    );

    let first_event_with_snapshot = payload_with_snapshot["event_tape"][0]
        .as_object()
        .expect("event record with snapshot must serialize as an object");

    let state_snapshot = first_event_with_snapshot
        .get("state_snapshot")
        .expect("event record must include state_snapshot when capture is enabled")
        .as_object()
        .expect("state_snapshot must serialize as an object");

    for required_snapshot_field in [
        "y_plus_plus",
        "y_plus_minus",
        "y_minus_plus",
        "y_minus_minus",
        "deseasonalized_lambda_plus",
        "deseasonalized_lambda_minus",
    ] {
        assert!(state_snapshot.contains_key(required_snapshot_field));
    }
}

#[test]
fn seeded_simulation_event_tape_serialization_is_deterministic() {
    let run_config = SeededTickSimulationRunConfig::try_new(
        9_049,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        2,
        valid_model_inputs(),
        Some(SeededTickSimulationRunOptions {
            debug_trace_enabled: false,
            debug_trace_options: SeededTickSimulationDebugTraceOptions::default(),
            state_snapshot_capture: SeededTickEventStateSnapshotCapture::PostEventTransition,
        }),
    )
    .expect("serialization determinism run config should initialize");

    let first_result = run_seeded_tick_event_simulation(run_config.clone())
        .expect("first serialization determinism run should complete");

    let second_result = run_seeded_tick_event_simulation(run_config)
        .expect("second serialization determinism run should complete");

    let first_json = first_result
        .serialize_event_tape_json()
        .expect("first event tape serialization should succeed");

    let second_json = second_result
        .serialize_event_tape_json()
        .expect("second event tape serialization should succeed");

    assert_eq!(first_json, second_json);
}

#[test]
fn seeded_simulation_tied_and_near_tied_timestamps_serialize_deterministically() {
    let first_run_config = SeededTickSimulationRunConfig::try_new(
        10_050,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 1.0, 0.0),
        None,
    )
    .expect("first serialization run config should initialize");

    let first_scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.8],
        sell_timestamp_draws: vec![0.0],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let first_result = run_seeded_tick_event_simulation_with_randomness_factory(
        first_run_config,
        &first_scripted_factory,
    )
    .expect("first serialization run should complete");

    let first_payload: Value = serde_json::from_str(
        &first_result
            .serialize_event_tape_json()
            .expect("first event tape serialization should succeed"),
    )
    .expect("first serialized payload should parse");

    let first_events = first_payload["event_tape"]
        .as_array()
        .expect("first event_tape must serialize as an array");

    assert_eq!(first_events.len(), 1);
    assert_eq!(first_events[0]["side"], "buy");
    assert_eq!(
        first_payload["metadata"]["timestamp_representation_policy"],
        SEEDED_TICK_EVENT_TIMESTAMP_REPRESENTATION_POLICY
    );
    assert_eq!(
        sorted_json_object_field_names(&first_events[0]),
        EVENT_TAPE_RECORD_FIELDS_WITHOUT_SNAPSHOT
    );

    let second_run_config = SeededTickSimulationRunConfig::try_new(
        10_051,
        DateTime::<Utc>::from_timestamp_nanos(SEEDED_TEST_START_UTC_NANOS),
        1,
        one_interval_model_inputs(1.0, 1.0, 0.0),
        None,
    )
    .expect("second serialization run config should initialize");

    let second_scripted_factory = ScriptedRandomnessFactory::new(ScriptedRandomnessFactoryDraws {
        buy_count_draws: vec![0.0],
        sell_count_draws: vec![0.0],
        buy_timestamp_draws: vec![0.99],
        sell_timestamp_draws: vec![0.0],
        news_count_draws: vec![1.0],
        news_timestamp_draws: vec![],
        news_severity_draws: vec![],
        news_return_jump_draws: vec![],
    });

    let second_result = run_seeded_tick_event_simulation_with_randomness_factory(
        second_run_config,
        &second_scripted_factory,
    )
    .expect("second serialization run should complete");

    let second_payload: Value = serde_json::from_str(
        &second_result
            .serialize_event_tape_json()
            .expect("second event tape serialization should succeed"),
    )
    .expect("second serialized payload should parse");

    let second_events = second_payload["event_tape"]
        .as_array()
        .expect("second event_tape must serialize as an array");

    assert_eq!(second_events.len(), 1);

    let first_timestamp = first_events[0]["observed_timestamp_utc_nanos"]
        .as_i64()
        .expect("first timestamp should parse");

    let second_timestamp = second_events[0]["observed_timestamp_utc_nanos"]
        .as_i64()
        .expect("second timestamp should parse");

    assert_ne!(first_timestamp, second_timestamp);
    assert_eq!(second_events[0]["side"], "buy");
    assert_eq!(
        sorted_json_object_field_names(&second_events[0]),
        EVENT_TAPE_RECORD_FIELDS_WITHOUT_SNAPSHOT
    );
}

fn sorted_json_object_field_names(value: &Value) -> Vec<&str> {
    let object = value
        .as_object()
        .expect("JSON value must serialize as an object");

    let mut field_names: Vec<&str> = object.keys().map(String::as_str).collect();

    field_names.sort_unstable();

    field_names
}

fn is_non_decreasing_timestamp_path(run_result: &super::SeededTickSimulationRunResult) -> bool {
    run_result
        .projected_event_path()
        .windows(2)
        .all(|event_window| {
            event_window[0].observed_timestamp_utc_nanos
                <= event_window[1].observed_timestamp_utc_nanos
        })
}

fn assert_state_snapshot_close(
    observed: super::SeededTickEventStateSnapshot,
    expected: super::SeededTickEventStateSnapshot,
) {
    assert_float_close(observed.y_plus_plus, expected.y_plus_plus);
    assert_float_close(observed.y_plus_minus, expected.y_plus_minus);
    assert_float_close(observed.y_minus_plus, expected.y_minus_plus);
    assert_float_close(observed.y_minus_minus, expected.y_minus_minus);
    assert_float_close(
        observed.deseasonalized_lambda_plus,
        expected.deseasonalized_lambda_plus,
    );
    assert_float_close(
        observed.deseasonalized_lambda_minus,
        expected.deseasonalized_lambda_minus,
    );
}

fn assert_float_close(observed: f64, expected: f64) {
    assert!(
        (observed - expected).abs() <= 1e-12,
        "float mismatch, expected {expected}, observed {observed}"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureRandomnessFactory;

impl SimulationRandomnessFactory for FixtureRandomnessFactory {
    type Stream = FixtureRandomnessStream;

    fn create_substream(
        &self,
        _root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
    ) -> Self::Stream {
        FixtureRandomnessStream {
            substream_domain,
            substream_index,
            draw_count: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FixtureRandomnessStream {
    substream_domain: SimulationRandomnessSubstreamDomain,
    substream_index: u64,
    draw_count: u64,
}

impl SimulationRandomnessStream for FixtureRandomnessStream {
    fn next_u64(&mut self) -> u64 {
        self.draw_count = self.draw_count.saturating_add(1);

        match self.substream_domain {
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => 0,
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                u64::MAX.saturating_sub(1)
            }
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                self.substream_index.wrapping_add(self.draw_count)
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                self.substream_index.wrapping_add(self.draw_count)
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsCount => 0,
            SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => 0,
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity
            | SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => {
                0x9E37_79B9_7F4A_7C15_u64.wrapping_mul(
                    self.substream_index
                        .wrapping_add(self.draw_count)
                        .wrapping_add(1),
                )
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsMarks
            | SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn
            | SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling => {
                self.substream_index.wrapping_add(self.draw_count)
            }
            SimulationRandomnessSubstreamDomain::LongVarianceDiffusion => {
                self.substream_index.wrapping_add(self.draw_count)
            }
            SimulationRandomnessSubstreamDomain::TimestampBatchingObservation
            | SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment
            | SimulationRandomnessSubstreamDomain::StylizedFactsValidationSimulationPath
            | SimulationRandomnessSubstreamDomain::ObservedOptionsPhysicalDistributionPath => {
                self.substream_index.wrapping_add(self.draw_count)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptedRandomnessFactory {
    buy_count_draws: Vec<f64>,
    sell_count_draws: Vec<f64>,
    buy_timestamp_draws: Vec<f64>,
    sell_timestamp_draws: Vec<f64>,
    news_count_draws: Vec<f64>,
    news_timestamp_draws: Vec<f64>,
    news_severity_draws: Vec<f64>,
    news_return_jump_draws: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptedRandomnessFactoryDraws {
    buy_count_draws: Vec<f64>,
    sell_count_draws: Vec<f64>,
    buy_timestamp_draws: Vec<f64>,
    sell_timestamp_draws: Vec<f64>,
    news_count_draws: Vec<f64>,
    news_timestamp_draws: Vec<f64>,
    news_severity_draws: Vec<f64>,
    news_return_jump_draws: Vec<f64>,
}

impl ScriptedRandomnessFactory {
    fn new(draws: ScriptedRandomnessFactoryDraws) -> Self {
        let ScriptedRandomnessFactoryDraws {
            buy_count_draws,
            sell_count_draws,
            buy_timestamp_draws,
            sell_timestamp_draws,
            news_count_draws,
            news_timestamp_draws,
            news_severity_draws,
            news_return_jump_draws,
        } = draws;

        Self {
            buy_count_draws,
            sell_count_draws,
            buy_timestamp_draws,
            sell_timestamp_draws,
            news_count_draws,
            news_timestamp_draws,
            news_severity_draws,
            news_return_jump_draws,
        }
    }

    fn combined_news_mark_draws(&self) -> Vec<f64> {
        const SEVERITY_DRAWS_PER_EVENT: usize = 3;
        const RETURN_SHOCK_DRAWS_PER_EVENT: usize = 2;

        let severity_chunks = self.news_severity_draws.chunks(SEVERITY_DRAWS_PER_EVENT);

        let return_shock_chunks = self
            .news_return_jump_draws
            .chunks(RETURN_SHOCK_DRAWS_PER_EVENT);

        let combined_capacity = self
            .news_severity_draws
            .len()
            .saturating_add(self.news_return_jump_draws.len());

        let mut combined_draws = Vec::with_capacity(combined_capacity);

        for (severity_chunk, return_shock_chunk) in severity_chunks.zip(return_shock_chunks) {
            combined_draws.extend_from_slice(severity_chunk);
            combined_draws.extend_from_slice(return_shock_chunk);
        }

        let paired_event_count = self
            .news_severity_draws
            .len()
            .div_ceil(SEVERITY_DRAWS_PER_EVENT)
            .min(
                self.news_return_jump_draws
                    .len()
                    .div_ceil(RETURN_SHOCK_DRAWS_PER_EVENT),
            );

        let paired_severity_draw_count = paired_event_count * SEVERITY_DRAWS_PER_EVENT;
        let paired_return_shock_draw_count = paired_event_count * RETURN_SHOCK_DRAWS_PER_EVENT;

        if paired_severity_draw_count < self.news_severity_draws.len() {
            combined_draws
                .extend_from_slice(&self.news_severity_draws[paired_severity_draw_count..]);
        }

        if paired_return_shock_draw_count < self.news_return_jump_draws.len() {
            combined_draws
                .extend_from_slice(&self.news_return_jump_draws[paired_return_shock_draw_count..]);
        }

        combined_draws
    }
}

impl SimulationRandomnessFactory for ScriptedRandomnessFactory {
    type Stream = ScriptedRandomnessStream;

    fn create_substream(
        &self,
        _root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        _substream_index: u64,
    ) -> Self::Stream {
        let draws = match substream_domain {
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => {
                self.buy_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => {
                self.sell_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                self.buy_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => {
                self.sell_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsCount => {
                self.news_count_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => {
                self.news_timestamp_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => {
                self.news_severity_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => {
                self.news_return_jump_draws.clone()
            }
            SimulationRandomnessSubstreamDomain::ExogenousNewsMarks => {
                self.combined_news_mark_draws()
            }
            SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn
            | SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling
            | SimulationRandomnessSubstreamDomain::LongVarianceDiffusion
            | SimulationRandomnessSubstreamDomain::TimestampBatchingObservation
            | SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment
            | SimulationRandomnessSubstreamDomain::StylizedFactsValidationSimulationPath
            | SimulationRandomnessSubstreamDomain::ObservedOptionsPhysicalDistributionPath => {
                Vec::new()
            }
        };

        ScriptedRandomnessStream::new(draws)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ScriptedRandomnessStream {
    draws: Vec<f64>,
    position: usize,
}

impl ScriptedRandomnessStream {
    fn new(draws: Vec<f64>) -> Self {
        Self { draws, position: 0 }
    }
}

impl SimulationRandomnessStream for ScriptedRandomnessStream {
    fn next_u64(&mut self) -> u64 {
        let unit_draw = self.next_unit_interval_f64();

        (unit_draw.clamp(0.0, 1.0) * u64::MAX as f64).floor() as u64
    }

    fn next_unit_interval_f64(&mut self) -> f64 {
        let next_draw = self.draws.get(self.position).copied().unwrap_or(0.0);

        self.position = self.position.saturating_add(1);

        next_draw.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundaryAdjacentRandomnessFactory;

impl SimulationRandomnessFactory for BoundaryAdjacentRandomnessFactory {
    type Stream = ScriptedRandomnessStream;

    fn create_substream(
        &self,
        _root_seed: u64,
        substream_domain: SimulationRandomnessSubstreamDomain,
        substream_index: u64,
    ) -> Self::Stream {
        let draws = match substream_domain {
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::EndogenousSellEventCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::EndogenousBuyEventTimestamp => {
                if substream_index == 0 {
                    vec![0.135_410_650_56]
                } else {
                    vec![0.999_999_999_999]
                }
            }
            SimulationRandomnessSubstreamDomain::EndogenousSellEventTimestamp => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsCount => vec![0.0],
            SimulationRandomnessSubstreamDomain::ExogenousNewsTimestamp => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsSeverity => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsReturnJump => vec![],
            SimulationRandomnessSubstreamDomain::ExogenousNewsMarks
            | SimulationRandomnessSubstreamDomain::CoarseTradingTimeReturn
            | SimulationRandomnessSubstreamDomain::CoarseStatisticsSampling
            | SimulationRandomnessSubstreamDomain::LongVarianceDiffusion
            | SimulationRandomnessSubstreamDomain::TimestampBatchingObservation
            | SimulationRandomnessSubstreamDomain::ScaleConsistencyPairedExperiment
            | SimulationRandomnessSubstreamDomain::StylizedFactsValidationSimulationPath
            | SimulationRandomnessSubstreamDomain::ObservedOptionsPhysicalDistributionPath => {
                vec![]
            }
        };

        ScriptedRandomnessStream::new(draws)
    }
}

fn one_interval_model_inputs(
    lambda_plus: f64,
    lambda_minus: f64,
    news_intensity: f64,
) -> SeededTickSimulationModelInputs {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let interval_start_utc_nanos = vec![start_utc_nanos];
    let interval_end_utc_nanos = vec![start_utc_nanos + NANOS_PER_HOUR];

    let interval_factor_grid = IntervalFactorGrid {
        schema_version: INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
        exchange_code: "XNYS".to_string(),
        exchange_timezone_name: "America/New_York".to_string(),
        request_start_utc_nanos: start_utc_nanos,
        request_end_utc_nanos: start_utc_nanos + NANOS_PER_HOUR,
        interval_duration_nanos: NANOS_PER_HOUR,
        interval_indices: vec![0],
        interval_start_utc_nanos: interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_end_utc_nanos.clone(),
        interval_start_exchange_local: vec!["2024-01-22T09:00:00-05:00".to_string()],
        interval_end_exchange_local: vec!["2024-01-22T10:00:00-05:00".to_string()],
        seasonality_factor: vec![1.0],
        known_news_intensity: vec![0.0],
        surprise_news_intensity: vec![news_intensity],
        news_intensity: vec![news_intensity],
        metadata: IntervalFactorGridMetadata {
            seasonality_materialization_method: DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD
                .to_string(),
            news_intensity_materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
                .to_string(),
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalFactorGridDiagnostics {
            interval_count: 1,
            interval_duration_nanos: NANOS_PER_HOUR,
            first_interval_start_utc_nanos: interval_start_utc_nanos[0],
            last_interval_end_utc_nanos: interval_end_utc_nanos[0],
            skipped_exchange_local_interval_count: 0,
            repeated_exchange_local_interval_count: 0,
        },
    };

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&interval_factor_grid);

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.5,
            1.5,
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("one-interval kernel parameter fixture should be valid"),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: lambda_plus - 0.5,
            y_plus_minus: 0.0,
            y_minus_plus: lambda_minus - 0.5,
            y_minus_minus: 0.0,
        },
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        interval_factor_grid,
        return_clock_exposure_parameters:
            janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        long_variance_parameters: LongVarianceParameters::neutral(),
        initial_long_variance_state: LongVarianceState::unit(),
        observed_price_tick_size: None,
        timestamp_batching_observation_model:
            janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel::disabled(
            ),
        calendar_registry: build_seeded_simulation_calendar_registry(),
        nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
        news_process_configuration: build_news_process_configuration(Vec::new()),
    }
}

fn one_interval_closed_market_model_inputs() -> SeededTickSimulationModelInputs {
    let mut model_inputs = one_interval_model_inputs(20.0, 20.0, 0.0);

    let closed_interval_start_utc_nanos = 1_705_928_400_000_000_000;
    let closed_interval_end_utc_nanos = closed_interval_start_utc_nanos + NANOS_PER_HOUR;

    model_inputs.interval_factor_grid.request_start_utc_nanos = closed_interval_start_utc_nanos;
    model_inputs.interval_factor_grid.request_end_utc_nanos = closed_interval_end_utc_nanos;

    model_inputs.interval_factor_grid.interval_start_utc_nanos =
        vec![closed_interval_start_utc_nanos];

    model_inputs.interval_factor_grid.interval_end_utc_nanos = vec![closed_interval_end_utc_nanos];

    model_inputs
        .interval_factor_grid
        .interval_start_exchange_local = vec!["2024-01-22T08:00:00-05:00".to_string()];

    model_inputs
        .interval_factor_grid
        .interval_end_exchange_local = vec!["2024-01-22T09:00:00-05:00".to_string()];

    model_inputs
        .interval_factor_grid
        .diagnostics
        .first_interval_start_utc_nanos = closed_interval_start_utc_nanos;

    model_inputs
        .interval_factor_grid
        .diagnostics
        .last_interval_end_utc_nanos = closed_interval_end_utc_nanos;

    model_inputs
}

fn two_interval_zero_event_carryover_model_inputs() -> SeededTickSimulationModelInputs {
    let mut model_inputs = valid_model_inputs();

    model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        0.5,
        1.25,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("zero-event carryover kernel parameter fixture should be valid");

    model_inputs.initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.1,
        y_plus_minus: 0.05,
        y_minus_plus: 0.08,
        y_minus_minus: 0.02,
    };

    model_inputs.interval_factor_grid.seasonality_factor = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.news_intensity = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.known_news_intensity = vec![0.0, 0.0];
    model_inputs.interval_factor_grid.surprise_news_intensity = vec![0.0, 0.0];

    model_inputs
}

fn two_interval_constant_intensity_model_inputs(
    second_interval_seasonality_factor: f64,
) -> SeededTickSimulationModelInputs {
    let mut model_inputs = valid_model_inputs();

    model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        2.0,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("constant-intensity kernel parameter fixture should be valid");

    model_inputs.initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    model_inputs.interval_factor_grid.seasonality_factor =
        vec![1.0, second_interval_seasonality_factor];

    model_inputs.interval_factor_grid.news_intensity = vec![0.0, 0.0];

    model_inputs
}

fn two_interval_boundary_adjacent_model_inputs() -> SeededTickSimulationModelInputs {
    let mut model_inputs = valid_model_inputs();

    model_inputs.kernel_parameters = HawkesKernelParameters::try_new(
        1.0,
        1.0,
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
        HawkesKernelLoadings {
            plus_plus: 0.0,
            plus_minus: 0.0,
            minus_plus: 0.0,
            minus_minus: 0.0,
        },
    )
    .expect("boundary-adjacent kernel parameter fixture should be valid");

    model_inputs.initial_excitation_state = HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    };

    model_inputs.interval_factor_grid.seasonality_factor = vec![1.0, 1.0];
    model_inputs.interval_factor_grid.news_intensity = vec![0.0, 0.0];

    model_inputs
}

fn override_interval_factor_grid(
    model_inputs: &mut SeededTickSimulationModelInputs,
    interval_start_utc_nanos: &[i64],
    interval_end_utc_nanos: &[i64],
    seasonality_factors: &[f64],
    news_intensity_factors: &[f64],
) {
    let interval_factor_grid = &mut model_inputs.interval_factor_grid;

    interval_factor_grid.request_start_utc_nanos = interval_start_utc_nanos[0];

    interval_factor_grid.request_end_utc_nanos =
        interval_end_utc_nanos[interval_end_utc_nanos.len() - 1];

    interval_factor_grid.interval_indices = (0..interval_start_utc_nanos.len())
        .map(|interval_position| interval_position as u64)
        .collect();

    interval_factor_grid.interval_start_utc_nanos = interval_start_utc_nanos.to_vec();
    interval_factor_grid.interval_end_utc_nanos = interval_end_utc_nanos.to_vec();

    interval_factor_grid.interval_start_exchange_local = interval_start_utc_nanos
        .iter()
        .map(|timestamp_utc_nanos| {
            DateTime::<Utc>::from_timestamp_nanos(*timestamp_utc_nanos).to_rfc3339()
        })
        .collect();

    interval_factor_grid.interval_end_exchange_local = interval_end_utc_nanos
        .iter()
        .map(|timestamp_utc_nanos| {
            DateTime::<Utc>::from_timestamp_nanos(*timestamp_utc_nanos).to_rfc3339()
        })
        .collect();

    interval_factor_grid.interval_duration_nanos =
        interval_end_utc_nanos[0] - interval_start_utc_nanos[0];

    interval_factor_grid.seasonality_factor = seasonality_factors.to_vec();
    interval_factor_grid.news_intensity = news_intensity_factors.to_vec();
    interval_factor_grid.diagnostics.interval_count = interval_start_utc_nanos.len();

    interval_factor_grid.diagnostics.interval_duration_nanos =
        interval_factor_grid.interval_duration_nanos;

    interval_factor_grid
        .diagnostics
        .first_interval_start_utc_nanos = interval_start_utc_nanos[0];

    interval_factor_grid.diagnostics.last_interval_end_utc_nanos =
        interval_end_utc_nanos[interval_end_utc_nanos.len() - 1];

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(interval_factor_grid);

    model_inputs.mean_reversion_fitted_parameters = mean_reversion_fitted_parameters;
    model_inputs.mean_reversion_runtime_interval_anchors = mean_reversion_runtime_interval_anchors;
    model_inputs.initial_mean_reversion_boundary_state = initial_mean_reversion_boundary_state;
}

fn timestamp_utc_nanos(raw_timestamp_utc: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw_timestamp_utc)
        .expect("fixture timestamp must be RFC3339")
        .with_timezone(&Utc)
        .timestamp_nanos_opt()
        .expect("fixture timestamp must be in nanosecond range")
}

fn valid_model_inputs() -> SeededTickSimulationModelInputs {
    let start_utc_nanos = SEEDED_TEST_START_UTC_NANOS;
    let interval_start_utc_nanos = vec![start_utc_nanos, start_utc_nanos + NANOS_PER_HOUR];

    let interval_end_utc_nanos = vec![
        start_utc_nanos + NANOS_PER_HOUR,
        start_utc_nanos + 2 * NANOS_PER_HOUR,
    ];

    let interval_factor_grid = IntervalFactorGrid {
        schema_version: INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
        exchange_code: "XNYS".to_string(),
        exchange_timezone_name: "America/New_York".to_string(),
        request_start_utc_nanos: start_utc_nanos,
        request_end_utc_nanos: start_utc_nanos + 2 * NANOS_PER_HOUR,
        interval_duration_nanos: NANOS_PER_HOUR,
        interval_indices: vec![0, 1],
        interval_start_utc_nanos: interval_start_utc_nanos.clone(),
        interval_end_utc_nanos: interval_end_utc_nanos.clone(),
        interval_start_exchange_local: vec![
            "2024-01-22T09:00:00-05:00".to_string(),
            "2024-01-22T10:00:00-05:00".to_string(),
        ],
        interval_end_exchange_local: vec![
            "2024-01-22T10:00:00-05:00".to_string(),
            "2024-01-22T11:00:00-05:00".to_string(),
        ],
        seasonality_factor: vec![0.95, 1.05],
        known_news_intensity: vec![0.0, 0.0],
        surprise_news_intensity: vec![0.02, 0.03],
        news_intensity: vec![0.02, 0.03],
        metadata: IntervalFactorGridMetadata {
            seasonality_materialization_method: DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD
                .to_string(),
            news_intensity_materialization_method: DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD
                .to_string(),
            interval_boundary_policy: INTERVAL_FACTOR_GRID_INTERVAL_BOUNDARY_POLICY,
            interval_alignment_policy: INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            timezone_transition_policy: INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY,
            spring_forward_policy: INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY,
            fall_back_policy: INTERVAL_FACTOR_GRID_FALL_BACK_POLICY,
        },
        diagnostics: IntervalFactorGridDiagnostics {
            interval_count: 2,
            interval_duration_nanos: NANOS_PER_HOUR,
            first_interval_start_utc_nanos: interval_start_utc_nanos[0],
            last_interval_end_utc_nanos: interval_end_utc_nanos[1],
            skipped_exchange_local_interval_count: 0,
            repeated_exchange_local_interval_count: 0,
        },
    };

    let (
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
    ) = disabled_mean_reversion_runtime_inputs(&interval_factor_grid);

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.8,
            1.5,
            HawkesKernelLoadings {
                plus_plus: 0.3,
                plus_minus: 0.15,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.05,
                plus_minus: 0.02,
                minus_plus: 0.03,
                minus_minus: 0.06,
            },
        )
        .expect("kernel parameter fixture should be valid"),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.15,
            y_minus_minus: 0.3,
        },
        mean_reversion_fitted_parameters,
        mean_reversion_runtime_interval_anchors,
        initial_mean_reversion_boundary_state,
        interval_factor_grid,
        return_clock_exposure_parameters:
            janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
        long_variance_parameters: LongVarianceParameters::neutral(),
        initial_long_variance_state: LongVarianceState::unit(),
        observed_price_tick_size: None,
        timestamp_batching_observation_model:
            janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel::disabled(
            ),
        calendar_registry: build_seeded_simulation_calendar_registry(),
        nig_shared_plus_shift_configuration: valid_nig_shared_plus_shift_configuration(),
        news_process_configuration: build_news_process_configuration(Vec::new()),
    }
}

fn disabled_mean_reversion_runtime_inputs(
    interval_factor_grid: &IntervalFactorGrid,
) -> (
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
) {
    let interval_deltas = interval_factor_grid
        .interval_start_utc_nanos
        .iter()
        .zip(interval_factor_grid.interval_end_utc_nanos.iter())
        .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
            (*interval_end_utc_nanos - *interval_start_utc_nanos) as f64 / NANOS_PER_HOUR as f64
        })
        .collect::<Vec<_>>();
    (
        MeanReversionFittedParameters::flat_anchor_mean_reversion(0.0)
            .expect("zero-gain mean reversion should construct"),
        MeanReversionRuntimeIntervalAnchors::try_new(
            interval_factor_grid.metadata.interval_alignment_policy,
            0.0,
            0.0,
            vec![0.0; interval_factor_grid.interval_indices.len()],
            vec![0.0; interval_factor_grid.interval_indices.len()],
            interval_deltas,
        )
        .expect("disabled mean-reversion runtime anchors should construct"),
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
            .expect("disabled mean-reversion boundary state should construct"),
    )
}

fn configure_mean_reversion_runtime_inputs(
    model_inputs: &mut SeededTickSimulationModelInputs,
    kappa: f64,
    start_anchor_value: f64,
    end_anchor_values: &[f64],
    initial_observed_log_price: f64,
) {
    let anchor_model = MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level();

    model_inputs.mean_reversion_fitted_parameters =
        MeanReversionFittedParameters::try_new(kappa, anchor_model)
            .expect("mean-reversion fitted parameters should construct");

    model_inputs.mean_reversion_runtime_interval_anchors =
        MeanReversionRuntimeIntervalAnchors::try_new(
            INTERVAL_FACTOR_GRID_INTERVAL_ALIGNMENT_POLICY,
            start_anchor_value,
            0.0,
            end_anchor_values.to_vec(),
            vec![0.0; end_anchor_values.len()],
            model_inputs
                .interval_factor_grid
                .interval_start_utc_nanos
                .iter()
                .zip(
                    model_inputs
                        .interval_factor_grid
                        .interval_end_utc_nanos
                        .iter(),
                )
                .map(|(interval_start_utc_nanos, interval_end_utc_nanos)| {
                    (*interval_end_utc_nanos - *interval_start_utc_nanos) as f64
                        / NANOS_PER_HOUR as f64
                })
                .collect(),
        )
        .expect("mean-reversion runtime interval anchors should construct");

    model_inputs.initial_mean_reversion_boundary_state = MeanReversionBoundaryState::try_new(
        initial_observed_log_price,
        start_anchor_value,
        0.0,
        initial_observed_log_price - start_anchor_value,
    )
    .expect("mean-reversion boundary state should construct");
}

fn build_seeded_simulation_calendar_registry() -> ExchangeCalendarRegistry {
    ExchangeCalendarRegistryLoader::load_from_json(
        r#"
            {
              "schema_version": "calendar.registry.v1",
              "artifact_date": "2026-01-31",
              "exchanges": [
                {
                  "schema_version": "calendar.v1",
                  "exchange_code": "XNYS",
                  "exchange_name": "New York Stock Exchange",
                  "timezone_name": "America/New_York",
                  "rule_metadata": {
                    "holiday_calendar_code": "US_EQUITIES",
                    "early_close_rule_set": "US_EQUITIES_EARLY_CLOSE"
                  },
                  "regular_sessions": [
                    {
                      "weekday": "Monday",
                      "windows": [
                        {"label": "regular", "open_time": "09:00", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Tuesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:00", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Wednesday",
                      "windows": [
                        {"label": "regular", "open_time": "09:00", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Thursday",
                      "windows": [
                        {"label": "regular", "open_time": "09:00", "close_time": "16:00"}
                      ]
                    },
                    {
                      "weekday": "Friday",
                      "windows": [
                        {"label": "regular", "open_time": "09:00", "close_time": "16:00"}
                      ]
                    }
                  ],
                  "holiday_dates": ["2024-12-25"],
                  "early_close_sessions": [
                    {
                      "date": "2024-11-29",
                      "windows": [
                        {"label": "post_thanksgiving", "open_time": "09:00", "close_time": "13:00"}
                      ]
                    }
                  ]
                }
              ]
            }
            "#,
    )
    .expect("calendar registry fixture should load")
}
