use crate::{
    compose_nig_trading_time_exponent, compose_total_return_characteristic_function,
    compose_total_return_characteristic_function_from_oracle_output,
    compose_total_return_characteristic_function_with_edge_mode, solve_anchored_local_pricing_tilt,
    AnchoredLocalTiltSolveError, AnchoredVanillaOptionPricingBoundaryInput,
    ReturnCharacteristicFunctionAssemblyError, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
    PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY, RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformOutput,
};
use janiform_foundations::complex_math::{complex_exponential, complex_multiply, ComplexScalar};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
};
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_decomposition::{
    ReturnDecomposition, ReturnDecompositionEdgeMode, ReturnDecompositionInput,
};
use janiform_model_kernels::risk_neutral::evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument;

use super::support::{
    assert_complex_close, disabled_boundary_input, oracle_output_with_transform_value,
    valid_model_parameters,
};

#[test]
fn pricing_path_preserves_sell_side_minus_u_sign_handling() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(1.1, 0.0);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.0,
        a_minus: 1.0,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let composed =
        compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition);

    let expected_sell_side_minus_u = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument.scale(-1.0));
    assert_complex_close(
        composed.sell_side_weighted_exponent,
        expected_sell_side_minus_u,
        1.0e-13,
    );

    let incorrect_positive_u = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument);
    let regression_distance = composed
        .sell_side_weighted_exponent
        .subtract(incorrect_positive_u)
        .norm();
    assert!(
        regression_distance > 1.0e-6,
        "sell-side sign-convention regression detected with distance={regression_distance}"
    );
}

#[test]
fn full_characteristic_function_assembly_uses_trading_time_factor_when_news_jump_factors_are_unity()
{
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(1.1, 0.0);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.0,
        a_minus: 1.0,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");
    let open_jump_characteristic_factor = ComplexScalar::new(1.0, 0.0);
    let closed_gap_characteristic_factor = ComplexScalar::new(1.0, 0.0);

    let assembled = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
    .expect("full CF assembly should succeed with valid finite inputs");

    let expected_sell_side_minus_u = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument.scale(-1.0));
    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .sell_side_weighted_exponent,
        expected_sell_side_minus_u,
        1.0e-13,
    );

    assert_complex_close(
        assembled.combined_news_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        assembled.trading_time_characteristic_factor,
        1.0e-13,
    );
    assert_eq!(
        assembled.composition_policy,
        RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY
    );
    assert_eq!(
        assembled.domain_guard_policy,
        RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY
    );
}

#[test]
fn full_characteristic_function_assembly_multiplies_exp_trading_term_and_component_factors() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(-0.4, 0.3);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.4,
        a_minus: 0.7,
        open_jump: 0.0,
        closed_gap: -0.2,
    })
    .expect("valid return decomposition payload should construct");
    let open_jump_characteristic_factor = ComplexScalar::new(0.95, 0.15);
    let closed_gap_characteristic_factor = ComplexScalar::new(0.85, -0.25);

    let assembled = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
    .expect("full CF assembly should succeed with valid finite inputs");

    let expected_trading_exponent =
        compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition)
            .combined_weighted_exponent;
    let expected_trading_factor = complex_exponential(expected_trading_exponent);
    let expected_news_jump_factor = complex_multiply(
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    );
    let expected_combined = complex_multiply(expected_trading_factor, expected_news_jump_factor);

    assert_complex_close(
        assembled.trading_time_characteristic_factor,
        expected_trading_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.open_jump_characteristic_factor,
        open_jump_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.closed_gap_characteristic_factor,
        closed_gap_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_news_jump_characteristic_factor,
        expected_news_jump_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        expected_combined,
        1.0e-13,
    );
}

#[test]
fn oracle_output_cf_assembly_matches_canonical_pricing_path_for_fixed_trading_factor() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.35, -0.15);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.8,
        a_minus: 1.1,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");
    let open_jump_characteristic_factor = ComplexScalar::new(0.92, -0.08);
    let closed_gap_characteristic_factor = ComplexScalar::new(0.87, 0.04);

    let canonical_assembly = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
    .expect("canonical pricing assembly should succeed");

    let oracle_factor_assembly = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(canonical_assembly.trading_time_characteristic_factor),
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
    .expect("oracle-output assembly should succeed");

    assert_complex_close(
        oracle_factor_assembly.trading_time_characteristic_factor,
        canonical_assembly.trading_time_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        oracle_factor_assembly.combined_news_jump_characteristic_factor,
        canonical_assembly.combined_news_jump_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        oracle_factor_assembly.combined_characteristic_factor,
        canonical_assembly.combined_characteristic_factor,
        1.0e-13,
    );
    assert_eq!(
        oracle_factor_assembly.composition_policy,
        RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY
    );
    assert_eq!(
        oracle_factor_assembly.domain_guard_policy,
        RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY
    );
}

