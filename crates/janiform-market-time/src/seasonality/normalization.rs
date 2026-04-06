use super::composition::SeasonalityNormalizationMetadata;
use super::numerics::{derived_mean_anchor_tolerance_f64, stable_mean_f64};
use super::{IDENTIFIABILITY_NORMALIZATION_POLICY, IDENTIFIABILITY_TARGET_MEAN_FACTOR};

#[derive(Debug, Clone, PartialEq)]
pub(super) enum SeasonalityNormalizationError {
    EmptyFitWindow,
    InvalidInputFactor {
        record_position: usize,
    },
    InvalidInputLogComponent {
        record_position: usize,
    },
    UnstableNormalizationConstant,
    IdentifiabilityGuardViolation {
        anchored_mean_factor: f64,
        target_mean_factor: f64,
        mean_anchor_tolerance: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SeasonalityNormalizationResult {
    pub(super) normalized_factor_by_event: Vec<f64>,
    pub(super) normalized_log_component_by_event: Vec<f64>,
    pub(super) normalization_metadata: SeasonalityNormalizationMetadata,
}

pub(super) fn normalize_seasonality_factors_for_identifiability(
    factor_by_event: &[f64],
    log_component_by_event: &[f64],
) -> Result<SeasonalityNormalizationResult, SeasonalityNormalizationError> {
    if factor_by_event.is_empty() {
        return Err(SeasonalityNormalizationError::EmptyFitWindow);
    }

    if log_component_by_event.len() != factor_by_event.len() {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    for (record_position, factor_value) in factor_by_event.iter().copied().enumerate() {
        if !factor_value.is_finite() || factor_value <= 0.0 {
            return Err(SeasonalityNormalizationError::InvalidInputFactor { record_position });
        }
    }

    for (record_position, log_component_value) in log_component_by_event.iter().copied().enumerate()
    {
        if !log_component_value.is_finite() {
            return Err(SeasonalityNormalizationError::InvalidInputLogComponent {
                record_position,
            });
        }
    }

    // Use compensated summation so long fit windows do not trip the identifiability guard due
    // to accumulation order alone.
    let pre_normalization_mean_factor = stable_mean_f64(factor_by_event);
    if !pre_normalization_mean_factor.is_finite() || pre_normalization_mean_factor <= 0.0 {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalization_constant = pre_normalization_mean_factor / IDENTIFIABILITY_TARGET_MEAN_FACTOR;
    if !normalization_constant.is_finite() || normalization_constant <= 0.0 {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalization_log_offset = normalization_constant.ln();
    if !normalization_log_offset.is_finite() {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    let normalized_factor_by_event: Vec<f64> = factor_by_event
        .iter()
        .map(|factor_value| *factor_value / normalization_constant)
        .collect();
    let normalized_log_component_by_event: Vec<f64> = log_component_by_event
        .iter()
        .map(|log_component_value| *log_component_value - normalization_log_offset)
        .collect();

    for (record_position, normalized_factor) in
        normalized_factor_by_event.iter().copied().enumerate()
    {
        if !normalized_factor.is_finite() || normalized_factor <= 0.0 {
            return Err(SeasonalityNormalizationError::InvalidInputFactor { record_position });
        }
    }

    for (record_position, normalized_log_component) in normalized_log_component_by_event
        .iter()
        .copied()
        .enumerate()
    {
        if !normalized_log_component.is_finite() {
            return Err(SeasonalityNormalizationError::InvalidInputLogComponent {
                record_position,
            });
        }
    }

    let post_normalization_mean_factor = stable_mean_f64(&normalized_factor_by_event);

    let mean_anchor_tolerance = derived_mean_anchor_tolerance_f64(
        factor_by_event.len(),
        IDENTIFIABILITY_TARGET_MEAN_FACTOR,
        normalization_constant,
        post_normalization_mean_factor,
    );

    if !mean_anchor_tolerance.is_finite() || mean_anchor_tolerance <= 0.0 {
        return Err(SeasonalityNormalizationError::UnstableNormalizationConstant);
    }

    if !post_normalization_mean_factor.is_finite()
        || (post_normalization_mean_factor - IDENTIFIABILITY_TARGET_MEAN_FACTOR).abs()
            > mean_anchor_tolerance
    {
        return Err(
            SeasonalityNormalizationError::IdentifiabilityGuardViolation {
                anchored_mean_factor: post_normalization_mean_factor,
                target_mean_factor: IDENTIFIABILITY_TARGET_MEAN_FACTOR,
                mean_anchor_tolerance,
            },
        );
    }

    Ok(SeasonalityNormalizationResult {
        normalized_factor_by_event,
        normalized_log_component_by_event,
        normalization_metadata: SeasonalityNormalizationMetadata {
            normalization_policy: IDENTIFIABILITY_NORMALIZATION_POLICY,
            fit_window_event_count: factor_by_event.len(),
            target_mean_factor: IDENTIFIABILITY_TARGET_MEAN_FACTOR,
            pre_normalization_mean_factor,
            normalization_constant,
            normalization_log_offset,
            post_normalization_mean_factor,
            mean_anchor_tolerance,
        },
    })
}
