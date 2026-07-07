use std::collections::BTreeSet;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use janiform_model_kernels::return_clock::SideSourceActivityDecomposition;

use super::bounds::{
    build_moment_seed_candidates, coordinate_to_ratio, coordinate_to_share,
    nig_configuration_from_search_point, nig_log_alpha_bounds_for_calibration_sample,
    nig_search_point_from_configuration, ratio_to_coordinate, relative_share_from_beta_ratios,
    sell_beta_ratio_from_relative_share, theoretical_alpha_bounds, theoretical_search_bounds,
};
use super::budget::{
    ContinuousNigFitBudgets, continuous_nig_search_coordinate_count,
    effective_zero_drift_nig_em_parameter_count, family_aligned_candidate_count,
    proxy_em_fitting_sample_target_count, proxy_em_iteration_budget,
};
use super::likelihood::{
    NetTradingTimeReturnLogDensityInput, alpha_beta_boundary_penalty,
    average_negative_log_likelihood_resolution, closed_form_single_side_log_density,
    continuous_block_likelihood_objective, continuous_block_likelihood_objective_with_failures,
    log_density_net_trading_time_return, log_density_net_trading_time_return_failure_reason,
    maximum_profiled_open_beta_ratio, profile_likelihood_score,
};
use super::optimization::{build_nig_moment_seed_configurations, build_nig_search_candidates};
use super::profile::{
    build_distribution_profile, build_scale_bucket_profiles, distribution_profile_fit_score,
};
use super::sample::{
    build_continuous_return_calibration_sample_from_components,
    build_continuous_return_optimizer_context, build_equal_business_time_blocks,
    sample_continuous_return_calibration_sample_by_equal_stride,
};
use super::shape_mapping::exact_configuration_and_gradient_from_shape_search_point;
use super::types::{
    ContinuousLikelihoodFidelityTier, ContinuousLikelihoodObjective, NigShapeSearchPoint,
    ReturnComponentsStageContinuousReturnCalibrationSample,
};
use crate::stages::return_components::contracts::{
    ReturnComponentsObservedOpenSessionReturn, ReturnComponentsStageExactLikelihoodEvaluatorKind,
    ReturnComponentsStageExactLikelihoodFailureReason,
    ReturnComponentsStageExactLikelihoodFailureScope,
};
use janiform_foundations::numeric::support_information_depth;

const TEST_OPTIMIZER_SAMPLE_COUNT: usize = 96;
const TEST_OPTIMIZER_MAX_ITERATIONS: usize = 12;
const TEST_OPTIMIZER_MOMENT_CANDIDATE_COUNT: usize = 24;

#[test]
fn equal_stride_calibration_sample_preserves_member_alignment() {
    let calibration_sample = build_continuous_return_calibration_sample_from_components(
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![10.0, 11.0, 12.0, 13.0, 14.0],
        vec![0.0, 0.1, 0.2, 0.3, 0.4],
        0.5,
    );

    let sampled_sample =
        sample_continuous_return_calibration_sample_by_equal_stride(&calibration_sample, 3);

    assert_eq!(sampled_sample.observed_returns, vec![0.0, 2.0, 4.0]);
    assert_eq!(sampled_sample.business_time_scales, vec![10.0, 12.0, 14.0]);
    assert_eq!(sampled_sample.sell_share_proxies, vec![0.0, 0.2, 0.4]);
}

#[test]
fn continuous_nig_fit_budgets_split_generation_breadth_from_user_iteration_cap() {
    let calibration_sample = test_optimizer_calibration_sample();
    let low_iteration_settings =
        CalibrationOptimizerSettings::try_new("adaptive_search", 1, 1, 1.0e-8, 1.0e-6, 1.0)
            .expect("solver settings should validate");
    let high_iteration_settings =
        CalibrationOptimizerSettings::try_new("adaptive_search", 4096, 1, 1.0e-8, 1.0e-6, 1.0)
            .expect("solver settings should validate");

    let low_iteration_budgets = ContinuousNigFitBudgets::from_calibration_sample(
        &calibration_sample,
        &low_iteration_settings,
    );
    let high_iteration_budgets = ContinuousNigFitBudgets::from_calibration_sample(
        &calibration_sample,
        &high_iteration_settings,
    );

    assert_eq!(low_iteration_budgets, high_iteration_budgets);

    let family_count = super::types::BasinSeedFamily::ALL.len();
    let support_depth = support_information_depth(calibration_sample.observed_profile.sample_count);
    let scale_regime_count = calibration_sample.scale_bucket_profiles.len().max(1);
    let expected_frontier_count =
        family_aligned_candidate_count(family_count * scale_regime_count, family_count);
    let expected_generation_count = family_aligned_candidate_count(
        family_count * continuous_nig_search_coordinate_count() * support_depth,
        family_count,
    );

    assert_eq!(
        low_iteration_budgets.seed_frontier_candidate_count,
        expected_frontier_count
    );
    assert_eq!(
        low_iteration_budgets.moment_candidate_generation_count,
        expected_generation_count
    );
    assert_eq!(
        low_iteration_budgets.basin_seed_candidate_count,
        family_count
    );
}