#[test]
fn oracle_output_cf_assembly_isolates_trading_open_jump_closed_gap_and_combined_paths() {
    let trading_only = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(ComplexScalar::new(0.84, -0.22)),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect("trading-only oracle composition should succeed");

    assert_complex_close(
        trading_only.combined_characteristic_factor,
        trading_only.trading_time_characteristic_factor,
        1.0e-13,
    );

    let open_jump_characteristic_factor = ComplexScalar::new(0.66, 0.19);
    let open_jump_only = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(ComplexScalar::new(1.0, 0.0)),
        open_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
    )
    .expect("open-jump-only oracle composition should succeed");

    assert_complex_close(
        open_jump_only.combined_characteristic_factor,
        open_jump_characteristic_factor,
        1.0e-13,
    );

    let closed_gap_characteristic_factor = ComplexScalar::new(0.73, -0.11);
    let closed_gap_only = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(ComplexScalar::new(1.0, 0.0)),
        ComplexScalar::new(1.0, 0.0),
        closed_gap_characteristic_factor,
    )
    .expect("closed-gap-only oracle composition should succeed");

    assert_complex_close(
        closed_gap_only.combined_characteristic_factor,
        closed_gap_characteristic_factor,
        1.0e-13,
    );

    let combined_news_jump_factor = complex_multiply(
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    );
    let combined = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(ComplexScalar::new(1.0, 0.0)),
        open_jump_characteristic_factor,
        closed_gap_characteristic_factor,
    )
    .expect("combined oracle composition should succeed");

    assert_complex_close(
        combined.combined_news_jump_characteristic_factor,
        combined_news_jump_factor,
        1.0e-13,
    );
    assert_complex_close(
        combined.combined_characteristic_factor,
        combined_news_jump_factor,
        1.0e-13,
    );
}

#[test]
fn oracle_output_cf_assembly_regression_guard_detects_positive_u_sell_side_sign_inversion() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(1.1, 0.0);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.0,
        a_minus: 1.0,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let correct_trading_exponent =
        compose_nig_trading_time_exponent(driver_parameters, argument, return_decomposition)
            .combined_weighted_exponent;
    let correct_trading_factor = complex_exponential(correct_trading_exponent);
    let incorrect_positive_u_trading_factor = complex_exponential(
        driver_parameters
            .sell_side_parameters
            .characteristic_exponent(argument),
    );

    let correct_assembly = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(correct_trading_factor),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect("correct-sign oracle assembly should succeed");
    let incorrect_assembly = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(incorrect_positive_u_trading_factor),
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect("incorrect-sign oracle assembly should still compose deterministically");

    let regression_distance = correct_assembly
        .combined_characteristic_factor
        .subtract(incorrect_assembly.combined_characteristic_factor)
        .norm();
    assert!(
        regression_distance > 1.0e-6,
        "sell-side sign-convention regression detected in oracle-output CF assembly with distance={regression_distance}"
    );
}

#[test]
fn oracle_output_cf_assembly_rejects_invalid_output_payload() {
    let error = compose_total_return_characteristic_function_from_oracle_output(
        ExponentialAffineTransformOutput {
            c_at_delta: ComplexScalar::new(f64::NAN, 0.0),
            d_at_delta: ExponentialAffineTransformDVector::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            transform_value: ComplexScalar::new(1.0, 0.0),
        },
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect_err("invalid oracle output must fail full CF assembly");

    assert!(matches!(
        error,
        ReturnCharacteristicFunctionAssemblyError::InvalidOracleTransformOutput { .. }
    ));
}

#[test]
fn edge_mode_buy_only_disables_sell_and_news_jump_components_in_full_cf_assembly() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(-0.25, 0.1);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.2,
        a_minus: 0.9,
        open_jump: 0.0,
        closed_gap: -0.35,
    })
    .expect("valid return decomposition payload should construct");

    let assembled = compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(0.8, -0.1),
        ComplexScalar::new(0.75, 0.22),
        ReturnDecompositionEdgeMode::BuyOnly,
    )
    .expect("buy-only edge mode CF assembly should succeed");

    let expected_buy = driver_parameters
        .buy_side_parameters
        .characteristic_exponent(argument)
        .scale(1.2);
    let expected_trading = complex_exponential(expected_buy);

    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .buy_side_weighted_exponent,
        expected_buy,
        1.0e-13,
    );
    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .sell_side_weighted_exponent,
        ComplexScalar::new(0.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.open_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.closed_gap_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_news_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        expected_trading,
        1.0e-13,
    );
}

