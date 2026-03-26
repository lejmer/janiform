import json
from typing import Any

from janiform.pricing import (
    VanillaOptionPricingExecutionInput,
    run_vanilla_option_pricing,
)

from .support._shared import (
    build_pricing_interval_factors,
    build_pricing_optional_output_selection,
    build_pricing_request,
)


def run_example() -> dict[str, Any]:
    request = build_pricing_request()
    execution_input = VanillaOptionPricingExecutionInput(
        request=request,
        interval_factors=build_pricing_interval_factors(),
        optional_output_selection=build_pricing_optional_output_selection(),
    )
    run_result = run_vanilla_option_pricing(execution_input)

    return {
        "request_summary": {
            "interval_delta": request.interval_delta,
            "payoff_type": request.payoff.payoff_type.value,
            "nu_h_known": execution_input.interval_factors[0].nu_h_known,
            "nu_h_surprise": execution_input.interval_factors[0].nu_h_surprise,
        },
        "run_result_summary": {
            "schema_version": run_result.schema_version,
            "price": round(run_result.response.price, 6),
            "forward_price": round(run_result.response.diagnostics.forward_price, 6),
            "run_diagnostics_included": run_result.run_diagnostics is not None,
            "calibration_hooks_included": run_result.calibration_hooks is not None,
        },
    }


def main() -> None:
    print(json.dumps(run_example(), indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