#[test]
fn nig_log_alpha_bounds_are_independent_of_moment_candidate_count() {
    let calibration_sample = test_optimizer_calibration_sample();
    let observed_profile = calibration_sample.observed_profile;
    let alpha_log_bounds = nig_log_alpha_bounds_for_calibration_sample(&calibration_sample);
    let coarse_search_bounds = theoretical_search_bounds(observed_profile, 2);
    let dense_search_bounds = theoretical_search_bounds(observed_profile, 512);

    assert_eq!(
        alpha_log_bounds,
        (
            coarse_search_bounds.lower_bounds[0],
            coarse_search_bounds.upper_bounds[0]
        )
    );
    assert_eq!(
        alpha_log_bounds,
        (
            dense_search_bounds.lower_bounds[0],
            dense_search_bounds.upper_bounds[0]
        )
    );
}

#[test]
fn proxy_em_iteration_budget_tracks_objective_resolution_and_support_depth() {
    let sample_count = TEST_OPTIMIZER_SAMPLE_COUNT;
    let solver_settings =
        CalibrationOptimizerSettings::try_new("proxy_em", 1, 1, 1.0e-8, 1.0e-6, 0.0)
            .expect("solver settings should validate");

    let objective_resolution = average_negative_log_likelihood_resolution(sample_count);
    let solver_objective_tolerance = solver_settings
        .absolute_tolerance
        .max(solver_settings.relative_tolerance);
    let mut resolved_tolerance = solver_objective_tolerance;
    let mut binary_resolution_depth = 0usize;
    while resolved_tolerance < objective_resolution {
        resolved_tolerance *= 2.0;
        binary_resolution_depth += 1;
    }

    let support_depth = support_information_depth(sample_count);
    let expected_budget = effective_zero_drift_nig_em_parameter_count()
        * binary_resolution_depth.max(support_depth)
        * support_depth;

    assert_eq!(
        proxy_em_iteration_budget(sample_count, &solver_settings),
        expected_budget
    );
    assert!(expected_budget > solver_settings.max_iterations);
}

#[test]
fn proxy_em_fitting_sample_target_tracks_search_geometry() {
    let calibration_sample = test_optimizer_calibration_sample();
    let family_count = super::types::BasinSeedFamily::ALL.len();
    let support_depth = support_information_depth(calibration_sample.observed_profile.sample_count);
    let scale_regime_count = calibration_sample.scale_bucket_profiles.len().max(1);

    let expected_target = effective_zero_drift_nig_em_parameter_count()
        * continuous_nig_search_coordinate_count()
        * family_count
        * scale_regime_count
        * support_depth;

    assert_eq!(
        proxy_em_fitting_sample_target_count(&calibration_sample),
        expected_target.min(calibration_sample.observed_profile.sample_count)
    );
}

#[test]
fn skew_ratio_coordinate_round_trip_stays_inside_open_domain() {
    let ratio = coordinate_to_ratio(ratio_to_coordinate(0.35));

    assert!(ratio.is_finite());
    assert!(ratio > -1.0);
    assert!(ratio < 1.0);
}

#[test]
fn sell_relative_coordinate_round_trip_preserves_ordering() {
    let buy_ratio = 0.4;
    let sell_ratio = sell_beta_ratio_from_relative_share(buy_ratio, coordinate_to_share(0.2));
    let recovered_share = relative_share_from_beta_ratios(buy_ratio, sell_ratio)
        .expect("relative share should be recoverable");

    assert!(sell_ratio < buy_ratio);
    assert!(recovered_share > 0.0);
    assert!(recovered_share < 1.0);
}

#[test]
fn configuration_round_trip_preserves_delta_when_business_time_scale_is_large() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let calibration_sample = synthetic_calibration_sample_from_configuration(configuration);

    let search_point = nig_search_point_from_configuration(&calibration_sample, configuration)
        .expect("search point should derive");
    let recovered_configuration =
        nig_configuration_from_search_point(&calibration_sample, search_point)
            .expect("configuration should recover");

    assert!((recovered_configuration.alpha - configuration.alpha).abs() <= 1.0e-10);
    assert!((recovered_configuration.beta_0 - configuration.beta_0).abs() <= 1.0e-10);
    assert!((recovered_configuration.delta - configuration.delta).abs() <= 1.0e-10);
    assert!((recovered_configuration.delta_beta - configuration.delta_beta).abs() <= 1.0e-10);
}

