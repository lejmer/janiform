use janiform_model_kernels::nig::{NigBuySellDriverParameters, NigSharedPlusShiftConfiguration};

use super::super::contracts::{
    ReturnComponentsStageExactLikelihoodEvaluatorKind,
    ReturnComponentsStageExactLikelihoodFailureReason,
};
use super::super::derived_contracts::return_components_stage_positive_domain_floor;
use super::super::execution::theoretical_net_process_moments;
use super::closed_form::evaluate_closed_form_exact_block;
use super::likelihood::{ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure};
use super::tilted_mixed::evaluate_tilted_mixed_exact_block;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExactBlockClassification {
    BuyOnly,
    SellOnly,
    Mixed,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExactBlockInput {
    pub(super) block_member_index: usize,
    pub(super) observed_return: f64,
    pub(super) business_time_scale: f64,
    pub(super) sell_share_proxy: f64,
    pub(super) integrated_buy_intensity: f64,
    pub(super) integrated_sell_intensity: f64,
    pub(super) block_standard_deviation: f64,
    pub(super) standardized_return: f64,
    pub(super) classification: ExactBlockClassification,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ExactLikelihoodFailureContext {
    pub(super) evaluator_kind: Option<ReturnComponentsStageExactLikelihoodEvaluatorKind>,
    pub(super) observed_return: f64,
    pub(super) business_time_scale: f64,
    pub(super) sell_share_proxy: f64,
    pub(super) integrated_buy_intensity: f64,
    pub(super) integrated_sell_intensity: f64,
    pub(super) block_standard_deviation: Option<f64>,
    pub(super) standardized_return: Option<f64>,
    pub(super) tilt_theta: Option<f64>,
    pub(super) tilt_domain_lower_theta_exclusive: Option<f64>,
    pub(super) tilt_domain_upper_theta_exclusive: Option<f64>,
    pub(super) standardized_decay_scale: Option<f64>,
    pub(super) integration_upper_bound: Option<f64>,
    pub(super) log_density_resolution: f64,
}

pub(super) fn evaluate_exact_block_likelihood(
    configuration: NigSharedPlusShiftConfiguration,
    driver_parameters: NigBuySellDriverParameters,
    block_input: ExactBlockInput,
    log_density_resolution: f64,
) -> Result<ExactBlockLikelihoodEvaluation, ExactBlockStructuralFailure> {
    match block_input.classification {
        ExactBlockClassification::BuyOnly => evaluate_closed_form_exact_block(
            driver_parameters.buy_side_parameters,
            block_input,
            log_density_resolution,
            ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideBuy,
        ),
        ExactBlockClassification::SellOnly => evaluate_closed_form_exact_block(
            driver_parameters.sell_side_parameters,
            ExactBlockInput {
                observed_return: -block_input.observed_return,
                ..block_input
            },
            log_density_resolution,
            ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideSell,
        ),
        ExactBlockClassification::Mixed => evaluate_tilted_mixed_exact_block(
            configuration,
            driver_parameters,
            block_input,
            log_density_resolution,
        ),
    }
}

pub(super) fn build_exact_block_input(
    configuration: NigSharedPlusShiftConfiguration,
    observed_return: f64,
    business_time_scale: f64,
    sell_share_proxy: f64,
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
    block_member_index: usize,
    log_density_resolution: f64,
) -> Result<ExactBlockInput, ExactBlockStructuralFailure> {
    let mut failure_context = ExactLikelihoodFailureContext {
        evaluator_kind: None,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        block_standard_deviation: None,
        standardized_return: None,
        tilt_theta: None,
        tilt_domain_lower_theta_exclusive: None,
        tilt_domain_upper_theta_exclusive: None,
        standardized_decay_scale: None,
        integration_upper_bound: None,
        log_density_resolution,
    };

    if !observed_return.is_finite()
        || !business_time_scale.is_finite()
        || business_time_scale <= 0.0
        || !sell_share_proxy.is_finite()
        || !(0.0..=1.0).contains(&sell_share_proxy)
        || !integrated_buy_intensity.is_finite()
        || !integrated_sell_intensity.is_finite()
        || integrated_buy_intensity < 0.0
        || integrated_sell_intensity < 0.0
        || !log_density_resolution.is_finite()
        || log_density_resolution <= 0.0
    {
        return Err(ExactBlockStructuralFailure::new(
            block_member_index,
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
            failure_context,
        ));
    }

    let classification = classify_exact_block(integrated_buy_intensity, integrated_sell_intensity)
        .ok_or_else(|| {
            ExactBlockStructuralFailure::new(
                block_member_index,
                ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
                failure_context,
            )
        })?;

    let block_standard_deviation =
        block_return_standard_deviation(configuration, business_time_scale, sell_share_proxy)
            .ok_or_else(|| {
                ExactBlockStructuralFailure::new(
            block_member_index,
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidBlockStandardDeviation,
            failure_context,
        )
            })?;
    failure_context.block_standard_deviation = Some(block_standard_deviation);

    let standardized_return = observed_return / block_standard_deviation;
    if !standardized_return.is_finite() {
        return Err(ExactBlockStructuralFailure::new(
            block_member_index,
            ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs,
            failure_context,
        ));
    }

    Ok(ExactBlockInput {
        block_member_index,
        observed_return,
        business_time_scale,
        sell_share_proxy,
        integrated_buy_intensity,
        integrated_sell_intensity,
        block_standard_deviation,
        standardized_return,
        classification,
    })
}

fn classify_exact_block(
    integrated_buy_intensity: f64,
    integrated_sell_intensity: f64,
) -> Option<ExactBlockClassification> {
    match (
        integrated_buy_intensity > 0.0,
        integrated_sell_intensity > 0.0,
    ) {
        (true, false) => Some(ExactBlockClassification::BuyOnly),
        (false, true) => Some(ExactBlockClassification::SellOnly),
        (true, true) => Some(ExactBlockClassification::Mixed),
        _ => None,
    }
}

fn block_return_standard_deviation(
    configuration: NigSharedPlusShiftConfiguration,
    business_time_scale: f64,
    sell_share_proxy: f64,
) -> Option<f64> {
    let (_, variance_per_unit, _, _) =
        theoretical_net_process_moments(configuration, sell_share_proxy)?;

    let variance = (variance_per_unit * business_time_scale)
        .max(return_components_stage_positive_domain_floor());
    let standard_deviation = variance.sqrt();

    standard_deviation.is_finite().then_some(standard_deviation)
}
