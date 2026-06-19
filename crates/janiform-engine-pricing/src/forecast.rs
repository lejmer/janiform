use janiform_foundations::complex_math::ComplexScalar;

use crate::VanillaOptionPricingRequest;
use crate::grids::SpectralThetaWarmStartKey;
use crate::runner::{VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError};
use crate::spectral::{
    TerminalAffineExpectedLogReturnInput, TerminalAffineExplicitLogPriceDomain,
    TerminalAffineMaturitySolveInput, TerminalAffineMeasurePolicy,
    TerminalAffineSpectralDomainInput, solve_terminal_affine_expected_log_return,
    solve_terminal_affine_maturity_law,
};
use crate::thread_pool::build_pricing_thread_pool;

/// Measure used by model-based affine forecast projection.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum AffineForecastMeasure {
    /// Use fitted physical dynamics without a martingale tilt or pricing-kernel density transform.
    Physical,
    /// Use the terminal-affine risk-neutral martingale transform.
    RiskNeutral,
}

impl AffineForecastMeasure {
    const fn terminal_affine_policy(self) -> TerminalAffineMeasurePolicy {
        match self {
            Self::Physical => TerminalAffineMeasurePolicy::Physical,
            Self::RiskNeutral => TerminalAffineMeasurePolicy::RiskNeutralMartingale,
        }
    }
}

/// Input for a terminal affine forecast law over an explicit log-return support.
pub struct AffineForecastTerminalLawInput<'a> {
    /// Validated pricing-engine request carrying the model, boundary state, and carry inputs.
    pub pricing_request: &'a VanillaOptionPricingRequest,
    /// Number of runtime intervals covered by the terminal law.
    pub maturity_count: usize,
    /// Terminal maturity in Janiform intraday hours.
    pub maturity_interval: f64,
    /// Runtime interval widths in Janiform intraday hours.
    pub interval_deltas: &'a [f64],
    /// Runtime interval factors aligned to `interval_deltas`.
    pub interval_factor_grids: &'a [VanillaOptionPricingIntervalFactors],
    /// Forecast measure policy.
    pub measure: AffineForecastMeasure,
    /// Lower terminal log-return grid support.
    pub terminal_log_return_lower_bound: f64,
    /// Upper terminal log-return grid support.
    pub terminal_log_return_upper_bound: f64,
    /// Number of COS frequency nodes.
    pub node_count: usize,
    /// Optional CPU count for terminal-affine frequency work.
    pub cpu_count: Option<usize>,
}

/// Input for expected return and arithmetic price-moment projection at one forecast prefix.
pub struct AffineForecastExpectedLogReturnInput<'a> {
    /// Validated pricing-engine request carrying the model, boundary state, and carry inputs.
    pub pricing_request: &'a VanillaOptionPricingRequest,
    /// Number of runtime intervals covered by this prefix.
    pub maturity_count: usize,
    /// Prefix maturity in Janiform intraday hours.
    pub maturity_interval: f64,
    /// Runtime interval widths in Janiform intraday hours.
    pub interval_deltas: &'a [f64],
    /// Runtime interval factors aligned to `interval_deltas`.
    pub interval_factor_grids: &'a [VanillaOptionPricingIntervalFactors],
    /// Forecast measure policy.
    pub measure: AffineForecastMeasure,
}

/// Terminal characteristic law used by the forecast distribution projector.
#[derive(Debug, Clone)]
pub struct AffineForecastTerminalLaw {
    /// Terminal maturity in Janiform intraday hours.
    pub maturity_interval: f64,
    /// Initial model log price coordinate.
    pub observed_log_price: f64,
    /// Expected terminal log return under the requested measure.
    pub expected_log_return: f64,
    /// Natural log of the arithmetic expected terminal price ratio under the requested measure.
    pub log_expected_price_ratio: f64,
    /// Lower terminal log-return grid support.
    pub terminal_log_return_lower_bound: f64,
    /// Upper terminal log-return grid support.
    pub terminal_log_return_upper_bound: f64,
    /// Lower terminal model log-price grid support.
    pub terminal_log_price_lower_bound: f64,
    /// Upper terminal model log-price grid support.
    pub terminal_log_price_upper_bound: f64,
    /// Number of COS frequency nodes.
    pub node_count: usize,
    /// Terminal characteristic function values for the model log-price coordinate.
    pub characteristic_function_values: Vec<ComplexScalar>,
}

/// Expected return and arithmetic price-moment projection for one forecast prefix.
#[derive(Debug, Clone, Copy)]
pub struct AffineForecastExpectedLogReturn {
    /// Prefix maturity in Janiform intraday hours.
    pub maturity_interval: f64,
    /// Expected log return from the forecast start.
    pub expected_log_return: f64,
    /// Natural log of the arithmetic expected price ratio from the forecast start.
    pub log_expected_price_ratio: f64,
}