#[test]
fn exact_search_point_configuration_gradient_matches_finite_difference() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(75.0, -10.0, 3.0e-4, 0.0, -6.5)
        .expect("configuration should validate");
    let calibration_sample = synthetic_calibration_sample_from_configuration(configuration);
    let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);
    let search_point = nig_search_point_from_configuration(&calibration_sample, configuration)
        .expect("search point should derive");
    let mapping =
        exact_configuration_and_gradient_from_shape_search_point(&optimizer_context, search_point)
            .expect("exact mapping should resolve");

    let epsilon = 1.0e-6;
    for coordinate_index in 0..4 {
        let forward_search_point = perturb_search_point(search_point, coordinate_index, epsilon);
        let backward_search_point = perturb_search_point(search_point, coordinate_index, -epsilon);
        let forward_mapping = exact_configuration_and_gradient_from_shape_search_point(
            &optimizer_context,
            forward_search_point,
        )
        .expect("forward mapping should resolve");
        let backward_mapping = exact_configuration_and_gradient_from_shape_search_point(
            &optimizer_context,
            backward_search_point,
        )
        .expect("backward mapping should resolve");

        let alpha_derivative = (forward_mapping.configuration.alpha
            - backward_mapping.configuration.alpha)
            / (2.0 * epsilon);
        let beta_0_derivative = (forward_mapping.configuration.beta_0
            - backward_mapping.configuration.beta_0)
            / (2.0 * epsilon);
        let delta_derivative = (forward_mapping.configuration.delta
            - backward_mapping.configuration.delta)
            / (2.0 * epsilon);
        let delta_beta_derivative = (forward_mapping.configuration.delta_beta
            - backward_mapping.configuration.delta_beta)
            / (2.0 * epsilon);

        assert_close(
            mapping.gradient.alpha_gradient[coordinate_index],
            alpha_derivative,
        );
        assert_close(
            mapping.gradient.beta_0_gradient[coordinate_index],
            beta_0_derivative,
        );
        assert_close(
            mapping.gradient.delta_gradient[coordinate_index],
            delta_derivative,
        );
        assert_close(
            mapping.gradient.delta_beta_gradient[coordinate_index],
            delta_beta_derivative,
        );
    }
}

#[test]
fn distribution_profile_reports_robust_quantile_and_l_moment_metrics() {
    let profile = build_distribution_profile(&[-0.03, -0.01, 0.0, 0.02, 0.08]);

    assert!(profile.abs_quantile_ratio_q99_q95 >= 1.0);
    assert!(profile.l_moment_lambda2.is_finite());
    assert!(profile.bowley_skewness.is_finite());
    assert!(profile.l_moment_tau3.is_finite());
}

#[test]
fn exact_likelihood_failure_reports_block_context_for_invalid_observed_inputs() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let mut calibration_sample = synthetic_calibration_sample_from_configuration(configuration);
    calibration_sample
        .business_time_scales
        .iter_mut()
        .for_each(|business_time_scale| *business_time_scale = f64::NAN);
    let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);

    let (_objective, exact_likelihood_diagnostics) =
        continuous_block_likelihood_objective_with_failures(
            configuration,
            &optimizer_context,
            ContinuousLikelihoodFidelityTier::Full,
        );

    assert!(!exact_likelihood_diagnostics.failures.is_empty());
    assert_eq!(
        exact_likelihood_diagnostics.failures[0].scope,
        ReturnComponentsStageExactLikelihoodFailureScope::Global
    );
    assert_eq!(
        exact_likelihood_diagnostics.failures[0].block_member_index,
        Some(0)
    );
    assert_eq!(
        exact_likelihood_diagnostics.failures[0].failure_reason,
        ReturnComponentsStageExactLikelihoodFailureReason::InvalidObservedInputs
    );
}

#[test]
fn buy_only_exact_density_matches_closed_form_reference() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let exact_log_density =
        log_density_net_trading_time_return(NetTradingTimeReturnLogDensityInput {
            driver_parameters,
            configuration,
            observed_return: 1.1e-4,
            business_time_scale: 1.0,
            sell_share_proxy: 0.0,
            integrated_buy_intensity: 0.8,
            integrated_sell_intensity: 0.0,
            log_density_resolution: average_negative_log_likelihood_resolution(1),
        })
        .expect("buy-only log density should evaluate");
    let closed_form_log_density = closed_form_single_side_log_density(
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideBuy,
        configuration,
        1.1e-4,
        0.8,
    )
    .expect("buy-only closed-form density should evaluate");

    assert_close(exact_log_density, closed_form_log_density);
}

