mod support;

use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_engine_pricing::compose_total_return_characteristic_function_with_edge_mode;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
    compose_nig_trading_time_exponent_with_edge_mode,
    sample_nig_trading_time_increment_with_edge_mode,
};

use support::return_decomposition_sign_fixture::load_return_decomposition_sign_analytic_fixture_document;

#[test]
fn shared_decomposition_mapping_preserves_component_assignment() {
    let driver_parameters = valid_nig_driver_parameters();
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.9,
        a_minus: 0.35,
        open_jump: 0.0,
        closed_gap: -0.2,
    })
    .expect("test decomposition payload should construct");
    let argument = ComplexScalar::new(0.55, -0.4);

    let observed = compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ReturnDecompositionEdgeMode::Disabled,
    );
    let manual_expected = driver_parameters
        .compose_trading_time_weighted_characteristic_exponent(argument, 1.9, 0.35);
    let manual_swapped = driver_parameters
        .compose_trading_time_weighted_characteristic_exponent(argument, 0.35, 1.9);

    assert_eq!(observed, manual_expected);
    assert!(
        observed
            .combined_weighted_exponent
            .subtract(manual_swapped.combined_weighted_exponent)
            .norm()
            > 1.0e-6,
        "component mapping regression detected from accidental a_plus and a_minus swap"
    );
}

#[test]
fn edge_mode_sampling_preserves_zeroed_components_and_sign_rules() {
    let driver_parameters = valid_nig_driver_parameters();
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.7,
        a_minus: 1.25,
        open_jump: 0.0,
        closed_gap: 0.1,
    })
    .expect("test decomposition payload should construct");

    let edge_mode = ReturnDecompositionEdgeMode::SellOnly;
    let mut simulation_stream = SplitMix64SimulationRandomnessStream::from_seed(704_991);
    let sampled = sample_nig_trading_time_increment_with_edge_mode(
        driver_parameters,
        return_decomposition,
        edge_mode,
        &mut simulation_stream,
    )
    .expect("shared simulation sampling should succeed");

    assert_eq!(sampled.buy_side_increment, 0.0);
    assert_eq!(sampled.combined_increment, -sampled.sell_side_increment);
}

#[test]
fn analytic_sign_fixture_cases_match_shared_composition_outputs() {
    let fixture_document = load_return_decomposition_sign_analytic_fixture_document()
        .expect("return decomposition sign analytic fixture should load");
    let driver_parameters = valid_nig_driver_parameters_from_fixture(
        fixture_document.driver_configuration.alpha,
        fixture_document.driver_configuration.beta_0,
        fixture_document.driver_configuration.delta,
        fixture_document.driver_configuration.mu,
        fixture_document.driver_configuration.delta_beta,
    );

    for (case_position, fixture_case) in fixture_document.cases.iter().enumerate() {
        let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
            a_plus: fixture_case.return_decomposition.a_plus,
            a_minus: fixture_case.return_decomposition.a_minus,
            open_jump: 0.0,
            closed_gap: fixture_case.return_decomposition.gap,
        })
        .expect("fixture decomposition payload should construct");
        let argument = fixture_case.argument.into_core();
        let edge_mode = edge_mode_from_fixture_intent(&fixture_case.scenario_intent);

        let exponent_contributions = compose_nig_trading_time_exponent_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            edge_mode,
        );
        let expected = fixture_case.expected_output;
        assert_complex_close(
            exponent_contributions.buy_side_weighted_exponent,
            expected.buy_side_weighted_exponent.into_core(),
            1.0e-12,
        );
        assert_complex_close(
            exponent_contributions.sell_side_weighted_exponent,
            expected.sell_side_weighted_exponent.into_core(),
            1.0e-12,
        );
        assert_complex_close(
            exponent_contributions.combined_weighted_exponent,
            expected.combined_weighted_exponent.into_core(),
            1.0e-12,
        );

        let assembled = compose_total_return_characteristic_function_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            ComplexScalar::new(1.0, 0.0),
            fixture_case.gap_characteristic_factor.into_core(),
            edge_mode,
        )
        .expect("shared characteristic-factor composition should succeed for fixture case");

        assert_complex_close(
            assembled.trading_time_characteristic_factor,
            expected.trading_time_characteristic_factor.into_core(),
            1.0e-12,
        );
        assert_complex_close(
            assembled.combined_characteristic_factor,
            expected.combined_characteristic_factor.into_core(),
            1.0e-12,
        );

        let wrong_positive_u_distance = exponent_contributions
            .sell_side_weighted_exponent
            .subtract(
                expected
                    .wrong_positive_u_sell_side_weighted_exponent
                    .into_core(),
            )
            .norm();
        assert!(
            wrong_positive_u_distance + 1.0e-12 >= expected.minimum_sell_sign_regression_distance,
            "fixture case {} violated positive-u sign regression threshold",
            fixture_case.fixture_id
        );

        let case_seed = 704_991_u64
            + u64::try_from(case_position).expect("fixture case position should fit in u64");
        let mut simulation_stream = SplitMix64SimulationRandomnessStream::from_seed(case_seed);
        let sampled = sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            return_decomposition,
            edge_mode,
            &mut simulation_stream,
        )
        .expect("shared sampling should succeed for fixture case");
        match edge_mode {
            ReturnDecompositionEdgeMode::BuyOnly => {
                assert_eq!(sampled.sell_side_increment, 0.0);
                assert_eq!(sampled.combined_increment, sampled.buy_side_increment);
            }
            ReturnDecompositionEdgeMode::SellOnly => {
                assert_eq!(sampled.buy_side_increment, 0.0);
                assert_eq!(sampled.combined_increment, -sampled.sell_side_increment);
            }
            ReturnDecompositionEdgeMode::OpenJumpOnly => {
                assert_eq!(sampled.buy_side_increment, 0.0);
                assert_eq!(sampled.sell_side_increment, 0.0);
                assert_eq!(sampled.combined_increment, 0.0);
            }
            ReturnDecompositionEdgeMode::ClosedGapOnly => {
                assert_eq!(sampled.buy_side_increment, 0.0);
                assert_eq!(sampled.sell_side_increment, 0.0);
                assert_eq!(sampled.combined_increment, 0.0);
            }
            ReturnDecompositionEdgeMode::Disabled => {}
        }
    }
}

fn edge_mode_from_fixture_intent(scenario_intent: &str) -> ReturnDecompositionEdgeMode {
    match scenario_intent {
        "buy_only" => ReturnDecompositionEdgeMode::BuyOnly,
        "sell_only" => ReturnDecompositionEdgeMode::SellOnly,
        "combined" => ReturnDecompositionEdgeMode::Disabled,
        _ => ReturnDecompositionEdgeMode::Disabled,
    }
}

fn valid_nig_driver_parameters() -> janiform_model_kernels::nig::NigBuySellDriverParameters {
    valid_nig_driver_parameters_from_fixture(2.3, -0.8, 0.7, 0.1, -0.2)
}

fn valid_nig_driver_parameters_from_fixture(
    alpha: f64,
    beta_0: f64,
    delta: f64,
    mu: f64,
    delta_beta: f64,
) -> janiform_model_kernels::nig::NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(alpha, beta_0, delta, mu, delta_beta)
        .expect("test shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("test shared-plus-shift payload should derive drivers")
}

fn assert_complex_close(observed: ComplexScalar, expected: ComplexScalar, tolerance: f64) {
    let distance = observed.subtract(expected).norm();
    assert!(
        distance <= tolerance,
        "complex values differ by {distance} which exceeds tolerance {tolerance}; observed={observed:?}, expected={expected:?}"
    );
}