/// Solve the terminal affine law for an explicit terminal log-return support.
pub fn solve_affine_forecast_terminal_law(
    input: AffineForecastTerminalLawInput<'_>,
) -> Result<AffineForecastTerminalLaw, VanillaOptionPricingRunError> {
    let driver_parameters = forecast_driver_parameters(input.pricing_request)?;

    let observed_log_price = input
        .pricing_request
        .boundary_input
        .observed_boundary_state
        .observed_log_price;

    let projection = solve_terminal_affine_expected_log_return(
        TerminalAffineExpectedLogReturnInput {
            pricing_request: input.pricing_request,
            maturity_count: input.maturity_count,
            maturity_interval: input.maturity_interval,
            interval_deltas: input.interval_deltas,
            interval_factor_grids: input.interval_factor_grids,
            driver_parameters,
            measure_policy: input.measure.terminal_affine_policy(),
            theta_warm_start_key: forecast_theta_warm_start_key(
                input.pricing_request,
                input.maturity_count,
            ),
        },
        None,
        &mut None,
    )?;

    let pricing_thread_pool = build_pricing_thread_pool(input.cpu_count);

    let solved_law = solve_terminal_affine_maturity_law(
        TerminalAffineMaturitySolveInput {
            pricing_request: input.pricing_request,
            maturity_count: input.maturity_count,
            maturity_interval: input.maturity_interval,
            spectral_domain_input: TerminalAffineSpectralDomainInput::ExplicitLogPrice(
                TerminalAffineExplicitLogPriceDomain {
                    lower_bound: observed_log_price + input.terminal_log_return_lower_bound,
                    upper_bound: observed_log_price + input.terminal_log_return_upper_bound,
                    node_count: input.node_count,
                },
            ),
            interval_deltas: input.interval_deltas,
            interval_factor_grids: input.interval_factor_grids,
            driver_parameters,
            measure_policy: input.measure.terminal_affine_policy(),
            include_node_price_sensitivities: false,
            pricing_thread_pool: pricing_thread_pool.as_ref(),
            theta_warm_start_key: forecast_theta_warm_start_key(
                input.pricing_request,
                input.maturity_count,
            ),
        },
        None,
        &mut None,
    )?;

    Ok(AffineForecastTerminalLaw {
        maturity_interval: solved_law.maturity_interval,
        observed_log_price: solved_law.observed_log_price,
        expected_log_return: solved_law.expected_log_return,
        log_expected_price_ratio: projection.log_expected_price_ratio,
        terminal_log_return_lower_bound: input.terminal_log_return_lower_bound,
        terminal_log_return_upper_bound: input.terminal_log_return_upper_bound,
        terminal_log_price_lower_bound: solved_law.terminal_anchor_value
            + solved_law.deviation_lower_bound,
        terminal_log_price_upper_bound: solved_law.terminal_anchor_value
            + solved_law.deviation_upper_bound,
        node_count: solved_law.node_count,
        characteristic_function_values: solved_law.characteristic_function_values,
    })
}

/// Project the expected log return and arithmetic expected price ratio for one forecast prefix.
pub fn solve_affine_forecast_expected_log_return(
    input: AffineForecastExpectedLogReturnInput<'_>,
) -> Result<AffineForecastExpectedLogReturn, VanillaOptionPricingRunError> {
    let driver_parameters = forecast_driver_parameters(input.pricing_request)?;

    let projection = solve_terminal_affine_expected_log_return(
        TerminalAffineExpectedLogReturnInput {
            pricing_request: input.pricing_request,
            maturity_count: input.maturity_count,
            maturity_interval: input.maturity_interval,
            interval_deltas: input.interval_deltas,
            interval_factor_grids: input.interval_factor_grids,
            driver_parameters,
            measure_policy: input.measure.terminal_affine_policy(),
            theta_warm_start_key: forecast_theta_warm_start_key(
                input.pricing_request,
                input.maturity_count,
            ),
        },
        None,
        &mut None,
    )?;

    Ok(AffineForecastExpectedLogReturn {
        maturity_interval: projection.maturity_interval,
        expected_log_return: projection.expected_log_return,
        log_expected_price_ratio: projection.log_expected_price_ratio,
    })
}

fn forecast_driver_parameters(
    pricing_request: &VanillaOptionPricingRequest,
) -> Result<janiform_model_kernels::nig::NigBuySellDriverParameters, VanillaOptionPricingRunError> {
    pricing_request
        .model_parameters
        .nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidDriverParameters {
                source: Box::new(source),
            },
        )
}

fn forecast_theta_warm_start_key(
    pricing_request: &VanillaOptionPricingRequest,
    maturity_count: usize,
) -> SpectralThetaWarmStartKey {
    SpectralThetaWarmStartKey::new(
        pricing_request.market_inputs.risk_free_rate.to_bits(),
        pricing_request.market_inputs.dividend_yield.to_bits(),
        maturity_count,
    )
}
