use super::contracts::ReturnComponentsStagePriceRangeSanityPolicy;

const RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO: f64 = 4.0;
const RETURN_COMPONENTS_STAGE_CONTINUOUS_REFERENCE_SPAN_MULTIPLIER: f64 = 6.0;
const RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_POLICY: &str =
    "continuous_reference_span_plus_fitted_jump_mass_thresholded_by_expansion_ratio.v1";

pub(crate) fn fit_price_range_sanity_policy() -> ReturnComponentsStagePriceRangeSanityPolicy {
    ReturnComponentsStagePriceRangeSanityPolicy {
        fit_policy: RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_POLICY,
        threshold_ratio: RETURN_COMPONENTS_STAGE_PRICE_RANGE_SANITY_THRESHOLD_RATIO,
        continuous_reference_span_multiplier:
            RETURN_COMPONENTS_STAGE_CONTINUOUS_REFERENCE_SPAN_MULTIPLIER,
    }
}

#[cfg(test)]
mod tests {
    use super::fit_price_range_sanity_policy;

    #[test]
    fn fitted_policy_uses_expected_defaults() {
        let policy = fit_price_range_sanity_policy();

        assert_eq!(policy.threshold_ratio, 4.0);
        assert_eq!(policy.continuous_reference_span_multiplier, 6.0);
    }
}
