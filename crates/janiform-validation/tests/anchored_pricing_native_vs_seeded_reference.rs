mod support;

use std::fs;

use chrono::{DateTime, Utc};
use janiform_engine_pricing::{
    AnchoredPricingRecursionDiagnostics, AnchoredVanillaOptionPricingBoundaryInput,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingExecutionInput,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingModelParameters, VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest, VanillaOptionPricingRunError, VanillaOptionPricingRunResult,
    run_vanilla_option_pricing,
};
use janiform_engine_simulation::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors, SeededTickSimulationModelInputs,
    SeededTickSimulationRunConfig, run_seeded_tick_event_simulation,
};
use janiform_foundations::time::ONE_HOUR_NANOS;
use janiform_market_time::calendar::{ExchangeCalendarRegistry, ExchangeCalendarRegistryLoader};
use janiform_market_time::hourly_factors::{
    DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD, DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD,
    HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION, HOURLY_FACTOR_FALL_BACK_POLICY,
    HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY, HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
    HOURLY_FACTOR_SPRING_FORWARD_POLICY, HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
    HourlyFactorContractDiagnostics, HourlyFactorContractOutput,
    HourlyFactorMaterializationMetadata,
};
use janiform_model_kernels::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform_model_kernels::news::{
    CoupledGapMarkConfiguration, DoubleExponentialBaseShockConfiguration,
    GammaSeverityMarkConfiguration as NewsGammaSeverityMarkConfiguration,
    KnownNewsArrivalConfiguration, NewsArrivalConfiguration, NewsComponentMarkScaleConfiguration,
    NewsProcessConfiguration, SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use serde::Deserialize;

use support::validation_fixture::fixtures_root_path;

const SYNTHETIC_START_UTC_NANOS: i64 = 1_705_932_000_000_000_000;
const SYNTHETIC_INTERVAL_ALIGNMENT_POLICY: &str =
    "request_start_anchored_fixed_width_utc_interval_grid.v1";

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct AnchoredPricingReferencePolicy {
    #[allow(dead_code)]
    schema_version: String,
    #[allow(dead_code)]
    policy_id: String,
    #[allow(dead_code)]
    policy_version: String,
    simulation_reference: AnchoredPricingSimulationReferencePolicy,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct AnchoredPricingSimulationReferencePolicy {
    path_count: usize,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct AnchoredPricingReferenceBaselineArtifact {
    #[allow(dead_code)]
    schema_version: String,
    #[allow(dead_code)]
    baseline_id: String,
    #[allow(dead_code)]
    baseline_version: String,
    regression_tolerances: AnchoredPricingReferenceRegressionTolerances,
    cases: Vec<AnchoredPricingReferenceBaselineCase>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct AnchoredPricingReferenceRegressionTolerances {
    native_price_absolute_tolerance: f64,
    native_minus_simulation_gap_absolute_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case", deny_unknown_fields)]
enum AnchoredPricingReferenceBaselineCase {
    Success {
        scenario_id: String,
        #[allow(dead_code)]
        kappa: f64,
        #[allow(dead_code)]
        observed_boundary_state: AnchoredPricingObservedBoundaryState,
        #[allow(dead_code)]
        current_anchor_value: f64,
        #[allow(dead_code)]
        next_anchor_value: f64,
        #[allow(dead_code)]
        observed_return_adjustment: f64,
        native_price: f64,
        simulation_reference_price: f64,
        native_minus_simulation_absolute_gap: f64,
        #[allow(dead_code)]
        local_martingale_diagnostic_mode: String,
        local_tilt_solve_count: usize,
        deviation_grid_node_count: usize,
        refined_deviation_grid_node_count: usize,
        interval_count: usize,
        maximum_tail_probability_mass: f64,
        maximum_row_probability_mass_error: f64,
        integration_numeric_quality_is_poor: bool,
        integration_convergence_check_passed: bool,
        integration_truncation_check_passed: bool,
    },
    ExpectedFailure {
        scenario_id: String,
        #[allow(dead_code)]
        kappa: f64,
        #[allow(dead_code)]
        observed_boundary_state: AnchoredPricingObservedBoundaryState,
        #[allow(dead_code)]
        current_anchor_value: f64,
        #[allow(dead_code)]
        next_anchor_value: f64,
        #[allow(dead_code)]
        observed_return_adjustment: f64,
        #[allow(dead_code)]
        local_martingale_diagnostic_mode: String,
        expected_error_substring: String,
        expected_error_detail_substring: String,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct AnchoredPricingObservedBoundaryState {
    #[allow(dead_code)]
    observed_log_price: f64,
    #[allow(dead_code)]
    anchor_value: f64,
    #[allow(dead_code)]
    deviation: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioAnchorKind {
    Endogenous,
}

#[derive(Debug, Clone, PartialEq)]
struct ScenarioDefinition {
    scenario_id: &'static str,
    anchor_kind: ScenarioAnchorKind,
    current_anchor_value: f64,
    next_anchor_value: f64,
    observed_boundary_state: MeanReversionBoundaryState,
    kappa: f64,
}

#[test]
#[ignore = "explicit governance benchmark for anchored native-vs-seeded pricing reference"]
fn anchored_pricing_native_prices_match_seeded_reference_baseline() {
    let policy = load_reference_policy_fixture();
    let baseline = load_reference_baseline_fixture();

    for baseline_case in &baseline.cases {
        let scenario = scenario_definition(baseline_case.scenario_id());

        match baseline_case {
            AnchoredPricingReferenceBaselineCase::Success {
                native_price,
                simulation_reference_price,
                native_minus_simulation_absolute_gap,
                local_tilt_solve_count,
                deviation_grid_node_count,
                refined_deviation_grid_node_count,
                interval_count,
                maximum_tail_probability_mass,
                maximum_row_probability_mass_error,
                integration_numeric_quality_is_poor,
                integration_convergence_check_passed,
                integration_truncation_check_passed,
                ..
            } => {
                let native_run_result = run_native_pricer_for_scenario(&scenario)
                    .expect("native anchored pricer should succeed");
                let observed_native_price = native_run_result.response.price;
                let observed_simulation_reference_price = estimate_seeded_reference_price(
                    &scenario,
                    policy.simulation_reference.path_count,
                );
                let observed_gap =
                    (observed_native_price - observed_simulation_reference_price).abs();

                assert!(
                    (observed_native_price - *native_price).abs()
                        <= baseline
                            .regression_tolerances
                            .native_price_absolute_tolerance,
                    "native anchored price drifted for scenario `{}`: observed={} baseline={}",
                    scenario.scenario_id,
                    observed_native_price,
                    native_price
                );
                assert_eq!(
                    observed_simulation_reference_price,
                    *simulation_reference_price
                );
                assert!(
                    (observed_gap - *native_minus_simulation_absolute_gap).abs()
                        <= baseline
                            .regression_tolerances
                            .native_minus_simulation_gap_absolute_tolerance,
                    "native-vs-seeded absolute gap drifted for scenario `{}`: observed={} baseline={}",
                    scenario.scenario_id,
                    observed_gap,
                    native_minus_simulation_absolute_gap
                );

                let run_diagnostics = native_run_result
                    .run_diagnostics
                    .as_ref()
                    .expect("run diagnostics should be present");
                let anchored_diagnostics = run_diagnostics
                    .anchored_recursion
                    .as_ref()
                    .expect("anchored recursion diagnostics should be present");

                assert_anchored_diagnostics_match_baseline(
                    anchored_diagnostics,
                    *local_tilt_solve_count,
                    *deviation_grid_node_count,
                    *refined_deviation_grid_node_count,
                    *interval_count,
                    *maximum_tail_probability_mass,
                    *maximum_row_probability_mass_error,
                );

                let integration_quality = run_diagnostics
                    .integration_quality
                    .as_ref()
                    .expect("integration diagnostics should be present");
                let numeric_quality = &integration_quality.numeric_quality_diagnostics;

                assert_eq!(
                    numeric_quality.numeric_quality_is_poor,
                    *integration_numeric_quality_is_poor
                );
                assert_eq!(
                    numeric_quality.convergence_check_passed,
                    *integration_convergence_check_passed
                );
                assert_eq!(
                    numeric_quality.truncation_check_passed,
                    *integration_truncation_check_passed
                );
            }
            AnchoredPricingReferenceBaselineCase::ExpectedFailure {
                expected_error_substring,
                expected_error_detail_substring,
                ..
            } => {
                let error_text = run_native_pricer_for_scenario(&scenario)
                    .expect_err("inadmissible anchored scenario should fail")
                    .to_string();
                assert!(
                    error_text.contains(expected_error_substring),
                    "unexpected anchored pricing failure for scenario `{}`: {}",
                    scenario.scenario_id,
                    error_text
                );
                assert!(
                    error_text.contains(expected_error_detail_substring),
                    "missing anchored admissibility detail for scenario `{}`: {}",
                    scenario.scenario_id,
                    error_text
                );
            }
        }
    }
}

impl AnchoredPricingReferenceBaselineCase {
    fn scenario_id(&self) -> &str {
        match self {
            Self::Success { scenario_id, .. } | Self::ExpectedFailure { scenario_id, .. } => {
                scenario_id.as_str()
            }
        }
    }
}

fn assert_anchored_diagnostics_match_baseline(
    diagnostics: &AnchoredPricingRecursionDiagnostics,
    local_tilt_solve_count: usize,
    deviation_grid_node_count: usize,
    refined_deviation_grid_node_count: usize,
    interval_count: usize,
    maximum_tail_probability_mass: f64,
    maximum_row_probability_mass_error: f64,
) {
    assert_eq!(
        diagnostics.deviation_grid_node_count,
        deviation_grid_node_count
    );
    assert_eq!(
        diagnostics.refined_deviation_grid_node_count,
        refined_deviation_grid_node_count
    );
    assert_eq!(diagnostics.interval_diagnostics.len(), interval_count);

    let observed_local_tilt_solve_count = diagnostics
        .interval_diagnostics
        .iter()
        .map(|interval_diagnostics| interval_diagnostics.local_tilt_solve_diagnostics.len())
        .sum::<usize>();
    assert_eq!(observed_local_tilt_solve_count, local_tilt_solve_count);

    let observed_maximum_tail_probability_mass = diagnostics
        .interval_diagnostics
        .iter()
        .map(|interval_diagnostics| interval_diagnostics.maximum_tail_probability_mass)
        .fold(0.0_f64, f64::max);
    let observed_maximum_row_probability_mass_error = diagnostics
        .interval_diagnostics
        .iter()
        .map(|interval_diagnostics| interval_diagnostics.maximum_row_probability_mass_error)
        .fold(0.0_f64, f64::max);

    assert_eq!(
        observed_maximum_tail_probability_mass,
        maximum_tail_probability_mass
    );
    assert_eq!(
        observed_maximum_row_probability_mass_error,
        maximum_row_probability_mass_error
    );
}

fn estimate_seeded_reference_price(scenario: &ScenarioDefinition, path_count: usize) -> f64 {
    let market_inputs = pricing_market_inputs();
    let discount_factor = (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();
    let initial_observed_log_price = scenario.observed_boundary_state.observed_log_price;

    let mut discounted_payoff_sum = 0.0_f64;

    for path_index in 0..path_count {
        let run_config = SeededTickSimulationRunConfig::try_new(
            10_000 + path_index as u64,
            DateTime::<Utc>::from_timestamp_nanos(SYNTHETIC_START_UTC_NANOS),
            1,
            seeded_model_inputs_for_scenario(scenario),
            None,
        )
        .expect("seeded reference run config should construct");

        let run_result = run_seeded_tick_event_simulation(run_config)
            .expect("seeded reference run should succeed");

        let terminal_observed_log_price = run_result
            .fine_path
            .last()
            .expect("seeded reference path should include interval-boundary record")
            .observed_log_price;

        let terminal_observed_log_return = terminal_observed_log_price - initial_observed_log_price;
        let terminal_spot = market_inputs.spot * terminal_observed_log_return.exp();

        let payoff = (terminal_spot - market_inputs.strike).max(0.0);

        discounted_payoff_sum += discount_factor * payoff;
    }

    discounted_payoff_sum / path_count as f64
}

fn run_native_pricer_for_scenario(
    scenario: &ScenarioDefinition,
) -> Result<VanillaOptionPricingRunResult, VanillaOptionPricingRunError> {
    let execution_input = VanillaOptionPricingExecutionInput::try_new(
        VanillaOptionPricingRequest::try_new(
            pricing_market_inputs(),
            VanillaOptionPayoffConfiguration {
                payoff_type: VanillaOptionPayoffType::Call,
            },
            anchored_boundary_input_for_scenario(scenario)
                .expect("anchored boundary input should construct"),
            pricing_model_parameters(),
        )
        .expect("native pricing request should construct"),
        vec![
            VanillaOptionPricingIntervalFactors::try_new(0.0, 0.0, 0.0)
                .expect("zero-factor pricing interval should construct"),
        ],
        VanillaOptionPricingOptionalOutputSelection::new(true, true, true),
    )
    .expect("native pricing execution input should construct");

    run_vanilla_option_pricing(execution_input)
}

fn pricing_market_inputs() -> VanillaOptionPricingMarketInputs {
    VanillaOptionPricingMarketInputs::try_new(100.0, 80.0, 1.0, 0.0, 0.0)
        .expect("synthetic pricing market inputs should construct")
}

fn pricing_model_parameters() -> VanillaOptionPricingModelParameters {
    VanillaOptionPricingModelParameters::try_new(
        HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.1,
                plus_minus: 0.05,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("pricing kernel parameters should construct"),
        NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: vec![],
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: NewsGammaSeverityMarkConfiguration::new(1.8, 0.7),
                double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::new(
                    0.35, 2.1, 3.0,
                ),
                coupled_gap_marks: CoupledGapMarkConfiguration::new(0.6),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_scale: 1.0,
            },
        )
        .expect("pricing news process configuration should construct"),
        NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
            .expect("pricing NIG configuration should construct"),
    )
    .expect("pricing model parameters should construct")
}

fn anchored_boundary_input_for_scenario(
    scenario: &ScenarioDefinition,
) -> Result<
    AnchoredVanillaOptionPricingBoundaryInput,
    janiform_engine_pricing::AnchoredVanillaOptionPricingBoundaryInputError,
> {
    AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        mean_reversion_fitted_parameters_for_scenario(scenario),
        1.0,
        scenario.observed_boundary_state.clone(),
    )
}

fn mean_reversion_fitted_parameters_for_scenario(
    scenario: &ScenarioDefinition,
) -> MeanReversionFittedParameters {
    let _ = scenario.anchor_kind;

    MeanReversionFittedParameters::try_new(
        scenario.kappa,
        MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
    )
    .expect("endogenous fitted parameters should construct")
}

fn mean_reversion_runtime_interval_anchors_for_scenario(
    scenario: &ScenarioDefinition,
) -> MeanReversionRuntimeIntervalAnchors {
    MeanReversionRuntimeIntervalAnchors::try_new(
        SYNTHETIC_INTERVAL_ALIGNMENT_POLICY,
        scenario.current_anchor_value,
        0.0,
        vec![scenario.next_anchor_value],
        vec![0.0],
        vec![1.0],
    )
    .expect("runtime interval anchors should construct")
}

fn seeded_model_inputs_for_scenario(
    scenario: &ScenarioDefinition,
) -> SeededTickSimulationModelInputs {
    let interval_start_utc_nanos = vec![SYNTHETIC_START_UTC_NANOS];
    let interval_end_utc_nanos = vec![SYNTHETIC_START_UTC_NANOS + ONE_HOUR_NANOS];

    SeededTickSimulationModelInputs {
        kernel_parameters: HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.1,
                plus_minus: 0.05,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("seeded kernel parameters should construct"),
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
            2.3, -0.8, 0.7, 0.1, -0.2,
        )
        .expect("seeded NIG configuration should construct"),
        news_process_configuration: build_news_process_configuration(),
        initial_excitation_state: HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        mean_reversion_fitted_parameters: mean_reversion_fitted_parameters_for_scenario(scenario),
        mean_reversion_runtime_interval_anchors:
            mean_reversion_runtime_interval_anchors_for_scenario(scenario),
        initial_mean_reversion_boundary_state: scenario.observed_boundary_state.clone(),
        hourly_factor_contract: HourlyFactorContractOutput {
            schema_version: HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
            exchange_code: "XNYS".to_string(),
            exchange_timezone_name: "America/New_York".to_string(),
            request_start_utc_nanos: SYNTHETIC_START_UTC_NANOS,
            request_end_utc_nanos: SYNTHETIC_START_UTC_NANOS + ONE_HOUR_NANOS,
            interval_duration_nanos: ONE_HOUR_NANOS,
            interval_indices: vec![0],
            interval_start_utc_nanos: interval_start_utc_nanos.clone(),
            interval_end_utc_nanos: interval_end_utc_nanos.clone(),
            interval_start_exchange_local: vec!["2024-01-22T09:00:00-05:00".to_string()],
            interval_end_exchange_local: vec!["2024-01-22T10:00:00-05:00".to_string()],
            s_h: vec![0.0],
            known_component_nu_h: vec![0.0],
            surprise_component_nu_h: vec![0.0],
            nu_h: vec![0.0],
            metadata: HourlyFactorMaterializationMetadata {
                seasonality_materialization_method: DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD
                    .to_string(),
                news_intensity_materialization_method:
                    DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD.to_string(),
                interval_boundary_policy: HOURLY_FACTOR_INTERVAL_BOUNDARY_POLICY,
                interval_alignment_policy: HOURLY_FACTOR_INTERVAL_ALIGNMENT_POLICY,
                timezone_transition_policy: HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY,
                spring_forward_policy: HOURLY_FACTOR_SPRING_FORWARD_POLICY,
                fall_back_policy: HOURLY_FACTOR_FALL_BACK_POLICY,
            },
            diagnostics: HourlyFactorContractDiagnostics {
                interval_count: 1,
                interval_duration_nanos: ONE_HOUR_NANOS,
                first_interval_start_utc_nanos: interval_start_utc_nanos[0],
                last_interval_end_utc_nanos: interval_end_utc_nanos[0],
                skipped_local_hour_interval_count: 0,
                repeated_local_hour_interval_count: 0,
            },
        },
        calendar_registry: build_calendar_registry(),
    }
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
            gamma_severity_marks: NewsGammaSeverityMarkConfiguration::try_new(1.5, 0.9)
                .expect("news gamma severity configuration should construct"),
            double_exponential_base_shock: DoubleExponentialBaseShockConfiguration::try_new(
                0.4, 1.3, 1.1,
            )
            .expect("news base-shock configuration should construct"),
            coupled_gap_marks: CoupledGapMarkConfiguration::new(0.75),
        },
        default_news_component_mark_scales(),
        default_news_component_mark_scales(),
    )
    .expect("news process configuration should construct")
}

fn default_news_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
    NewsComponentMarkScaleConfiguration {
        activity_coupling_scale: 0.6,
        return_jump_scale: 1.2,
    }
}

fn build_calendar_registry() -> ExchangeCalendarRegistry {
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
    .expect("calendar registry should construct")
}

fn scenario_definition(scenario_id: &str) -> ScenarioDefinition {
    match scenario_id {
        "endogenous_zero_deviation_zero_kappa" => ScenarioDefinition {
            scenario_id: "endogenous_zero_deviation_zero_kappa",
            anchor_kind: ScenarioAnchorKind::Endogenous,
            current_anchor_value: 0.0,
            next_anchor_value: 0.0,
            observed_boundary_state: MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
                .expect("zero-deviation observed boundary state should construct"),
            kappa: 0.0,
        },
        "endogenous_aligned_anchor_zero_deviation" => ScenarioDefinition {
            scenario_id: "endogenous_aligned_anchor_zero_deviation",
            anchor_kind: ScenarioAnchorKind::Endogenous,
            current_anchor_value: 0.25,
            next_anchor_value: 0.25,
            observed_boundary_state: MeanReversionBoundaryState::try_new(0.25, 0.25, 0.0, 0.0)
                .expect("aligned-anchor observed boundary state should construct"),
            kappa: 0.35,
        },
        "endogenous_flat_anchor_zero_deviation" => ScenarioDefinition {
            scenario_id: "endogenous_flat_anchor_zero_deviation",
            anchor_kind: ScenarioAnchorKind::Endogenous,
            current_anchor_value: 0.25,
            next_anchor_value: 0.25,
            observed_boundary_state: MeanReversionBoundaryState::try_new(0.25, 0.25, 0.0, 0.0)
                .expect("flat-anchor observed boundary state should construct"),
            kappa: 0.35,
        },
        "endogenous_positive_deviation_admissibility_failure" => ScenarioDefinition {
            scenario_id: "endogenous_positive_deviation_admissibility_failure",
            anchor_kind: ScenarioAnchorKind::Endogenous,
            current_anchor_value: 0.0,
            next_anchor_value: 0.0,
            observed_boundary_state: MeanReversionBoundaryState::try_new(0.01, 0.0, 0.0, 0.01)
                .expect("positive-deviation observed boundary state should construct"),
            kappa: 0.35,
        },
        _ => panic!("unsupported anchored pricing reference scenario `{scenario_id}`"),
    }
}

fn load_reference_policy_fixture() -> AnchoredPricingReferencePolicy {
    serde_json::from_str(&load_benchmark_fixture_payload(
        "anchored_pricing_reference_policy.json",
    ))
    .expect("anchored pricing reference policy should parse")
}

fn load_reference_baseline_fixture() -> AnchoredPricingReferenceBaselineArtifact {
    serde_json::from_str(&load_benchmark_fixture_payload(
        "anchored_pricing_reference_baseline_v1.json",
    ))
    .expect("anchored pricing reference baseline should parse")
}

fn load_benchmark_fixture_payload(file_name: &str) -> String {
    let fixture_path = fixtures_root_path().join("benchmarks").join(file_name);
    fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read benchmark fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}
