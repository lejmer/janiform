use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformInput, ExponentialAffineTransformInputError,
    ExponentialAffineTransformNewsIntensityInput, ExponentialAffineTransformSolverSettings,
};
use janiform_engine_pricing::{
    VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError,
};
use janiform_engine_sufficient_statistics::{
    SufficientStatisticsTransformSliceContext, SufficientStatisticsTransformSliceContextError,
    SufficientStatisticsTransformSliceSettings, SufficientStatisticsTransformSlices,
    solve_sufficient_statistics_transform_slices_with_solver_settings,
};
use janiform_foundations::complex_math::ComplexScalar;
use janiform_model_kernels::hawkes::HawkesKernelParameters;
use janiform_model_kernels::long_variance::LongVarianceParameters;
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsComponentMarkScaleConfiguration,
};

use super::catalog::BenchmarkScenarioWorkloadProfile;
use super::harness::OraclePricerSufficientStatisticsBenchmarkHarnessError;
use super::policy::OraclePricerSufficientStatisticsBenchmarkPolicy;
use super::utilities::{derive_invocation_seed, interpolate_range, unit_interval_from_seed};

pub fn build_equation_specification(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
) -> Result<
    ExponentialAffineTransformEquationSpecification,
    OraclePricerSufficientStatisticsBenchmarkHarnessError,
> {
    let model_parameters = benchmark_policy.model_parameters;
    let hawkes_kernel_parameters = HawkesKernelParameters::try_new(
        profile
            .model_state
            .baseline_deseasonalized_intensity_lambda_bar,
        model_parameters.hawkes_beta,
        model_parameters.hawkes_alpha,
        model_parameters.hawkes_rho,
    )
    .map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidModelKernelParameters {
            profile_id: profile.profile_id.clone(),
            source: Box::new(source),
        }
    })?;

    let gamma_mark_configuration = GammaSeverityMarkConfiguration::try_new(
        model_parameters.gamma_shape_k,
    )
    .map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidModelGammaMarkConfiguration {
            profile_id: profile.profile_id.clone(),
            source: Box::new(source),
        }
    })?;

    Ok(ExponentialAffineTransformEquationSpecification::new(
        hawkes_kernel_parameters,
        gamma_mark_configuration,
        LongVarianceParameters::neutral(),
    ))
}

pub fn build_oracle_transform_input(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    invocation_seed: u64,
) -> Result<ExponentialAffineTransformInput, OraclePricerSufficientStatisticsBenchmarkHarnessError>
{
    let noise = unit_interval_from_seed(invocation_seed);

    let complementary_noise = 1.0 - noise;
    let solver_probes = benchmark_policy.oracle_solver_probes;

    let news_intensity_factor = interpolate_range(
        solver_probes.news_intensity_factor_min,
        solver_probes.news_intensity_factor_max,
        complementary_noise,
    );

    let known_news_intensity_factor = news_intensity_factor * noise;
    let surprise_news_intensity_factor = news_intensity_factor - known_news_intensity_factor;

    let news_intensity_input = ExponentialAffineTransformNewsIntensityInput::try_new(
        known_news_intensity_factor,
        surprise_news_intensity_factor,
    )
    .map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidOracleTransformInput {
            profile_id: profile.profile_id.clone(),
            source: Box::new(
                ExponentialAffineTransformInputError::InvalidNewsIntensityInput { source },
            ),
        }
    })?;

    ExponentialAffineTransformInput::try_new(
        equation_specification,
        ExponentialAffineTransformActivityLoadings::total_activity(
            ComplexScalar::new(
                interpolate_range(solver_probes.u_probe_min, solver_probes.u_probe_max, noise),
                0.0,
            ),
            ComplexScalar::new(
                interpolate_range(
                    solver_probes.u_probe_min,
                    solver_probes.u_probe_max,
                    complementary_noise,
                ),
                0.0,
            ),
        ),
        ComplexScalar::new(
            interpolate_range(solver_probes.z_probe_min, solver_probes.z_probe_max, noise),
            0.0,
        ),
        ComplexScalar::new(
            interpolate_range(
                solver_probes.z_probe_min,
                solver_probes.z_probe_max,
                complementary_noise,
            ),
            0.0,
        ),
        ComplexScalar::new(0.0, 0.0),
        interpolate_range(
            solver_probes.seasonality_factor_min,
            solver_probes.seasonality_factor_max,
            noise,
        ),
        news_intensity_input,
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_drift: 0.0,
            return_jump_volatility: 0.0,
        },
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 1.0,
            return_jump_drift: 0.0,
            return_jump_volatility: 0.0,
        },
        profile.interval_settings.interval_delta,
        profile.model_state.initial_excitation_state,
    )
    .map_err(|source| {
        OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidOracleTransformInput {
            profile_id: profile.profile_id.clone(),
            source: Box::new(source),
        }
    })
}

