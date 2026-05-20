use super::interval_operator::SolvedMaturityLaw;
use super::payoff::strike_log_boundary_from_reference_log;
use super::{
    MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT, NodePriceSharedPlusShiftSensitivity,
    RiskNeutralAnchorMaterialization, SpectralPricingDiagnostics,
    VanillaOptionGridBatchPricingRunError, VanillaOptionMaturitySummary,
    VanillaOptionNodePriceSensitivity, VanillaOptionPayoffType,
    VanillaOptionPricingOptionalOutputSelection, VanillaOptionPricingRequest,
    VanillaOptionPricingRunError, asset_reference_log_price_from_observed_spot,
    build_spectral_compatibility_integration_diagnostics,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct SpectralDomain {
    pub(super) deviation_lower_bound: f64,
    pub(super) deviation_upper_bound: f64,
    pub(super) node_count: usize,
}

impl SpectralDomain {
    pub(super) fn width(self) -> f64 {
        self.deviation_upper_bound - self.deviation_lower_bound
    }

    pub(super) fn collocation_nodes(self) -> Vec<f64> {
        let width = self.width();

        (0..self.node_count)
            .map(|node_index| {
                self.deviation_lower_bound
                    + (node_index as f64 + 0.5) * width / self.node_count as f64
            })
            .collect()
    }

    pub(super) fn basis_frequency(self, basis_index: usize) -> f64 {
        basis_index as f64 * std::f64::consts::PI / self.width()
    }
}

pub(super) fn build_spectral_domain(
    pricing_request: &VanillaOptionPricingRequest,
    materialization: &RiskNeutralAnchorMaterialization,
    maturity_count: usize,
    _maturity_interval: f64,
    strikes: &[f64],
    payoff_types: &[VanillaOptionPayoffType],
    collocation_node_count_override: Option<usize>,
) -> Result<SpectralDomain, VanillaOptionPricingRunError> {
    let terminal_anchor =
        materialization.runtime_interval_anchors.end_anchor_values[maturity_count - 1];

    let current_observed_log_price = pricing_request
        .boundary_input
        .observed_boundary_state
        .observed_log_price;

    let current_anchor = pricing_request
        .boundary_input
        .current_anchor_value()
        .map_err(
            |source| VanillaOptionPricingRunError::InvalidExecutionInput {
                source: Box::new(crate::runner::VanillaOptionPricingExecutionInputError::InvalidRequest {
                    source:
                        crate::contracts::VanillaOptionPricingRequestError::InvalidBoundaryInput {
                            source,
                        },
                }),
            },
        )?;

    let mut anchor_min = current_anchor;
    let mut anchor_max = current_anchor;

    for anchor_value in
        &materialization.runtime_interval_anchors.end_anchor_values[..maturity_count]
    {
        anchor_min = anchor_min.min(*anchor_value);
        anchor_max = anchor_max.max(*anchor_value);
    }

    let asset_reference_log_price = asset_reference_log_price_from_observed_spot(
        pricing_request.market_inputs.spot,
        current_observed_log_price,
    )
    .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
        message: "COS domain construction requires a positive finite asset reference price"
            .to_string(),
    })?;

    let anchor_span = anchor_max - anchor_min;

    let mut domain_points = vec![
        current_observed_log_price,
        current_observed_log_price + anchor_span,
        current_observed_log_price - anchor_span,
    ];

    let mut strike_boundaries = Vec::with_capacity(strikes.len());

    for strike in strikes {
        let strike_boundary =
            strike_log_boundary_from_reference_log(*strike, asset_reference_log_price).ok_or_else(
                || VanillaOptionPricingRunError::InternalNumericsFailure {
                    message: "COS domain construction requires positive finite strikes".to_string(),
                },
            )?;

        strike_boundaries.push(strike_boundary);
        domain_points.push(strike_boundary);
    }

    let observed_log_center = current_observed_log_price;
    let observed_log_half_width = observed_log_domain_half_width(
        observed_log_center,
        domain_points.as_slice(),
    )
    .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
        message: "COS domain construction requires a non-degenerate observed log-price envelope"
            .to_string(),
    })?;

    let (observed_log_lower_bound, observed_log_upper_bound) =
        observed_log_domain_bounds_with_payoff_side_tail(
            observed_log_center,
            observed_log_half_width,
            payoff_types,
            strike_boundaries.as_slice(),
        )
        .ok_or_else(|| VanillaOptionPricingRunError::InternalNumericsFailure {
            message: "COS domain construction requires finite payoff-side domain bounds"
                .to_string(),
        })?;

    let node_count = collocation_node_count_override
        .unwrap_or_else(|| spectral_collocation_node_count_from_domain_points(&domain_points));

    Ok(SpectralDomain {
        deviation_lower_bound: observed_log_lower_bound - terminal_anchor,
        deviation_upper_bound: observed_log_upper_bound - terminal_anchor,
        node_count,
    })
}

