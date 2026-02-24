mod support;

use std::collections::BTreeSet;
use std::fs;

use janiform_core::nig::{
    ComplexScalar, NIG_TRADING_TIME_DRIVER_INTERFACE_POLICY,
    NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY, NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY,
    NigSharedPlusShiftConfiguration,
};
use janiform_core::pricing::compose_nig_trading_time_exponent_for_pricing;
use janiform_core::randomness::SplitMix64SimulationRandomnessStream;
use janiform_core::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};
use janiform_core::simulation::sample_nig_trading_time_increment_for_simulation;
use serde::Deserialize;

use support::validation_fixture::fixtures_root_path;

const NIG_CROSS_ENGINE_REFERENCE_FIXTURE_SCHEMA_VERSION: &str =
    "nig.cross_engine.reference_fixture.v1";
const NIG_CROSS_ENGINE_REFERENCE_FIXTURE_SET_ID: &str = "nig_cross_engine_reference_fixture_v1";
const NIG_REFERENCE_FIXTURE_LOADING_ORDER_POLICY: &str = "preserve_declared_case_order.v1";
const NIG_REFERENCE_FIXTURE_CONTENT_HASH_SCOPE_POLICY: &str = "document_payload_bytes.v1";
const NIG_REFERENCE_FIXTURE_CONTENT_HASH_ALGORITHM_POLICY: &str = "fnv1a_64_hex_utf8.v1";
const NIG_REFERENCE_FIXTURE_SEED_POLICY: &str = "explicit_seed_per_sampling_case.v1";
const EXPONENT_ABSOLUTE_TOLERANCE: f64 = 1.0e-13;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigCrossEngineReferenceFixtureDocument {
    schema_version: String,
    fixture_set_id: String,
    interface_policy: String,
    determinism_metadata: NigReferenceDeterminismMetadata,
    driver_configuration: NigSharedPlusShiftDriverConfigurationFixture,
    exponent_cases: Vec<NigExponentReferenceCaseFixture>,
    sampling_sanity_cases: Vec<NigSamplingSanityCaseFixture>,
    canonical_regression_metadata: NigCanonicalRegressionMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigReferenceDeterminismMetadata {
    fixture_loading_order: String,
    content_hash_scope: String,
    content_hash_algorithm: String,
    seed_policy: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigSharedPlusShiftDriverConfigurationFixture {
    alpha: f64,
    beta_0: f64,
    delta: f64,
    mu: f64,
    delta_beta: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigExponentReferenceCaseFixture {
    fixture_id: String,
    argument: ComplexScalarFixture,
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
    expected_buy_side_weighted_exponent: ComplexScalarFixture,
    expected_sell_side_weighted_exponent: ComplexScalarFixture,
    expected_combined_weighted_exponent: ComplexScalarFixture,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigSamplingSanityCaseFixture {
    fixture_id: String,
    seed: u64,
    sample_count: usize,
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
    characteristic_function_argument_real: f64,
    expected_return_mean: f64,
    expected_return_variance: f64,
    mean_tolerance: f64,
    variance_tolerance: f64,
    empirical_characteristic_function_absolute_tolerance: f64,
    tags: Vec<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct NigCanonicalRegressionMetadata {
    required_tags: Vec<String>,
    stable_exponent_fixture_ids: Vec<String>,
    stable_sampling_fixture_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComplexScalarFixture {
    real: f64,
    imaginary: f64,
}

impl ComplexScalarFixture {
    fn into_core(self) -> ComplexScalar {
        ComplexScalar::new(self.real, self.imaginary)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SamplingSanitySummary {
    empirical_mean: f64,
    empirical_variance: f64,
    empirical_characteristic_function: ComplexScalar,
}

#[test]
fn nig_cross_engine_reference_fixture_contract_is_valid() {
    let fixture_document = load_nig_cross_engine_reference_fixture_document();

    assert_eq!(
        fixture_document.schema_version,
        NIG_CROSS_ENGINE_REFERENCE_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        NIG_CROSS_ENGINE_REFERENCE_FIXTURE_SET_ID
    );
    assert_eq!(
        fixture_document.interface_policy,
        NIG_TRADING_TIME_DRIVER_INTERFACE_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.fixture_loading_order,
        NIG_REFERENCE_FIXTURE_LOADING_ORDER_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_scope,
        NIG_REFERENCE_FIXTURE_CONTENT_HASH_SCOPE_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.content_hash_algorithm,
        NIG_REFERENCE_FIXTURE_CONTENT_HASH_ALGORITHM_POLICY
    );
    assert_eq!(
        fixture_document.determinism_metadata.seed_policy,
        NIG_REFERENCE_FIXTURE_SEED_POLICY
    );

    assert!(!fixture_document.exponent_cases.is_empty());
    assert!(!fixture_document.sampling_sanity_cases.is_empty());

    let observed_exponent_fixture_ids: BTreeSet<String> = fixture_document
        .exponent_cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let observed_sampling_fixture_ids: BTreeSet<String> = fixture_document
        .sampling_sanity_cases
        .iter()
        .map(|fixture_case| fixture_case.fixture_id.clone())
        .collect();
    let expected_exponent_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_exponent_fixture_ids
        .iter()
        .cloned()
        .collect();
    let expected_sampling_fixture_ids: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .stable_sampling_fixture_ids
        .iter()
        .cloned()
        .collect();

    assert_eq!(observed_exponent_fixture_ids, expected_exponent_fixture_ids);
    assert_eq!(observed_sampling_fixture_ids, expected_sampling_fixture_ids);

    let observed_tags: BTreeSet<String> = fixture_document
        .exponent_cases
        .iter()
        .flat_map(|fixture_case| fixture_case.tags.iter().cloned())
        .chain(
            fixture_document
                .sampling_sanity_cases
                .iter()
                .flat_map(|fixture_case| fixture_case.tags.iter().cloned()),
        )
        .collect();
    let required_tags: BTreeSet<String> = fixture_document
        .canonical_regression_metadata
        .required_tags
        .iter()
        .cloned()
        .collect();

    assert!(required_tags.is_subset(&observed_tags));
}

#[test]
fn exponent_reference_cases_match_pricing_interface_values() {
    let fixture_document = load_nig_cross_engine_reference_fixture_document();
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    for fixture_case in &fixture_document.exponent_cases {
        let return_decomposition = fixture_return_decomposition(
            fixture_case.integrated_buy_intensity,
            fixture_case.integrated_sell_intensity,
        );
        let observed = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            fixture_case.argument.into_core(),
            return_decomposition,
        );

        assert_eq!(
            observed.policy,
            NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY
        );
        assert_complex_close(
            observed.buy_side_weighted_exponent,
            fixture_case.expected_buy_side_weighted_exponent.into_core(),
            EXPONENT_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
        );
        assert_complex_close(
            observed.sell_side_weighted_exponent,
            fixture_case
                .expected_sell_side_weighted_exponent
                .into_core(),
            EXPONENT_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
        );
        assert_complex_close(
            observed.combined_weighted_exponent,
            fixture_case.expected_combined_weighted_exponent.into_core(),
            EXPONENT_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
        );
    }
}

#[test]
fn seeded_sampling_sanity_cases_match_expected_statistics_and_pricing_cf() {
    let fixture_document = load_nig_cross_engine_reference_fixture_document();
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    for fixture_case in &fixture_document.sampling_sanity_cases {
        let observed_summary = run_sampling_sanity_summary(driver_parameters, fixture_case);

        let return_decomposition = fixture_return_decomposition(
            fixture_case.integrated_buy_intensity,
            fixture_case.integrated_sell_intensity,
        );
        let expected_exponent_contributions = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            ComplexScalar::new(fixture_case.characteristic_function_argument_real, 0.0),
            return_decomposition,
        );
        let expected_characteristic_function =
            complex_exponential(expected_exponent_contributions.combined_weighted_exponent);

        assert!(
            (observed_summary.empirical_mean - fixture_case.expected_return_mean).abs()
                <= fixture_case.mean_tolerance,
            "fixture {} ({}) mean mismatch: observed={}, expected={}, tolerance={}",
            fixture_case.fixture_id,
            fixture_case.description,
            observed_summary.empirical_mean,
            fixture_case.expected_return_mean,
            fixture_case.mean_tolerance
        );
        assert!(
            (observed_summary.empirical_variance - fixture_case.expected_return_variance).abs()
                <= fixture_case.variance_tolerance,
            "fixture {} ({}) variance mismatch: observed={}, expected={}, tolerance={}",
            fixture_case.fixture_id,
            fixture_case.description,
            observed_summary.empirical_variance,
            fixture_case.expected_return_variance,
            fixture_case.variance_tolerance
        );

        let characteristic_function_distance = observed_summary
            .empirical_characteristic_function
            .subtract(expected_characteristic_function)
            .norm();
        assert!(
            characteristic_function_distance
                <= fixture_case.empirical_characteristic_function_absolute_tolerance,
            "fixture {} ({}) empirical CF mismatch: observed={:?}, expected={:?}, distance={}, tolerance={}",
            fixture_case.fixture_id,
            fixture_case.description,
            observed_summary.empirical_characteristic_function,
            expected_characteristic_function,
            characteristic_function_distance,
            fixture_case.empirical_characteristic_function_absolute_tolerance
        );
    }
}

#[test]
fn seeded_sampling_sanity_cases_are_reproducible_for_fixed_seed() {
    let fixture_document = load_nig_cross_engine_reference_fixture_document();
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    for fixture_case in &fixture_document.sampling_sanity_cases {
        let first_summary = run_sampling_sanity_summary(driver_parameters, fixture_case);
        let second_summary = run_sampling_sanity_summary(driver_parameters, fixture_case);

        assert_eq!(first_summary, second_summary);
    }
}

fn load_nig_cross_engine_reference_fixture_document() -> NigCrossEngineReferenceFixtureDocument {
    let fixture_path = fixtures_root_path()
        .join("nig")
        .join("nig_cross_engine_reference_fixture.json");
    let fixture_payload = fs::read_to_string(&fixture_path).unwrap_or_else(|source| {
        panic!(
            "failed to read NIG cross-engine reference fixture `{}`: {source}",
            fixture_path.display()
        )
    });

    serde_json::from_str(&fixture_payload).unwrap_or_else(|source| {
        panic!(
            "failed to parse NIG cross-engine reference fixture `{}`: {source}",
            fixture_path.display()
        )
    })
}

fn derive_driver_parameters(
    configuration_fixture: NigSharedPlusShiftDriverConfigurationFixture,
) -> janiform_core::nig::NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(
        configuration_fixture.alpha,
        configuration_fixture.beta_0,
        configuration_fixture.delta,
        configuration_fixture.mu,
        configuration_fixture.delta_beta,
    )
    .expect("fixture shared-plus-shift configuration should be valid")
    .derive_driver_parameters()
    .expect("fixture shared-plus-shift configuration should derive drivers")
}

fn run_sampling_sanity_summary(
    driver_parameters: janiform_core::nig::NigBuySellDriverParameters,
    fixture_case: &NigSamplingSanityCaseFixture,
) -> SamplingSanitySummary {
    assert!(
        fixture_case.sample_count >= 2,
        "fixture {} sample_count must be at least 2 for variance estimation",
        fixture_case.fixture_id
    );

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(fixture_case.seed);
    let mut running_mean = 0.0_f64;
    let mut running_squared_error = 0.0_f64;
    let mut characteristic_function_real_sum = 0.0_f64;
    let mut characteristic_function_imaginary_sum = 0.0_f64;

    for sample_number in 1..=fixture_case.sample_count {
        let return_decomposition = fixture_return_decomposition(
            fixture_case.integrated_buy_intensity,
            fixture_case.integrated_sell_intensity,
        );
        let sampled_increment_contributions = sample_nig_trading_time_increment_for_simulation(
            driver_parameters,
            return_decomposition,
            &mut randomness_stream,
        )
        .expect("fixture sampling case should succeed");

        assert_eq!(
            sampled_increment_contributions.policy,
            NIG_TRADING_TIME_INCREMENT_COMPOSITION_POLICY
        );

        let sampled_return_increment = sampled_increment_contributions.combined_increment;
        let sample_number_as_float = sample_number as f64;
        let delta_from_mean = sampled_return_increment - running_mean;

        running_mean += delta_from_mean / sample_number_as_float;
        running_squared_error += delta_from_mean * (sampled_return_increment - running_mean);

        let argument_phase =
            fixture_case.characteristic_function_argument_real * sampled_return_increment;

        characteristic_function_real_sum += argument_phase.cos();
        characteristic_function_imaginary_sum += argument_phase.sin();
    }

    let sample_count_as_float = fixture_case.sample_count as f64;
    SamplingSanitySummary {
        empirical_mean: running_mean,
        empirical_variance: running_squared_error / (sample_count_as_float - 1.0),
        empirical_characteristic_function: ComplexScalar::new(
            characteristic_function_real_sum / sample_count_as_float,
            characteristic_function_imaginary_sum / sample_count_as_float,
        ),
    }
}

fn fixture_return_decomposition(
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
) -> ReturnDecomposition {
    ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: integrated_buy_intensity,
        a_minus: integrated_sell_intensity,
        gap: 0.0,
    })
    .expect("fixture decomposition components should be valid")
}

fn complex_exponential(value: ComplexScalar) -> ComplexScalar {
    let real_exponential = value.real.exp();
    ComplexScalar::new(
        real_exponential * value.imaginary.cos(),
        real_exponential * value.imaginary.sin(),
    )
}

fn assert_complex_close(
    observed: ComplexScalar,
    expected: ComplexScalar,
    tolerance: f64,
    fixture_id: &str,
) {
    let difference = observed.subtract(expected).norm();
    assert!(
        difference <= tolerance,
        "fixture {fixture_id} complex mismatch: observed={observed:?}, expected={expected:?}, difference={difference}, tolerance={tolerance}"
    );
}
