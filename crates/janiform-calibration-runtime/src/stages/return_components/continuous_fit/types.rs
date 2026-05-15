use std::{
    cmp::Ordering,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;
use serde::{Deserialize, Serialize};

use super::super::contracts::{
    ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    ReturnComponentsStageContinuousOptimizerBasinDiagnostic,
    ReturnComponentsStageReturnDistributionDiagnostic,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ContinuousLikelihoodFidelityTier {
    Seed,
    Local,
    Full,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ContinuousLikelihoodObjective {
    pub(crate) shifted_objective_score: f64,
    pub(crate) global_negative_log_likelihood: f64,
    pub(crate) conditional_negative_log_likelihood: f64,
    pub(crate) boundary_penalty: f64,
    pub(crate) log_likelihood_shift: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContinuousLikelihoodEvaluatorCounts {
    pub(crate) closed_form_buy: usize,
    pub(crate) closed_form_sell: usize,
    pub(crate) tilted_mixed: usize,
}

impl ContinuousLikelihoodEvaluatorCounts {
    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            closed_form_buy: self.closed_form_buy.saturating_add(other.closed_form_buy),
            closed_form_sell: self.closed_form_sell.saturating_add(other.closed_form_sell),
            tilted_mixed: self.tilted_mixed.saturating_add(other.tilted_mixed),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuousReturnOptimizerDiagnostic {
    pub seed_objective_score: f64,
    pub local_objective_score: f64,
    pub final_objective_score: f64,
    pub local_evaluation_count: usize,
    pub stop_reason: &'static str,
    pub basin_diagnostics: Vec<ReturnComponentsStageContinuousOptimizerBasinDiagnostic>,
}

#[derive(Debug, Clone)]
pub(crate) struct ReturnComponentsStageContinuousReturnCalibrationSample {
    pub(super) observed_returns: Vec<f64>,
    pub(super) business_time_scales: Vec<f64>,
    pub(super) sell_share_proxies: Vec<f64>,
    pub(super) observed_profile: ContinuousReturnDistributionProfile,
    pub(super) scale_bucket_profiles: Vec<ContinuousReturnScaleBucketProfile>,
    pub(super) open_session_sell_share_proxy: f64,
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuousReturnCandidateEvaluation {
    pub objective_score: f64,
    pub diagnostic: ReturnComponentsStageReturnDistributionDiagnostic,
    pub fitted_returns: Vec<f64>,
    pub fit_breakdown: ReturnComponentsStageContinuousDistributionFitBreakdownDiagnostic,
    pub optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ContinuousReturnDistributionProfile {
    pub(super) sample_count: usize,
    pub(super) mean_return: f64,
    pub(super) return_volatility: f64,
    pub(super) adjusted_skewness: f64,
    pub(super) adjusted_excess_kurtosis: f64,
    pub(super) median_return: f64,
    pub(super) lower_quartile_return: f64,
    pub(super) upper_quartile_return: f64,
    pub(super) lower_tail_return_q05: f64,
    pub(super) upper_tail_return_q95: f64,
    pub(super) extreme_left_tail_return_q01: f64,
    pub(super) extreme_right_tail_return_q99: f64,
    pub(super) mean_abs_return: f64,
    pub(super) median_abs_return: f64,
    pub(super) abs_quantile_ratio_q99_q95: f64,
    pub(super) bowley_skewness: f64,
    pub(super) l_moment_lambda2: f64,
    pub(super) l_moment_tau3: f64,
}

#[derive(Debug, Clone)]
pub(super) struct ContinuousReturnScaleBucketProfile {
    pub(super) member_indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NigShapeSearchPoint {
    pub(crate) log_alpha: f64,
    pub(crate) buy_skew_coordinate: f64,
    pub(crate) sell_relative_coordinate: f64,
    pub(crate) log_delta_offset: f64,
}

impl NigShapeSearchPoint {
    pub(crate) fn from_slice(point: &[f64]) -> Option<Self> {
        let [
            log_alpha,
            buy_skew_coordinate,
            sell_relative_coordinate,
            log_delta_offset,
        ] = point
        else {
            return None;
        };

        Some(Self {
            log_alpha: *log_alpha,
            buy_skew_coordinate: *buy_skew_coordinate,
            sell_relative_coordinate: *sell_relative_coordinate,
            log_delta_offset: *log_delta_offset,
        })
    }

    pub(crate) fn to_vector(self) -> [f64; Self::DIMENSION] {
        [
            self.log_alpha,
            self.buy_skew_coordinate,
            self.sell_relative_coordinate,
            self.log_delta_offset,
        ]
    }

    pub(crate) const DIMENSION: usize = 4;
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NigSearchRegime {
    pub(super) log_alpha_center: f64,
    pub(super) buy_skew_center: f64,
    pub(super) sell_relative_center: f64,
    pub(super) log_delta_center: f64,
    pub(super) log_alpha_half_span: f64,
    pub(super) buy_skew_half_span: f64,
    pub(super) sell_relative_half_span: f64,
    pub(super) log_delta_half_span: f64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ContinuousMomentTargets {
    pub(super) return_volatility: f64,
    pub(super) adjusted_skewness: f64,
    pub(super) adjusted_excess_kurtosis: f64,
    pub(super) skewness_standard_error: f64,
    pub(super) excess_kurtosis_standard_error: f64,
}

impl ContinuousReturnDistributionProfile {
    pub(super) fn skewness_standard_error(self) -> f64 {
        (6.0 / self.sample_count.max(3) as f64).sqrt()
    }

    pub(super) fn excess_kurtosis_standard_error(self) -> f64 {
        (24.0 / self.sample_count.max(4) as f64).sqrt()
    }
}

impl ContinuousMomentTargets {
    pub(super) fn from_profile(observed_profile: ContinuousReturnDistributionProfile) -> Self {
        Self {
            return_volatility: observed_profile.return_volatility,
            adjusted_skewness: observed_profile.adjusted_skewness,
            adjusted_excess_kurtosis: observed_profile.adjusted_excess_kurtosis.max(0.0),
            skewness_standard_error: observed_profile.skewness_standard_error(),
            excess_kurtosis_standard_error: observed_profile.excess_kurtosis_standard_error(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ScoredNigCandidate {
    pub(super) search_point: NigShapeSearchPoint,
    pub(super) configuration: NigSharedPlusShiftConfiguration,
    pub(super) objective_score: f64,
}

impl ScoredNigCandidate {
    pub(super) fn compare_by_objective_and_configuration(left: &Self, right: &Self) -> Ordering {
        left.objective_score
            .total_cmp(&right.objective_score)
            .then_with(|| {
                left.configuration
                    .alpha
                    .total_cmp(&right.configuration.alpha)
            })
            .then_with(|| {
                left.configuration
                    .beta_0
                    .total_cmp(&right.configuration.beta_0)
            })
            .then_with(|| {
                left.configuration
                    .delta
                    .total_cmp(&right.configuration.delta)
            })
            .then_with(|| {
                left.configuration
                    .delta_beta
                    .total_cmp(&right.configuration.delta_beta)
            })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum BasinSeedFamily {
    Overall,
    HighAlphaLowDelta,
    MediumAlphaMediumDelta,
    LowAlphaHighDelta,
    SkewLight,
    SkewHeavy,
    SymmetricLike,
    AsymmetricLike,
}

impl BasinSeedFamily {
    pub(super) const ALL: [Self; 8] = [
        Self::Overall,
        Self::HighAlphaLowDelta,
        Self::MediumAlphaMediumDelta,
        Self::LowAlphaHighDelta,
        Self::SkewLight,
        Self::SkewHeavy,
        Self::SymmetricLike,
        Self::AsymmetricLike,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Overall => "overall",
            Self::HighAlphaLowDelta => "high_alpha_low_delta",
            Self::MediumAlphaMediumDelta => "medium_alpha_medium_delta",
            Self::LowAlphaHighDelta => "low_alpha_high_delta",
            Self::SkewLight => "skew_light",
            Self::SkewHeavy => "skew_heavy",
            Self::SymmetricLike => "symmetric_like",
            Self::AsymmetricLike => "asymmetric_like",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SeededNigCandidate {
    pub(super) family: BasinSeedFamily,
    pub(super) scored_candidate: ScoredNigCandidate,
}

#[derive(Debug, Clone)]
pub(crate) struct OptimizedNigCandidate {
    pub configuration: NigSharedPlusShiftConfiguration,
    pub optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone)]
pub(super) struct OptimizedNigSearchCandidate {
    pub(super) configuration: NigSharedPlusShiftConfiguration,
    pub(super) family: BasinSeedFamily,
    pub(super) seed_search_point: NigShapeSearchPoint,
    pub(super) final_search_point: NigShapeSearchPoint,
    pub(super) optimizer_diagnostic: ContinuousReturnOptimizerDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ContinuousLikelihoodObjectiveCacheKey {
    pub(super) fidelity_tier: ContinuousLikelihoodFidelityTier,
    pub(super) log_alpha_bits: u64,
    pub(super) buy_skew_coordinate_bits: u64,
    pub(super) sell_relative_coordinate_bits: u64,
    pub(super) log_delta_offset_bits: u64,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CachedSearchPointObjectiveEvaluation {
    pub(super) configuration: NigSharedPlusShiftConfiguration,
    pub(super) objective: ContinuousLikelihoodObjective,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct NigShapeSearchBounds {
    pub(super) lower_bounds: [f64; 4],
    pub(super) upper_bounds: [f64; 4],
}

#[derive(Debug, Clone)]
pub(crate) struct ContinuousReturnOptimizerContext {
    pub(super) observed_returns: Vec<f64>,
    pub(super) business_time_scales: Vec<f64>,
    pub(super) mean_business_time_scale: f64,
    pub(super) sell_share_proxies: Vec<f64>,
    pub(super) integrated_buy_intensities: Vec<f64>,
    pub(super) integrated_sell_intensities: Vec<f64>,
    pub(super) scale_bucket_profiles: Vec<ContinuousReturnScaleBucketProfile>,
    pub(super) observed_return_volatility: f64,
    pub(super) log_likelihood_shift: f64,
    pub(super) open_session_sell_share_proxy: f64,
    pub(super) seed_member_indices: Vec<usize>,
    pub(super) local_member_indices: Vec<usize>,
    pub(super) full_member_indices: Vec<usize>,
    pub(super) cache: Arc<
        Mutex<
            BTreeMap<
                ContinuousLikelihoodObjectiveCacheKey,
                Option<CachedSearchPointObjectiveEvaluation>,
            >,
        >,
    >,
}