pub fn build_pricing_market_inputs(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    invocation_seed: u64,
) -> Result<VanillaOptionPricingMarketInputs, VanillaOptionPricingMarketInputsError> {
    let spot_noise = unit_interval_from_seed(invocation_seed ^ 0x91);
    let strike_noise = unit_interval_from_seed(invocation_seed ^ 0x53);
    let maturity_noise = unit_interval_from_seed(invocation_seed ^ 0xC7);

    let pricing_ranges = benchmark_policy.pricing_input_ranges;

    let spot = pricing_ranges.spot_base
        + profile.portfolio_shape.underlying_count as f64
            * pricing_ranges.spot_underlying_count_scale
        + pricing_ranges.spot_noise_span * spot_noise;

    let strike_ratio = interpolate_range(
        pricing_ranges.strike_ratio_min,
        pricing_ranges.strike_ratio_max,
        strike_noise,
    );

    let strike = spot * strike_ratio;

    let maturity_interval = profile.interval_settings.interval_delta
        * (pricing_ranges.maturity_multiplier_min
            + maturity_noise
                * (profile.portfolio_shape.maturity_bucket_count_per_underlying as f64
                    * pricing_ranges.maturity_bucket_scale));

    let risk_free_rate = interpolate_range(
        pricing_ranges.risk_free_rate_min,
        pricing_ranges.risk_free_rate_max,
        spot_noise,
    );

    let dividend_yield = interpolate_range(
        pricing_ranges.dividend_yield_min,
        pricing_ranges.dividend_yield_max,
        strike_noise,
    );

    VanillaOptionPricingMarketInputs::try_new_single_node(
        spot,
        strike,
        maturity_interval,
        risk_free_rate,
        dividend_yield,
    )
}

pub(crate) fn build_sufficient_statistics_transform_slices(
    profile: &BenchmarkScenarioWorkloadProfile,
    benchmark_policy: &OraclePricerSufficientStatisticsBenchmarkPolicy,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    solver_settings: ExponentialAffineTransformSolverSettings,
    component_seed: u64,
) -> Result<
    Vec<SufficientStatisticsTransformSlices>,
    OraclePricerSufficientStatisticsBenchmarkHarnessError,
> {
    let mut slices_by_interval: Vec<SufficientStatisticsTransformSlices> =
        Vec::with_capacity(profile.interval_settings.horizon_interval_count);

    let sampling_settings = SufficientStatisticsTransformSliceSettings::default();

    for interval_index in 0..profile.interval_settings.horizon_interval_count {
        let interval_seed = derive_invocation_seed(component_seed, interval_index + 1);

        let factor_noise = unit_interval_from_seed(interval_seed);

        let seasonality_factor = interpolate_range(
            benchmark_policy.oracle_solver_probes.seasonality_factor_min,
            benchmark_policy.oracle_solver_probes.seasonality_factor_max,
            factor_noise,
        );

        let news_intensity_factor = interpolate_range(
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_min,
            benchmark_policy
                .oracle_solver_probes
                .news_intensity_factor_max,
            1.0 - factor_noise,
        );

        let known_news_intensity_factor = news_intensity_factor * factor_noise;
        let surprise_news_intensity_factor = news_intensity_factor - known_news_intensity_factor;

        let news_intensity_input = ExponentialAffineTransformNewsIntensityInput::try_new(
            known_news_intensity_factor,
            surprise_news_intensity_factor,
        )
        .map_err(|source| {
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidRiskSliceContext {
                profile_id: profile.profile_id.clone(),
                interval_index,
                source: Box::new(
                    SufficientStatisticsTransformSliceContextError::InvalidNewsIntensity {
                        interval_index,
                        source,
                    },
                ),
            }
        })?;

        let context = SufficientStatisticsTransformSliceContext::try_new(
            interval_index,
            seasonality_factor,
            news_intensity_input,
            profile.interval_settings.interval_delta,
            profile.model_state.initial_excitation_state,
        )
        .map_err(|source| {
            OraclePricerSufficientStatisticsBenchmarkHarnessError::InvalidRiskSliceContext {
                profile_id: profile.profile_id.clone(),
                interval_index,
                source: Box::new(source),
            }
        })?;

        let solved_slices = solve_sufficient_statistics_transform_slices_with_solver_settings(
            equation_specification,
            context,
            sampling_settings,
            Some(solver_settings),
        )
        .map_err(|source| {
            OraclePricerSufficientStatisticsBenchmarkHarnessError::SufficientStatisticsSliceSolveFailure {
                profile_id: profile.profile_id.clone(),
                interval_index,
                source: Box::new(source),
            }
        })?;

        slices_by_interval.push(solved_slices);
    }

    Ok(slices_by_interval)
}
