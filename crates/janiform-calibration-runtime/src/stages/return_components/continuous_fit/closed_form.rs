use std::f64::consts::PI;

use janiform_model_kernels::nig::NigParameters;

use super::super::contracts::{
    ReturnComponentsStageExactLikelihoodEvaluatorKind,
    ReturnComponentsStageExactLikelihoodFailureReason,
};
use super::bessel::log_modified_bessel_k1;
use super::exact_block::{ExactBlockInput, ExactLikelihoodFailureContext};
use super::likelihood::{ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure};

pub(super) fn evaluate_closed_form_exact_block(
    parameters: NigParameters,
    block_input: ExactBlockInput,
    log_density_resolution: f64,
    evaluator_kind: ReturnComponentsStageExactLikelihoodEvaluatorKind,
) -> Result<ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure> {
    let failure_context = ExactLikelihoodFailureContext {
        evaluator_kind: Some(evaluator_kind),
        observed_return: block_input.observed_return,
        business_time_scale: block_input.business_time_scale,
        sell_share_proxy: block_input.sell_share_proxy,
        integrated_buy_intensity: block_input.integrated_buy_intensity,
        integrated_sell_intensity: block_input.integrated_sell_intensity,
        block_standard_deviation: Some(block_input.block_standard_deviation),
        standardized_return: Some(block_input.standardized_return),
        tilt_theta: None,
        tilt_domain_lower_theta_exclusive: None,
        tilt_domain_upper_theta_exclusive: None,
        standardized_decay_scale: None,
        integration_upper_bound: None,
        log_density_resolution,
    };

    let time_scale = match evaluator_kind {
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideBuy => {
            block_input.integrated_buy_intensity
        }
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideSell => {
            block_input.integrated_sell_intensity
        }
        ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion => unreachable!(),
    };

    let log_density =
        closed_form_nig_log_density(parameters, block_input.observed_return, time_scale).map_err(
            |failure_reason| {
                ExactBlockStructuralFailure::new(
                    block_input.block_member_index,
                    failure_reason,
                    failure_context,
                )
            },
        )?;

    Ok(ExactBlockLikelihoodEvaluation {
        evaluator_kind,
        log_density_estimate: log_density,
        log_density_error_upper_bound: 0.0,
        tilt_theta: None,
        tilt_domain_lower_theta_exclusive: None,
        tilt_domain_upper_theta_exclusive: None,
        block_standard_deviation: block_input.block_standard_deviation,
        standardized_return: block_input.standardized_return,
        standardized_decay_scale: None,
        integration_upper_bound: None,
    })
}

pub(super) fn closed_form_nig_log_density(
    parameters: NigParameters,
    observed_return: f64,
    time_scale: f64,
) -> Result<f64, ReturnComponentsStageExactLikelihoodFailureReason> {
    if !observed_return.is_finite() || !time_scale.is_finite() || time_scale <= 0.0 {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs);
    }

    let gamma_squared =
        (parameters.alpha - parameters.beta.abs()) * (parameters.alpha + parameters.beta.abs());
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs);
    }

    let gamma = gamma_squared.sqrt();

    let scaled_delta = parameters.delta * time_scale;
    if !scaled_delta.is_finite() || scaled_delta <= 0.0 {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs);
    }

    let conditional_scale =
        (scaled_delta * scaled_delta + observed_return * observed_return).sqrt();
    if !conditional_scale.is_finite() || conditional_scale <= 0.0 {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidDensity);
    }

    let bessel_argument = parameters.alpha * conditional_scale;
    if !bessel_argument.is_finite() || bessel_argument <= 0.0 {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidDensity);
    }

    let log_bessel_k1 = log_modified_bessel_k1(bessel_argument)
        .ok_or(ReturnComponentsStageExactLikelihoodFailureReason::InvalidDensity)?;

    let log_density = parameters.alpha.ln() + scaled_delta.ln() - PI.ln() - conditional_scale.ln()
        + scaled_delta * gamma
        + parameters.beta * observed_return
        + log_bessel_k1;
    if !log_density.is_finite() {
        return Err(ReturnComponentsStageExactLikelihoodFailureReason::InvalidDensity);
    }

    Ok(log_density)
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::nig::NigParameters;

    use super::closed_form_nig_log_density;

    #[test]
    fn density_remains_finite_when_bessel_k1_underflows() {
        let buy_parameters = NigParameters::try_new(
            113_937.28642135783,
            13_916.779004903387,
            0.000029959179,
            0.0,
        )
        .expect("buy parameters should validate");

        let log_density =
            closed_form_nig_log_density(buy_parameters, 0.000009286603146238112, 402.4404796359019)
                .expect("large-argument closed-form density should evaluate");

        assert!(log_density.is_finite());
    }
}
