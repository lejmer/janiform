from typing import Any

from janiform.simulation import (
    MeanReversionAnchorModel,
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)
from tests.support.calibration import build_disabled_timestamp_batching_observation_model_payload


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
            anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
        ),
        MeanReversionRuntimeIntervalAnchors(
            interval_alignment_policy="explicit_runtime_interval_grid_alignment.v1",
            start_anchor_value=start_anchor_value,
            start_pressure_state=0.0,
            end_anchor_values=tuple(start_anchor_value for _ in range(interval_count)),
            end_pressure_states=tuple(0.0 for _ in range(interval_count)),
            interval_deltas=tuple(interval_delta_hours for _ in range(interval_count)),
        ),
        MeanReversionBoundaryState(
            observed_log_price=observed_log_price,
            anchor_value=start_anchor_value,
            pressure_state=0.0,
            deviation=observed_log_price - start_anchor_value,
        ),
    )


def build_seeded_timing_diagnostics_payload() -> dict[str, Any]:
    return {
        "schema_version": "simulation.seeded_tick_event.timing_diagnostics.v1",
        "timestamp_batching_observation_model": (
            build_disabled_timestamp_batching_observation_model_payload()
        ),
        "latent_event_interarrivals": {
            "interarrival_count": 0,
            "zero_interarrival_count": 0,
        },
        "observed_timestamp_interarrivals": {
            "interarrival_count": 0,
            "zero_interarrival_count": 0,
        },
        "latent_interval_ownership_policy": (
            "latent_event_time_uses_interval_factor_grid_start_inclusive_end_exclusive_ownership.v1"
        ),
        "observed_timestamp_interval_ownership_policy": (
            "observed_timestamp_ownership_follows_timestamp_batching_policy_within_latent_interval.v1"
        ),
        "coarse_interval_compatibility_policy": (
            "observed_timestamp_ownership_constrained_to_latent_interval_for_direct_coarse_consistency.v1"
        ),
    }