#[test]
fn sell_only_exact_density_matches_closed_form_reference() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let exact_log_density =
        log_density_net_trading_time_return(NetTradingTimeReturnLogDensityInput {
            driver_parameters,
            configuration,
            observed_return: -9.5e-5,
            business_time_scale: 1.0,
            sell_share_proxy: 1.0,
            integrated_buy_intensity: 0.0,
            integrated_sell_intensity: 0.65,
            log_density_resolution: average_negative_log_likelihood_resolution(1),
        })
        .expect("sell-only log density should evaluate");
    let closed_form_log_density = closed_form_single_side_log_density(
        ReturnComponentsStageExactLikelihoodEvaluatorKind::ClosedFormSingleSideSell,
        configuration,
        -9.5e-5,
        0.65,
    )
    .expect("sell-only closed-form density should evaluate");

    assert_close(exact_log_density, closed_form_log_density);
}

#[test]
fn mixed_exact_likelihood_reports_tilted_block_counts() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let calibration_sample = synthetic_calibration_sample_from_configuration(configuration);
    let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);

    let (_objective, exact_likelihood_diagnostics) =
        continuous_block_likelihood_objective_with_failures(
            configuration,
            &optimizer_context,
            ContinuousLikelihoodFidelityTier::Full,
        );
    let expected_tilted_mixed_blocks = calibration_sample.observed_returns.len()
        + optimizer_context
            .scale_bucket_profiles
            .iter()
            .map(|profile| profile.member_indices.len())
            .sum::<usize>();

    assert_eq!(
        exact_likelihood_diagnostics
            .evaluator_block_counts
            .tilted_mixed_inversion_block_count,
        expected_tilted_mixed_blocks,
        "failures={:?}",
        exact_likelihood_diagnostics.failures
    );
    assert_eq!(
        exact_likelihood_diagnostics
            .evaluator_block_counts
            .closed_form_single_side_buy_block_count,
        0
    );
    assert_eq!(
        exact_likelihood_diagnostics
            .evaluator_block_counts
            .closed_form_single_side_sell_block_count,
        0
    );
}

pub(super) fn synthetic_calibration_sample_from_configuration(
    configuration: NigSharedPlusShiftConfiguration,
) -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");
    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(73);
    let business_time_scales: Vec<f64> = (0..128)
        .map(|sample_index| 200.0 + sample_index as f64 * 4.0)
        .collect();
    let sell_share_proxies: Vec<f64> = (0..128)
        .map(|sample_index| 0.30 + 0.40 * ((sample_index % 7) as f64 / 6.0))
        .collect();
    let observed_returns: Vec<f64> = business_time_scales
        .iter()
        .zip(sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
            let buy_weight = 1.0 - sell_weight;
            driver_parameters
                .sample_trading_time_return_increment(
                    business_time_scale * buy_weight,
                    business_time_scale * sell_weight,
                    &mut randomness_stream,
                )
                .expect("synthetic increment should sample")
                .combined_increment
        })
        .collect();

    ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns: observed_returns.clone(),
        business_time_scales: business_time_scales.clone(),
        sell_share_proxies: sell_share_proxies.clone(),
        observed_profile: build_distribution_profile(observed_returns.as_slice()),
        scale_bucket_profiles: build_scale_bucket_profiles(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
        ),
        open_session_sell_share_proxy: 0.5,
    }
}

fn perturb_search_point(
    search_point: NigShapeSearchPoint,
    coordinate_index: usize,
    coordinate_delta: f64,
) -> NigShapeSearchPoint {
    let mut perturbed_search_point = search_point;
    match coordinate_index {
        0 => {
            perturbed_search_point.log_alpha += coordinate_delta;
        }
        1 => {
            perturbed_search_point.buy_skew_coordinate += coordinate_delta;
        }
        2 => {
            perturbed_search_point.sell_relative_coordinate += coordinate_delta;
        }
        3 => {
            perturbed_search_point.log_delta_offset += coordinate_delta;
        }
        _ => panic!("coordinate_index should be in [0, 3]"),
    }

    perturbed_search_point
}

fn assert_close(actual: f64, expected: f64) {
    let tolerance = 1.0e-6f64.max(1.0e-5 * expected.abs().max(actual.abs()));
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual={actual} expected={expected} tolerance={tolerance}"
    );
}

#[test]
fn scale_bucket_profiles_cover_multiple_business_time_regimes() {
    let observed_returns: Vec<f64> = (0..128)
        .map(|sample_index| sample_index as f64 / 10_000.0)
        .collect();
    let business_time_scales: Vec<f64> = (0..128)
        .map(|sample_index| 0.1 + sample_index as f64 / 32.0)
        .collect();

    let scale_bucket_profiles =
        build_scale_bucket_profiles(observed_returns.as_slice(), business_time_scales.as_slice());

    assert!(scale_bucket_profiles.len() >= 3);
    assert!(
        scale_bucket_profiles
            .iter()
            .all(|bucket| !bucket.member_indices.is_empty())
    );
}

