use janiform_calibration_artifacts::{
    ReturnComponentsStageFittedParameters, SurpriseNewsDeterministicIntensityProfile,
};
use janiform_foundations::numeric::{
    round_positive_to_domain, sample_adjusted_excess_kurtosis, sample_adjusted_skewness,
    sample_standard_deviation,
};
use janiform_model_kernels::mean_reversion::MeanReversionFittedParameters;
use janiform_model_kernels::news::NewsProcessConfiguration;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::super::constants::{
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY,
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY,
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
};
use super::super::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    ReturnComponentsStageActivityCouplingDiagnostic, ReturnComponentsStageCandidateInput,
    ReturnComponentsStageConstraintViolationDiagnostic,
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageDiagnosticPolicy, ReturnComponentsStageGapDiagnostic,
    ReturnComponentsStageJumpDiagnostic, ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityDiagnostic, ReturnComponentsStagePriceRangeSanityPolicy,
    ReturnComponentsStageReturnDistributionDiagnostic,
    ReturnComponentsStageSurpriseArrivalDiagnostic,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use super::super::derived_contracts::{
    return_components_stage_positive_domain_floor,
    return_components_stage_surprise_intensity_weekly_profile_bucket_count,
};
use super::super::diagnostic_policy_fit;
use super::super::surprise_intensity_fit;
use janiform_model_kernels::news::SharedNewsMarkLawConfiguration;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReturnComponentsStageReturnDistributionSummary {
    pub(crate) mean_return: f64,
    pub(crate) volatility: f64,
    pub(crate) skewness: f64,
    pub(crate) excess_kurtosis: f64,
}

pub(crate) fn build_jump_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageJumpDiagnostic {
    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageJumpDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_jump_count: summary_statistics.selected_surprise_jump_episode_count,
        observed_mean_abs_jump_return: if summary_statistics.selected_surprise_jump_episode_count
            == 0
        {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_open_jump_return
        },
        fitted_mean_abs_jump_return,
        fitted_sigma_g: return_jump_family.sigma_g,
    }
}

pub(crate) fn build_surprise_arrival_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    fitted_surprise_deterministic_intensity_profile: &SurpriseNewsDeterministicIntensityProfile,
) -> ReturnComponentsStageSurpriseArrivalDiagnostic {
    let candidate_surprise_profile = SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers: surprise_arrival_family.weekly_hour_utc_multipliers.clone(),
        profile_resolution_hours: surprise_arrival_family.profile_resolution_hours,
        fit_policy: fitted_surprise_deterministic_intensity_profile
            .fit_policy
            .clone(),
        normalization_policy: fitted_surprise_deterministic_intensity_profile
            .normalization_policy
            .clone(),
    };

    ReturnComponentsStageSurpriseArrivalDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_jump_count: summary_statistics.selected_surprise_jump_episode_count,
        observed_jump_frequency_per_hour: if summary_statistics.selected_surprise_jump_episode_count
            == 0
        {
            0.0
        } else {
            summary_statistics.selected_surprise_jump_episode_count as f64
                / summary_statistics
                    .source_window_duration_hours
                    .max(return_components_stage_positive_domain_floor())
        },
        fitted_surprise_arrival_intensity_per_hour: surprise_arrival_family.baseline_intensity,
        surprise_arrival_profile_fit_error: if summary_statistics
            .selected_surprise_jump_episode_count
            == 0
        {
            0.0
        } else {
            surprise_intensity_fit::profile_mean_absolute_error(
                &candidate_surprise_profile,
                fitted_surprise_deterministic_intensity_profile,
            )
        },
    }
}

pub(crate) fn build_gap_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> ReturnComponentsStageGapDiagnostic {
    let fitted_mean_abs_gap_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    ReturnComponentsStageGapDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.close_open_gap_episode_count,
        )
        .to_string(),
        observed_gap_count: summary_statistics.close_open_gap_episode_count,
        observed_gap_frequency_per_hour: if summary_statistics.close_open_gap_episode_count == 0 {
            0.0
        } else {
            summary_statistics.close_open_gap_episode_count as f64
                / summary_statistics
                    .source_window_duration_hours
                    .max(return_components_stage_positive_domain_floor())
        },
        observed_mean_abs_gap_return: if summary_statistics.close_open_gap_episode_count == 0 {
            0.0
        } else {
            summary_statistics.mean_abs_surprise_closed_gap_return
        },
        fitted_mean_abs_gap_return,
        fitted_sigma_g: return_jump_family.sigma_g,
    }
}

