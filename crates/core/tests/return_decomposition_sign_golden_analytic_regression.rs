mod support;

use std::collections::BTreeSet;

use janiform_core::nig::{
    ComplexScalar, NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY, NigSharedPlusShiftConfiguration,
};
use janiform_core::pricing::{
    RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY, compose_nig_trading_time_exponent,
    compose_total_return_characteristic_function,
};
use janiform_core::return_decomposition::{ReturnDecomposition, ReturnDecompositionInput};

use support::return_decomposition_sign_fixture::{
    RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SCHEMA_VERSION,
    RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SET_ID,
    load_return_decomposition_sign_analytic_fixture_document,
};

const ANALYTIC_ABSOLUTE_TOLERANCE: f64 = 1.0e-12;

#[test]
fn return_decomposition_sign_analytic_fixture_contract_is_machine_readable() {
    let fixture_document = load_return_decomposition_sign_analytic_fixture_document()
        .expect("return decomposition sign analytic fixture should load");

    assert_eq!(
        fixture_document.schema_version,
        RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SCHEMA_VERSION
    );
    assert_eq!(
        fixture_document.fixture_set_id,
        RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SET_ID
    );
    assert!(!fixture_document.fixture_set_version.trim().is_empty());

    let required_scenario_intents: BTreeSet<String> = fixture_document
        .analytic_metadata
        .required_scenario_intents
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        required_scenario_intents,
        BTreeSet::from([
            "buy_only".to_string(),
            "sell_only".to_string(),
            "combined".to_string()
        ])
    );
    assert!(
        fixture_document
            .cases
            .iter()
            .all(|fixture_case| !fixture_case.scenario_intent.trim().is_empty())
    );
}

#[test]
fn return_decomposition_sign_analytic_cases_match_expected_pricing_outputs() {
    let fixture_document = load_return_decomposition_sign_analytic_fixture_document()
        .expect("return decomposition sign analytic fixture should load");
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    for fixture_case in &fixture_document.cases {
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: fixture_case.return_decomposition.a_plus,
            a_minus: fixture_case.return_decomposition.a_minus,
            open_jump: 0.0,
            closed_gap: fixture_case.return_decomposition.gap,
        })
        .expect("fixture return decomposition payload should construct");
        let argument = fixture_case.argument.into_core();

        let exponent_contributions =
            compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition);
        assert_eq!(
            exponent_contributions.policy,
            NIG_TRADING_TIME_RETURN_SIGN_CONVENTION_POLICY
        );
        assert_complex_close(
            exponent_contributions.buy_side_weighted_exponent,
            fixture_case
                .expected_output
                .buy_side_weighted_exponent
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "buy_side_weighted_exponent",
        );
        assert_complex_close(
            exponent_contributions.sell_side_weighted_exponent,
            fixture_case
                .expected_output
                .sell_side_weighted_exponent
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "sell_side_weighted_exponent",
        );
        assert_complex_close(
            exponent_contributions.combined_weighted_exponent,
            fixture_case
                .expected_output
                .combined_weighted_exponent
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "combined_weighted_exponent",
        );

        let assembled = compose_total_return_characteristic_function(
            driver_parameters,
            argument,
            return_decomposition,
            fixture_case.gap_characteristic_factor.into_core(),
        )
        .expect("fixture full characteristic-function assembly should succeed");
        assert_eq!(
            assembled.composition_policy,
            RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY
        );
        assert_complex_close(
            assembled.trading_time_characteristic_factor,
            fixture_case
                .expected_output
                .trading_time_characteristic_factor
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "trading_time_characteristic_factor",
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            fixture_case
                .expected_output
                .combined_characteristic_factor
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "combined_characteristic_factor",
        );
    }
}

#[test]
fn return_decomposition_sign_analytic_cases_detect_positive_u_sell_side_regression() {
    let fixture_document = load_return_decomposition_sign_analytic_fixture_document()
        .expect("return decomposition sign analytic fixture should load");
    let driver_parameters = derive_driver_parameters(fixture_document.driver_configuration);

    for fixture_case in &fixture_document.cases {
        if fixture_case.return_decomposition.a_minus == 0.0 {
            continue;
        }

        let argument = fixture_case.argument.into_core();
        let observed_sell_side_exponent = compose_nig_trading_time_exponent(
            driver_parameters,
            argument,
            ReturnDecomposition::try_new(ReturnDecompositionInput {
                a_plus: fixture_case.return_decomposition.a_plus,
                a_minus: fixture_case.return_decomposition.a_minus,
                open_jump: 0.0,
                closed_gap: fixture_case.return_decomposition.gap,
            })
            .expect("fixture return decomposition payload should construct"),
        )
        .sell_side_weighted_exponent;
        let positive_u_sell_side_exponent = driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument)
            .scale(fixture_case.return_decomposition.a_minus);

        assert_complex_close(
            positive_u_sell_side_exponent,
            fixture_case
                .expected_output
                .wrong_positive_u_sell_side_weighted_exponent
                .into_core(),
            ANALYTIC_ABSOLUTE_TOLERANCE,
            &fixture_case.fixture_id,
            "wrong_positive_u_sell_side_weighted_exponent",
        );

        let regression_distance = observed_sell_side_exponent
            .subtract(positive_u_sell_side_exponent)
            .norm();
        assert!(
            regression_distance + ANALYTIC_ABSOLUTE_TOLERANCE
                >= fixture_case
                    .expected_output
                    .minimum_sell_sign_regression_distance,
            "fixture {} ({}) sell-side sign regression distance dropped below floor: observed={}, expected_floor={}",
            fixture_case.fixture_id,
            fixture_case.description,
            regression_distance,
            fixture_case
                .expected_output
                .minimum_sell_sign_regression_distance
        );
    }
}

fn derive_driver_parameters(
    driver_configuration: support::return_decomposition_sign_fixture::ReturnDecompositionSignDriverConfiguration,
) -> janiform_core::nig::NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(
        driver_configuration.alpha,
        driver_configuration.beta_0,
        driver_configuration.delta,
        driver_configuration.mu,
        driver_configuration.delta_beta,
    )
    .expect("fixture driver configuration should construct")
    .derive_driver_parameters()
    .expect("fixture driver configuration should derive buy and sell parameters")
}

fn assert_complex_close(
    observed: ComplexScalar,
    expected: ComplexScalar,
    tolerance: f64,
    fixture_id: &str,
    field_name: &str,
) {
    let distance = observed.subtract(expected).norm();
    assert!(
        distance <= tolerance,
        "fixture {} field {} mismatch: observed={:?}, expected={:?}, distance={}, tolerance={}",
        fixture_id,
        field_name,
        observed,
        expected,
        distance,
        tolerance
    );
}
