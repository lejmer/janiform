"""Public current-state estimation contracts for fitted Janiform models."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import TYPE_CHECKING, Any, Optional

from ..._native_api import native_module
from ..._timestamps import datetime_to_utc_nanos
from ...contracts import (
    ContractModel,
    MeanReversionBoundaryState,
    require_datetime_utc,
    require_positive_finite_number,
    require_sequence,
)
from ...contracts.base import require_only_supported_fields
from ...simulation import HawkesExcitationState, SeededTickEventSide
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error

if TYPE_CHECKING:
    from .model import JaniformModel


@dataclass(frozen=True, slots=True)
class CurrentStateObservation(ContractModel):
    """One observed signed trade used to estimate the current fitted-model state."""

    timestamp_utc: datetime
    trade_price: float
    trade_side: SeededTickEventSide

    def __post_init__(self) -> None:
        require_datetime_utc(
            self.timestamp_utc,
            "timestamp_utc",
            CALIBRATION_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"trade_price": self.trade_price},
            "trade_price",
            "trade_price",
            CALIBRATION_ERROR_FACTORY,
        )
        if not isinstance(self.trade_side, SeededTickEventSide):
            raise _calibration_error("trade_side", "must be a SeededTickEventSide")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "current_state_observation",
    ) -> CurrentStateObservation:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("timestamp_utc", "trade_price", "trade_side"),
            field_path=field_path,
            error_factory=CALIBRATION_ERROR_FACTORY,
        )

        raw_trade_side = raw_payload.get("trade_side")
        if not isinstance(raw_trade_side, str):
            raise _calibration_error(f"{field_path}.trade_side", "must be a string")

        try:
            trade_side = SeededTickEventSide(raw_trade_side)
        except ValueError as e:
            raise _calibration_error(
                f"{field_path}.trade_side",
                "must be one of: buy, sell",
            ) from e

        return CurrentStateObservation(
            timestamp_utc=require_datetime_utc(
                raw_payload.get("timestamp_utc"),
                f"{field_path}.timestamp_utc",
                CALIBRATION_ERROR_FACTORY,
            ),
            trade_price=require_positive_finite_number(
                raw_payload,
                "trade_price",
                f"{field_path}.trade_price",
                CALIBRATION_ERROR_FACTORY,
            ),
            trade_side=trade_side,
        )


@dataclass(frozen=True, slots=True)
class CurrentStateEstimationRequest(ContractModel):
    """Observed data required to estimate the current fitted-model runtime state."""

    observed_trades: tuple[CurrentStateObservation, ...]
    observed_price: Optional[float] = None

    def __post_init__(self) -> None:
        if len(self.observed_trades) == 0:
            raise _calibration_error("observed_trades", "must contain at least one observation")

        previous_timestamp_utc: Optional[datetime] = None
        for observation_index, observation in enumerate(self.observed_trades):
            if not isinstance(observation, CurrentStateObservation):
                raise _calibration_error(
                    f"observed_trades[{observation_index}]",
                    "must be a CurrentStateObservation",
                )
            if (
                previous_timestamp_utc is not None
                and observation.timestamp_utc < previous_timestamp_utc
            ):
                raise _calibration_error(
                    f"observed_trades[{observation_index}].timestamp_utc",
                    "must be greater than or equal to the previous timestamp",
                )

            previous_timestamp_utc = observation.timestamp_utc

        if self.observed_price is not None:
            require_positive_finite_number(
                {"observed_price": self.observed_price},
                "observed_price",
                "observed_price",
                CALIBRATION_ERROR_FACTORY,
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str = "current_state_estimation_request",
    ) -> CurrentStateEstimationRequest:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        require_only_supported_fields(
            raw_payload,
            supported_fields=("observed_trades", "observed_price"),
            field_path=field_path,
            error_factory=CALIBRATION_ERROR_FACTORY,
        )

        raw_observed_trades = require_sequence(
            raw_payload,
            "observed_trades",
            f"{field_path}.observed_trades",
            CALIBRATION_ERROR_FACTORY,
        )
        observed_price = (
            require_positive_finite_number(
                raw_payload,
                "observed_price",
                f"{field_path}.observed_price",
                CALIBRATION_ERROR_FACTORY,
            )
            if "observed_price" in raw_payload and raw_payload["observed_price"] is not None
            else None
        )

        return CurrentStateEstimationRequest(
            observed_trades=tuple(
                CurrentStateObservation.from_mapping(
                    raw_observation,
                    field_path=f"{field_path}.observed_trades[{observation_index}]",
                )
                for observation_index, raw_observation in enumerate(raw_observed_trades)
            ),
            observed_price=observed_price,
        )


@dataclass(frozen=True, slots=True)
class CurrentStateEstimationResult(ContractModel):
    """Current runtime state derived from a fitted model and observed signed trades."""

    hawkes_excitation_state: HawkesExcitationState
    mean_reversion_boundary_state: MeanReversionBoundaryState

    def __post_init__(self) -> None:
        if not isinstance(self.hawkes_excitation_state, HawkesExcitationState):
            raise _calibration_error(
                "hawkes_excitation_state",
                "must be a HawkesExcitationState",
            )
        if not isinstance(self.mean_reversion_boundary_state, MeanReversionBoundaryState):
            raise _calibration_error(
                "mean_reversion_boundary_state",
                "must be a MeanReversionBoundaryState",
            )


def estimate_current_state_for_model(
    model: JaniformModel,
    *,
    state_time_utc: datetime,
    request: CurrentStateEstimationRequest,
) -> CurrentStateEstimationResult:
    from . import model as fit_model_module

    fit_model_module._require_timezone_aware_datetime(
        state_time_utc,
        field_path="state_time_utc",
    )
    observed_price = (
        request.observed_price
        if request.observed_price is not None
        else request.observed_trades[-1].trade_price
    )
    observed_log_price = fit_model_module._pricing_observed_log_price_from_spot(
        model,
        spot=observed_price,
    )

    native_payload = native_module().estimate_current_state_from_observed_data(
        fit_model_module._hawkes_kernel_parameters(model).to_payload(),
        fit_model_module._mean_reversion_fitted_parameters(model).to_payload(),
        datetime_to_utc_nanos(state_time_utc),
        observed_log_price,
        tuple(
            {
                "timestamp_utc_nanos": datetime_to_utc_nanos(observation.timestamp_utc),
                "trade_price": observation.trade_price,
                "trade_side": observation.trade_side.value,
            }
            for observation in request.observed_trades
        ),
    )

    return CurrentStateEstimationResult(
        hawkes_excitation_state=HawkesExcitationState.from_mapping(
            dict(native_payload["hawkes_excitation_state"]),
            field_path="hawkes_excitation_state",
        ),
        mean_reversion_boundary_state=MeanReversionBoundaryState.from_mapping(
            dict(native_payload["mean_reversion_boundary_state"]),
            field_path="mean_reversion_boundary_state",
        ),
    )


__all__ = [
    "CurrentStateEstimationRequest",
    "CurrentStateEstimationResult",
    "CurrentStateObservation",
]
