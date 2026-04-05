from datetime import UTC, datetime

from janiform.calendar import CalendarRegistry
from janiform.hourly_factors import HourlyFactorContract, build_hourly_factor_contract
from janiform.pricing import (
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
    request: VanillaOptionPricingRequest = VanillaOptionPricingRequest(
        market_inputs=build_pricing_market_inputs(),
        payoff=build_pricing_payoff(),
        boundary_input=build_pricing_boundary_input(),
        model_parameters=build_pricing_model_parameters(),
    )

    calendar_registry: CalendarRegistry = CalendarRegistry(calendar_registry_json())
    hourly_factor_contract: HourlyFactorContract = build_hourly_factor_contract(
        calendar_registry=calendar_registry,
        exchange_code="XNYS",
        range_start_utc=datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
        range_end_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        seasonality_hourly_factors=[1.0],
        news_intensity_hourly_factors=[0.0],
    )

    execution_input: VanillaOptionPricingExecutionInput = VanillaOptionPricingExecutionInput(
        request=request,
        hourly_factor_contract=hourly_factor_contract,
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
            "strike": request.market_inputs.strike,
            "interval_delta": request.interval_delta,
        },
    )
    print(f"option price: {run_result.response.price:.6f}")


if __name__ == "__main__":
    main()