pub(crate) fn build_activity_coupling_diagnostic(
    diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    activity_coupling_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsStageActivityCouplingDiagnostic {
    ReturnComponentsStageActivityCouplingDiagnostic {
        evidence_state: diagnostic_policy_fit::evidence_state_from_count(
            diagnostic_policy,
            summary_statistics.selected_surprise_jump_episode_count,
        )
        .to_string(),
        observed_known_post_shock_activity_lift: summary_statistics.known_activity_coupling_target,
        observed_surprise_post_shock_activity_lift: if summary_statistics
            .selected_surprise_jump_episode_count
            == 0
        {
            0.0
        } else {
            summary_statistics.surprise_activity_coupling_target
        },
        fitted_known_activity_coupling_scale: activity_coupling_family
            .known_activity_coupling_scale,
        fitted_surprise_activity_coupling_scale: activity_coupling_family
            .surprise_activity_coupling_scale,
    }
}

pub(crate) fn build_price_range_sanity_diagnostic(
    _diagnostic_policy: &ReturnComponentsStageDiagnosticPolicy,
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    return_distribution_diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    surprise_arrival_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    price_range_sanity_policy: &ReturnComponentsStagePriceRangeSanityPolicy,
) -> ReturnComponentsStagePriceRangeSanityDiagnostic {
    let effective_continuous_sample_count = summary_statistics.continuous_fit_return_count.max(1);
    let effective_mean_business_time_scale = summary_statistics
        .mean_continuous_business_time_scale
        .max(return_components_stage_positive_domain_floor());

    let continuous_reference_span = if summary_statistics.observed_continuous_log_price_span
        <= return_components_stage_positive_domain_floor()
    {
        0.0
    } else if summary_statistics.continuous_return_volatility
        <= return_components_stage_positive_domain_floor()
    {
        price_range_sanity_policy.observed_continuous_log_price_span
    } else {
        price_range_sanity_policy.observed_continuous_log_price_span
            * return_distribution_diagnostic
                .fitted_return_volatility
                .max(0.0)
            / summary_statistics.continuous_return_volatility
    };

    let fitted_mean_abs_jump_return =
        fitted_mean_abs_jump_return(shared_mark_law_template, return_jump_family);

    let fitted_reference_log_price_span = continuous_reference_span
        + (summary_statistics.known_realized_sample_count as f64
            + surprise_arrival_family.baseline_intensity
                * summary_statistics.source_window_duration_hours)
            * fitted_mean_abs_jump_return
        + if summary_statistics.close_open_gap_episode_count > 0 {
            summary_statistics.close_open_gap_candidate_count as f64 * fitted_mean_abs_jump_return
        } else {
            0.0
        };

    let observed_component_reference_log_price_span = price_range_sanity_policy
        .observed_component_reference_log_price_span
        .max(summary_statistics.observed_log_price_span);

    let admissible_reference_log_price_span = observed_component_reference_log_price_span
        .min(price_range_sanity_policy.maximum_representable_log_price_span)
        .max(return_components_stage_positive_domain_floor());

    let expansion_ratio = fitted_reference_log_price_span / admissible_reference_log_price_span;

    let is_flagged = fitted_reference_log_price_span > admissible_reference_log_price_span;

    ReturnComponentsStagePriceRangeSanityDiagnostic {
        policy: price_range_sanity_policy.clone(),
        is_flagged,
        observed_log_price_span: summary_statistics.observed_log_price_span,
        observed_component_reference_log_price_span,
        admissible_reference_log_price_span,
        fitted_reference_log_price_span,
        continuous_reference_log_price_span: continuous_reference_span,
        effective_continuous_sample_count,
        effective_mean_business_time_scale,
        fitted_mean_abs_jump_return,
        expansion_ratio,
    }
}

pub(crate) fn theoretical_net_process_moments(
    configuration: NigSharedPlusShiftConfiguration,
    open_session_sell_share_proxy: f64,
) -> Option<(f64, f64, f64, f64)> {
    let buy_unit_cumulants = nig_unit_cumulants(
        configuration.alpha,
        configuration.beta_0,
        configuration.delta,
        configuration.mu,
    )?;
    let sell_unit_cumulants = nig_unit_cumulants(
        configuration.alpha,
        configuration.derived_sell_beta(),
        configuration.delta,
        configuration.mu,
    )?;

    let sell_weight = open_session_sell_share_proxy.clamp(0.0, 1.0);
    let buy_weight = 1.0 - sell_weight;

    let mean_return = 0.0;
    let variance = buy_weight * buy_unit_cumulants.1 + sell_weight * sell_unit_cumulants.1;
    if !variance.is_finite() || variance <= return_components_stage_positive_domain_floor() {
        return None;
    }

    let third_cumulant = buy_weight * buy_unit_cumulants.2 - sell_weight * sell_unit_cumulants.2;
    let fourth_cumulant = buy_weight * buy_unit_cumulants.3 + sell_weight * sell_unit_cumulants.3;
    let skewness = third_cumulant / variance.powf(1.5);
    let excess_kurtosis = fourth_cumulant / (variance * variance);

    Some((mean_return, variance, skewness, excess_kurtosis))
}

pub(crate) fn arithmetic_mean_signed(values: &[f64]) -> f64 {
    let finite_values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect();
    if finite_values.is_empty() {
        return 0.0;
    }

    finite_values.iter().sum::<f64>() / finite_values.len() as f64
}

pub(crate) fn round_positive_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = round_positive_to_domain(value.max(0.0), 12);
    if value > 0.0 && rounded <= 0.0 {
        return_components_stage_positive_domain_floor()
    } else {
        rounded
    }
}

