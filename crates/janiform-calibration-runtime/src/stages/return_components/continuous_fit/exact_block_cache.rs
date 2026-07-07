use std::collections::BTreeMap;
use std::sync::Mutex;

use super::exact_block::ExactBlockInput;
use super::likelihood::ExactBlockLikelihoodEvaluation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct ExactBlockEvaluationCacheKey {
    observed_return_bits: u64,
    business_time_scale_bits: u64,
    sell_share_proxy_bits: u64,
    integrated_buy_intensity_bits: u64,
    integrated_sell_intensity_bits: u64,
}

pub(super) type ExactBlockEvaluationCache =
    Mutex<BTreeMap<ExactBlockEvaluationCacheKey, ExactBlockLikelihoodEvaluation>>;

impl ExactBlockEvaluationCacheKey {
    pub(super) fn from_block_input(block_input: &ExactBlockInput) -> Self {
        Self {
            observed_return_bits: block_input.observed_return.to_bits(),
            business_time_scale_bits: block_input.business_time_scale.to_bits(),
            sell_share_proxy_bits: block_input.sell_share_proxy.to_bits(),
            integrated_buy_intensity_bits: block_input.integrated_buy_intensity.to_bits(),
            integrated_sell_intensity_bits: block_input.integrated_sell_intensity.to_bits(),
        }
    }
}

pub(super) fn cached_exact_block_evaluation(
    exact_block_cache: &ExactBlockEvaluationCache,
    cache_key: ExactBlockEvaluationCacheKey,
    log_density_resolution: f64,
) -> Option<ExactBlockLikelihoodEvaluation> {
    let Ok(cache_guard) = exact_block_cache.lock() else {
        return None;
    };

    cache_guard
        .get(&cache_key)
        .copied()
        .filter(|evaluation| evaluation.log_density_error_upper_bound <= log_density_resolution)
}

pub(super) fn record_exact_block_evaluation(
    exact_block_cache: &ExactBlockEvaluationCache,
    cache_key: ExactBlockEvaluationCacheKey,
    block_evaluation: ExactBlockLikelihoodEvaluation,
) {
    let Ok(mut cache_guard) = exact_block_cache.lock() else {
        return;
    };

    let should_replace = cache_guard
        .get(&cache_key)
        .map(|cached_evaluation| {
            block_evaluation.log_density_error_upper_bound
                < cached_evaluation.log_density_error_upper_bound
        })
        .unwrap_or(true);

    if should_replace {
        cache_guard.insert(cache_key, block_evaluation);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExactBlockEvaluationCache, ExactBlockEvaluationCacheKey, cached_exact_block_evaluation,
        record_exact_block_evaluation,
    };
    use crate::stages::return_components::continuous_fit::likelihood::ExactBlockLikelihoodEvaluation;
    use crate::stages::return_components::contracts::ReturnComponentsStageExactLikelihoodEvaluatorKind;

    fn cached_evaluation(error_upper_bound: f64) -> ExactBlockLikelihoodEvaluation {
        ExactBlockLikelihoodEvaluation {
            evaluator_kind: ReturnComponentsStageExactLikelihoodEvaluatorKind::TiltedMixedInversion,
            log_density_estimate: -1.0,
            log_density_error_upper_bound: error_upper_bound,
            tilt_theta: Some(0.0),
            tilt_domain_lower_theta_exclusive: Some(-1.0),
            tilt_domain_upper_theta_exclusive: Some(1.0),
            block_standard_deviation: 1.0,
            standardized_return: 0.0,
            standardized_decay_scale: Some(1.0),
            integration_upper_bound: Some(1.0),
        }
    }

    #[test]
    fn exact_block_cache_respects_resolution_and_keeps_best_certification() {
        let cache = ExactBlockEvaluationCache::default();
        let cache_key = ExactBlockEvaluationCacheKey {
            observed_return_bits: 0.0f64.to_bits(),
            business_time_scale_bits: 1.0f64.to_bits(),
            sell_share_proxy_bits: 0.5f64.to_bits(),
            integrated_buy_intensity_bits: 0.5f64.to_bits(),
            integrated_sell_intensity_bits: 0.5f64.to_bits(),
        };

        record_exact_block_evaluation(&cache, cache_key, cached_evaluation(0.25));

        assert!(cached_exact_block_evaluation(&cache, cache_key, 0.20).is_none());
        assert_eq!(
            cached_exact_block_evaluation(&cache, cache_key, 0.30)
                .expect("cached evaluation should satisfy looser resolution")
                .log_density_error_upper_bound,
            0.25
        );

        record_exact_block_evaluation(&cache, cache_key, cached_evaluation(0.15));

        assert_eq!(
            cached_exact_block_evaluation(&cache, cache_key, 0.20)
                .expect("better cached evaluation should satisfy tighter resolution")
                .log_density_error_upper_bound,
            0.15
        );

        record_exact_block_evaluation(&cache, cache_key, cached_evaluation(0.40));

        assert_eq!(
            cached_exact_block_evaluation(&cache, cache_key, 0.20)
                .expect("worse cached evaluation should not replace better certification")
                .log_density_error_upper_bound,
            0.15
        );
    }
}
