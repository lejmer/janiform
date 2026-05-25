use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformDVector, ExponentialAffineTransformIntegrationResult,
    ExponentialAffineTransformOutput,
};
use janiform_foundations::complex_math::{ComplexScalar, complex_exponential, complex_multiply};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use super::super::domain::SpectralDomain;
use super::super::{
    RiskNeutralAnchorMaterialization, SpectralPricingIntervalDiagnostics,
    VanillaOptionPricingIntervalFactors, VanillaOptionPricingRequest, VanillaOptionPricingRunError,
    install_pricing_parallelism,
};
use super::core::{SolvedMaturityLaw, interval_transition_context_from_parts};
use crate::runner::solve_pricing_log_return_transform_for_context;

#[derive(Debug, Clone, Copy)]
struct AffineExponent {
    c_value: ComplexScalar,
    d_vector: ExponentialAffineTransformDVector,
}

impl AffineExponent {
    fn zero() -> Self {
        Self {
            c_value: ComplexScalar::new(0.0, 0.0),
            d_vector: ExponentialAffineTransformDVector::zero(),
        }
    }

    fn from_output(output: ExponentialAffineTransformOutput) -> Self {
        Self {
            c_value: output.c_at_delta,
            d_vector: output.d_at_delta,
        }
    }

    fn subtract(self, other: Self) -> Self {
        Self {
            c_value: self.c_value.subtract(other.c_value),
            d_vector: self.d_vector.subtract(other.d_vector),
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            c_value: self.c_value.plus(other.c_value),
            d_vector: self.d_vector.plus(other.d_vector),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FrequencySolveOutput {
    characteristic_value: ComplexScalar,
    representative_transform: Option<ExponentialAffineTransformIntegrationResult>,
}

#[derive(Clone, Copy)]
struct DirectAffineMaturitySolveContext<'a> {
    pricing_request: &'a VanillaOptionPricingRequest,
    materialization: &'a RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    spectral_domain: SpectralDomain,
    interval_factors: &'a [VanillaOptionPricingIntervalFactors],
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    denominator_exponents: &'a [AffineExponent],
}

pub(in crate::spectral::pricer) fn can_solve_maturity_laws_with_direct_affine_composition(
    pricing_request: &VanillaOptionPricingRequest,
    include_node_price_sensitivities: bool,
) -> bool {
    pricing_request
        .boundary_input
        .mean_reversion_fitted_parameters
        .kappa
        == 0.0
        && !include_node_price_sensitivities
}

pub(in crate::spectral::pricer) fn solve_direct_affine_maturity_law(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    spectral_domain: SpectralDomain,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
) -> Result<SolvedMaturityLaw, VanillaOptionPricingRunError> {
    validate_direct_affine_maturity_inputs(materialization, maturity_count, interval_factors)?;

    let denominator_exponents = build_denominator_exponents(
        pricing_request,
        materialization,
        maturity_count,
        interval_factors,
        driver_parameters,
    )?;

    let frequency_outputs = solve_frequency_outputs(
        DirectAffineMaturitySolveContext {
            pricing_request,
            materialization,
            maturity_count,
            spectral_domain,
            interval_factors,
            driver_parameters,
            denominator_exponents: denominator_exponents.as_slice(),
        },
        pricing_thread_pool,
    )?;

    let representative_transform = frequency_outputs
        .iter()
        .find_map(|output| output.representative_transform);

    Ok(SolvedMaturityLaw {
        maturity_interval: materialization.runtime_interval_anchors.interval_deltas
            [..maturity_count]
            .iter()
            .sum::<f64>(),
        terminal_anchor_value: materialization.runtime_interval_anchors.end_anchor_values
            [maturity_count - 1],
        deviation_lower_bound: spectral_domain.deviation_lower_bound,
        deviation_upper_bound: spectral_domain.deviation_upper_bound,
        node_count: spectral_domain.node_count,
        observed_log_price: pricing_request
            .boundary_input
            .observed_boundary_state
            .observed_log_price,
        representative_transform,
        characteristic_function_values: frequency_outputs
            .into_iter()
            .map(|output| output.characteristic_value)
            .collect(),
        characteristic_function_sensitivities: None,
        interval_summaries: build_direct_interval_summaries(
            pricing_request,
            materialization,
            maturity_count,
        ),
    })
}

fn validate_direct_affine_maturity_inputs(
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
) -> Result<(), VanillaOptionPricingRunError> {
    if maturity_count == 0
        || maturity_count > interval_factors.len()
        || maturity_count > materialization.local_transition_theta_path.len()
        || maturity_count + 1 > materialization.expected_hawkes_state_path.len()
    {
        return Err(VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "direct affine maturity solve received inconsistent interval inputs"
                .to_string(),
        });
    }

    Ok(())
}

fn build_denominator_exponents(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    interval_factors: &[VanillaOptionPricingIntervalFactors],
    driver_parameters: janiform_model_kernels::nig::NigBuySellDriverParameters,
) -> Result<Vec<AffineExponent>, VanillaOptionPricingRunError> {
    let mut denominator_exponents = Vec::with_capacity(maturity_count);

    for interval_offset in 0..maturity_count {
        let transition_context = interval_transition_context_from_parts(
            pricing_request,
            materialization,
            interval_factors[interval_offset],
            interval_offset,
        )?;

        let theta = materialization.local_transition_theta_path[interval_offset];

        let result = solve_pricing_log_return_transform_for_context(
            &transition_context.base_context,
            driver_parameters,
            ComplexScalar::new(0.0, -theta),
            ExponentialAffineTransformDVector::zero(),
        )?;

        denominator_exponents.push(AffineExponent::from_output(required_transform_output(
            result,
            "direct affine denominator",
        )?));
    }

    Ok(denominator_exponents)
}

fn solve_frequency_outputs(
    context: DirectAffineMaturitySolveContext<'_>,
    pricing_thread_pool: Option<&rayon::ThreadPool>,
) -> Result<Vec<FrequencySolveOutput>, VanillaOptionPricingRunError> {
    if let Some(pricing_thread_pool) = pricing_thread_pool {
        return install_pricing_parallelism(Some(pricing_thread_pool), || {
            (0..context.spectral_domain.node_count)
                .into_par_iter()
                .map(|frequency_index| solve_one_frequency(context, frequency_index))
                .collect()
        });
    }

    (0..context.spectral_domain.node_count)
        .map(|frequency_index| solve_one_frequency(context, frequency_index))
        .collect()
}

fn solve_one_frequency(
    context: DirectAffineMaturitySolveContext<'_>,
    frequency_index: usize,
) -> Result<FrequencySolveOutput, VanillaOptionPricingRunError> {
    let frequency = context.spectral_domain.basis_frequency(frequency_index);

    let mut continuation = AffineExponent::zero();
    let mut representative_transform = None;

    for interval_offset in (0..context.maturity_count).rev() {
        let transition_context = interval_transition_context_from_parts(
            context.pricing_request,
            context.materialization,
            context.interval_factors[interval_offset],
            interval_offset,
        )?;

        let theta = context.materialization.local_transition_theta_path[interval_offset];

        let result = solve_pricing_log_return_transform_for_context(
            &transition_context.base_context,
            context.driver_parameters,
            ComplexScalar::new(frequency, -theta),
            continuation.d_vector,
        )?;

        if representative_transform.is_none() {
            representative_transform = Some(result);
        }

        let numerator = AffineExponent::from_output(required_transform_output(
            result,
            "direct affine numerator",
        )?);

        continuation = continuation
            .add(numerator)
            .subtract(context.denominator_exponents[interval_offset]);
    }

    let initial_state = context.pricing_request.boundary_input.hawkes_boundary_state;

    let stochastic_exponent = continuation
        .c_value
        .plus(continuation.d_vector.dot_state(initial_state));

    let stochastic_factor = complex_exponential(stochastic_exponent);

    let initial_log_price_phase = complex_exponential(ComplexScalar::new(
        0.0,
        frequency
            * context
                .pricing_request
                .boundary_input
                .observed_boundary_state
                .observed_log_price,
    ));

    Ok(FrequencySolveOutput {
        characteristic_value: complex_multiply(initial_log_price_phase, stochastic_factor),
        representative_transform,
    })
}

fn required_transform_output(
    result: ExponentialAffineTransformIntegrationResult,
    context: &'static str,
) -> Result<ExponentialAffineTransformOutput, VanillaOptionPricingRunError> {
    result
        .output
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!(
                "{context} transform returned no output with status {:?}",
                result.status
            ),
        })
}

fn build_direct_interval_summaries(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
) -> Vec<SpectralPricingIntervalDiagnostics> {
    (0..maturity_count)
        .map(|interval_offset| {
            let runtime_anchors = &materialization.runtime_interval_anchors;

            let current_anchor_value = if interval_offset == 0 {
                runtime_anchors.start_anchor_value
            } else {
                runtime_anchors.end_anchor_values[interval_offset - 1]
            };

            let theta = materialization.local_transition_theta_path[interval_offset];

            SpectralPricingIntervalDiagnostics {
                interval_index: pricing_request.boundary_input.interval_index + interval_offset,
                current_anchor_value,
                next_anchor_value: runtime_anchors.end_anchor_values[interval_offset],
                interval_delta: runtime_anchors.interval_deltas[interval_offset],
                expected_hawkes_boundary_state_before: materialization.expected_hawkes_state_path
                    [interval_offset],
                expected_hawkes_boundary_state_after: materialization.expected_hawkes_state_path
                    [interval_offset + 1],
                minimum_solved_theta: theta,
                maximum_solved_theta: theta,
            }
        })
        .collect()
}
