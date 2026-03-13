from argparse import _SubParsersAction
from pathlib import Path

from .shared import (
    _add_optional_path_argument,
    _add_output_mode_argument,
    _add_required_json_path_argument,
)
from .._constants import (
    DEFAULT_CALENDAR_FIXTURE_PATH,
    DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH,
    CliOutputMode,
)
from ..commands.development import (
    _run_deterministic_regression_command,
    _run_open_closed_gap_leakage_regression_command,
)


def _build_developer_parser(subparsers: _SubParsersAction) -> None:
    parser = subparsers.add_parser(
        "dev",
        help="Developer-only utilities.",
        description="Developer-only utilities.",
    )
    parser.set_defaults(help_target=parser)

    developer_subparsers = parser.add_subparsers(dest="developer_command")

    regression_parser = developer_subparsers.add_parser(
        "regression",
        help="Regression validation suites.",
        description="Regression validation suites.",
    )
    regression_parser.set_defaults(help_target=regression_parser)

    regression_subparsers = regression_parser.add_subparsers(dest="regression_command")

    deterministic_parser = regression_subparsers.add_parser(
        "deterministic",
        help="Run deterministic calendar and hourly-factor regression harness.",
        description="Run deterministic calendar and hourly-factor regression harness.",
    )
    deterministic_parser.set_defaults(
        handler=_run_deterministic_regression_command,
        help_target=deterministic_parser,
    )

    _add_output_mode_argument(deterministic_parser, default_output_mode=CliOutputMode.HUMAN)
    _add_required_json_path_argument(
        deterministic_parser,
        flag_name="--calendar-registry-json",
        contract_label=(
            "a calendar registry JSON object consumed by the deterministic regression "
            "harness"
        ),
    )
    _add_required_json_path_argument(
        deterministic_parser,
        flag_name="--calendar-cases-json",
        contract_label="a calendar regression case document with a top-level `cases` list",
    )
    _add_required_json_path_argument(
        deterministic_parser,
        flag_name="--seasonality-cases-json",
        contract_label=(
            "an hourly-factor regression case document with a top-level "
            "`canonical_output_cases` list"
        ),
    )
    deterministic_parser.add_argument(
        "--seasonality-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for deterministic seasonality factor mismatch detection.",
    )
    deterministic_parser.add_argument(
        "--news-intensity-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for deterministic news-intensity mismatch detection.",
    )

    gap_parser = regression_subparsers.add_parser(
        "open-closed-gap-leakage",
        help="Run open and closed gap leakage regression harness.",
        description="Run open and closed gap leakage regression harness.",
    )
    gap_parser.set_defaults(
        handler=_run_open_closed_gap_leakage_regression_command,
        help_target=gap_parser,
    )

    _add_output_mode_argument(gap_parser, default_output_mode=CliOutputMode.HUMAN)
    _add_optional_path_argument(
        parser=gap_parser,
        flag_name="--calendar-fixture-path",
        default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
        help_text="Path to the calendar regression fixture JSON.",
    )
    _add_optional_path_argument(
        parser=gap_parser,
        flag_name="--gap-behavior-fixture-path",
        default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
        help_text="Path to the open and closed gap behavior fixture JSON.",
    )
    gap_parser.add_argument(
        "--component-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for component-level semantic mismatch detection.",
    )


__all__ = ["_build_developer_parser"]
