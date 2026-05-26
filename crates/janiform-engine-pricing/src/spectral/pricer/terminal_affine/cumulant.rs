use std::collections::BTreeMap;

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineScheduleCumulantOutput, ExponentialAffineScheduleCumulantProgress,
    ExponentialAffineScheduleCumulantRequest, ExponentialAffineScheduleIntervalInput,
    ExponentialAffineTransformActivityLoadings, ExponentialAffineTransformInputTangent,
    solve_exponential_affine_schedule_cumulant,
    solve_exponential_affine_schedule_cumulant_with_progress,
};
use janiform_foundations::complex_math::{ComplexScalar, complex_multiply};

use crate::characteristic::build_pricing_equation_specification;
use crate::progress::PricingProgressEvent;

use super::super::{VanillaOptionPricingIntervalFactors, VanillaOptionPricingRunError};
use super::core::TerminalAffineMaturityContext;

#[derive(Debug, Clone, Copy)]
pub(super) struct TerminalAffineCumulantEvaluation {
    pub(super) exponent: ComplexScalar,
    pub(super) argument_derivative: ComplexScalar,
    pub(super) buy_activity_premium_derivative: ComplexScalar,
    pub(super) sell_activity_premium_derivative: ComplexScalar,
}

#[derive(Default)]
pub(super) struct TerminalAffineCumulantCache {
    values: BTreeMap<(u64, u64), TerminalAffineCumulantEvaluation>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TerminalAffineCumulantArgument {
    value: ComplexScalar,
}

impl TerminalAffineCumulantArgument {
    pub(super) fn from_theta(theta: f64) -> Self {
        Self {
            value: ComplexScalar::new(theta, 0.0),
        }
    }

    pub(super) fn from_theta_shift(theta: f64, shift: f64) -> Self {
        Self {
            value: ComplexScalar::new(theta + shift, 0.0),
        }
    }

    pub(super) fn from_theta_and_frequency(theta: f64, frequency: f64) -> Self {
        Self {
            value: ComplexScalar::new(theta, frequency),
        }
    }

    fn fourier_argument(self) -> ComplexScalar {
        ComplexScalar::new(self.value.imaginary, -self.value.real)
    }

    fn fourier_argument_tangent(interval_return_weight: f64) -> ComplexScalar {
        ComplexScalar::new(0.0, -interval_return_weight)
    }
}

impl TerminalAffineCumulantCache {
    pub(super) fn evaluate(
        &mut self,
        context: &TerminalAffineMaturityContext<'_>,
        argument: TerminalAffineCumulantArgument,
    ) -> Result<TerminalAffineCumulantEvaluation, VanillaOptionPricingRunError> {
        let mut no_progress_observer = None;

        self.evaluate_with_progress(context, argument, &mut no_progress_observer)
    }