#[test]
fn equal_business_time_blocks_preserve_order_and_total_mass() {
    let observed_returns = vec![
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 1,
            elapsed_same_session_hours: 1.0,
            log_return: 0.01,
            post_shock_activity_lift: 1.0,
            sell_share_proxy: 0.0,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.0),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 2,
            elapsed_same_session_hours: 1.0,
            log_return: -0.02,
            post_shock_activity_lift: 2.0,
            sell_share_proxy: 1.0,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(1.0),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 3,
            elapsed_same_session_hours: 1.0,
            log_return: 0.03,
            post_shock_activity_lift: 3.0,
            sell_share_proxy: 0.0,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(0.0),
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 4,
            elapsed_same_session_hours: 1.0,
            log_return: 0.04,
            post_shock_activity_lift: 4.0,
            sell_share_proxy: 1.0,
            source_activity: SideSourceActivityDecomposition::from_sell_share_proxy(1.0),
        },
    ];
    let business_time_scales = [1.0, 1.0, 1.0, 1.0];

    let aggregated_blocks = build_equal_business_time_blocks(
        observed_returns.as_slice(),
        business_time_scales.as_slice(),
        2,
    );

    assert_eq!(aggregated_blocks.len(), 2);
    assert_eq!(aggregated_blocks[0].0.return_timestamp_utc_nanos, 2);
    assert_eq!(aggregated_blocks[1].0.return_timestamp_utc_nanos, 4);
    assert!((aggregated_blocks[0].0.log_return + 0.01).abs() <= 1.0e-12);
    assert!((aggregated_blocks[1].0.log_return - 0.07).abs() <= 1.0e-12);
    assert!((aggregated_blocks[0].1 - 2.0).abs() <= 1.0e-12);
    assert!((aggregated_blocks[1].1 - 2.0).abs() <= 1.0e-12);
    assert!((aggregated_blocks[0].0.sell_share_proxy - 0.5).abs() <= 1.0e-12);
    assert!((aggregated_blocks[1].0.sell_share_proxy - 0.5).abs() <= 1.0e-12);
}

#[test]
fn block_log_density_is_finite_for_representative_configuration() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");
    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let log_density = log_density_net_trading_time_return(NetTradingTimeReturnLogDensityInput {
        driver_parameters,
        configuration,
        observed_return: 1.5e-4,
        business_time_scale: 1.2,
        sell_share_proxy: 0.55,
        integrated_buy_intensity: 0.54,
        integrated_sell_intensity: 0.66,
        log_density_resolution: average_negative_log_likelihood_resolution(1),
    });
    let failure_reason =
        log_density_net_trading_time_return_failure_reason(NetTradingTimeReturnLogDensityInput {
            driver_parameters,
            configuration,
            observed_return: 1.5e-4,
            business_time_scale: 1.2,
            sell_share_proxy: 0.55,
            integrated_buy_intensity: 0.54,
            integrated_sell_intensity: 0.66,
            log_density_resolution: average_negative_log_likelihood_resolution(1),
        });

    assert!(
        log_density.is_ok(),
        "mixed exact block failed with {failure_reason:?}"
    );
    assert!(log_density.expect("log density should exist").is_finite());
}

#[test]
fn profile_likelihood_score_symmetrically_averages_global_and_conditional_terms() {
    let objective = ContinuousLikelihoodObjective {
        shifted_objective_score: 0.0,
        global_negative_log_likelihood: 2.0,
        conditional_negative_log_likelihood: 4.0,
        boundary_penalty: 1.5,
        log_likelihood_shift: 0.0,
    };

    assert!((profile_likelihood_score(objective) - 4.5).abs() <= 1.0e-12);
}

#[test]
fn theoretical_alpha_bounds_remain_finite_for_nearly_gaussian_profiles() {
    let observed_returns: Vec<f64> = (0..512)
        .map(|sample_index| {
            let centered_index = sample_index as f64 - 255.5;
            centered_index / 1_000_000.0
        })
        .collect();
    let observed_profile = build_distribution_profile(observed_returns.as_slice());

    let (lower_alpha_bound, upper_alpha_bound) = theoretical_alpha_bounds(observed_profile);

    assert!(lower_alpha_bound.is_finite());
    assert!(upper_alpha_bound.is_finite());
    assert!(lower_alpha_bound > 0.0);
    assert!(upper_alpha_bound >= lower_alpha_bound);
}

#[test]
fn distribution_profile_fit_score_is_zero_for_identical_samples() {
    let observed_returns = [-0.002, -0.001, -0.0002, 0.0, 0.0003, 0.0015, 0.0028];

    let fit_breakdown =
        distribution_profile_fit_score(observed_returns.as_slice(), observed_returns.as_slice());

    assert!(fit_breakdown.global_fit_score.abs() <= 1.0e-12);
    assert!(fit_breakdown.profile_fit_score.abs() <= 1.0e-12);
}

