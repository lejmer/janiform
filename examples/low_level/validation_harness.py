from typing import Any

from janiform.calendar import CalendarRegistry
from janiform.validation import (
    DeterministicRegressionHarnessReport,
    run_deterministic_regression_harness,
)

from examples._shared import (
    calendar_registry_json,
    calendar_regression_cases,
    hourly_factor_fixture_document,
    hourly_factor_regression_canonical_cases,
)


def main() -> None:
    threshold_configuration: dict[str, Any] = hourly_factor_fixture_document()[
        "threshold_configuration"
    ]
    tolerance_config: dict[str, Any] = threshold_configuration["tolerance_config"]

    report: DeterministicRegressionHarnessReport = run_deterministic_regression_harness(
        calendar_registry=CalendarRegistry(calendar_registry_json()),
        calendar_cases=calendar_regression_cases(),
        seasonality_cases=hourly_factor_regression_canonical_cases(),
        seasonality_absolute_tolerance=tolerance_config["s_h_absolute_tolerance"],
        news_intensity_absolute_tolerance=tolerance_config["nu_h_absolute_tolerance"],
    )

    print(
        "deterministic regression summary:",
        {
            "total_case_count": report["summary"]["total_case_count"],
            "passed_case_count": report["summary"]["passed_case_count"],
            "failed_case_count": report["summary"]["failed_case_count"],
        },
    )


if __name__ == "__main__":
    main()
