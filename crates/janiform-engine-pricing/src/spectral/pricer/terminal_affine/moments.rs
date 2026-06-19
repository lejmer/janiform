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

    use super::super::core::{
        TerminalAffineExpectedLogReturn, TerminalAffineExpectedLogReturnInput,
        TerminalAffineMeasurePolicy, solve_terminal_affine_expected_log_return,
    };
    use crate::VanillaOptionPricingIntervalFactors;
    use crate::grids::SpectralThetaWarmStartKey;
    use crate::tests::support::build_valid_vanilla_option_request;

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

    fn solve_test_expected_projection(
        pricing_request: &crate::VanillaOptionPricingRequest,
        measure_policy: TerminalAffineMeasurePolicy,
    ) -> TerminalAffineExpectedLogReturn {
        let interval_factor_grids = [VanillaOptionPricingIntervalFactors::try_new(1.0, 0.0, 0.0)
            .expect("interval factors should construct")];

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