fn observed_log_domain_half_width(observed_log_center: f64, domain_points: &[f64]) -> Option<f64> {
    if !observed_log_center.is_finite() {
        return None;
    }

    let half_width = domain_points
        .iter()
        .copied()
        .filter(|point| point.is_finite())
        .map(|point| (point - observed_log_center).abs())
        .max_by(f64::total_cmp)?;

    (half_width > 0.0 && half_width.is_finite()).then_some(half_width)
}

fn observed_log_domain_bounds_with_payoff_side_tail(
    observed_log_center: f64,
    envelope_half_width: f64,
    payoff_types: &[VanillaOptionPayoffType],
    strike_boundaries: &[f64],
) -> Option<(f64, f64)> {
    if !observed_log_center.is_finite()
        || envelope_half_width <= 0.0
        || !envelope_half_width.is_finite()
        || payoff_types.is_empty()
    {
        return None;
    }

    let mut lower_bound = observed_log_center - envelope_half_width;
    let mut upper_bound = observed_log_center + envelope_half_width;

    for payoff_type in payoff_types {
        // The domain policy mirrors the farthest quoted strike on
        // the payoff-active side around the observed log price.
        match payoff_type {
            VanillaOptionPayoffType::Call => {
                let payoff_side_boundary = strike_boundaries
                    .iter()
                    .copied()
                    .filter(|strike_boundary| {
                        strike_boundary.is_finite() && *strike_boundary > observed_log_center
                    })
                    .max_by(f64::total_cmp)
                    .unwrap_or(observed_log_center + envelope_half_width);

                let required_upper_bound =
                    payoff_side_boundary + (payoff_side_boundary - observed_log_center);
                if !required_upper_bound.is_finite() {
                    return None;
                }

                upper_bound = upper_bound.max(required_upper_bound);
            }
            VanillaOptionPayoffType::Put => {
                let payoff_side_boundary = strike_boundaries
                    .iter()
                    .copied()
                    .filter(|strike_boundary| {
                        strike_boundary.is_finite() && *strike_boundary < observed_log_center
                    })
                    .min_by(f64::total_cmp)
                    .unwrap_or(observed_log_center - envelope_half_width);

                let required_lower_bound =
                    payoff_side_boundary - (observed_log_center - payoff_side_boundary);
                if !required_lower_bound.is_finite() {
                    return None;
                }

                lower_bound = lower_bound.min(required_lower_bound);
            }
        }
    }

    (lower_bound < upper_bound && lower_bound.is_finite() && upper_bound.is_finite())
        .then_some((lower_bound, upper_bound))
}

fn spectral_collocation_node_count_from_domain_points(domain_points: &[f64]) -> usize {
    let mut sorted_points = domain_points
        .iter()
        .copied()
        .filter(|point| point.is_finite())
        .collect::<Vec<_>>();

    sorted_points.sort_by(f64::total_cmp);
    sorted_points.dedup_by(|left, right| left.total_cmp(right).is_eq());

    sorted_points
        .len()
        .max(MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT)
}

#[cfg(test)]
mod tests {
    use super::super::payoff::payoff_cos_coefficient;
    use super::{
        VanillaOptionPayoffType, observed_log_domain_bounds_with_payoff_side_tail,
        observed_log_domain_half_width, spectral_collocation_node_count_from_domain_points,
    };
    use crate::constants::MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT;

    #[test]
    fn spectral_domain_half_width_is_derived_from_envelope_points() {
        let half_width = observed_log_domain_half_width(0.2, &[0.2, -0.1, 0.35, 0.05])
            .expect("non-degenerate envelope should resolve");

        assert!((half_width - 0.3).abs() <= f64::EPSILON);
    }

    #[test]
    fn spectral_domain_half_width_rejects_degenerate_envelope() {
        let half_width = observed_log_domain_half_width(0.2, &[0.2, 0.2]);

        assert_eq!(half_width, None);
    }

    #[test]
    fn spectral_domain_bounds_extend_the_payoff_side_beyond_wing_strikes() {
        let call_bounds = observed_log_domain_bounds_with_payoff_side_tail(
            0.0,
            0.25,
            &[VanillaOptionPayoffType::Call],
            &[0.25],
        )
        .expect("call payoff-side bounds should resolve");

        let put_bounds = observed_log_domain_bounds_with_payoff_side_tail(
            0.0,
            0.25,
            &[VanillaOptionPayoffType::Put],
            &[-0.25],
        )
        .expect("put payoff-side bounds should resolve");

        assert_eq!(call_bounds, (-0.25, 0.5));
        assert_eq!(put_bounds, (-0.5, 0.25));

        let call_wing_strike_boundary = 0.25_f64;
        let call_wing_strike = call_wing_strike_boundary.exp();
        let call_wing_coefficient = payoff_cos_coefficient(
            VanillaOptionPayoffType::Call,
            1.0,
            call_wing_strike,
            call_bounds.0,
            call_bounds.1,
            call_wing_strike_boundary,
            0,
        )
        .expect("call wing coefficient should resolve");

        let put_wing_strike_boundary = -0.25_f64;
        let put_wing_strike = put_wing_strike_boundary.exp();
        let put_wing_coefficient = payoff_cos_coefficient(
            VanillaOptionPayoffType::Put,
            1.0,
            put_wing_strike,
            put_bounds.0,
            put_bounds.1,
            put_wing_strike_boundary,
            0,
        )
        .expect("put wing coefficient should resolve");

        assert!(call_wing_coefficient > 0.0);
        assert!(put_wing_coefficient > 0.0);
    }

