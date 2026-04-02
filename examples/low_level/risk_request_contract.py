from datetime import UTC, datetime

from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import HourlyFactorContract, build_hourly_factor_contract
from janiform.risk import (
    SufficientStatisticsRiskRunRequest,
    serialize_sufficient_statistics_risk_run_request,
    validate_sufficient_statistics_risk_run_request,
)

from examples._contracts import build_risk_model_inputs
from examples._shared import calendar_registry_json


def main() -> None:
    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())
    hourly_factor_contract: HourlyFactorContract = build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
        seasonality_hourly_factors=[1.1, 0.9],
        news_intensity_hourly_factors=[0.2, 0.1],
    )
    request: SufficientStatisticsRiskRunRequest = SufficientStatisticsRiskRunRequest(
        seed=17,
        horizon_interval_count=2,
        interval_delta=1.0,
        model_inputs=build_risk_model_inputs(interval_count=2),
        hourly_factor_contract=hourly_factor_contract,
    )
    validated_request: SufficientStatisticsRiskRunRequest = validate_sufficient_statistics_risk_run_request(
        request.to_payload(),
    )
    serialized_request: str = serialize_sufficient_statistics_risk_run_request(validated_request)

    print(
        "validated request:",
        {
            "seed": validated_request.seed,
            "horizon_interval_count": validated_request.horizon_interval_count,
            "interval_delta": validated_request.interval_delta,
        },
    )
    print(f"serialized payload bytes: {len(serialized_request.encode('utf-8'))}")


if __name__ == "__main__":
    main()
