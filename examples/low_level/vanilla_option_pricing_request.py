from datetime import UTC, datetime

from janiform._timestamps import datetime_to_utc_nanos
from janiform.calendar import CalendarRegistry
from janiform.interval_factor_grids import IntervalFactorGridContract, build_interval_factor_grid
from janiform.pricing import (
    PricingIntervalSchedule,
    PricingIntervalScheduleMetadata,
    VanillaOptionPricingExecutionInput,
    VanillaOptionPricingOptionalOutputSelection,
    VanillaOptionPricingRequest,
    VanillaOptionPricingRunResult,
    run_vanilla_option_pricing,
)

from examples._contracts import (
    build_pricing_boundary_input,
    build_pricing_market_inputs,
    build_pricing_model_parameters,
    build_pricing_payoff,
)
from examples._shared import calendar_registry_json


def main() -> None:
    valuation_time_utc = datetime(2026, 2, 2, 14, 0, tzinfo=UTC)
    maturity_time_utc = datetime(2026, 2, 2, 15, 0, tzinfo=UTC)

    request: VanillaOptionPricingRequest = VanillaOptionPricingRequest(
        market_inputs=build_pricing_market_inputs(),
        payoff=build_pricing_payoff(),
        boundary_input=build_pricing_boundary_input(),
        model_parameters=build_pricing_model_parameters(),
    )

    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())

    interval_factor_grid: IntervalFactorGridContract = build_interval_factor_grid(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=valuation_time_utc,
        range_end_utc=maturity_time_utc,
        interval="1h",
        seasonality_factors=[1.0],
        news_intensity_factors=[0.0],
    )

    runtime_interval_schedule: PricingIntervalSchedule = PricingIntervalSchedule(
        valuation_time_utc_nanos=datetime_to_utc_nanos(valuation_time_utc),
        metadata=PricingIntervalScheduleMetadata(
            policy_id="explicit_interval_end_timestamps_utc.v1",
            derivation_policy="example.explicit_interval_end_timestamps_utc.v1",
            factor_assignment_policy="overlap_weighted_resampling_from_interval_factor_grid_source_intervals.v1",
        ),
        interval_end_timestamps_utc_nanos=(datetime_to_utc_nanos(maturity_time_utc),),
        interval_deltas_hours=(1.0,),
    )

    execution_input: VanillaOptionPricingExecutionInput = VanillaOptionPricingExecutionInput(
        request=request,
        runtime_interval_schedule=runtime_interval_schedule,
        interval_factor_grid=interval_factor_grid,
        optional_output_selection=VanillaOptionPricingOptionalOutputSelection(
            include_transform_diagnostics=True,
            include_integration_quality_diagnostics=True,
            include_calibration_hooks=True,
        ),
    )

    run_result: VanillaOptionPricingRunResult = run_vanilla_option_pricing(execution_input)

    print(
        "request summary:",
        {
            "spot": request.market_inputs.spot,
            "strike": request.market_inputs.single_strike,
            "interval_delta": request.interval_delta,
        },
    )
    print(f"option price: {run_result.response.price:.6f}")


if __name__ == "__main__":
    main()