#[test]
fn distribution_profile_fit_score_penalizes_missing_excess_kurtosis() {
    let mut observed_returns = Vec::new();
    let mut fitted_returns = Vec::new();

    for _sample_position in 0..256 {
        observed_returns.push(-0.0001);
        observed_returns.push(0.0001);
        fitted_returns.push(-0.0001);
        fitted_returns.push(0.0001);
    }

    observed_returns.extend([-0.02, 0.02]);
    fitted_returns.extend([-0.0001, 0.0001]);

    let identical_breakdown =
        distribution_profile_fit_score(observed_returns.as_slice(), observed_returns.as_slice());
    let low_tail_breakdown =
        distribution_profile_fit_score(fitted_returns.as_slice(), observed_returns.as_slice());

    assert_eq!(identical_breakdown.excess_kurtosis_fit, 0.0);
    assert!(low_tail_breakdown.excess_kurtosis_fit > identical_breakdown.excess_kurtosis_fit);
    assert!(low_tail_breakdown.profile_fit_score > identical_breakdown.profile_fit_score);
}

#[test]
fn profiled_beta_ratio_reserves_coarse_probe_stability_margin() {
    let maximum_ratio = maximum_profiled_open_beta_ratio(0.02);

    assert!(maximum_ratio.is_finite());
    assert!(maximum_ratio > 0.0);
    assert!(maximum_ratio < 1.0);
}

#[test]
fn alpha_beta_boundary_penalty_increases_for_probe_unstable_configuration() {
    let stable_configuration = NigSharedPlusShiftConfiguration::try_new(2.0, -0.6, 0.3, 0.0, -0.2)
        .expect("stable configuration should validate");
    let unstable_configuration =
        NigSharedPlusShiftConfiguration::try_new(0.02, -0.01999, 0.3, 0.0, -0.000001)
            .expect("near-boundary configuration should still validate");

    let stable_penalty = alpha_beta_boundary_penalty(stable_configuration);
    let unstable_penalty = alpha_beta_boundary_penalty(unstable_configuration);

    assert_eq!(stable_penalty, 0.0);
    assert!(unstable_penalty.is_finite());
    assert!(unstable_penalty > stable_penalty);
}

#[test]
fn broad_search_is_deterministic_across_parallel_cpu_counts() {
    let calibration_sample = test_optimizer_calibration_sample();
    let single_thread_solver_settings = CalibrationOptimizerSettings::try_new(
        "adaptive_search",
        TEST_OPTIMIZER_MAX_ITERATIONS,
        1,
        1.0e-8,
        1.0e-6,
        1.0,
    )
    .expect("solver settings should validate");
    let multi_thread_solver_settings = CalibrationOptimizerSettings::try_new(
        "adaptive_search",
        TEST_OPTIMIZER_MAX_ITERATIONS,
        4,
        1.0e-8,
        1.0e-6,
        1.0,
    )
    .expect("solver settings should validate");

    let single_thread_candidates =
        build_nig_search_candidates(&calibration_sample, 17, &single_thread_solver_settings);
    let multi_thread_candidates =
        build_nig_search_candidates(&calibration_sample, 17, &multi_thread_solver_settings);

    assert_eq!(
        single_thread_candidates[0].configuration,
        multi_thread_candidates[0].configuration
    );
    assert_eq!(
        single_thread_candidates[0]
            .optimizer_diagnostic
            .final_objective_score
            .to_bits(),
        multi_thread_candidates[0]
            .optimizer_diagnostic
            .final_objective_score
            .to_bits()
    );
}

#[test]
fn optimized_candidates_surface_distinct_basin_diagnostics() {
    let calibration_sample = test_optimizer_calibration_sample();
    let solver_settings = CalibrationOptimizerSettings::try_new(
        "adaptive_search",
        TEST_OPTIMIZER_MAX_ITERATIONS,
        4,
        1.0e-8,
        1.0e-6,
        1.0,
    )
    .expect("solver settings should validate");

    let optimized_candidates =
        build_nig_search_candidates(&calibration_sample, 17, &solver_settings);
    let basin_diagnostics = &optimized_candidates[0]
        .optimizer_diagnostic
        .basin_diagnostics;

    assert!(!basin_diagnostics.is_empty());
    assert!(basin_diagnostics.len() <= super::types::BasinSeedFamily::ALL.len());

    let family_labels: BTreeSet<String> = basin_diagnostics
        .iter()
        .map(|diagnostic| diagnostic.family_label.clone())
        .collect();
    assert!(!family_labels.is_empty());
    let final_coordinate_sets: BTreeSet<(u64, u64, u64, u64)> = basin_diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.final_log_alpha.to_bits(),
                diagnostic.final_buy_skew_coordinate.to_bits(),
                diagnostic.final_sell_relative_coordinate.to_bits(),
                diagnostic.final_log_delta_offset.to_bits(),
            )
        })
        .collect();
    assert_eq!(final_coordinate_sets.len(), basin_diagnostics.len());
    assert!(
        basin_diagnostics
            .iter()
            .all(|diagnostic| diagnostic.final_objective.is_finite())
    );
}

