use crate::{
    AnchoredLocalTiltSolveError, AnchoredVanillaOptionPricingBoundaryInput,
    PRICING_LOCAL_ANCHORED_TILT_SOLVE_POLICY, RETURN_CHARACTERISTIC_FUNCTION_COMPOSITION_POLICY,
    RETURN_CHARACTERISTIC_FUNCTION_DOMAIN_GUARD_POLICY, ReturnCharacteristicFunctionAssemblyError,
    VanillaOptionPayoffConfiguration, VanillaOptionPayoffType, VanillaOptionPricingMarketInputs,
    VanillaOptionPricingRequest,
    characteristic::{
        LocalTiltResidualEvaluation, build_local_tilt_bracket_with_residual_evaluator,
        differentiate_local_tilt_root_for_shared_plus_shift_configuration,
    },
    compose_nig_trading_time_exponent, compose_total_return_characteristic_function,
    compose_total_return_characteristic_function_from_oracle_output,
    compose_total_return_characteristic_function_with_edge_mode, solve_anchored_local_pricing_tilt,
    time_scaling::intraday_year_fraction,
};
use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformOutput,
};
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
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
            MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
        )
        .expect("mean reversion parameters should construct"),
        0.5,
        MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0, 0.0)
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
                MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
            )
            .expect("mean reversion parameters should construct"),
            interval_delta,
            MeanReversionBoundaryState::try_new(deviation, 0.0, 0.0, deviation)
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
fn anchored_local_tilt_implicit_root_sensitivity_matches_finite_difference() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct");
    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let return_decomposition = ReturnDecomposition::try_new(ReturnDecompositionInput {
        a_plus: 0.9,
        a_minus: 0.6,
        open_jump: 0.0,
        closed_gap: 0.0,
    })
    .expect("valid return decomposition payload should construct");

    let request = pricing_request_for_local_tilt(
        return_decomposition,
        -0.03,
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.1,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.1,
            },
            MeanReversionFittedParameters::try_new(
                0.8,
                MeanReversionAnchorModel::zero_gain_endogenous_core_pressure_level(),
            )
            .expect("mean reversion parameters should construct"),
            0.5,
            MeanReversionBoundaryState::try_new(0.15, 0.0, 0.0, 0.15)
                .expect("observed boundary state should construct"),
        )
        .expect("anchored pricing boundary input should construct"),
    );

    let solved_theta = solve_anchored_local_pricing_tilt(
        driver_parameters,
        return_decomposition,
        request.clone(),
        1.0e-8,
    )
    .expect("local tilt should solve")
    .theta;

    let implicit_sensitivity = differentiate_local_tilt_root_for_shared_plus_shift_configuration(
        driver_parameters,
        return_decomposition,
        solved_target_log_moment_exponent(&request),
        solved_theta,
    )
    .expect("implicit theta sensitivity should exist");
    let alpha_finite_difference = finite_difference_local_tilt_theta_sensitivity(
        return_decomposition,
        &request,
        |value| {
            NigSharedPlusShiftConfiguration::try_new(
                value,
                configuration.beta_0,
                configuration.delta,
                configuration.mu,
                configuration.delta_beta,
            )
            .expect("perturbed alpha configuration should construct")
        },
        configuration.alpha,
    );
    let beta_0_finite_difference = finite_difference_local_tilt_theta_sensitivity(
        return_decomposition,
        &request,
        |value| {
            NigSharedPlusShiftConfiguration::try_new(
                configuration.alpha,
                value,
                configuration.delta,
                configuration.mu,
                configuration.delta_beta,
            )
            .expect("perturbed beta_0 configuration should construct")
        },
        configuration.beta_0,
    );

    assert!(
        (implicit_sensitivity.alpha_derivative - alpha_finite_difference).abs() < 1.0e-2,
        "alpha analytic={} finite_difference={}",
        implicit_sensitivity.alpha_derivative,
        alpha_finite_difference,
    );
    assert!(
        (implicit_sensitivity.beta_0_derivative - beta_0_finite_difference).abs() < 1.0e-2,
        "beta_0 analytic={} finite_difference={}",
        implicit_sensitivity.beta_0_derivative,
        beta_0_finite_difference,
    );
    assert!(
        (implicit_sensitivity.delta_derivative
            - finite_difference_local_tilt_theta_sensitivity(
                return_decomposition,
                &request,
                |value| NigSharedPlusShiftConfiguration::try_new(
                    configuration.alpha,
                    configuration.beta_0,
                    value,
                    configuration.mu,
                    configuration.delta_beta,
                )
                .expect("perturbed delta configuration should construct"),
                configuration.delta,
            ))
        .abs()
            < 1.0e-2
    );
    assert!(
        (implicit_sensitivity.mu_derivative
            - finite_difference_local_tilt_theta_sensitivity(
                return_decomposition,
                &request,
                |value| NigSharedPlusShiftConfiguration::try_new(
                    configuration.alpha,
                    configuration.beta_0,
                    configuration.delta,
                    value,
                    configuration.delta_beta,
                )
                .expect("perturbed mu configuration should construct"),
                configuration.mu,
            ))
        .abs()
            < 1.0e-2
    );
    assert!(
        (implicit_sensitivity.delta_beta_derivative
            - finite_difference_local_tilt_theta_sensitivity(
                return_decomposition,
                &request,
                |value| NigSharedPlusShiftConfiguration::try_new(
                    configuration.alpha,
                    configuration.beta_0,
                    configuration.delta,
                    configuration.mu,
                    value,
                )
                .expect("perturbed delta_beta configuration should construct"),
                configuration.delta_beta,
            ))
        .abs()
            < 1.0e-2
    );
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

    let request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(101.0, 100.0, 1.0, 25.0, -25.0)
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