pub(crate) fn round_signed_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    (value * 1_000_000_000_000.0).round() / 1_000_000_000_000.0
}

pub(crate) fn round_negative_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return -return_components_stage_positive_domain_floor();
    }

    ((value.min(-return_components_stage_positive_domain_floor())) * 1_000_000_000_000.0).round()
        / 1_000_000_000_000.0
}

pub(super) fn round_candidate_value(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }

    let rounded = (value * 1_000_000_000_000.0).round() / 1_000_000_000_000.0;
    if rounded < 0.0 { 0.0 } else { rounded }
}

pub(crate) fn summarize_return_distribution(
    values: &[f64],
    business_time_scales: &[f64],
) -> ReturnComponentsStageReturnDistributionSummary {
    let normalized_mean_values: Vec<f64> = values
        .iter()
        .zip(business_time_scales.iter())
        .filter_map(|(value, business_time_scale)| {
            if !value.is_finite() || !business_time_scale.is_finite() || *business_time_scale <= 0.0
            {
                return None;
            }

            Some(*value / business_time_scale.max(return_components_stage_positive_domain_floor()))
        })
        .collect();

    let normalized_variance_values: Vec<f64> = values
        .iter()
        .zip(business_time_scales.iter())
        .filter_map(|(value, business_time_scale)| {
            if !value.is_finite() || !business_time_scale.is_finite() || *business_time_scale <= 0.0
            {
                return None;
            }

            Some(
                *value
                    / business_time_scale
                        .max(return_components_stage_positive_domain_floor())
                        .sqrt(),
            )
        })
        .collect();

    ReturnComponentsStageReturnDistributionSummary {
        mean_return: arithmetic_mean_signed(normalized_mean_values.as_slice()),
        volatility: sample_standard_deviation(normalized_variance_values.as_slice()),
        skewness: sample_adjusted_skewness(normalized_variance_values.as_slice()).unwrap_or(0.0),
        excess_kurtosis: sample_adjusted_excess_kurtosis(normalized_variance_values.as_slice())
            .unwrap_or(0.0),
    }
}

pub(super) fn collect_return_components_stage_constraint_violations(
    candidate_input: &ReturnComponentsStageCandidateInput,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = collect_return_components_exact_candidate_constraint_violations(
        &ReturnComponentsStageFittedParameters {
            nig_shared_plus_shift_configuration: candidate_input
                .nig_shared_plus_shift_configuration,
            news_process_configuration: candidate_input.news_process_configuration.clone(),
            surprise_deterministic_intensity_profile:
                default_surprise_deterministic_intensity_profile(),
            mean_reversion_fitted_parameters:
                MeanReversionFittedParameters::zero_kappa_flat_anchor_mean_reversion(),
        },
    );

    violations.extend(
        collect_return_components_exact_likelihood_constraint_violations(
            &candidate_input.continuous_distribution_fit_breakdown,
        ),
    );
    violations
}

