use std::collections::BTreeSet;

use janiform_calibration_artifacts::CalibrationOptimizerSettings;
use janiform_foundations::randomness::SplitMix64SimulationRandomnessStream;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::bounds::{
    build_moment_seed_candidates, coordinate_to_ratio, coordinate_to_share,
    nig_configuration_from_search_point, nig_search_point_from_configuration, ratio_to_coordinate,
    relative_share_from_beta_ratios, sell_beta_ratio_from_relative_share, theoretical_alpha_bounds,
    theoretical_search_bounds,
};
use super::likelihood::{
    alpha_beta_boundary_penalty, average_negative_log_likelihood_resolution,
    closed_form_single_side_log_density, continuous_block_likelihood_objective_with_failures,
    log_density_net_trading_time_return, log_density_net_trading_time_return_failure_reason,
    maximum_profiled_open_beta_ratio, profile_likelihood_score,
};
use super::optimization::{build_nig_moment_seed_configurations, build_nig_search_candidates};
use super::profile::{
    build_distribution_profile, build_scale_bucket_profiles, distribution_profile_fit_score,
};
use super::sample::{build_continuous_return_optimizer_context, build_equal_business_time_blocks};
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

const TEST_OPTIMIZER_SAMPLE_COUNT: usize = 96;
const TEST_OPTIMIZER_MAX_ITERATIONS: usize = 12;
const TEST_OPTIMIZER_MOMENT_CANDIDATE_COUNT: usize = 24;

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

    let exact_log_density = log_density_net_trading_time_return(
        driver_parameters,
        configuration,
        1.1e-4,
        1.0,
        0.0,
        0.8,
        0.0,
        average_negative_log_likelihood_resolution(1),
    )
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

    let exact_log_density = log_density_net_trading_time_return(
        driver_parameters,
        configuration,
        -9.5e-5,
        1.0,
        1.0,
        0.0,
        0.65,
        average_negative_log_likelihood_resolution(1),
    )
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

    assert_eq!(
        exact_likelihood_diagnostics
            .evaluator_block_counts
            .tilted_mixed_inversion_block_count,
        calibration_sample.observed_returns.len(),
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
            log_return: 0.01,
            post_shock_activity_lift: 1.0,
            sell_share_proxy: 0.0,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 2,
            log_return: -0.02,
            post_shock_activity_lift: 2.0,
            sell_share_proxy: 1.0,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 3,
            log_return: 0.03,
            post_shock_activity_lift: 3.0,
            sell_share_proxy: 0.0,
        },
        ReturnComponentsObservedOpenSessionReturn {
            return_timestamp_utc_nanos: 4,
            log_return: 0.04,
            post_shock_activity_lift: 4.0,
            sell_share_proxy: 1.0,
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

    let log_density = log_density_net_trading_time_return(
        driver_parameters,
        configuration,
        1.5e-4,
        1.2,
        0.55,
        0.54,
        0.66,
        average_negative_log_likelihood_resolution(1),
    );
    let failure_reason = log_density_net_trading_time_return_failure_reason(
        driver_parameters,
        configuration,
        1.5e-4,
        1.2,
        0.55,
        0.54,
        0.66,
        average_negative_log_likelihood_resolution(1),
    );

    assert!(
        log_density.is_some(),
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
fn optimizer_candidates_report_proxy_em_frontier_without_local_iterations() {
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

    assert_eq!(
        candidates[0].optimizer_diagnostic.stop_reason,
        "proxy_em_frontier"
    );
    assert_eq!(candidates[0].optimizer_diagnostic.local_evaluation_count, 0);
    assert_eq!(
        candidates[0]
            .optimizer_diagnostic
            .seed_objective_score
            .to_bits(),
        candidates[0]
            .optimizer_diagnostic
            .local_objective_score
            .to_bits()
    );
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