#[test]
fn edge_mode_sell_only_preserves_minus_u_sign_and_disables_news_jump_components() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.9, -0.2);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.4,
        a_minus: 1.1,
        open_jump: 0.0,
        closed_gap: -0.1,
    })
    .expect("valid return decomposition payload should construct");

    let assembled = compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(0.7, 0.05),
        ComplexScalar::new(0.68, -0.04),
        ReturnDecompositionEdgeMode::SellOnly,
    )
    .expect("sell-only edge mode CF assembly should succeed");

    let expected_sell = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument.scale(-1.0))
        .scale(1.1);
    let expected_trading = complex_exponential(expected_sell);

    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .buy_side_weighted_exponent,
        ComplexScalar::new(0.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .sell_side_weighted_exponent,
        expected_sell,
        1.0e-13,
    );
    assert_complex_close(
        assembled.open_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.closed_gap_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_news_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        expected_trading,
        1.0e-13,
    );
}

#[test]
fn edge_mode_open_jump_only_isolates_open_jump_factor_in_full_cf_assembly() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.15, -0.35);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.7,
        open_jump: 0.6,
        closed_gap: 0.4,
    })
    .expect("valid return decomposition payload should construct");
    let open_jump_characteristic_factor = ComplexScalar::new(0.78, 0.22);

    let assembled = compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        open_jump_characteristic_factor,
        ComplexScalar::new(0.65, -0.35),
        ReturnDecompositionEdgeMode::OpenJumpOnly,
    )
    .expect("open-jump-only edge mode CF assembly should succeed");

    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .combined_weighted_exponent,
        ComplexScalar::new(0.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.trading_time_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.open_jump_characteristic_factor,
        open_jump_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.closed_gap_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        open_jump_characteristic_factor,
        1.0e-13,
    );
}

#[test]
fn edge_mode_closed_gap_only_isolates_closed_gap_factor_in_full_cf_assembly() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.35, 0.25);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.7,
        open_jump: 0.0,
        closed_gap: 0.6,
    })
    .expect("valid return decomposition payload should construct");
    let closed_gap_characteristic_factor = ComplexScalar::new(0.65, -0.35);

    let assembled = compose_total_return_characteristic_function_with_edge_mode(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(0.83, 0.28),
        closed_gap_characteristic_factor,
        ReturnDecompositionEdgeMode::ClosedGapOnly,
    )
    .expect("closed-gap-only edge mode CF assembly should succeed");

    assert_complex_close(
        assembled
            .trading_time_exponent_contributions
            .combined_weighted_exponent,
        ComplexScalar::new(0.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.trading_time_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.open_jump_characteristic_factor,
        ComplexScalar::new(1.0, 0.0),
        1.0e-13,
    );
    assert_complex_close(
        assembled.closed_gap_characteristic_factor,
        closed_gap_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_news_jump_characteristic_factor,
        closed_gap_characteristic_factor,
        1.0e-13,
    );
    assert_complex_close(
        assembled.combined_characteristic_factor,
        closed_gap_characteristic_factor,
        1.0e-13,
    );
}

#[test]
fn full_characteristic_function_assembly_rejects_non_finite_open_jump_factor() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.1, 0.0);
    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.0,
        a_minus: 0.4,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let error = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(f64::NAN, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect_err("non-finite open-jump factor must fail full CF assembly");

    assert!(matches!(
        error,
        ReturnCharacteristicFunctionAssemblyError::InvalidOpenJumpCharacteristicFactor { value }
        if value.real.is_nan()
    ));
}

