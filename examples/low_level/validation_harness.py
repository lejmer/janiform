from janiform.calendar import CalendarRegistry
from janiform.validation import (
    DeterministicRegressionHarnessReport,
    run_deterministic_regression_harness,
)

from examples._shared import (
    calendar_registry_json,
    calendar_regression_cases,
)


def main() -> None:
    report: DeterministicRegressionHarnessReport = run_deterministic_regression_harness(
        calendar_registry=CalendarRegistry(calendar_registry_json()),
        calendar_cases=calendar_regression_cases(),
        seasonality_cases=[],
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
