from __future__ import annotations

from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from ....calibration.market_data.price_coordinates import ResolvedPriceCoordinateSet


def resolve_price_coordinate_from_native_mapping(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> ResolvedPriceCoordinateSet:
    # Lazy import avoids a pricing to calibration import cycle at module load.
    from ....calibration.market_data.price_coordinates import ResolvedPriceCoordinateSet

    return ResolvedPriceCoordinateSet.from_mapping(
        raw_payload,
        field_path=field_path,
    )
