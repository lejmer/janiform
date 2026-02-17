from datetime import datetime

from .calendar import CalendarRegistry
from .hourly_factors import (
    HourlyFactorContract,
    HourlyNewsIntensityProviderSource,
    HourlySeasonalityProviderSource,
    provide_hourly_factor_contract,
)


def load_hourly_factor_contract_for_pricing(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
    seasonality_source: HourlySeasonalityProviderSource,
    news_intensity_source: HourlyNewsIntensityProviderSource,
) -> HourlyFactorContract:
    """Load interval-aligned hourly factors for the pricing path."""

    return provide_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code=exchange_code,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
        seasonality_source=seasonality_source,
        news_intensity_source=news_intensity_source,
    )


__all__ = [
    "load_hourly_factor_contract_for_pricing",
]