#[test]
fn moment_seed_candidates_surface_multiple_search_regimes() {
    let calibration_sample = test_optimizer_calibration_sample();
    let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);
    let theoretical_bounds = theoretical_search_bounds(
        calibration_sample.observed_profile,
        TEST_OPTIMIZER_MOMENT_CANDIDATE_COUNT,
    );

    let moment_seed_candidates = build_moment_seed_candidates(
        &optimizer_context,
        &calibration_sample,
        theoretical_bounds,
        TEST_OPTIMIZER_MOMENT_CANDIDATE_COUNT,
        super::types::BasinSeedFamily::ALL.len(),
        None,
    );

    let distinct_seed_points: BTreeSet<(u64, u64, u64, u64)> = moment_seed_candidates
        .iter()
        .map(|candidate| {
            (
                candidate.search_point.log_alpha.to_bits(),
                candidate.search_point.buy_skew_coordinate.to_bits(),
                candidate.search_point.sell_relative_coordinate.to_bits(),
                candidate.search_point.log_delta_offset.to_bits(),
            )
        })
        .collect();

    assert_eq!(distinct_seed_points.len(), moment_seed_candidates.len());
    assert!(moment_seed_candidates.len() > 1);
}

#[test]
fn optimizer_candidates_report_local_basin_search_diagnostics() {
    let calibration_sample = test_optimizer_calibration_sample();
    let solver_settings = CalibrationOptimizerSettings::try_new(
        "adaptive_search",
        TEST_OPTIMIZER_MAX_ITERATIONS,
        1,
        1.0e-12,
        1.0e-12,
        1.0,
    )
    .expect("solver settings should validate");

    let candidates = build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

    assert!(candidates[0].optimizer_diagnostic.local_evaluation_count > 0);
    assert!(
        candidates[0]
            .optimizer_diagnostic
            .local_objective_score
            .is_finite()
    );
    assert!(!candidates[0].optimizer_diagnostic.stop_reason.is_empty());
}

#[test]
fn repeated_builds_keep_objective_cache_observable_results_stable() {
    let calibration_sample = test_optimizer_calibration_sample();
    let solver_settings = CalibrationOptimizerSettings::try_new(
        "adaptive_search",
        TEST_OPTIMIZER_MAX_ITERATIONS,
        2,
        1.0e-8,
        1.0e-6,
        1.0,
    )
    .expect("solver settings should validate");

    let first_candidates = build_nig_search_candidates(&calibration_sample, 17, &solver_settings);
    let second_candidates = build_nig_search_candidates(&calibration_sample, 17, &solver_settings);

    assert_eq!(
        first_candidates[0].configuration,
        second_candidates[0].configuration
    );
    assert_eq!(
        first_candidates[0]
            .optimizer_diagnostic
            .local_evaluation_count,
        second_candidates[0]
            .optimizer_diagnostic
            .local_evaluation_count
    );
}

#[test]
fn optimizer_context_uses_reduced_fidelity_supports_when_scale_buckets_exist() {
    let calibration_sample = test_optimizer_calibration_sample();
    let optimizer_context = build_continuous_return_optimizer_context(&calibration_sample);
    let scale_bucket_count = calibration_sample.scale_bucket_profiles.len();

    assert!(scale_bucket_count > 1);
    assert_eq!(
        optimizer_context.seed_member_indices.len(),
        scale_bucket_count
    );
    assert!(optimizer_context.local_member_indices.len() >= scale_bucket_count);
    assert!(
        optimizer_context.local_member_indices.len() <= scale_bucket_count * scale_bucket_count
    );
    assert!(
        optimizer_context.local_member_indices.len() < optimizer_context.full_member_indices.len()
    );
    assert_eq!(
        optimizer_context.full_member_indices.len(),
        calibration_sample.observed_returns.len()
    );
}

#[test]
fn optimizer_context_keeps_small_samples_at_full_fidelity() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -12.0, 2.0e-4, 0.0, -8.0)
        .expect("configuration should validate");

    let calibration_sample = synthetic_calibration_sample_from_configuration(configuration);

    let small_sample = ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns: calibration_sample.observed_returns[..48].to_vec(),
        business_time_scales: calibration_sample.business_time_scales[..48].to_vec(),
        sell_share_proxies: calibration_sample.sell_share_proxies[..48].to_vec(),
        observed_profile: build_distribution_profile(
            calibration_sample.observed_returns[..48].as_ref(),
        ),
        scale_bucket_profiles: build_scale_bucket_profiles(
            calibration_sample.observed_returns[..48].as_ref(),
            calibration_sample.business_time_scales[..48].as_ref(),
        ),
        open_session_sell_share_proxy: calibration_sample.open_session_sell_share_proxy,
    };

    let optimizer_context = build_continuous_return_optimizer_context(&small_sample);

    assert!(small_sample.scale_bucket_profiles.is_empty());
    assert_eq!(
        optimizer_context.seed_member_indices.len(),
        small_sample.observed_returns.len()
    );
    assert_eq!(
        optimizer_context.local_member_indices.len(),
        small_sample.observed_returns.len()
    );
    assert_eq!(
        optimizer_context.full_member_indices.len(),
        small_sample.observed_returns.len()
    );
}

