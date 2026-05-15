use super::super::contracts::ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic;

pub(crate) fn continuous_distribution_metric_score(
    fit_breakdown: &ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
) -> f64 {
    fit_breakdown.profile_fit_score + fit_breakdown.boundary_penalty
}

#[cfg(test)]
mod tests {
    use super::continuous_distribution_metric_score;
    use crate::stages::return_components::contracts::ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic;

    fn test_fit_breakdown() -> ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
        ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic {
            global_fit_score: 2.0,
            conditional_fit_score: 3.0,
            boundary_penalty: 0.5,
            profile_fit_score: 7.0,
            log_likelihood_shift: 1.0,
            optimizer_seed_objective: 0.0,
            optimizer_local_objective: 0.0,
            optimizer_final_objective: 0.0,
            optimizer_local_evaluation_count: 0,
            optimizer_stop_reason: "test".to_string(),
            optimizer_basin_diagnostics: Vec::new(),
            exact_likelihood_failures: Vec::new(),
            exact_closed_form_single_side_buy_block_count: 0,
            exact_closed_form_single_side_sell_block_count: 0,
            exact_tilted_mixed_inversion_block_count: 0,
            exact_structural_failure_count: 0,
            exact_uncertified_scope_count: 0,
            exact_global_average_log_likelihood_error_upper_bound: None,
            exact_conditional_average_log_likelihood_error_upper_bound: None,
            mean_return_fit: 0.0,
            volatility_fit: 0.0,
            median_return_fit: 0.0,
            lower_quartile_fit: 0.0,
            upper_quartile_fit: 0.0,
            lower_tail_q05_fit: 0.0,
            upper_tail_q95_fit: 0.0,
            extreme_left_tail_q01_fit: 0.0,
            extreme_right_tail_q99_fit: 0.0,
            mean_abs_return_fit: 0.0,
            median_abs_return_fit: 0.0,
            abs_quantile_ratio_fit: 0.0,
            bowley_skewness_fit: 0.0,
            l_moment_lambda2_fit: 0.0,
            l_moment_tau3_fit: 0.0,
        }
    }

    #[test]
    fn continuous_distribution_metric_score_uses_nonnegative_distribution_distance() {
        let fit_breakdown = test_fit_breakdown();

        assert_eq!(continuous_distribution_metric_score(&fit_breakdown), 7.5);
    }

    #[test]
    fn continuous_distribution_metric_score_does_not_depend_on_likelihood_availability() {
        let mut fit_breakdown = test_fit_breakdown();
        fit_breakdown.global_fit_score = f64::NAN;

        assert_eq!(continuous_distribution_metric_score(&fit_breakdown), 7.5);
    }
}
