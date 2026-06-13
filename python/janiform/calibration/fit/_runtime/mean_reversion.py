from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, cast

from ...._native_api import native_module
from ...._timestamps import nanoseconds_to_hours
from ....interval_factor_grids import IntervalFactorGridContract
from ....mean_reversion import (
    MeanReversionBoundaryState,
    MeanReversionFittedParameters,
    MeanReversionRuntimeIntervalAnchors,
)
from ....simulation.shared import HawkesExcitationState
from .payloads import mean_reversion_fitted_parameters_for_model
from .shared import PreparedModelRuntimeFactors

if TYPE_CHECKING:
    from ....model import JaniformModel


def prepare_model_mean_reversion_runtime_inputs(
    model: JaniformModel,
    *,
    runtime_factors: PreparedModelRuntimeFactors,
    interval_factor_grid: IntervalFactorGridContract,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float = 0.0,
) -> tuple[MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors, MeanReversionBoundaryState]:
    fitted_parameters = mean_reversion_fitted_parameters_for_model(model)

    runtime_interval_anchors = materialize_model_mean_reversion_runtime_interval_anchors(
        fitted_parameters=fitted_parameters,
        runtime_factors=runtime_factors,
        interval_factor_grid=interval_factor_grid,
        initial_excitation_state=initial_excitation_state,
        start_anchor_value=start_anchor_value,
    )

    initial_boundary_state = MeanReversionBoundaryState(
        observed_log_price=runtime_interval_anchors.start_anchor_value,
        anchor_value=runtime_interval_anchors.start_anchor_value,
        pressure_state=runtime_interval_anchors.start_pressure_state,
        deviation=0.0,
    )

    return fitted_parameters, runtime_interval_anchors, initial_boundary_state


def materialize_model_mean_reversion_runtime_interval_anchors(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    runtime_factors: PreparedModelRuntimeFactors,
    interval_factor_grid: IntervalFactorGridContract,
    initial_excitation_state: HawkesExcitationState,
    start_anchor_value: float,
) -> MeanReversionRuntimeIntervalAnchors:
    return materialize_endogenous_model_mean_reversion_runtime_interval_anchors(
        fitted_parameters=fitted_parameters,
        runtime_factors=runtime_factors,
        interval_factor_grid=interval_factor_grid,
        initial_excitation_state=initial_excitation_state,
        interval_alignment_policy=interval_factor_grid.interval_alignment_policy,
        start_anchor_value=start_anchor_value,
    )


def materialize_endogenous_model_mean_reversion_runtime_interval_anchors(
    *,
    fitted_parameters: MeanReversionFittedParameters,
    runtime_factors: PreparedModelRuntimeFactors,
    interval_factor_grid: IntervalFactorGridContract,
    initial_excitation_state: HawkesExcitationState,
    interval_alignment_policy: str,
    start_anchor_value: float,
) -> MeanReversionRuntimeIntervalAnchors:
    runtime_anchor_payload = native_module().materialize_endogenous_mean_reversion_runtime_interval_anchors(
        runtime_factors.kernel_parameters.to_payload(),
        runtime_factors.nig_shared_plus_shift_configuration.to_payload(),
        runtime_factors.news_process_configuration.to_payload(),
        runtime_factors.long_variance_parameters.to_payload(),
        initial_excitation_state.to_payload(),
        fitted_parameters.to_payload(),
        interval_alignment_policy,
        start_anchor_value,
        tuple(interval_factor_grid.seasonality_factor),
        tuple(interval_factor_grid.known_news_intensity),
        tuple(interval_factor_grid.surprise_news_intensity),
        tuple(
            nanoseconds_to_hours(interval_factor_grid.interval_duration_nanos)
            for _ in range(interval_factor_grid.interval_count)
        ),
    )

    return MeanReversionRuntimeIntervalAnchors.from_mapping(
        dict(cast(Mapping[str, Any], runtime_anchor_payload)),
        field_path="mean_reversion_runtime_interval_anchors",
    )