pub(super) fn collect_return_components_exact_candidate_constraint_violations(
    fitted_parameters: &ReturnComponentsStageFittedParameters,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    let mut violations = Vec::new();

    if !fitted_parameters
        .nig_shared_plus_shift_configuration
        .delta_beta
        .is_finite()
        || fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta
            >= 0.0
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration.delta_beta".to_string(),
            message:
                "delta_beta must be finite and strictly negative to preserve sell-side skew asymmetry"
                    .to_string(),
        });
    }

    if let Err(source) = NigSharedPlusShiftConfiguration::try_new(
        fitted_parameters.nig_shared_plus_shift_configuration.alpha,
        fitted_parameters.nig_shared_plus_shift_configuration.beta_0,
        fitted_parameters.nig_shared_plus_shift_configuration.delta,
        fitted_parameters.nig_shared_plus_shift_configuration.mu,
        fitted_parameters
            .nig_shared_plus_shift_configuration
            .delta_beta,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "nig_shared_plus_shift_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if let Err(source) = NewsProcessConfiguration::try_new(
        fitted_parameters
            .news_process_configuration
            .arrivals
            .clone(),
        fitted_parameters.news_process_configuration.shared_mark_law,
        fitted_parameters
            .news_process_configuration
            .known_component_mark_scales,
        fitted_parameters
            .news_process_configuration
            .surprise_component_mark_scales,
    ) {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "news_process_configuration".to_string(),
            message: source.to_string(),
        });
    }

    if fitted_parameters
        .surprise_deterministic_intensity_profile
        .profile_resolution_hours
        != RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "surprise_deterministic_intensity_profile.profile_resolution_hours".to_string(),
            message: format!(
                "profile_resolution_hours must equal {}",
                RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
            ),
        });
    }

    let expected_bucket_count =
        return_components_stage_surprise_intensity_weekly_profile_bucket_count();
    if fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers
        .len()
        != expected_bucket_count
    {
        violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
            field: "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers"
                .to_string(),
            message: format!(
                "weekly_hour_utc_multipliers must contain exactly {} values",
                expected_bucket_count
            ),
        });
    }

    for (bucket_index, multiplier) in fitted_parameters
        .surprise_deterministic_intensity_profile
        .weekly_hour_utc_multipliers
        .iter()
        .copied()
        .enumerate()
    {
        if !multiplier.is_finite() || multiplier <= 0.0 {
            violations.push(ReturnComponentsStageConstraintViolationDiagnostic {
                field: format!(
                    "surprise_deterministic_intensity_profile.weekly_hour_utc_multipliers[{bucket_index}]"
                ),
                message: format!(
                    "weekly surprise intensity multiplier must be finite and strictly positive, found {multiplier}"
                ),
            });
        }
    }

    violations
}

pub(crate) fn collect_return_components_exact_likelihood_constraint_violations(
    fit_breakdown: &ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
) -> Vec<ReturnComponentsStageConstraintViolationDiagnostic> {
    if fit_breakdown.exact_structural_failure_count == 0 {
        return Vec::new();
    }

    let first_failure = fit_breakdown.exact_likelihood_failures.first();
    let first_failure_reason = first_failure
        .map(|failure| format!("{:?}", failure.failure_reason))
        .unwrap_or_else(|| "unknown".to_string());
    let first_scope = first_failure
        .map(|failure| format!("{:?}", failure.scope))
        .unwrap_or_else(|| "unknown".to_string());

    vec![ReturnComponentsStageConstraintViolationDiagnostic {
        field: "continuous_distribution_fit_breakdown.exact_likelihood_failures".to_string(),
        message: format!(
            "exact likelihood produced {} structural failure(s); first failure scope={} reason={}",
            fit_breakdown.exact_structural_failure_count, first_scope, first_failure_reason,
        ),
    }]
}

pub(crate) fn default_surprise_deterministic_intensity_profile()
-> SurpriseNewsDeterministicIntensityProfile {
    SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers: vec![
            1.0;
            return_components_stage_surprise_intensity_weekly_profile_bucket_count()
        ],
        profile_resolution_hours:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
        fit_policy: RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY.to_string(),
        normalization_policy:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY.to_string(),
    }
}

fn shared_mark_law_mean_abs_jump_multiplier(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
) -> f64 {
    let severity_scale_proxy = (shared_mark_law_template.gamma_severity_marks.shape_k
        * shared_mark_law_template.gamma_severity_marks.scale_theta)
        .sqrt();

    let base_shock_mean_absolute_value = shared_mark_law_template
        .double_exponential_base_shock
        .probability_positive
        / shared_mark_law_template
            .double_exponential_base_shock
            .eta_plus
        + (1.0
            - shared_mark_law_template
                .double_exponential_base_shock
                .probability_positive)
            / shared_mark_law_template
                .double_exponential_base_shock
                .eta_minus;

    severity_scale_proxy * base_shock_mean_absolute_value
}

fn fitted_mean_abs_jump_return(
    shared_mark_law_template: SharedNewsMarkLawConfiguration,
    return_jump_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> f64 {
    return_jump_family.sigma_g * shared_mark_law_mean_abs_jump_multiplier(shared_mark_law_template)
}

fn nig_unit_cumulants(alpha: f64, beta: f64, delta: f64, mu: f64) -> Option<(f64, f64, f64, f64)> {
    let gamma_squared = alpha * alpha - beta * beta;
    if !gamma_squared.is_finite() || gamma_squared <= 0.0 {
        return None;
    }

    let gamma = gamma_squared.sqrt();

    let kappa_1 = mu + delta * beta / gamma;
    let kappa_2 = delta * alpha * alpha / gamma.powi(3);
    let kappa_3 = 3.0 * delta * beta * alpha * alpha / gamma.powi(5);
    let kappa_4 = 3.0 * delta * alpha * alpha * (alpha * alpha + 4.0 * beta * beta) / gamma.powi(7);

    Some((kappa_1, kappa_2, kappa_3, kappa_4))
}
