use crate::characteristic::MARTINGALE_FIRST_LOG_MOMENT_SHIFT;

use super::super::VanillaOptionPricingRunError;
use super::core::{TerminalAffineMaturityContext, TerminalAffineThetaSolution};
use super::cumulant::{
    TerminalAffineCumulantArgument, TerminalAffineCumulantCache, TerminalAffineCumulantEvaluation,
};

pub(super) fn expected_log_return_from_theta_solution(
    context: &TerminalAffineMaturityContext<'_>,
    theta_solution: TerminalAffineThetaSolution,
) -> Result<f64, VanillaOptionPricingRunError> {
    let expected_log_return = context
        .mean_reversion_path
        .terminal_deterministic_log_return
        + theta_solution.denominator.argument_derivative.real;

    if !expected_log_return.is_finite() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "terminal affine expected log return is non-finite".to_string(),
        });
    }

    Ok(expected_log_return)
}

pub(super) fn log_expected_price_ratio_from_theta_solution(
    context: &TerminalAffineMaturityContext<'_>,
    theta_solution: TerminalAffineThetaSolution,
) -> Result<f64, VanillaOptionPricingRunError> {
    let mut cache = TerminalAffineCumulantCache::default();

    let numerator = cache.evaluate(
        context,
        TerminalAffineCumulantArgument::from_theta_shift(
            theta_solution.theta,
            MARTINGALE_FIRST_LOG_MOMENT_SHIFT,
        ),
    )?;

    let log_expected_price_ratio =
        terminal_affine_first_log_moment(context, numerator, theta_solution.denominator);

    if !log_expected_price_ratio.is_finite() {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "terminal affine log expected price ratio is non-finite".to_string(),
        });
    }

    Ok(log_expected_price_ratio)
}

pub(super) fn terminal_affine_first_log_moment(
    context: &TerminalAffineMaturityContext<'_>,
    numerator: TerminalAffineCumulantEvaluation,
    denominator: TerminalAffineCumulantEvaluation,
) -> f64 {
    context
        .mean_reversion_path
        .terminal_deterministic_log_return
        + numerator.exponent.subtract(denominator.exponent).real
}

#[cfg(test)]
mod tests {
    use janiform_foundations::numeric::square_root_machine_epsilon;
    use janiform_model_kernels::market_pressure::{
        MarketPressureParameters, MarketPressureState, MarketPressureTransitionRates,
    };
    use janiform_model_kernels::return_clock::ReturnClockExposureParameters;

    use super::super::core::{
        TerminalAffineExpectedLogReturn, TerminalAffineExpectedLogReturnInput,
        TerminalAffineMeasurePolicy, solve_terminal_affine_expected_log_return,
    };
    use crate::grids::SpectralThetaWarmStartKey;
    use crate::tests::support::{
        build_unit_interval_factor_grids, build_valid_vanilla_option_request,
    };
    use crate::{VanillaOptionPricingModelParameters, VanillaOptionPricingRequest};

    #[test]
    fn risk_neutral_projection_exposes_arithmetic_first_price_moment() {
        let pricing_request = build_valid_vanilla_option_request();

        let projection = solve_test_expected_projection(
            &pricing_request,
            TerminalAffineMeasurePolicy::RiskNeutralMartingale,
        );

        let target_log_expected_price_ratio = (pricing_request.market_inputs.risk_free_rate
            - pricing_request.market_inputs.dividend_yield)
            * crate::time_scaling::intraday_year_fraction(1.0);

        assert!(
            (projection.log_expected_price_ratio - target_log_expected_price_ratio).abs()
                <= square_root_machine_epsilon()
        );
    }

    #[test]
    fn physical_projection_scales_market_pressure_anchor_drift_by_return_clock_exposure() {
        let neutral_exposure_contribution =
            market_pressure_anchor_drift_contribution(ReturnClockExposureParameters::neutral());

        let amplified_exposure_contribution = market_pressure_anchor_drift_contribution(
            ReturnClockExposureParameters::try_new(3.0, 3.0)
                .expect("amplified return-clock exposure should construct"),
        );

        assert!(neutral_exposure_contribution > 0.0);
        assert!(amplified_exposure_contribution > neutral_exposure_contribution);
    }

    fn market_pressure_anchor_drift_contribution(
        return_clock_exposure_parameters: ReturnClockExposureParameters,
    ) -> f64 {
        let active_projection = solve_test_expected_projection(
            &market_pressure_projection_request(1.0, return_clock_exposure_parameters),
            TerminalAffineMeasurePolicy::Physical,
        );

        let inactive_projection = solve_test_expected_projection(
            &market_pressure_projection_request(0.0, return_clock_exposure_parameters),
            TerminalAffineMeasurePolicy::Physical,
        );

        active_projection.expected_log_return - inactive_projection.expected_log_return
    }

    fn market_pressure_projection_request(
        anchor_drift_per_signed_balance: f64,
        return_clock_exposure_parameters: ReturnClockExposureParameters,
    ) -> VanillaOptionPricingRequest {
        let mut pricing_request = build_valid_vanilla_option_request();

        let base_parameters = pricing_request.model_parameters.clone();

        let market_pressure_parameters = MarketPressureParameters::try_new(
            vec![1.0],
            MarketPressureTransitionRates::single_state(),
            anchor_drift_per_signed_balance,
        )
        .expect("market-pressure parameters should construct");

        pricing_request
            .boundary_input
            .market_pressure_boundary_state =
            MarketPressureState::neutral_for_parameters(&market_pressure_parameters)
                .expect("market-pressure boundary state should construct");

        pricing_request.model_parameters = VanillaOptionPricingModelParameters::try_new(
            base_parameters.hawkes_kernel_parameters,
            base_parameters.news_process_configuration,
            base_parameters.nig_shared_plus_shift_configuration,
            return_clock_exposure_parameters,
            base_parameters.long_variance_parameters,
            market_pressure_parameters,
            base_parameters.pricing_kernel_parameters,
        )
        .expect("model parameters should construct");

        pricing_request
            .validate()
            .expect("market-pressure projection request should validate");

        pricing_request
    }

    fn solve_test_expected_projection(
        pricing_request: &VanillaOptionPricingRequest,
        measure_policy: TerminalAffineMeasurePolicy,
    ) -> TerminalAffineExpectedLogReturn {
        let interval_factor_grids = build_unit_interval_factor_grids(1);
        let interval_deltas = [1.0_f64];

        let driver_parameters = pricing_request
            .model_parameters
            .nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .expect("NIG driver parameters should derive");

        let mut progress_observer = None;

        solve_terminal_affine_expected_log_return(
            TerminalAffineExpectedLogReturnInput {
                pricing_request,
                maturity_count: 1,
                maturity_interval: 1.0,
                interval_deltas: &interval_deltas,
                interval_factor_grids: &interval_factor_grids,
                driver_parameters,
                measure_policy,
                theta_warm_start_key: SpectralThetaWarmStartKey::new(
                    0.02_f64.to_bits(),
                    0.01_f64.to_bits(),
                    1,
                ),
            },
            None,
            &mut progress_observer,
        )
        .expect("terminal affine expected projection should solve")
    }
}
