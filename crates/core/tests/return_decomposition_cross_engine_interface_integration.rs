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

fn valid_nig_driver_parameters() -> janiform_core::nig::NigBuySellDriverParameters {
    NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("test shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("test shared-plus-shift payload should derive drivers")
}
