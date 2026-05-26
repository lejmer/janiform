//! Derived work-budget policy for continuous NIG fitting.
//!
//! Breadth is tied to search-coordinate coverage, basin-family coverage, observed
//! support depth, and observed business-time scale regimes. Proxy EM depth is tied
//! to the effective zero-drift NIG parameter count, the binary resolution gap
//! between the sample likelihood resolution and the configured solver tolerance,
//! and the observed support information depth. Proxy EM breadth is tied to the
//! same coordinate, family, support, and scale-regime dimensions so compressed
//! fitting samples preserve the search geometry instead of using a fixed row cap.

use janiform_calibration_artifacts::CalibrationOptimizerSettings;

use crate::information_depth::support_information_depth;

use super::likelihood::average_negative_log_likelihood_resolution;
use super::types::{BasinSeedFamily, ReturnComponentsStageContinuousReturnCalibrationSample};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ContinuousNigFitBudgets {
    pub(super) moment_candidate_generation_count: usize,
    pub(super) seed_frontier_candidate_count: usize,
    pub(super) proxy_em_iteration_count: usize,
    pub(super) basin_seed_candidate_count: usize,
}

impl ContinuousNigFitBudgets {
    pub(super) fn from_calibration_sample(
        calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
        solver_settings: &CalibrationOptimizerSettings,
    ) -> Self {
        let sample_count = calibration_sample.observed_profile.sample_count;
        let support_depth = support_information_depth(sample_count);
        let family_count = BasinSeedFamily::ALL.len().max(1);
        let scale_regime_count = calibration_sample.scale_bucket_profiles.len().max(1);
        let search_coordinate_count = continuous_nig_search_coordinate_count().max(1);

        let seed_frontier_candidate_count = family_aligned_candidate_count(
            family_count.saturating_mul(scale_regime_count),
            family_count,
        );

        let moment_candidate_generation_count = family_aligned_candidate_count(
            family_count
                .saturating_mul(search_coordinate_count)
                .saturating_mul(support_depth)
                .max(seed_frontier_candidate_count),
            family_count,
        );

        Self {
            moment_candidate_generation_count,
            seed_frontier_candidate_count,
            proxy_em_iteration_count: proxy_em_iteration_budget(sample_count, solver_settings),
            basin_seed_candidate_count: family_count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContinuousNigSearchCoordinate {
    LogAlpha,
    BuySkew,
    SellRelative,
    LogDeltaOffset,
}

impl ContinuousNigSearchCoordinate {
    const ALL: [Self; 4] = [
        Self::LogAlpha,
        Self::BuySkew,
        Self::SellRelative,
        Self::LogDeltaOffset,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveZeroDriftNigEmParameter {
    Alpha,
    Beta,
    Delta,
}

impl EffectiveZeroDriftNigEmParameter {
    const ALL: [Self; 3] = [Self::Alpha, Self::Beta, Self::Delta];
}

pub(super) fn family_aligned_candidate_count(
    candidate_budget: usize,
    family_count: usize,
) -> usize {
    if family_count == 0 {
        return candidate_budget.max(1);
    }

    let requested_budget = candidate_budget.max(family_count);
    let rounded_family_count = requested_budget
        .div_ceil(family_count)
        .saturating_mul(family_count);

    rounded_family_count.max(family_count)
}

pub(super) fn proxy_em_iteration_budget(
    sample_count: usize,
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    let support_depth = support_information_depth(sample_count).max(1);

    effective_zero_drift_nig_em_parameter_count()
        .max(1)
        .saturating_mul(proxy_em_objective_resolution_depth(
            sample_count,
            solver_settings,
        ))
        .saturating_mul(support_depth)
        .max(1)
}

pub(super) fn continuous_nig_search_coordinate_count() -> usize {
    ContinuousNigSearchCoordinate::ALL.len()
}

pub(super) fn effective_zero_drift_nig_em_parameter_count() -> usize {
    EffectiveZeroDriftNigEmParameter::ALL.len()
}

pub(super) fn proxy_em_fitting_sample_target_count(
    calibration_sample: &ReturnComponentsStageContinuousReturnCalibrationSample,
) -> usize {
    let sample_count = calibration_sample.observed_profile.sample_count;
    if sample_count <= 1 {
        return sample_count;
    }

    let support_depth = support_information_depth(sample_count).max(1);
    let family_count = BasinSeedFamily::ALL.len().max(1);
    let scale_regime_count = calibration_sample.scale_bucket_profiles.len().max(1);
    let search_coordinate_count = continuous_nig_search_coordinate_count().max(1);

    effective_zero_drift_nig_em_parameter_count()
        .max(1)
        .saturating_mul(search_coordinate_count)
        .saturating_mul(family_count)
        .saturating_mul(scale_regime_count)
        .saturating_mul(support_depth)
        .clamp(1, sample_count)
}

fn proxy_em_objective_resolution_depth(
    sample_count: usize,
    solver_settings: &CalibrationOptimizerSettings,
) -> usize {
    let objective_resolution = average_negative_log_likelihood_resolution(sample_count);
    let solver_objective_tolerance = solver_settings
        .absolute_tolerance
        .max(solver_settings.relative_tolerance);

    if !objective_resolution.is_finite()
        || !solver_objective_tolerance.is_finite()
        || solver_objective_tolerance <= 0.0
        || objective_resolution <= solver_objective_tolerance
    {
        return support_information_depth(sample_count);
    }

    let mut resolved_tolerance = solver_objective_tolerance;
    let mut binary_resolution_depth = 0usize;
    while resolved_tolerance < objective_resolution {
        resolved_tolerance *= 2.0;
        binary_resolution_depth = binary_resolution_depth.saturating_add(1);

        if !resolved_tolerance.is_finite() {
            return usize::MAX;
        }
    }

    binary_resolution_depth.max(support_information_depth(sample_count))
}
