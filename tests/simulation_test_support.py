from janiform.simulation import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)


def build_endogenous_mean_reversion_runtime_contracts(
    *,
    interval_count: int,
    start_anchor_value: float = 0.0,
    observed_log_price: float = 0.0,
    interval_delta_hours: float = 1.0,
) -> tuple[
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
    MeanReversionBoundaryState,
]:
    return (
        MeanReversionFittedParameters(
            kappa=0.0,
            anchor_model=MeanReversionAnchorModel.endogenous_core_expectation_one_interval(),
        ),
        MeanReversionRuntimeIntervalAnchors(
            interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
            start_anchor_value=start_anchor_value,
            end_anchor_values=tuple(start_anchor_value for _ in range(interval_count)),
            interval_deltas=tuple(interval_delta_hours for _ in range(interval_count)),
        ),
        MeanReversionBoundaryState(
            observed_log_price=observed_log_price,
            anchor_value=start_anchor_value,
            deviation=observed_log_price - start_anchor_value,
        ),
    )