#[test]
fn full_characteristic_function_assembly_rejects_non_finite_closed_gap_factor() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(0.1, 0.0);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.0,
        a_minus: 0.4,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let error = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(f64::NAN, 0.0),
    )
    .expect_err("non-finite closed-gap factor must fail full CF assembly");

    assert!(matches!(
        error,
        ReturnCharacteristicFunctionAssemblyError::InvalidClosedGapCharacteristicFactor { value }
        if value.real.is_nan()
    ));
}

#[test]
fn oracle_output_cf_assembly_rejects_non_finite_combined_news_jump_factor() {
    let error = compose_total_return_characteristic_function_from_oracle_output(
        oracle_output_with_transform_value(ComplexScalar::new(1.0, 0.0)),
        ComplexScalar::new(1.0e308, 1.0e308),
        ComplexScalar::new(1.0e308, 1.0e308),
    )
    .expect_err("non-finite combined news-jump factor must fail assembly");

    assert!(matches!(
        error,
        ReturnCharacteristicFunctionAssemblyError::NonFiniteCombinedNewsJumpCharacteristicFactor { .. }
    ));
}

#[test]
fn full_characteristic_function_assembly_rejects_invalid_return_decomposition_payload() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let invalid_return_decomposition = ReturnDecomposition {
        a_plus: -0.1,
        a_minus: 0.3,
        open_jump: 0.0,
        closed_gap: 0.0,
    };

    let error = compose_total_return_characteristic_function(
        driver_parameters,
        ComplexScalar::new(0.0, 0.0),
        invalid_return_decomposition,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect_err("invalid return decomposition must fail full CF assembly");

    assert!(matches!(
        error,
        ReturnCharacteristicFunctionAssemblyError::InvalidReturnDecomposition { .. }
    ));
}

#[test]
fn full_characteristic_function_regression_guard_detects_sell_side_sign_inversion() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let argument = ComplexScalar::new(1.1, 0.0);

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.0,
        a_minus: 1.0,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let assembled = compose_total_return_characteristic_function(
        driver_parameters,
        argument,
        return_decomposition,
        ComplexScalar::new(1.0, 0.0),
        ComplexScalar::new(1.0, 0.0),
    )
    .expect("full CF assembly should succeed");

    let incorrect_sell_side_positive_u = driver_parameters
        .sell_side_parameters
        .characteristic_exponent(argument);
    let incorrect_positive_u_combined = complex_exponential(
        assembled
            .trading_time_exponent_contributions
            .buy_side_weighted_exponent
            .add(incorrect_sell_side_positive_u),
    );
    let regression_distance = assembled
        .combined_characteristic_factor
        .subtract(incorrect_positive_u_combined)
        .norm();

    assert!(
        regression_distance > 1.0e-6,
        "sell-side sign-convention regression detected in full CF assembly with distance={regression_distance}"
    );
}

#[test]
fn anchored_local_tilt_solver_accepts_kappa_zero_nested_case() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.1,
        a_minus: 0.7,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let theta = -0.05;

    let request = pricing_request_for_local_tilt(
        return_decomposition,
        theta,
        disabled_boundary_input(
            0.5,
            HawkesExcitationState {
                y_plus_plus: 0.2,
                y_plus_minus: 0.1,
                y_minus_plus: 0.3,
                y_minus_minus: 0.4,
            },
        ),
    );

    let diagnostics =
        solve_anchored_local_pricing_tilt(driver_parameters, return_decomposition, request, 1.0e-8)
            .expect("kappa=0 nested case should solve");

    assert_eq!(diagnostics.policy, PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY);
    assert!(diagnostics.absolute_residual <= diagnostics.tolerance);
}

#[test]
fn anchored_local_tilt_solver_accepts_positive_kappa_with_zero_deviation() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 1.0,
        a_minus: 0.8,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let theta = -0.04;

    let boundary_input = AnchoredVanillaOptionPricingBoundaryInput::try_new(
        0,
        HawkesExcitationState {
            y_plus_plus: 0.0,
            y_plus_minus: 0.0,
            y_minus_plus: 0.0,
            y_minus_minus: 0.0,
        },
        MeanReversionFittedParameters::try_new(
            0.7,
            MeanReversionAnchorModel::constant(0.0).expect("constant anchor should construct"),
        )
        .expect("mean reversion parameters should construct"),
        MeanReversionRuntimeIntervalAnchors::try_new(
            "pricing_constant_anchor_interval_grid.v1",
            0.0,
            vec![0.0],
            vec![0.5],
        )
        .expect("runtime interval anchors should construct"),
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0)
            .expect("observed boundary state should construct"),
    )
    .expect("anchored pricing boundary input should construct");

    let request = pricing_request_for_local_tilt(return_decomposition, theta, boundary_input);

    let diagnostics =
        solve_anchored_local_pricing_tilt(driver_parameters, return_decomposition, request, 1.0e-8)
            .expect("positive-kappa zero-deviation case should solve");

    assert!(diagnostics.absolute_residual <= diagnostics.tolerance);
    assert_eq!(
        diagnostics
            .boundary_state_summary
            .observed_boundary_state
            .deviation,
        0.0
    );
}