    pub(super) fn evaluate_with_progress(
        &mut self,
        context: &TerminalAffineMaturityContext<'_>,
        argument: TerminalAffineCumulantArgument,
        progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
    ) -> Result<TerminalAffineCumulantEvaluation, VanillaOptionPricingRunError> {
        let cache_key = (
            argument.value.real.to_bits(),
            argument.value.imaginary.to_bits(),
        );
        if let Some(evaluation) = self.values.get(&cache_key).copied() {
            return Ok(evaluation);
        }

        let evaluation = evaluate_terminal_affine_cumulant(context, argument, progress_observer)?;

        self.values.insert(cache_key, evaluation);

        Ok(evaluation)
    }
}

fn evaluate_terminal_affine_cumulant(
    context: &TerminalAffineMaturityContext<'_>,
    argument: TerminalAffineCumulantArgument,
    progress_observer: &mut Option<&mut dyn FnMut(PricingProgressEvent)>,
) -> Result<TerminalAffineCumulantEvaluation, VanillaOptionPricingRunError> {
    let intervals = (0..context.maturity_count)
        .map(|interval_offset| {
            build_terminal_affine_schedule_interval_input(
                context,
                context.interval_factors[interval_offset],
                context.interval_deltas[interval_offset],
                context.mean_reversion_path.interval_return_weights[interval_offset],
                argument,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    let request = ExponentialAffineScheduleCumulantRequest {
        initial_state: context.pricing_request.boundary_input.hawkes_boundary_state,
        intervals,
    };

    let output = match progress_observer.as_deref_mut() {
        Some(progress_observer) => {
            let mut pricing_progress_observer =
                |progress: ExponentialAffineScheduleCumulantProgress| {
                    progress_observer(PricingProgressEvent::interval_progress(
                        progress.completed_interval_count,
                        progress.total_interval_count,
                        progress.current_interval_position,
                        progress.total_interval_count,
                    ));
                };

            solve_exponential_affine_schedule_cumulant_with_progress(
                &request,
                Some(&mut pricing_progress_observer),
            )
        }
        None => solve_exponential_affine_schedule_cumulant(&request),
    }
    .map_err(
        |source| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!("terminal affine schedule cumulant failed: {source}"),
        },
    )?;

    Ok(cumulant_evaluation_from_schedule_output(output))
}

fn cumulant_evaluation_from_schedule_output(
    output: ExponentialAffineScheduleCumulantOutput,
) -> TerminalAffineCumulantEvaluation {
    TerminalAffineCumulantEvaluation {
        exponent: output.exponent,
        argument_derivative: output.argument_derivative,
        buy_activity_premium_derivative: output.buy_activity_premium_derivative,
        sell_activity_premium_derivative: output.sell_activity_premium_derivative,
    }
}

fn build_terminal_affine_schedule_interval_input(
    context: &TerminalAffineMaturityContext<'_>,
    interval_factors: VanillaOptionPricingIntervalFactors,
    interval_delta: f64,
    interval_return_weight: f64,
    argument: TerminalAffineCumulantArgument,
) -> Result<ExponentialAffineScheduleIntervalInput, VanillaOptionPricingRunError> {
    let weighted_argument = argument.fourier_argument().scale(interval_return_weight);

    let weighted_argument_tangent =
        TerminalAffineCumulantArgument::fourier_argument_tangent(interval_return_weight);

    let buy_side_characteristic_exponent = context
        .driver_parameters
        .buy_side_characteristic_exponent(weighted_argument);

    let sell_side_characteristic_exponent = context
        .driver_parameters
        .sell_side_characteristic_exponent_for_return_composition(weighted_argument);

    let buy_side_characteristic_exponent_tangent = complex_multiply(
        context
            .driver_parameters
            .differentiate_buy_side_characteristic_exponent_wrt_argument(weighted_argument)
            .map_err(
                |source| VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: format!(
                        "terminal affine buy-side argument derivative failed: {source}"
                    ),
                },
            )?,
        weighted_argument_tangent,
    );

    let sell_side_characteristic_exponent_tangent = complex_multiply(
        context
            .driver_parameters
            .differentiate_sell_side_characteristic_exponent_for_return_composition_wrt_argument(
                weighted_argument,
            )
            .map_err(
                |source| VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: format!(
                        "terminal affine sell-side argument derivative failed: {source}"
                    ),
                },
            )?,
        weighted_argument_tangent,
    );

    let activity_transform_inputs = build_terminal_affine_activity_transform_inputs(
        context,
        buy_side_characteristic_exponent,
        sell_side_characteristic_exponent,
        buy_side_characteristic_exponent_tangent,
        sell_side_characteristic_exponent_tangent,
    );

    let news_intensity = interval_factors.news_intensity_input().map_err(|source| {
        VanillaOptionPricingRunError::InternalNumericsFailure {
            message: format!("terminal affine interval factors are invalid: {source}"),
        }
    })?;

    let transform_input =
        janiform_affine_oracle::affine_oracle::ExponentialAffineTransformInput::new(
            build_pricing_equation_specification(context.pricing_request),
            activity_transform_inputs.activity_loadings,
            activity_transform_inputs.z_plus,
            activity_transform_inputs.z_minus,
            interval_factors.s_h,
            news_intensity,
            interval_delta,
            context.pricing_request.boundary_input.hawkes_boundary_state,
        );

    Ok(ExponentialAffineScheduleIntervalInput {
        transform_input,
        argument_tangent: activity_transform_inputs.argument_tangent,
        buy_activity_premium_tangent: activity_transform_inputs.buy_activity_premium_tangent,
        sell_activity_premium_tangent: activity_transform_inputs.sell_activity_premium_tangent,
    })
}

#[derive(Debug, Clone, Copy)]
struct TerminalAffineActivityTransformInputs {
    activity_loadings: ExponentialAffineTransformActivityLoadings,
    z_plus: ComplexScalar,
    z_minus: ComplexScalar,
    argument_tangent: ExponentialAffineTransformInputTangent,
    buy_activity_premium_tangent: ExponentialAffineTransformInputTangent,
    sell_activity_premium_tangent: ExponentialAffineTransformInputTangent,
}

fn build_terminal_affine_activity_transform_inputs(
    context: &TerminalAffineMaturityContext<'_>,
    buy_side_characteristic_exponent: ComplexScalar,
    sell_side_characteristic_exponent: ComplexScalar,
    buy_side_characteristic_exponent_tangent: ComplexScalar,
    sell_side_characteristic_exponent_tangent: ComplexScalar,
) -> TerminalAffineActivityTransformInputs {
    let exposure_parameters = context
        .pricing_request
        .model_parameters
        .return_clock_exposure_parameters;

    let buy_baseline_loading = buy_side_characteristic_exponent.scale(-1.0);
    let buy_endogenous_loading = buy_side_characteristic_exponent
        .scale(-exposure_parameters.buy_excitation_return_clock_scale);

    let sell_baseline_loading = sell_side_characteristic_exponent.scale(-1.0);
    let sell_endogenous_loading = sell_side_characteristic_exponent
        .scale(-exposure_parameters.sell_excitation_return_clock_scale);

    let buy_baseline_loading_tangent = buy_side_characteristic_exponent_tangent.scale(-1.0);
    let buy_endogenous_loading_tangent = buy_side_characteristic_exponent_tangent
        .scale(-exposure_parameters.buy_excitation_return_clock_scale);

    let sell_baseline_loading_tangent = sell_side_characteristic_exponent_tangent.scale(-1.0);
    let sell_endogenous_loading_tangent = sell_side_characteristic_exponent_tangent
        .scale(-exposure_parameters.sell_excitation_return_clock_scale);

    let pricing_kernel_parameters = context
        .pricing_request
        .model_parameters
        .pricing_kernel_parameters;

    let buy_kernel_inputs = pricing_kernel_parameters.buy_terminal_density_transform_inputs(
        buy_endogenous_loading,
        ComplexScalar::new(1.0, 0.0),
    );

    let sell_kernel_inputs = pricing_kernel_parameters.sell_terminal_density_transform_inputs(
        sell_endogenous_loading,
        ComplexScalar::new(1.0, 0.0),
    );

    TerminalAffineActivityTransformInputs {
        activity_loadings: ExponentialAffineTransformActivityLoadings::new(
            buy_baseline_loading,
            buy_kernel_inputs.u_input,
            sell_baseline_loading,
            sell_kernel_inputs.u_input,
        ),
        z_plus: buy_kernel_inputs.z_input,
        z_minus: sell_kernel_inputs.z_input,
        argument_tangent: ExponentialAffineTransformInputTangent {
            activity_loadings: ExponentialAffineTransformActivityLoadings::new(
                buy_baseline_loading_tangent,
                buy_endogenous_loading_tangent,
                sell_baseline_loading_tangent,
                sell_endogenous_loading_tangent,
            ),
            z_plus: ComplexScalar::new(0.0, 0.0),
            z_minus: ComplexScalar::new(0.0, 0.0),
        },
        buy_activity_premium_tangent: ExponentialAffineTransformInputTangent {
            activity_loadings: ExponentialAffineTransformActivityLoadings::new(
                ComplexScalar::new(0.0, 0.0),
                buy_kernel_inputs.log_activity_premium_u_derivative,
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
            ),
            z_plus: buy_kernel_inputs.log_activity_premium_z_derivative,
            z_minus: ComplexScalar::new(0.0, 0.0),
        },
        sell_activity_premium_tangent: ExponentialAffineTransformInputTangent {
            activity_loadings: ExponentialAffineTransformActivityLoadings::new(
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                ComplexScalar::new(0.0, 0.0),
                sell_kernel_inputs.log_activity_premium_u_derivative,
            ),
            z_plus: ComplexScalar::new(0.0, 0.0),
            z_minus: sell_kernel_inputs.log_activity_premium_z_derivative,
        },
    }
}