#[test]
fn anchored_local_tilt_bracket_search_shrinks_inward_from_non_admissible_side_boundary() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let (theta, evaluation) = build_local_tilt_bracket_with_residual_evaluator(
        driver_parameters,
        -0.5,
        1.0e-8,
        |theta| {
            if theta < -1.5 {
                return Err(
                    AnchoredLocalTiltSolveError::NonRealObservedLogMomentExponent {
                        theta,
                        value: ComplexScalar::new(0.0, 1.0),
                    },
                );
            }

            Ok(LocalTiltResidualEvaluation {
                observed_log_moment_exponent: theta,
                residual: theta + 0.5,
            })
        },
    )
    .expect("solver should shrink inward from the invalid side boundary and recover the root");

    assert!((theta + 0.5).abs() <= 1.0e-8);
    assert!(evaluation.residual.abs() <= 1.0e-8);
}

#[test]
fn anchored_local_tilt_bracket_search_recovers_from_retryable_invalid_midpoint() {
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let (theta, evaluation) = build_local_tilt_bracket_with_residual_evaluator(
        driver_parameters,
        -0.2,
        1.0e-8,
        |theta| {
            if theta > -0.3 && theta < -0.2 {
                return Err(
                    AnchoredLocalTiltSolveError::NonRealObservedLogMomentExponent {
                        theta,
                        value: ComplexScalar::new(0.0, 1.0),
                    },
                );
            }

            Ok(LocalTiltResidualEvaluation {
                observed_log_moment_exponent: theta,
                residual: theta + 0.2,
            })
        },
    )
    .expect("solver should shrink inward from a retryable invalid midpoint and recover the root");

    assert!((theta + 0.2).abs() <= 1.0e-8);
    assert!(evaluation.residual.abs() <= 1.0e-8);
}

#[test]
fn anchored_local_tilt_bracket_search_recovers_from_retryable_invalid_midpoint_during_sign_search()
{
    let driver_parameters = NigSharedPlusShiftConfiguration::try_new(2.3, -0.8, 0.7, 0.1, -0.2)
        .expect("valid shared-plus-shift payload should construct")
        .derive_driver_parameters()
        .expect("valid shared-plus-shift payload should derive drivers");

    let (theta, evaluation) =
        build_local_tilt_bracket_with_residual_evaluator(driver_parameters, 0.0, 1.0e-8, |theta| {
            if theta > -0.8 && theta < -0.7 {
                return Err(
                    AnchoredLocalTiltSolveError::NonRealObservedLogMomentExponent {
                        theta,
                        value: ComplexScalar::new(0.0, 1.0),
                    },
                );
            }

            Ok(LocalTiltResidualEvaluation {
                observed_log_moment_exponent: theta,
                residual: (theta + 1.4) * (theta + 0.2),
            })
        })
        .expect("solver should shrink inward from a retryable invalid midpoint during sign search");

    assert!((theta + 0.2).abs() <= 1.0e-8);
    assert!(evaluation.residual.abs() <= 1.0e-8);
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
        .local_martingale_deviation_adjustment()
        .expect("boundary input should provide observed-return adjustment");

    let interval_delta = 0.5;
    let risk_free_rate = (combined + target_adjustment) / intraday_year_fraction(interval_delta);

    VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new_single_node(
            101.0,
            100.0,
            1.0,
            risk_free_rate,
            0.0,
        )
        .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        boundary_input,
        valid_model_parameters(),
        interval_delta,
    )
    .expect("request should construct")
}

fn solved_target_log_moment_exponent(request: &VanillaOptionPricingRequest) -> f64 {
    let target_adjustment = request
        .boundary_input
        .local_martingale_deviation_adjustment()
        .expect("boundary input should provide observed-return adjustment");

    (request.market_inputs.risk_free_rate - request.market_inputs.dividend_yield)
        * intraday_year_fraction(request.interval_delta)
        - target_adjustment
}

fn finite_difference_local_tilt_theta_sensitivity(
    return_decomposition: ReturnDecomposition,
    request: &VanillaOptionPricingRequest,
    mut rebuild_configuration: impl FnMut(f64) -> NigSharedPlusShiftConfiguration,
    parameter_value: f64,
) -> f64 {
    let step = 1.0e-5 * parameter_value.abs().max(1.0);
    let lower_configuration = rebuild_configuration(parameter_value - step);
    let upper_configuration = rebuild_configuration(parameter_value + step);

    let lower_theta = solve_anchored_local_pricing_tilt(
        lower_configuration
            .derive_driver_parameters()
            .expect("lower perturbed driver payload should derive"),
        return_decomposition,
        request.clone(),
        1.0e-8,
    )
    .expect("lower perturbed local tilt should solve")
    .theta;
    let upper_theta = solve_anchored_local_pricing_tilt(
        upper_configuration
            .derive_driver_parameters()
            .expect("upper perturbed driver payload should derive"),
        return_decomposition,
        request.clone(),
        1.0e-8,
    )
    .expect("upper perturbed local tilt should solve")
    .theta;

    (upper_theta - lower_theta) / (2.0 * step)
}