fn test_optimizer_calibration_sample() -> ReturnComponentsStageContinuousReturnCalibrationSample {
    let configuration = NigSharedPlusShiftConfiguration::try_new(120.0, -24.0, 2.5e-4, 0.0, -10.0)
        .expect("configuration should validate");

    let driver_parameters = configuration
        .derive_driver_parameters()
        .expect("driver parameters should derive");

    let mut randomness_stream = SplitMix64SimulationRandomnessStream::from_seed(91);

    let business_time_scales: Vec<f64> = (0..TEST_OPTIMIZER_SAMPLE_COUNT)
        .map(|sample_index| 0.8 + sample_index as f64 / 80.0)
        .collect();

    let sell_share_proxies: Vec<f64> = (0..TEST_OPTIMIZER_SAMPLE_COUNT)
        .map(|sample_index| 0.25 + 0.50 * ((sample_index % 13) as f64 / 12.0))
        .collect();

    let observed_returns: Vec<f64> = business_time_scales
        .iter()
        .zip(sell_share_proxies.iter())
        .map(|(business_time_scale, sell_share_proxy)| {
            let sell_weight = sell_share_proxy.clamp(0.0, 1.0);
            let buy_weight = 1.0 - sell_weight;
            driver_parameters
                .sample_trading_time_return_increment(
                    business_time_scale * buy_weight,
                    business_time_scale * sell_weight,
                    &mut randomness_stream,
                )
                .expect("synthetic increment should sample")
                .combined_increment
        })
        .collect();

    ReturnComponentsStageContinuousReturnCalibrationSample {
        observed_returns: observed_returns.clone(),
        business_time_scales: business_time_scales.clone(),
        sell_share_proxies: sell_share_proxies.clone(),
        observed_profile: build_distribution_profile(observed_returns.as_slice()),
        scale_bucket_profiles: build_scale_bucket_profiles(
            observed_returns.as_slice(),
            business_time_scales.as_slice(),
        ),
        open_session_sell_share_proxy: janiform_foundations::numeric::arithmetic_mean_or_zero(
            sell_share_proxies.as_slice(),
        ),
    }
}

#[test]
fn moment_seed_configurations_are_available_without_local_optimization() {
    let calibration_sample = test_optimizer_calibration_sample();

    let solver_settings =
        CalibrationOptimizerSettings::try_new("return-components-test", 64, 1, 1.0e-6, 1.0e-6, 1.0)
            .expect("test optimizer settings should be valid");

    let configurations =
        build_nig_moment_seed_configurations(&calibration_sample, &solver_settings);

    assert!(!configurations.is_empty());
}

#[test]
fn weighted_representative_likelihood_matches_repeated_observations() {
    let configuration = NigSharedPlusShiftConfiguration::try_new(80.0, -8.0, 0.002, 0.0, -3.0)
        .expect("configuration should validate");

    let compressed_sample = build_continuous_return_calibration_sample_from_components(
        vec![-0.003, 0.004, 0.011],
        vec![1.0, 2.0, 4.0],
        vec![0.6, 0.4, 0.5],
        0.5,
    );
    let mut compressed_context = build_continuous_return_optimizer_context(&compressed_sample);
    compressed_context.member_weights = vec![3.0, 2.0, 5.0];

    let repeated_sample = build_continuous_return_calibration_sample_from_components(
        vec![
            -0.003, -0.003, -0.003, 0.004, 0.004, 0.011, 0.011, 0.011, 0.011, 0.011,
        ],
        vec![1.0, 1.0, 1.0, 2.0, 2.0, 4.0, 4.0, 4.0, 4.0, 4.0],
        vec![0.6, 0.6, 0.6, 0.4, 0.4, 0.5, 0.5, 0.5, 0.5, 0.5],
        0.5,
    );
    let repeated_context = build_continuous_return_optimizer_context(&repeated_sample);

    let compressed_objective = continuous_block_likelihood_objective(
        configuration,
        &compressed_context,
        ContinuousLikelihoodFidelityTier::Representative,
    );
    let repeated_objective = continuous_block_likelihood_objective(
        configuration,
        &repeated_context,
        ContinuousLikelihoodFidelityTier::Representative,
    );

    assert!(
        (compressed_objective.global_negative_log_likelihood
            - repeated_objective.global_negative_log_likelihood)
            .abs()
            <= 1.0e-12
    );
}
