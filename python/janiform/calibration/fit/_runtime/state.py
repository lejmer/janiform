from __future__ import annotations

from datetime import datetime
from typing import TYPE_CHECKING, Optional

from ..state import CurrentStateEstimationRequest, CurrentStateEstimationResult

if TYPE_CHECKING:
    from ....model import JaniformModel


def estimate_model_state_from_observed_data(
    model: JaniformModel,
    *,
    state_time_utc: datetime,
    request: Optional[CurrentStateEstimationRequest],
    observed_price: Optional[float] = None,
) -> Optional[CurrentStateEstimationResult]:
    if request is None:
        return None

    effective_request = request
    if observed_price is not None and request.observed_price is None:
        effective_request = CurrentStateEstimationRequest(
            observed_tick_data=request.observed_tick_data,
            tick_data_column_mapping=request.tick_data_column_mapping,
            observed_price=observed_price,
        )

    return model.estimate_current_state(
        state_time_utc=state_time_utc,
        request=effective_request,
    )
