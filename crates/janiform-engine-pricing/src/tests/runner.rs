use crate::{
    AnchoredVanillaOptionPricingBoundaryInput, VanillaOptionPayoffConfiguration,
    VanillaOptionPayoffType, VanillaOptionPricingMarketInputs, VanillaOptionPricingRequest,
    use_nested_transition_measure_for_local_request,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::{
    MeanReversionAnchorModel, MeanReversionBoundaryState, MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
};

use super::support::valid_model_parameters;

#[test]
fn nested_transition_measure_detection_accepts_disabled_multi_interval_boundary() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 105.0, 1.0, 0.0, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            MeanReversionFittedParameters::disabled(),
            MeanReversionRuntimeIntervalAnchors::try_new(
                "pricing_disabled_flat_anchor_interval_grid.v1",
                0.0,
                vec![0.0, 0.0],
                vec![1.0, 1.0],
            )
            .expect("runtime anchors should construct"),
            MeanReversionBoundaryState::try_new(0.0, 0.0, 0.0)
                .expect("observed boundary state should construct"),
        )
        .expect("boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("pricing request should construct");

    assert!(
        use_nested_transition_measure_for_local_request(&pricing_request)
            .expect("nested transition measure detection should succeed")
    );
}

#[test]
fn nested_transition_measure_detection_rejects_nonzero_observed_return_adjustment() {
    let pricing_request = VanillaOptionPricingRequest::try_new(
        VanillaOptionPricingMarketInputs::try_new(100.0, 105.0, 1.0, 0.0, 0.0)
            .expect("market inputs should construct"),
        VanillaOptionPayoffConfiguration {
            payoff_type: VanillaOptionPayoffType::Call,
        },
        AnchoredVanillaOptionPricingBoundaryInput::try_new(
            0,
            HawkesExcitationState {
                y_plus_plus: 0.0,
                y_plus_minus: 0.0,
                y_minus_plus: 0.0,
                y_minus_minus: 0.0,
            },
            MeanReversionFittedParameters::try_new(
                0.5,
                MeanReversionAnchorModel::constant(0.0)
                    .expect("constant anchor model should construct"),
            )
            .expect("mean-reversion fitted parameters should construct"),
            MeanReversionRuntimeIntervalAnchors::try_new(
                "pricing_constant_anchor_interval_grid.v1",
                0.0,
                vec![0.0],
                vec![1.0],
            )
            .expect("runtime anchors should construct"),
            MeanReversionBoundaryState::try_new(0.2, 0.0, 0.2)
                .expect("observed boundary state should construct"),
        )
        .expect("boundary input should construct"),
        valid_model_parameters(),
    )
    .expect("pricing request should construct");

    assert!(
        !use_nested_transition_measure_for_local_request(&pricing_request)
            .expect("nested transition measure detection should succeed")
    );
}