#[test]
fn anchored_local_tilt_solver_accepts_positive_kappa_with_non_zero_deviation() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.6,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let theta = -0.03;
    let kappa: f64 = 0.8;
    let deviation: f64 = 0.15;
    let interval_delta: f64 = 0.5;

    let target_shift = ((-kappa * interval_delta).exp() - 1.0) * deviation;

    let request = pricing_request_for_local_tilt(
        return_decomposition,
        theta,
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.1,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.1,
            },
            MeanReversionFittedParameters::try_new(
                kappa,
                MeanReversionAnchorModel::constant(0.0).expect("constant anchor should construct"),
            )
            .expect("mean reversion parameters should construct"),
            MeanReversionRuntimeIntervalAnchors::try_new(
                "pricing_constant_anchor_interval_grid.v1",
                0.0,
                vec![0.0],
                vec![interval_delta],
            )
            .expect("runtime interval anchors should construct"),
            MeanReversionBoundaryState::try_new(deviation, 0.0, deviation)
                .expect("observed boundary state should construct"),
        )
        .expect("anchored pricing boundary input should construct"),
    );

    let diagnostics =
        solve_anchored_local_pricing_tilt(driver_parameters, return_decomposition, request, 1.0e-8)
            .expect("positive-kappa non-zero-deviation case should solve");

    assert_eq!(
        diagnostics.target_log_moment_exponent,
        diagnostics.observed_log_moment_exponent - diagnostics.residual
    );
    assert!(
        (diagnostics.boundary_state_summary.deviation_decay_factor
            - (-kappa * interval_delta).exp())
        .abs()
            < 1.0e-12
    );
    assert!((target_shift - ((-kappa * interval_delta).exp() - 1.0) * deviation).abs() < 1.0e-12);
    assert!(diagnostics.absolute_residual <= diagnostics.tolerance);
}

#[test]
fn anchored_local_tilt_solver_rejects_inadmissible_states() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 1.2,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let request = VanillaOptionPricingRequest::try_new_with_interval_delta(
        VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, 25.0, -25.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        disabled_boundary_input(
            0.5,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
        ),
        valid_model_parameters(),
        0.5,
    )
    .expect("request should construct");

    let error =
        solve_anchored_local_pricing_tilt(driver_parameters, return_decomposition, request, 1.0e-8)
            .expect_err("inadmissible target should fail");

    assert!(matches!(
        error,
        AnchoredLocalTiltSolveError::NoAdmissibleLocalTilt { .. }
    ));
}

fn pricing_request_for_local_tilt(
    return_decomposition: ReturnDecomposition,
    theta: f64,
    boundary_input: AnchoredVanillaOptionPricingBoundaryInput,
) -> VanillaOptionPricingRequest {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");
    let buy_side_log_moment_exponent =
        evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
            driver_parameters.buy_side_parameters,
            -1.0,
            theta,
        )
        .expect("buy-side transformed exponent should construct");
    let sell_side_log_moment_exponent =
        evaluate_nig_esscher_log_moment_exponent_for_pure_imaginary_argument(
            driver_parameters.sell_side_parameters,
            1.0,
            theta,
        )
        .expect("sell-side transformed exponent should construct");

    let combined = return_decomposition.a_plus.mul_add(
        buy_side_log_moment_exponent,
        return_decomposition.a_minus * sell_side_log_moment_exponent,
    );

    let target_adjustment = boundary_input
        .observed_return_adjustment()
        .expect("boundary input should provide observed-return adjustment");

    let risk_free_rate = (combined + target_adjustment) / 0.5;

    VanillaOptionPricingRequest::try_new_with_interval_delta(
        VanillaOptionPricingMarketInputs::try_new(101.0, 100.0, 1.0, risk_free_rate, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        boundary_input,
        valid_model_parameters(),
        0.5,
    )
    .expect("request should construct")
}
