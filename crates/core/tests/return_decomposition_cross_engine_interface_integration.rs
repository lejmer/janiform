mod support;

use janiform_core::nig::{ComplexScalar, NigSharedPlusShiftConfiguration};
use janiform_core::pricing::{
    compose_nig_trading_time_exponent_for_pricing,
    compose_nig_trading_time_exponent_for_pricing_with_edge_mode,
};
use janiform_core::randomness::SplitMix64SimulationRandomnessStream;
use janiform_core::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
};
use janiform_core::simulation::sample_nig_trading_time_increment_for_simulation_with_edge_mode;

use support::return_decomposition_sign_fixture::load_return_decomposition_sign_analytic_fixture_document;

#[test]
fn pricing_and_shared_decomposition_interface_use_equivalent_component_mapping() {
    let driver_parameters = valid_nig_driver_parameters();
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.9,
        a_minus: 0.35,
        gap: -0.2,
    })
    .expect("test decomposition payload should construct");
    let argument = ComplexScalar::new(0.55, -0.4);

    let pricing_contributions = compose_nig_trading_time_exponent_for_pricing(
        driver_parameters,
        argument,
        return_decomposition,
    );
    let shared_contributions =
        return_decomposition.compose_nig_trading_time_exponent(driver_parameters, argument);
    let manual_expected = driver_parameters
        .compose_trading_time_weighted_characteristic_exponent(argument, 1.9, 0.35);
    let manual_swapped = driver_parameters
        .compose_trading_time_weighted_characteristic_exponent(argument, 0.35, 1.9);

    assert_eq!(pricing_contributions, shared_contributions);
    assert_eq!(shared_contributions, manual_expected);
    assert!(
        shared_contributions
            .combined_weighted_exponent
            .subtract(manual_swapped.combined_weighted_exponent)
            .norm()
            > 1.0e-6,
        "component mapping regression detected from accidental a_plus and a_minus swap"
    );
}

#[test]
fn simulation_and_pricing_adapters_match_shared_interface_under_edge_mode() {
    let driver_parameters = valid_nig_driver_parameters();
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.7,
        a_minus: 1.25,
        gap: 0.1,
    })
    .expect("test decomposition payload should construct");
    let edge_mode = ReturnDecompositionEdgeMode::SellOnly;
    let argument = ComplexScalar::new(0.2, 0.1);

    let pricing_observed = compose_nig_trading_time_exponent_for_pricing_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        edge_mode,
    );
    let pricing_expected = return_decomposition.compose_nig_trading_time_exponent_with_edge_mode(
        driver_parameters,
        argument,
        edge_mode,
    );
    assert_eq!(pricing_observed, pricing_expected);

    let mut simulation_stream = SplitMix64SimulationRandomnessStream::from_seed(704_991);
    let mut shared_stream = SplitMix64SimulationRandomnessStream::from_seed(704_991);
    let simulation_observed = sample_nig_trading_time_increment_for_simulation_with_edge_mode(
        driver_parameters,
        return_decomposition,
        edge_mode,
        &mut simulation_stream,
    )
    .expect("simulation adapter sampling should succeed");
    let simulation_expected = return_decomposition
        .sample_nig_trading_time_increment_with_edge_mode(
            driver_parameters,
            edge_mode,
            &mut shared_stream,
        )
        .expect("shared decomposition adapter sampling should succeed");

    assert_eq!(simulation_observed, simulation_expected);
}

#[test]
fn analytic_sign_fixture_cases_are_reusable_across_cross_engine_adapters() {
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
            gap: fixture_case.return_decomposition.gap,
        })
        .expect("fixture decomposition payload should construct");
        let argument = fixture_case.argument.into_core();

        let pricing_observed = compose_nig_trading_time_exponent_for_pricing(
            driver_parameters,
            argument,
            return_decomposition,
        );
        let shared_expected =
            return_decomposition.compose_nig_trading_time_exponent(driver_parameters, argument);
        assert_eq!(pricing_observed, shared_expected);

        let edge_mode = match fixture_case.scenario_intent.as_str() {
            "buy_only" => ReturnDecompositionEdgeMode::BuyOnly,
            "sell_only" => ReturnDecompositionEdgeMode::SellOnly,
            "combined" => ReturnDecompositionEdgeMode::Disabled,
            _ => ReturnDecompositionEdgeMode::Disabled,
        };
        let edge_pricing_observed = compose_nig_trading_time_exponent_for_pricing_with_edge_mode(
            driver_parameters,
            argument,
            return_decomposition,
            edge_mode,
        );
        let edge_shared_expected = return_decomposition
            .compose_nig_trading_time_exponent_with_edge_mode(
                driver_parameters,
                argument,
                edge_mode,
            );
        assert_eq!(edge_pricing_observed, edge_shared_expected);

        let case_seed = 704_991_u64
            + u64::try_from(case_position).expect("fixture case position should fit in u64");
        let mut simulation_stream = SplitMix64SimulationRandomnessStream::from_seed(case_seed);
        let mut shared_stream = SplitMix64SimulationRandomnessStream::from_seed(case_seed);
        let simulation_observed = sample_nig_trading_time_increment_for_simulation_with_edge_mode(
            driver_parameters,
            return_decomposition,
            edge_mode,
            &mut simulation_stream,
        )
        .expect("simulation adapter sampling should succeed for fixture case");
        let simulation_expected = return_decomposition
            .sample_nig_trading_time_increment_with_edge_mode(
                driver_parameters,
                edge_mode,
                &mut shared_stream,
            )
            .expect("shared decomposition adapter sampling should succeed for fixture case");
        assert_eq!(simulation_observed, simulation_expected);
    }
}

fn valid_nig_driver_parameters() -> janiform_core::nig::NigBuySellDriverParameters {
    valid_nig_driver_parameters_from_fixture(2.3, -0.8, 0.7, 0.1, -0.2)
}

fn valid_nig_driver_parameters_from_fixture(
    alpha: f64,
    beta_0: f64,
    delta: f64,
    mu: f64,
    delta_beta: f64,
) -> janiform_core::nig::NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(alpha, beta_0, delta, mu, delta_beta)
        .expect("test shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("test shared-plus-shift payload should derive drivers")
}
