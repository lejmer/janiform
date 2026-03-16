mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use janiform_engine_pricing::{
    FourierInversionContour, FourierInversionDiagnostics, FourierInversionEngineSettings,
    FourierInversionIntegrationStabilityControls, FourierInversionNumericQualityDiagnostics,
    FourierInversionNumericQualityGate, FourierInversionNumericSettings,
    FourierInversionPricingError, FourierInversionQuadratureRule, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    price_vanilla_option_via_fourier_inversion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use support::pricing_reference::{
    black_scholes_log_return_characteristic_function, black_scholes_price,
    no_arbitrage_price_bounds,
};
use support::validation_fixture::fixtures_root_path;

const PRICING_BENCHMARK_FIXTURE_SCHEMA_VERSION: &str =
    "pricing.vanilla_option.benchmark_fixture.v1";
const PRICING_BENCHMARK_FIXTURE_SET_ID: &str = "vanilla_option_pricing_benchmark_fixture_v1";
const PRICING_BENCHMARK_RESULT_SCHEMA_VERSION: &str = "pricing.vanilla_option.benchmark_result.v1";
const PRICING_BENCHMARK_TOLERANCE_POLICY_SCHEMA_VERSION: &str =
    "pricing.vanilla_option.benchmark_tolerance_policy.v1";
const PRICING_BENCHMARK_FIXTURE_LOADING_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
const PRICING_BENCHMARK_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
const PRICING_BENCHMARK_CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    fixture_set_version: String,
    determinism_metadata: PricingBenchmarkDeterminismMetadata,
    benchmark_result_schema_metadata: PricingBenchmarkResultSchemaMetadata,
    tolerance_policy: PricingBenchmarkTolerancePolicy,
    sanity_guardrails: PricingBenchmarkSanityGuardrails,
    canonical_regression_metadata: PricingBenchmarkCanonicalRegressionMetadata,
    cases: Vec<PricingBenchmarkCaseFixture>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkDeterminismMetadata {
    fixture_loading_order: String,
    content_hash_scope: String,
    content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkResultSchemaMetadata {
    schema_version: String,
    required_result_fields: Vec<String>,
    required_case_result_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkTolerancePolicy {
    schema_version: String,
    policy_version: String,
    baseline_absolute_price_tolerance: f64,
    baseline_relative_price_tolerance: f64,
    put_call_parity_absolute_tolerance: f64,
    change_log: Vec<PricingBenchmarkToleranceChangeLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkToleranceChangeLogEntry {
    policy_version: String,
    recorded_at_utc: String,
    rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkSanityGuardrails {
    no_arbitrage_absolute_tolerance: f64,
    monotonicity_absolute_tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkCanonicalRegressionMetadata {
    required_tags: Vec<String>,
    stable_fixture_ids: Vec<String>,
    put_call_parity_pairs: Vec<PricingBenchmarkPutCallParityPair>,
    monotonic_strike_sequences: Vec<PricingBenchmarkMonotonicStrikeSequence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkPutCallParityPair {
    pair_id: String,
    call_fixture_id: String,
    put_fixture_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkMonotonicStrikeSequence {
    sequence_id: String,
    payoff_type: VanillaOptionPayoffType,
    relation: PricingBenchmarkMonotonicRelation,
    fixture_ids_in_strike_ascending: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PricingBenchmarkMonotonicRelation {
    NonIncreasing,
    NonDecreasing,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkCaseFixture {
    fixture_id: String,
    payoff_type: VanillaOptionPayoffType,
    market_inputs: VanillaOptionPricingMarketInputs,
    volatility: f64,
    engine_settings: PricingBenchmarkEngineSettingsFixture,
    expected_outcome: PricingBenchmarkExpectedOutcome,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkEngineSettingsFixture {
    contour_damping_factor: f64,
    integration_upper_bound: f64,
    integration_step_count: usize,
    quadrature_rule: FourierInversionQuadratureRule,
    stability_controls: PricingBenchmarkStabilityControlsFixture,
}

impl PricingBenchmarkEngineSettingsFixture {
    fn to_core(self, fixture_id: &str) -> FourierInversionEngineSettings {
        let contour = FourierInversionContour::try_new(self.contour_damping_factor).unwrap_or_else(
            |source| {
                panic!(
                    "fixture {} has invalid contour_damping_factor {}: {}",
                    fixture_id, self.contour_damping_factor, source
                )
            },
        );
        let numeric_settings = FourierInversionNumericSettings::try_new(
            self.integration_upper_bound,
            self.integration_step_count,
            self.quadrature_rule,
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} has invalid numeric settings (upper_bound={}, step_count={}): {}",
                fixture_id, self.integration_upper_bound, self.integration_step_count, source
            )
        });
        let stability_controls = FourierInversionIntegrationStabilityControls::try_new(
            self.stability_controls.convergence_absolute_tolerance,
            self.stability_controls.convergence_relative_tolerance,
            self.stability_controls
                .terminal_integrand_absolute_tolerance,
            self.stability_controls.terminal_integrand_ratio_tolerance,
            self.stability_controls.quality_gate,
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} has invalid stability controls: {}",
                fixture_id, source
            )
        });

        FourierInversionEngineSettings::try_new_with_stability_controls(
            contour,
            numeric_settings,
            stability_controls,
        )
        .unwrap_or_else(|source| {
            panic!(
                "fixture {} has invalid engine settings: {}",
                fixture_id, source
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct PricingBenchmarkStabilityControlsFixture {
    convergence_absolute_tolerance: f64,
    convergence_relative_tolerance: f64,
    terminal_integrand_absolute_tolerance: f64,
    terminal_integrand_ratio_tolerance: f64,
    quality_gate: FourierInversionNumericQualityGate,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "outcome_kind", rename_all = "snake_case", deny_unknown_fields)]
enum PricingBenchmarkExpectedOutcome {
    Success {
        expected_price: f64,
        expected_numeric_quality_is_poor: bool,
        expected_convergence_check_passed: bool,
        expected_truncation_check_passed: bool,
    },
    NumericQualityFailure {
        expected_numeric_quality_is_poor: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct PricingBenchmarkObservedCaseResult {
    fixture_id: String,
    market_inputs: VanillaOptionPricingMarketInputs,
    payoff_type: VanillaOptionPayoffType,
    outcome: PricingBenchmarkObservedOutcome,
}

#[derive(Debug, Clone, PartialEq)]
enum PricingBenchmarkObservedOutcome {
    Success {
        observed_price: f64,
        diagnostics: FourierInversionDiagnostics,
    },
    NumericQualityFailure {
        diagnostics: FourierInversionNumericQualityDiagnostics,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct PricingBenchmarkResultArtifact {
    schema_version: String,
    fixture_set_id: String,
    tolerance_policy_version: String,
    case_results: Vec<PricingBenchmarkCaseResultArtifact>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
struct PricingBenchmarkCaseResultArtifact {
    fixture_id: String,
    outcome_kind: String,
    observed_price: Option<f64>,
    expected_price: Option<f64>,
    absolute_error: Option<f64>,
    relative_error: Option<f64>,
    numeric_quality_is_poor: bool,
    convergence_check_passed: bool,
    truncation_check_passed: bool,
}

#[test]
fn pricing_benchmark_fixture_schema_and_metadata_are_valid() {
    let fixture_document = load_pricing_benchmark_fixture_document();

    assert_eq!(
        fixture_document.schema_version,
        PRICING_BENCHMARK_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        PRICING_BENCHMARK_FIXTURE_SET_ID
    );
    assert!(!fixture_document.fixture_set_version.trim().is_empty());
    assert_eq!(
        fixture_document.determinism_metadata.fixture_loading_order,
        PRICING_BENCHMARK_FIXTURE_LOADING_ORDER_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_scope,
        PRICING_BENCHMARK_CONTENT_HASH_SCOPE_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_algorithm,
        PRICING_BENCHMARK_CONTENT_HASH_ALGORITHM_POLICY
    );

    assert_eq!(
        fixture_document
            .benchmark_result_schema_metadata
            .schema_version,
        PRICING_BENCHMARK_RESULT_SCHEMA_VERSION
    );
    assert!(
        !fixture_document
            .benchmark_result_schema_metadata
            .required_result_fields
            .is_empty()
    );
    assert!(
        !fixture_document
            .benchmark_result_schema_metadata
            .required_case_result_fields
            .is_empty()
    );

    assert_eq!(
        fixture_document.tolerance_policy.schema_version,
        PRICING_BENCHMARK_TOLERANCE_POLICY_SCHEMA_VERSION
    );
    assert!(
        fixture_document
            .tolerance_policy
            .baseline_absolute_price_tolerance
            > 0.0
    );
    assert!(
        fixture_document
            .tolerance_policy
            .baseline_relative_price_tolerance
            > 0.0
    );
    assert!(
        fixture_document
            .tolerance_policy
            .put_call_parity_absolute_tolerance
            > 0.0
    );
    assert!(!fixture_document.tolerance_policy.change_log.is_empty());
    assert!(
        fixture_document
            .tolerance_policy
            .change_log
            .iter()
            .any(|change_log_entry| {
                change_log_entry.policy_version == fixture_document.tolerance_policy.policy_version
            })
    );

    assert!(!fixture_document.cases.is_empty());
    assert!(
        fixture_document
            .sanity_guardrails
            .no_arbitrage_absolute_tolerance
            > 0.0
    );
    assert!(
        fixture_document
            .sanity_guardrails
            .monotonicity_absolute_tolerance
            > 0.0
    );

    let observed_fixture_ids: BTreeSet<String> = fixture_document
        .cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let expected_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_fixture_ids
        .iter()
        .cloned()
        .collect();
    assert_eq!(observed_fixture_ids, expected_fixture_ids);

    let observed_tags: BTreeSet<String> = fixture_document
        .cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter().cloned())
        .collect();
    let required_tags: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .cloned()
        .collect();
    assert!(required_tags.is_subset(&observed_tags));

    for parity_pair in &fixture_document
        .canonical_regression_metadata
        .put_call_parity_pairs
    {
        assert!(observed_fixture_ids.contains(&parity_pair.call_fixture_id));
        assert!(observed_fixture_ids.contains(&parity_pair.put_fixture_id));
    }

    let case_by_fixture_id: BTreeMap<String, &PricingBenchmarkCaseFixture> = fixture_document
        .cases
        .iter()
        .map(|fixture_case| (fixture_case.fixture_id.clone(), fixture_case))
        .collect();
    for monotonic_sequence in &fixture_document
        .canonical_regression_metadata
        .monotonic_strike_sequences
    {
        assert!(!monotonic_sequence.sequence_id.trim().is_empty());
        assert!(monotonic_sequence.fixture_ids_in_strike_ascending.len() >= 2);

        for fixture_id in &monotonic_sequence.fixture_ids_in_strike_ascending {
            let fixture_case = case_by_fixture_id.get(fixture_id).unwrap_or_else(|| {
                panic!("unknown fixture id in monotonic sequence: {}", fixture_id)
            });
            assert_eq!(fixture_case.payoff_type, monotonic_sequence.payoff_type);
        }
    }

    for fixture_case in &fixture_document.cases {
        assert!(!fixture_case.tags.is_empty());
        assert!(!fixture_case.description.trim().is_empty());

        if let PricingBenchmarkExpectedOutcome::Success { expected_price, .. } =
            fixture_case.expected_outcome
        {
            // Use Black-Scholes closed form as a stable baseline for fixture values.
            let expected_benchmark_price = black_scholes_price(
                fixture_case.market_inputs,
                fixture_case.volatility,
                fixture_case.payoff_type,
            );
            assert!(
                (expected_price - expected_benchmark_price).abs() <= 2.0e-5,
                "fixture {} expected price drifted from benchmark formula: expected={}, benchmark={}",
                fixture_case.fixture_id,
                expected_price,
                expected_benchmark_price
            );
        }
    }
}

#[test]
fn pricing_benchmark_result_schema_is_stable_and_machine_readable() {
    let fixture_document = load_pricing_benchmark_fixture_document();
    let observed_case_results = run_all_benchmark_cases(&fixture_document);
    let result_artifact = build_result_artifact(&fixture_document, &observed_case_results);

    let serialized_result =
        serde_json::to_value(result_artifact).expect("benchmark result artifact should serialize");
    assert_required_object_fields(
        &serialized_result,
        &fixture_document
            .benchmark_result_schema_metadata
            .required_result_fields,
        "result",
    );

    let serialized_case_results = serialized_result
        .get("case_results")
        .and_then(Value::as_array)
        .expect("result.case_results must be an array");
    assert_eq!(serialized_case_results.len(), fixture_document.cases.len());
    for serialized_case_result in serialized_case_results {
        assert_required_object_fields(
            serialized_case_result,
            &fixture_document
                .benchmark_result_schema_metadata
                .required_case_result_fields,
            "result.case_results[]",
        );
    }
}

#[test]
fn pricing_reference_cases_remain_within_tolerance_bands() {
    let fixture_document = load_pricing_benchmark_fixture_document();
    let observed_case_results = run_all_benchmark_cases(&fixture_document);

    for fixture_case in &fixture_document.cases {
        let observed_case_result = observed_case_results
            .get(&fixture_case.fixture_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing observed benchmark case result: {}",
                    fixture_case.fixture_id
                )
            });

        match fixture_case.expected_outcome {
            PricingBenchmarkExpectedOutcome::Success {
                expected_price,
                expected_numeric_quality_is_poor,
                expected_convergence_check_passed,
                expected_truncation_check_passed,
            } => match &observed_case_result.outcome {
                PricingBenchmarkObservedOutcome::Success {
                    observed_price,
                    diagnostics,
                } => {
                    let absolute_error = (observed_price - expected_price).abs();
                    let relative_error = absolute_error / expected_price.abs().max(1.0);
                    let tolerance_limit = fixture_document
                        .tolerance_policy
                        .baseline_absolute_price_tolerance
                        + fixture_document
                            .tolerance_policy
                            .baseline_relative_price_tolerance
                            * expected_price.abs();

                    assert!(
                        absolute_error <= tolerance_limit,
                        "fixture {} ({}) exceeded tolerance: observed={}, expected={}, absolute_error={}, tolerance_limit={}",
                        fixture_case.fixture_id,
                        fixture_case.description,
                        observed_price,
                        expected_price,
                        absolute_error,
                        tolerance_limit
                    );
                    assert!(
                        relative_error
                            <= fixture_document
                                .tolerance_policy
                                .baseline_relative_price_tolerance
                                + fixture_document
                                    .tolerance_policy
                                    .baseline_absolute_price_tolerance,
                        "fixture {} ({}) relative error exceeded policy bound: relative_error={}",
                        fixture_case.fixture_id,
                        fixture_case.description,
                        relative_error
                    );

                    let observed_quality = diagnostics.numeric_quality_diagnostics;
                    assert_eq!(
                        observed_quality.numeric_quality_is_poor, expected_numeric_quality_is_poor,
                        "fixture {} ({}) numeric quality flag drifted",
                        fixture_case.fixture_id, fixture_case.description
                    );
                    assert_eq!(
                        observed_quality.convergence_check_passed,
                        expected_convergence_check_passed,
                        "fixture {} ({}) convergence diagnostic drifted",
                        fixture_case.fixture_id,
                        fixture_case.description
                    );
                    assert_eq!(
                        observed_quality.truncation_check_passed, expected_truncation_check_passed,
                        "fixture {} ({}) truncation diagnostic drifted",
                        fixture_case.fixture_id, fixture_case.description
                    );
                }
                PricingBenchmarkObservedOutcome::NumericQualityFailure { diagnostics } => panic!(
                    "fixture {} ({}) unexpectedly failed with numeric quality error: {:?}",
                    fixture_case.fixture_id, fixture_case.description, diagnostics
                ),
            },
            PricingBenchmarkExpectedOutcome::NumericQualityFailure {
                expected_numeric_quality_is_poor,
            } => match &observed_case_result.outcome {
                PricingBenchmarkObservedOutcome::NumericQualityFailure { diagnostics } => {
                    assert_eq!(
                        diagnostics.numeric_quality_is_poor, expected_numeric_quality_is_poor,
                        "fixture {} ({}) pathology flag drifted",
                        fixture_case.fixture_id, fixture_case.description
                    );
                }
                PricingBenchmarkObservedOutcome::Success {
                    observed_price,
                    diagnostics,
                } => panic!(
                    "fixture {} ({}) unexpectedly succeeded with price {} and diagnostics {:?}",
                    fixture_case.fixture_id, fixture_case.description, observed_price, diagnostics
                ),
            },
        }
    }
}

#[test]
fn pricing_sanity_bounds_and_pathology_flags_hold_for_benchmark_fixture() {
    let fixture_document = load_pricing_benchmark_fixture_document();
    let observed_case_results = run_all_benchmark_cases(&fixture_document);

    let mut success_case_prices: BTreeMap<String, f64> = BTreeMap::new();

    for fixture_case in &fixture_document.cases {
        let observed_case_result = observed_case_results
            .get(&fixture_case.fixture_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing observed benchmark case result: {}",
                    fixture_case.fixture_id
                )
            });

        match &observed_case_result.outcome {
            PricingBenchmarkObservedOutcome::Success {
                observed_price,
                diagnostics,
            } => {
                let (lower_bound, upper_bound) =
                    no_arbitrage_price_bounds(fixture_case.market_inputs, fixture_case.payoff_type);
                let tolerance = fixture_document
                    .sanity_guardrails
                    .no_arbitrage_absolute_tolerance;

                assert!(
                    *observed_price >= lower_bound - tolerance,
                    "fixture {} ({}) violated lower no-arbitrage bound: observed={}, lower_bound={}, tolerance={}",
                    fixture_case.fixture_id,
                    fixture_case.description,
                    observed_price,
                    lower_bound,
                    tolerance
                );
                assert!(
                    *observed_price <= upper_bound + tolerance,
                    "fixture {} ({}) violated upper no-arbitrage bound: observed={}, upper_bound={}, tolerance={}",
                    fixture_case.fixture_id,
                    fixture_case.description,
                    observed_price,
                    upper_bound,
                    tolerance
                );
                assert!(diagnostics.integral_estimate.is_finite());
                assert!(diagnostics.discounted_call_price.is_finite());

                success_case_prices.insert(fixture_case.fixture_id.clone(), *observed_price);
            }
            PricingBenchmarkObservedOutcome::NumericQualityFailure { diagnostics } => {
                assert!(diagnostics.numeric_quality_is_poor);
            }
        }
    }

    for parity_pair in &fixture_document
        .canonical_regression_metadata
        .put_call_parity_pairs
    {
        let call_price = success_case_prices
            .get(&parity_pair.call_fixture_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing successful call result for parity pair {}",
                    parity_pair.pair_id
                )
            });
        let put_price = success_case_prices
            .get(&parity_pair.put_fixture_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing successful put result for parity pair {}",
                    parity_pair.pair_id
                )
            });

        let call_case = fixture_document
            .cases
            .iter()
            .find(|fixture_case| fixture_case.fixture_id == parity_pair.call_fixture_id)
            .unwrap_or_else(|| panic!("missing call fixture case {}", parity_pair.call_fixture_id));
        let put_case = fixture_document
            .cases
            .iter()
            .find(|fixture_case| fixture_case.fixture_id == parity_pair.put_fixture_id)
            .unwrap_or_else(|| panic!("missing put fixture case {}", parity_pair.put_fixture_id));

        assert_eq!(call_case.market_inputs, put_case.market_inputs);

        let market_inputs = call_case.market_inputs;
        let discounted_spot = market_inputs.spot
            * (-market_inputs.dividend_yield * market_inputs.maturity_interval).exp();
        let discounted_strike = market_inputs.strike
            * (-market_inputs.risk_free_rate * market_inputs.maturity_interval).exp();
        let expected_parity_gap = discounted_spot - discounted_strike;
        let observed_parity_gap = call_price - put_price;

        assert!(
            (observed_parity_gap - expected_parity_gap).abs()
                <= fixture_document
                    .tolerance_policy
                    .put_call_parity_absolute_tolerance,
            "parity pair {} violated parity tolerance: observed_gap={}, expected_gap={}, tolerance={}",
            parity_pair.pair_id,
            observed_parity_gap,
            expected_parity_gap,
            fixture_document
                .tolerance_policy
                .put_call_parity_absolute_tolerance
        );
    }

    for monotonic_sequence in &fixture_document
        .canonical_regression_metadata
        .monotonic_strike_sequences
    {
        let tolerance = fixture_document
            .sanity_guardrails
            .monotonicity_absolute_tolerance;

        for fixture_pair in monotonic_sequence
            .fixture_ids_in_strike_ascending
            .windows(2)
        {
            let first_fixture_id = &fixture_pair[0];
            let second_fixture_id = &fixture_pair[1];

            let first_price = success_case_prices
                .get(first_fixture_id)
                .unwrap_or_else(|| {
                    panic!(
                        "missing successful monotonic sequence fixture {}",
                        first_fixture_id
                    )
                });
            let second_price = success_case_prices
                .get(second_fixture_id)
                .unwrap_or_else(|| {
                    panic!(
                        "missing successful monotonic sequence fixture {}",
                        second_fixture_id
                    )
                });

            match monotonic_sequence.relation {
                PricingBenchmarkMonotonicRelation::NonIncreasing => {
                    assert!(
                        *first_price >= *second_price - tolerance,
                        "monotonic sequence {} violated non_increasing relation between {} and {} with tolerance {}",
                        monotonic_sequence.sequence_id,
                        first_fixture_id,
                        second_fixture_id,
                        tolerance
                    );
                }
                PricingBenchmarkMonotonicRelation::NonDecreasing => {
                    assert!(
                        *first_price <= *second_price + tolerance,
                        "monotonic sequence {} violated non_decreasing relation between {} and {} with tolerance {}",
                        monotonic_sequence.sequence_id,
                        first_fixture_id,
                        second_fixture_id,
                        tolerance
                    );
                }
            }
        }
    }
}

fn load_pricing_benchmark_fixture_document() -> PricingBenchmarkFixtureDocument {
    let fixture_path = fixtures_root_path()
        .join("pricing")
        .join("vanilla_option_pricing_benchmark_fixture.json");
    let fixture_payload = fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read pricing benchmark fixture `{}`: {source}",
            fixture_path.display()
        )
    });

    serde_json::from_str::<PricingBenchmarkFixtureDocument>(&fixture_payload).unwrap_or_else(
        |source| {
            panic!(
                "failed to parse pricing benchmark fixture `{}`: {source}",
                fixture_path.display()
            )
        },
    )
}

fn run_all_benchmark_cases(
    fixture_document: &PricingBenchmarkFixtureDocument,
) -> BTreeMap<String, PricingBenchmarkObservedCaseResult> {
    fixture_document
        .cases
        .iter()
        .map(run_benchmark_case)
        .map(|observed_case| (observed_case.fixture_id.clone(), observed_case))
        .collect()
}

fn run_benchmark_case(
    fixture_case: &PricingBenchmarkCaseFixture,
) -> PricingBenchmarkObservedCaseResult {
    let engine_settings = fixture_case
        .engine_settings
        .to_core(&fixture_case.fixture_id);

    let outcome = match price_vanilla_option_via_fourier_inversion(
        fixture_case.market_inputs,
        VanillaOptionPayoffConfiguration {
            payoff_type: fixture_case.payoff_type,
        },
        engine_settings,
        |argument| {
            // Keep benchmark CF external to Janiform so pricing drift is isolated.
            black_scholes_log_return_characteristic_function(
                fixture_case.market_inputs,
                fixture_case.volatility,
                argument,
            )
        },
    ) {
        Ok(pricing_result) => PricingBenchmarkObservedOutcome::Success {
            observed_price: pricing_result.price,
            diagnostics: pricing_result.diagnostics,
        },
        Err(FourierInversionPricingError::NumericQualityFailure { diagnostics }) => {
            PricingBenchmarkObservedOutcome::NumericQualityFailure { diagnostics }
        }
        Err(source) => panic!(
            "fixture {} ({}) failed with unexpected pricing error: {}",
            fixture_case.fixture_id, fixture_case.description, source
        ),
    };

    PricingBenchmarkObservedCaseResult {
        fixture_id: fixture_case.fixture_id.clone(),
        market_inputs: fixture_case.market_inputs,
        payoff_type: fixture_case.payoff_type,
        outcome,
    }
}

fn build_result_artifact(
    fixture_document: &PricingBenchmarkFixtureDocument,
    observed_case_results: &BTreeMap<String, PricingBenchmarkObservedCaseResult>,
) -> PricingBenchmarkResultArtifact {
    let mut artifact_case_results: Vec<PricingBenchmarkCaseResultArtifact> = Vec::new();

    for fixture_case in &fixture_document.cases {
        let observed_case_result = observed_case_results
            .get(&fixture_case.fixture_id)
            .unwrap_or_else(|| {
                panic!(
                    "missing observed benchmark case result: {}",
                    fixture_case.fixture_id
                )
            });

        let case_result_artifact = match (
            &fixture_case.expected_outcome,
            &observed_case_result.outcome,
        ) {
            (
                PricingBenchmarkExpectedOutcome::Success { expected_price, .. },
                PricingBenchmarkObservedOutcome::Success {
                    observed_price,
                    diagnostics,
                },
            ) => {
                let absolute_error = (observed_price - expected_price).abs();
                let relative_error = absolute_error / expected_price.abs().max(1.0);

                PricingBenchmarkCaseResultArtifact {
                    fixture_id: fixture_case.fixture_id.clone(),
                    outcome_kind: "success".to_string(),
                    observed_price: Some(*observed_price),
                    expected_price: Some(*expected_price),
                    absolute_error: Some(absolute_error),
                    relative_error: Some(relative_error),
                    numeric_quality_is_poor: diagnostics
                        .numeric_quality_diagnostics
                        .numeric_quality_is_poor,
                    convergence_check_passed: diagnostics
                        .numeric_quality_diagnostics
                        .convergence_check_passed,
                    truncation_check_passed: diagnostics
                        .numeric_quality_diagnostics
                        .truncation_check_passed,
                }
            }
            (
                PricingBenchmarkExpectedOutcome::NumericQualityFailure { .. },
                PricingBenchmarkObservedOutcome::NumericQualityFailure { diagnostics },
            ) => PricingBenchmarkCaseResultArtifact {
                fixture_id: fixture_case.fixture_id.clone(),
                outcome_kind: "numeric_quality_failure".to_string(),
                observed_price: None,
                expected_price: None,
                absolute_error: None,
                relative_error: None,
                numeric_quality_is_poor: diagnostics.numeric_quality_is_poor,
                convergence_check_passed: diagnostics.convergence_check_passed,
                truncation_check_passed: diagnostics.truncation_check_passed,
            },
            _ => panic!(
                "fixture {} ({}) observed outcome does not match expected outcome kind",
                fixture_case.fixture_id, fixture_case.description
            ),
        };

        artifact_case_results.push(case_result_artifact);
    }

    PricingBenchmarkResultArtifact {
        schema_version: fixture_document
            .benchmark_result_schema_metadata
            .schema_version
            .clone(),
        fixture_set_id: fixture_document.fixture_set_id.clone(),
        tolerance_policy_version: fixture_document.tolerance_policy.policy_version.clone(),
        case_results: artifact_case_results,
    }
}

fn assert_required_object_fields(payload: &Value, required_fields: &[String], field_scope: &str) {
    let object_payload = payload.as_object().unwrap_or_else(|| {
        panic!(
            "{} must serialize to a JSON object, observed payload: {}",
            field_scope, payload
        )
    });

    for required_field in required_fields {
        assert!(
            object_payload.contains_key(required_field),
            "{} is missing required field `{}`",
            field_scope,
            required_field
        );
    }
}