    #[test]
    fn spectral_domain_bounds_extend_payoff_side_from_envelope_when_no_wing_strike_exists() {
        let call_bounds = observed_log_domain_bounds_with_payoff_side_tail(
            0.0,
            0.25,
            &[VanillaOptionPayoffType::Call],
            &[-0.25],
        )
        .expect("call payoff-side bounds should resolve from envelope");

        let put_bounds = observed_log_domain_bounds_with_payoff_side_tail(
            0.0,
            0.25,
            &[VanillaOptionPayoffType::Put],
            &[0.25],
        )
        .expect("put payoff-side bounds should resolve from envelope");

        assert_eq!(call_bounds, (-0.25, 0.5));
        assert_eq!(put_bounds, (-0.5, 0.25));
    }

    #[test]
    fn spectral_domain_bounds_union_both_payoff_sides_for_mixed_grids() {
        let bounds = observed_log_domain_bounds_with_payoff_side_tail(
            0.0,
            0.25,
            &[VanillaOptionPayoffType::Call, VanillaOptionPayoffType::Put],
            &[-0.25, 0.25],
        )
        .expect("mixed payoff-side bounds should resolve");

        assert_eq!(bounds, (-0.5, 0.5));
    }

    #[test]
    fn default_spectral_collocation_node_count_tracks_distinct_envelope_points() {
        let node_count =
            spectral_collocation_node_count_from_domain_points(&[0.0, 0.2, 0.2, -0.1, 0.5]);

        assert_eq!(node_count, 4);
        assert_eq!(
            spectral_collocation_node_count_from_domain_points(&[0.0]),
            MINIMUM_SPECTRAL_COSINE_COLLOCATION_NODE_COUNT
        );
    }
}

pub(super) fn build_maturity_summary(
    solved_maturity: &SolvedMaturityLaw,
    optional_output_selection: VanillaOptionPricingOptionalOutputSelection,
    representative_price: Option<f64>,
) -> Result<VanillaOptionMaturitySummary, VanillaOptionGridBatchPricingRunError> {
    let spectral_diagnostics = if optional_output_selection.include_integration_quality_diagnostics
    {
        Some(
            SpectralPricingDiagnostics::try_new(
                solved_maturity.deviation_lower_bound,
                solved_maturity.deviation_upper_bound,
                solved_maturity.node_count,
                solved_maturity.interval_summaries.clone(),
            )
            .map_err(|source| {
                VanillaOptionGridBatchPricingRunError::NodePricingFailure {
                    message: format!("spectral maturity diagnostics are invalid: {source}"),
                }
            })?,
        )
    } else {
        None
    };

    let integration_quality =
        if let (Some(price), Some(diagnostics)) = (representative_price, spectral_diagnostics) {
            Some(build_spectral_compatibility_integration_diagnostics(
                price,
                solved_maturity.maturity_interval,
                &diagnostics,
            ))
        } else {
            None
        };

    Ok(VanillaOptionMaturitySummary {
        maturity_interval: solved_maturity.maturity_interval,
        terminal_anchor_value: solved_maturity.terminal_anchor_value,
        deviation_domain_lower_bound: solved_maturity.deviation_lower_bound,
        deviation_domain_upper_bound: solved_maturity.deviation_upper_bound,
        collocation_node_count: solved_maturity.node_count,
        collocation_nodes: SpectralDomain {
            deviation_lower_bound: solved_maturity.deviation_lower_bound,
            deviation_upper_bound: solved_maturity.deviation_upper_bound,
            node_count: solved_maturity.node_count,
        }
        .collocation_nodes(),
        interval_summaries: solved_maturity.interval_summaries.clone(),
        representative_transform: if optional_output_selection.include_transform_diagnostics
            || optional_output_selection.include_calibration_hooks
        {
            solved_maturity.representative_transform
        } else {
            None
        },
        integration_quality,
    })
}

pub(super) fn resolve_bounded_node_price_sensitivity(
    raw_price_sensitivity: NodePriceSharedPlusShiftSensitivity,
    raw_price: f64,
    bounded_price: f64,
) -> VanillaOptionNodePriceSensitivity {
    if raw_price == bounded_price {
        raw_price_sensitivity.into_output()
    } else {
        // The reported node price is the clipped no-arbitrage output. Away from the exact
        // kink, the active bound is constant with respect to the pricing parameters, so the
        // local derivative of the clipped output is zero.
        NodePriceSharedPlusShiftSensitivity::zero().into_output()
    }
}
