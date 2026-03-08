import json
import os
import sys
from argparse import ArgumentParser, Namespace
from collections.abc import Callable, Mapping
from datetime import timedelta
from enum import IntEnum, StrEnum
from pathlib import Path
from typing import Any, NoReturn, Optional, cast

from . import __version__
from .calibration import (
    CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION,
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationDuplicateRecordPolicy,
    CalibrationExecutionMode,
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationStagedExecutionRuntimeResources,
    CalibrationTickPriceInputPolicy,
    MarketDataCalibrationRequest,
    PreparedMarketDataCalibration,
    StagedCalibrationExecutionInput,
    prepare_market_data_calibration,
    run_staged_calibration,
    validate_market_data_calibration_request,
    validate_staged_calibration_execution_input,
)
from .calendar import (
    CalendarInvalidTimestampError,
    CalendarInvalidTimezoneError,
    CalendarMissingExchangeError,
    CalendarRegistry,
    CalendarRegistryLoadError,
)
from .errors import (
    ContractValidationFailure,
    JaniformError,
    JaniformErrorDetails,
    JaniformErrorCategory,
    JaniformIntegrationError,
    JaniformValidationError,
)
from .hourly_factors import (
    HourlyNewsIntensityProviderExplicitSource,
    HourlySeasonalityProviderExplicitSource,
)
from .pricing import run_vanilla_option_pricing, validate_vanilla_option_pricing_execution_input
from .seasonality import SeasonalityInputValidationError
from .simulation import (
    SeededTickSimulationHourlyFactorSummary,
    SeededTickSimulationRequest,
    load_hourly_factor_contract_for_simulation,
    run_seeded_tick_event_simulation,
)
from .validation import run_open_closed_gap_leakage_regression_harness

DEFAULT_CALENDAR_FIXTURE_PATH: str = "tests/fixtures/calendar/calendar_service_regression_matrix.json"
DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH: str = (
    "tests/fixtures/gap_behavior/open_closed_gap_behavior_scenarios.json"
)
CLI_PATH_METAVAR = "PATH"
CLI_DIRECTORY_METAVAR = "DIRECTORY"
CLI_REPOSITORY_RELATIVE_PATH_POLICY = "Relative paths are resolved from the repository root."
CLI_CALIBRATION_EXECUTION_MODE_CHOICES: tuple[str, str] = (
    CalibrationExecutionMode.DISK.value,
    CalibrationExecutionMode.MEMORY.value,
)
CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES: tuple[str, str] = (
    CalibrationInputFileFormat.CSV.value,
    CalibrationInputFileFormat.PARQUET.value,
)
CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES: tuple[str, str, str] = (
    CalibrationInputFileFormat.CSV.value,
    CalibrationInputFileFormat.PARQUET.value,
    CalibrationInputFileFormat.JSON.value,
)
CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES: tuple[str, str, str] = (
    CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY.value,
    CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE.value,
    CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE.value,
)
CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES: tuple[str, str, str] = (
    CalibrationDuplicateRecordPolicy.ERROR.value,
    CalibrationDuplicateRecordPolicy.KEEP_FIRST.value,
    CalibrationDuplicateRecordPolicy.KEEP_LAST.value,
)
CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES: tuple[str, str, str, str] = (
    CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN.value,
    CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN.value,
    CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT.value,
    CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN.value,
)
CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES: tuple[str, str, str] = (
    CalibrationOptionQuoteInputPolicy.MID_PRICE_COLUMN.value,
    CalibrationOptionQuoteInputPolicy.BID_ASK_MIDPOINT.value,
    CalibrationOptionQuoteInputPolicy.IMPLIED_VOLATILITY_COLUMN.value,
)
CLI_CALIBRATION_EXECUTION_MODE_HELP = (
    "Calibration execution mode override. Precedence: --execution-mode, then the CLI default "
    "disk. This overrides request.execution_mode from --execution-input-json."
)
CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP = (
    "Calibration execution mode recorded in the market-data request. The CLI default is disk so "
    "automation-friendly runs keep persistence explicit."
)
CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT = "simulation.seeded_ticks"
CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID = "cli.validation.simulation.seeded_ticks.invalid_input"
CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID = "cli.internal.simulation.seeded_ticks.failed"
CLI_PRICING_VANILLA_OPTION_ENTRYPOINT = "pricing.vanilla_option"
CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID = "cli.validation.pricing.vanilla_option.invalid_input"
CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID = "cli.internal.pricing.vanilla_option.failed"
CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT = "calibration.market_data"
CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID = "cli.validation.calibration.market_data.invalid_input"
CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID = "cli.internal.calibration.market_data.failed"
CLI_CALIBRATION_STAGED_ENTRYPOINT = "calibration.staged"
CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID = "cli.validation.calibration.staged.invalid_input"
CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID = "cli.internal.calibration.staged.failed"
CLI_OPEN_CLOSED_GAP_ENTRYPOINT = "dev.regression.open_closed_gap_leakage"
CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID = "cli.validation.open_closed_gap_leakage.invalid_input"
CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID = "cli.internal.open_closed_gap_leakage.failed"
CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION = "cli.calibration.market_data.summary.v1"
CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY = "user_facing_workflow_metadata_only.v1"
CLI_COMMAND_CONFIG_SCHEMA_VERSION = "cli.command_config.v1"
CLI_ARGUMENT_RESOLUTION_ENTRYPOINT = "cli.argument_resolution"
CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID = "cli.validation.arguments.invalid_input"
CLI_ARGUMENT_PRECEDENCE_POLICY = "explicit_flags_override_environment_override_config_override_defaults.v1"
CLI_CONFIG_JSON_ENV_VAR = "JANIFORM_CLI_CONFIG_JSON"
CLI_OUTPUT_ENV_VAR = "JANIFORM_CLI_OUTPUT"
CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR = "JANIFORM_CLI_CALIBRATION_EXECUTION_MODE"
CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR = "JANIFORM_CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY"
CLI_OUTPUT_MODE_HELP = (
    "Output mode. Use json for deterministic machine-readable output and human for concise "
    "operator-oriented text."
)
CLI_COMMAND_CONFIG_HELP = (
    "Path to a cli.command_config.v1 JSON object for this command. The arguments object uses "
    "long flag names without the leading --. Precedence: explicit flags override environment "
    "variables, which override config-file arguments."
)
CLI_PRIMARY_COMMAND_TREE: dict[str, dict[str, dict[str, dict[str, Any]]]] = {
    "simulation": {
        "seeded-ticks": {},
    },
    "calibration": {
        "market-data": {},
        "staged": {},
    },
    "pricing": {
        "vanilla-option": {},
    },
}
CLI_PRIMARY_COMMAND_IDENTIFIERS: dict[tuple[str, ...], str] = {
    ("simulation", "seeded-ticks"): "simulation.seeded-ticks",
    ("calibration", "market-data"): "calibration.market-data",
    ("calibration", "staged"): "calibration.staged",
    ("pricing", "vanilla-option"): "pricing.vanilla-option",
}


class CliExitCode(IntEnum):
    SUCCESS = 0
    VALIDATION_ERROR = 2
    RUNTIME_ERROR = 3
    INTERNAL_ERROR = 4


class CliOutputMode(StrEnum):
    HUMAN = 'human'
    JSON = 'json'


CLI_OUTPUT_MODE_CHOICES: tuple[str, str] = (
    CliOutputMode.HUMAN.value,
    CliOutputMode.JSON.value,
)


def _add_output_mode_argument(
    parser: ArgumentParser,
    *,
    default_output_mode: CliOutputMode,
) -> None:
    parser.add_argument(
        "--output",
        choices=CLI_OUTPUT_MODE_CHOICES,
        default=default_output_mode.value,
        help=f"{CLI_OUTPUT_MODE_HELP} Default: {default_output_mode.value}.",
    )


def _add_command_config_argument(parser: ArgumentParser) -> None:
    parser.add_argument(
        "--config-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=f"{CLI_COMMAND_CONFIG_HELP} {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )


def _add_required_json_path_argument(
    parser: ArgumentParser,
    *,
    flag_name: str,
    contract_label: str,
    semantic_note: str | None = None,
) -> None:
    help_segments: list[str] = [f"Path to {contract_label}.", CLI_REPOSITORY_RELATIVE_PATH_POLICY]
    if semantic_note is not None:
        help_segments.append(semantic_note)

    parser.add_argument(
        flag_name,
        type=Path,
        required=True,
        metavar=CLI_PATH_METAVAR,
        help=" ".join(help_segments),
    )


def _add_optional_path_argument(
    parser: ArgumentParser,
    *,
    flag_name: str,
    help_text: str,
    default: Path | None = None,
    directory: bool = False,
) -> None:
    parser.add_argument(
        flag_name,
        type=Path,
        default=default,
        metavar=CLI_DIRECTORY_METAVAR if directory else CLI_PATH_METAVAR,
        help=f"{help_text} {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )


def _build_simulation_parser(subparsers) -> None:
    simulation_parser = subparsers.add_parser(
        "simulation",
        help="Simulation workflows.",
        description="Simulation workflows.",
    )
    simulation_parser.set_defaults(help_target=simulation_parser)
    simulation_subparsers = simulation_parser.add_subparsers(dest="simulation_command")

    simulation_seeded_ticks_parser = simulation_subparsers.add_parser(
        "seeded-ticks",
        help="Seeded tick event simulation workflow.",
        description="Seeded tick event simulation workflow.",
    )
    simulation_seeded_ticks_parser.set_defaults(
        handler=_run_simulation_seeded_ticks_command,
        help_target=simulation_seeded_ticks_parser,
    )

    _add_command_config_argument(simulation_seeded_ticks_parser)
    _add_output_mode_argument(
        simulation_seeded_ticks_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        simulation_seeded_ticks_parser,
        flag_name="--request-json",
        contract_label="a seeded-tick simulation request JSON object",
        semantic_note=(
            "The request summary must preserve open-jump versus closed-gap hourly factors."
        ),
    )
    _add_required_json_path_argument(
        simulation_seeded_ticks_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used to rehydrate hourly factors",
    )


def _add_market_data_calibration_arguments(calibration_market_data_parser: ArgumentParser) -> None:
    _add_command_config_argument(calibration_market_data_parser)
    _add_output_mode_argument(
        calibration_market_data_parser,
        default_output_mode=CliOutputMode.JSON,
    )

    calibration_market_data_parser.add_argument(
        "--run-id",
        required=True,
        help="Stable run identifier recorded in the market-data request payload.",
    )
    calibration_market_data_parser.add_argument(
        "--workflow-scope",
        required=True,
        choices=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
        help=(
            "High-level calibration scope. Use seasonality_only for activity-only estimation, "
            "full_without_option_surface for full calibration from tick data only, and "
            "full_with_option_surface when option-surface data is supplied."
        ),
    )
    calibration_market_data_parser.add_argument(
        "--tick-data-path",
        type=Path,
        required=True,
        metavar=CLI_PATH_METAVAR,
        help=f"Path to the input tick-data file. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )
    calibration_market_data_parser.add_argument(
        "--tick-data-format",
        required=True,
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Tick-data file format.",
    )
    calibration_market_data_parser.add_argument(
        "--tick-data-provenance-label",
        required=True,
        help="Stable provenance label recorded for the tick-data source.",
    )
    calibration_market_data_parser.add_argument(
        "--tick-timestamp-column",
        required=True,
        help="Tick-data timestamp column name.",
    )
    calibration_market_data_parser.add_argument(
        "--tick-price-input-policy",
        choices=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
        help="Explicit tick-price normalization policy for non-seasonality workflows.",
    )
    calibration_market_data_parser.add_argument("--tick-price-column", help="Tick-data explicit price column.")
    calibration_market_data_parser.add_argument(
        "--tick-trade-price-column",
        help="Tick-data trade price column.",
    )
    calibration_market_data_parser.add_argument("--tick-bid-column", help="Tick-data bid column.")
    calibration_market_data_parser.add_argument("--tick-ask-column", help="Tick-data ask column.")
    calibration_market_data_parser.add_argument(
        "--tick-close-price-column",
        help="Tick-data close price column.",
    )
    calibration_market_data_parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state and hourly factors.",
    )
    calibration_market_data_parser.add_argument(
        "--session-definition-path",
        type=Path,
        required=True,
        metavar=CLI_PATH_METAVAR,
        help=f"Path to the session-definition file. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )
    calibration_market_data_parser.add_argument(
        "--session-definition-format",
        required=True,
        choices=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
        help="Session-definition file format.",
    )
    calibration_market_data_parser.add_argument(
        "--session-definition-provenance-label",
        required=True,
        help="Stable provenance label recorded for the session-definition source.",
    )
    calibration_market_data_parser.add_argument(
        "--session-timezone",
        required=True,
        help="Timezone used to interpret session-definition timestamps.",
    )

    _add_optional_path_argument(
        parser=calibration_market_data_parser,
        flag_name="--known-news-path",
        help_text="Optional path to the known-news calendar file.",
    )

    calibration_market_data_parser.add_argument(
        "--known-news-format",
        choices=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
        help="Known-news calendar file format.",
    )
    calibration_market_data_parser.add_argument(
        "--known-news-provenance-label",
        help="Stable provenance label recorded for the known-news source.",
    )
    calibration_market_data_parser.add_argument(
        "--known-news-timezone",
        help="Optional timezone for known-news timestamps.",
    )
    calibration_market_data_parser.add_argument(
        "--known-news-timestamp-column",
        help="Known-news timestamp column name for csv or parquet inputs.",
    )
    calibration_market_data_parser.add_argument(
        "--known-news-event-id-column",
        help="Optional known-news event identifier column name.",
    )

    _add_optional_path_argument(
        parser=calibration_market_data_parser,
        flag_name="--option-surface-path",
        help_text="Optional path to the option-surface file.",
    )

    calibration_market_data_parser.add_argument(
        "--option-surface-format",
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Option-surface file format.",
    )
    calibration_market_data_parser.add_argument(
        "--option-surface-provenance-label",
        help="Stable provenance label recorded for the option-surface source.",
    )
    calibration_market_data_parser.add_argument(
        "--option-quote-input-policy",
        choices=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
        help="Explicit option-quote normalization policy.",
    )
    calibration_market_data_parser.add_argument(
        "--option-quote-timestamp-column",
        help="Option-surface quote timestamp column name.",
    )
    calibration_market_data_parser.add_argument(
        "--option-expiry-column",
        help="Option-surface expiry column name.",
    )
    calibration_market_data_parser.add_argument("--option-strike-column", help="Option-surface strike column name.")
    calibration_market_data_parser.add_argument("--option-type-column", help="Option-surface option type column name.")
    calibration_market_data_parser.add_argument(
        "--option-mid-price-column",
        help="Option-surface mid-price column name.",
    )
    calibration_market_data_parser.add_argument("--option-bid-column", help="Option-surface bid column name.")
    calibration_market_data_parser.add_argument("--option-ask-column", help="Option-surface ask column name.")
    calibration_market_data_parser.add_argument(
        "--option-implied-volatility-column",
        help="Option-surface implied volatility column name.",
    )
    calibration_market_data_parser.add_argument(
        "--timestamp-timezone",
        required=True,
        help="Timezone used to normalize tick-data timestamps before staged translation.",
    )
    calibration_market_data_parser.add_argument(
        "--duplicate-record-policy",
        choices=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
        default=CalibrationDuplicateRecordPolicy.ERROR.value,
        help="Duplicate-record normalization policy recorded in provenance.",
    )
    calibration_market_data_parser.add_argument(
        "--disable-timestamp-sort",
        action="store_true",
        help="Record that source data should not be re-sorted by timestamp before translation.",
    )
    calibration_market_data_parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.DISK.value,
        help=CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
    )

    _add_optional_path_argument(
        parser=calibration_market_data_parser,
        flag_name="--disk-checkpoint-directory",
        directory=True,
        help_text=(
            "Disk checkpoint directory recorded in output_preferences. Required when "
            "--execution-mode disk is selected."
        ),
    )

    calibration_market_data_parser.add_argument(
        "--emit-derived-staged-execution-input",
        action="store_true",
        help="Request emission of the derived staged execution input for replay or debugging.",
    )
    calibration_market_data_parser.add_argument(
        "--omit-input-provenance-report",
        action="store_true",
        help="Disable provenance-report emission in output_preferences.",
    )


def _add_staged_calibration_arguments(calibration_staged_parser: ArgumentParser) -> None:
    _add_command_config_argument(calibration_staged_parser)
    _add_output_mode_argument(
        calibration_staged_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        calibration_staged_parser,
        flag_name="--execution-input-json",
        contract_label="a staged calibration execution-input JSON object",
        semantic_note=(
            "The execution input must provide explicit known-news return samples and raw "
            "close-open boundary observations for internal gap detection."
        ),
    )
    _add_required_json_path_argument(
        calibration_staged_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object consumed by the staged calibration runner",
    )

    calibration_staged_parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.DISK.value,
        help=CLI_CALIBRATION_EXECUTION_MODE_HELP,
    )

    _add_optional_path_argument(
        parser=calibration_staged_parser,
        flag_name="--disk-checkpoint-directory",
        directory=True,
        help_text=(
            "Checkpoint directory override. Use only with --execution-mode disk. When provided, "
            "this overrides runtime_resources.disk_checkpoint_directory from --execution-input-json."
        ),
    )


def _build_calibration_parser(subparsers) -> None:
    calibration_parser = subparsers.add_parser(
        "calibration",
        help="Calibration workflows.",
        description="Calibration workflows.",
    )
    calibration_parser.set_defaults(help_target=calibration_parser)
    calibration_subparsers = calibration_parser.add_subparsers(dest="calibration_command")

    calibration_market_data_parser = calibration_subparsers.add_parser(
        "market-data",
        help="Market-data calibration request workflow.",
        description=(
            "Market-data calibration request workflow. "
            "This command validates file-driven inputs and emits a user-facing "
            "market-data calibration summary with preprocessing diagnostics and provenance."
        ),
    )
    calibration_market_data_parser.set_defaults(
        handler=_run_calibration_market_data_command,
        help_target=calibration_market_data_parser,
    )

    _add_market_data_calibration_arguments(calibration_market_data_parser)

    calibration_staged_parser = calibration_subparsers.add_parser(
        "staged",
        help="Staged calibration workflow.",
        description="Staged calibration workflow.",
    )
    calibration_staged_parser.set_defaults(
        handler=_run_calibration_staged_command,
        help_target=calibration_staged_parser,
    )

    _add_staged_calibration_arguments(calibration_staged_parser)


def _build_pricing_parser(subparsers) -> None:
    pricing_parser = subparsers.add_parser(
        "pricing",
        help="Pricing workflows.",
        description="Pricing workflows.",
    )
    pricing_parser.set_defaults(help_target=pricing_parser)
    pricing_subparsers = pricing_parser.add_subparsers(dest="pricing_command")

    pricing_vanilla_option_parser = pricing_subparsers.add_parser(
        "vanilla-option",
        help="Vanilla option pricing workflow.",
        description="Vanilla option pricing workflow.",
    )
    pricing_vanilla_option_parser.set_defaults(
        handler=_run_pricing_vanilla_option_command,
        help_target=pricing_vanilla_option_parser,
    )
    _add_command_config_argument(pricing_vanilla_option_parser)
    _add_output_mode_argument(
        pricing_vanilla_option_parser,
        default_output_mode=CliOutputMode.JSON,
    )

    _add_required_json_path_argument(
        pricing_vanilla_option_parser,
        flag_name="--execution-input-json",
        contract_label="a vanilla-option pricing execution-input JSON object",
        semantic_note="Interval factors keep known and surprise news intensity separate.",
    )


def _build_developer_parser(subparsers) -> None:
    developer_parser = subparsers.add_parser(
        "dev",
        help="Developer-only utilities.",
        description="Developer-only utilities.",
    )
    developer_parser.set_defaults(help_target=developer_parser)
    developer_subparsers = developer_parser.add_subparsers(dest="developer_command")

    regression_parser = developer_subparsers.add_parser(
        "regression",
        help="Regression validation suites.",
        description="Regression validation suites.",
    )
    regression_parser.set_defaults(help_target=regression_parser)
    regression_subparsers = regression_parser.add_subparsers(dest="regression_command")

    gap_leakage_parser = regression_subparsers.add_parser(
        "open-closed-gap-leakage",
        help="Run open and closed gap leakage regression harness.",
        description="Run open and closed gap leakage regression harness.",
    )
    gap_leakage_parser.set_defaults(
        handler=_run_open_closed_gap_leakage_regression_command,
        help_target=gap_leakage_parser,
    )

    _add_output_mode_argument(
        gap_leakage_parser,
        default_output_mode=CliOutputMode.HUMAN,
    )
    _add_optional_path_argument(
        parser=gap_leakage_parser,
        flag_name="--calendar-fixture-path",
        default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
        help_text="Path to the calendar regression fixture JSON.",
    )
    _add_optional_path_argument(
        parser=gap_leakage_parser,
        flag_name="--gap-behavior-fixture-path",
        default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
        help_text="Path to the open and closed gap behavior fixture JSON.",
    )

    gap_leakage_parser.add_argument(
        "--component-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for component-level semantic mismatch detection.",
    )


def _build_parser() -> ArgumentParser:
    parser: ArgumentParser = ArgumentParser(
        prog="janiform",
        description="Command line interface for Janiform workflows and developer utilities.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.set_defaults(
        command=None,
        handler=None,
        help_target=parser,
    )

    subparsers = parser.add_subparsers(dest="command")

    _build_simulation_parser(subparsers)
    _build_calibration_parser(subparsers)
    _build_pricing_parser(subparsers)
    _build_developer_parser(subparsers)

    return parser


def _parse_cli_arguments(
    arguments: list[str] | None = None,
    *,
    environment: Mapping[str, str] | None = None,
) -> Namespace:
    parser = _build_parser()

    raw_arguments = list(sys.argv[1:] if arguments is None else arguments)
    resolved_arguments = _apply_cli_argument_defaults(
        parser,
        raw_arguments,
        environment=os.environ if environment is None else environment,
    )

    return parser.parse_args(resolved_arguments)


def _apply_cli_argument_defaults(
    parser: ArgumentParser,
    raw_arguments: list[str],
    *,
    environment: Mapping[str, str],
) -> list[str]:
    command_path = _extract_primary_command_path(raw_arguments)
    if command_path not in CLI_PRIMARY_COMMAND_IDENTIFIERS:
        return raw_arguments

    command_parser = _resolve_command_parser(parser, command_path)
    if command_parser is None:
        return raw_arguments

    repository_root = Path(__file__).resolve().parents[2]
    config_path = _resolve_command_config_path(
        command_path=command_path,
        raw_arguments=raw_arguments,
        environment=environment,
        repository_root=repository_root,
    )

    config_arguments = (
        _build_command_config_argument_vector(
            command_parser,
            command_path=command_path,
            config_path=config_path,
        )
        if config_path is not None
        else []
    )
    environment_arguments = _build_environment_argument_vector(
        command_path=command_path,
        environment=environment,
    )
    if not config_arguments and not environment_arguments:
        return raw_arguments

    return [
        *raw_arguments[: len(command_path)],
        *config_arguments,
        *environment_arguments,
        *raw_arguments[len(command_path) :],
    ]


def _extract_primary_command_path(raw_arguments: list[str]) -> tuple[str, ...]:
    command_path: list[str] = []
    subtree: dict[str, dict[str, Any]] = CLI_PRIMARY_COMMAND_TREE
    for raw_argument in raw_arguments:
        if raw_argument.startswith("-"):
            break
        if raw_argument not in subtree:
            break
        command_path.append(raw_argument)
        subtree = subtree[raw_argument]

    return tuple(command_path)


def _resolve_command_parser(
    parser: ArgumentParser,
    command_path: tuple[str, ...],
) -> ArgumentParser | None:
    current_parser = parser
    for command_segment in command_path:
        next_parser: ArgumentParser | None = None
        for action in current_parser._actions:
            action_choices = getattr(action, "choices", None)
            if isinstance(action_choices, dict) and command_segment in action_choices:
                next_parser = cast(ArgumentParser, action_choices[command_segment])
                break

        if next_parser is None:
            return None

        current_parser = next_parser

    return current_parser


def _resolve_command_config_path(
    *,
    command_path: tuple[str, ...],
    raw_arguments: list[str],
    environment: Mapping[str, str],
    repository_root: Path,
) -> Path | None:
    explicit_config_path = _extract_explicit_option_value(
        raw_arguments[len(command_path) :],
        flag_name="--config-json",
    )
    if explicit_config_path is not None:
        return _resolve_path_from_repository_root(repository_root, Path(explicit_config_path))

    environment_config_path = environment.get(CLI_CONFIG_JSON_ENV_VAR)
    if environment_config_path is None:
        return None
    if not environment_config_path.strip():
        _raise_cli_argument_resolution_error(
            field_path=CLI_CONFIG_JSON_ENV_VAR,
            message="environment variable must be non-empty when provided",
        )

    return _resolve_path_from_repository_root(repository_root, Path(environment_config_path))


def _extract_explicit_option_value(arguments: list[str], *, flag_name: str) -> str | None:
    explicit_value: str | None = None
    argument_index = 0
    while argument_index < len(arguments):
        current_argument = arguments[argument_index]
        if current_argument != flag_name:
            argument_index += 1
            continue

        next_index = argument_index + 1
        if next_index >= len(arguments):
            _raise_cli_argument_resolution_error(
                field_path=flag_name.lstrip("-").replace("-", "_"),
                message="flag requires a value",
            )

        explicit_value = arguments[next_index]
        argument_index += 2

    return explicit_value


def _build_command_config_argument_vector(
    command_parser: ArgumentParser,
    *,
    command_path: tuple[str, ...],
    config_path: Path,
) -> list[str]:
    config_document = _load_cli_command_config_document(config_path)
    expected_command = CLI_PRIMARY_COMMAND_IDENTIFIERS[command_path]
    configured_command = config_document.get("command")
    if configured_command != expected_command:
        _raise_cli_argument_resolution_error(
            field_path="command",
            message=f"must be {expected_command!r} for this command",
            context={"config_path": str(config_path)},
        )

    raw_arguments = config_document.get("arguments")
    if not isinstance(raw_arguments, dict):
        _raise_cli_argument_resolution_error(
            field_path="arguments",
            message="must be an object",
            context={"config_path": str(config_path)},
        )

    action_by_flag = _build_configurable_action_map(command_parser)
    argument_vector: list[str] = []
    for raw_flag_name, raw_value in raw_arguments.items():
        if not isinstance(raw_flag_name, str):
            _raise_cli_argument_resolution_error(
                field_path="arguments",
                message="argument keys must be strings",
                context={"config_path": str(config_path)},
            )

        action = action_by_flag.get(raw_flag_name)
        if action is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="is not a supported command-config argument",
                context={"config_path": str(config_path)},
            )

        option_string = _select_long_option_string(action, field_path=f"arguments.{raw_flag_name}")
        if action.nargs == 0:
            if not isinstance(raw_value, bool):
                _raise_cli_argument_resolution_error(
                    field_path=f"arguments.{raw_flag_name}",
                    message="must be a boolean for flag-style arguments",
                    context={"config_path": str(config_path)},
                )
            if raw_value:
                argument_vector.append(option_string)
            continue

        if isinstance(raw_value, bool) or raw_value is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="must be a scalar value",
                context={"config_path": str(config_path)},
            )

        argument_vector.extend((option_string, str(raw_value)))

    return argument_vector


def _load_cli_command_config_document(config_path: Path) -> dict[str, Any]:
    try:
        config_document = _load_json_document(config_path, "command config")
    except FileNotFoundError:
        _raise_cli_argument_resolution_error(
            field_path="config_json",
            message="command config file could not be found",
            context={"path": str(config_path)},
        )
    except OSError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_json",
            message=f"command config file could not be read: {e}",
            context={"path": str(config_path)},
        )
    except json.JSONDecodeError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_document",
            message="command config file must contain valid JSON",
            context={
                "path": str(config_path),
                "line_number": e.lineno,
                "column_number": e.colno,
            },
        )
    except ValueError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_document",
            message=str(e),
            context={"path": str(config_path)},
        )

    schema_version = config_document.get("schema_version")
    if schema_version != CLI_COMMAND_CONFIG_SCHEMA_VERSION:
        _raise_cli_argument_resolution_error(
            field_path="schema_version",
            message=f"must be {CLI_COMMAND_CONFIG_SCHEMA_VERSION!r}",
            context={"config_path": str(config_path)},
        )

    return config_document


def _build_configurable_action_map(command_parser: ArgumentParser) -> dict[str, Any]:
    action_by_flag: dict[str, Any] = {}
    for action in command_parser._actions:
        for option_string in cast(list[str], action.option_strings):
            if not option_string.startswith("--"):
                continue

            flag_name = option_string[2:]
            if flag_name == "config-json":
                continue

            action_by_flag[flag_name] = action

    return action_by_flag


def _select_long_option_string(action: Any, *, field_path: str) -> str:
    for option_string in cast(list[str], action.option_strings):
        if option_string.startswith("--"):
            return option_string

    _raise_cli_argument_resolution_error(
        field_path=field_path,
        message="is not configurable through command config",
    )


def _build_environment_argument_vector(
    *,
    command_path: tuple[str, ...],
    environment: Mapping[str, str],
) -> list[str]:
    environment_argument_vector: list[str] = []
    output_mode = environment.get(CLI_OUTPUT_ENV_VAR)
    if output_mode is not None:
        if not output_mode.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_OUTPUT_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )
        environment_argument_vector.extend(("--output", output_mode))

    if command_path[:1] != ("calibration",):
        return environment_argument_vector

    execution_mode = environment.get(CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR)
    if execution_mode is not None:
        if not execution_mode.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )
        environment_argument_vector.extend(("--execution-mode", execution_mode))

    disk_checkpoint_directory = environment.get(CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR)
    if disk_checkpoint_directory is not None:
        if not disk_checkpoint_directory.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )
        environment_argument_vector.extend(("--disk-checkpoint-directory", disk_checkpoint_directory))

    return environment_argument_vector


def _raise_cli_argument_resolution_error(
    *,
    field_path: str,
    message: str,
    context: dict[str, Any] | None = None,
) -> NoReturn:
    raise JaniformValidationError(
        ContractValidationFailure.from_context(
            error_id=CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
            workflow="cli",
            entrypoint=CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
            field_path=field_path,
            message=message,
            context=context,
        )
    )


def _run_simulation_seeded_ticks_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    request_path = _resolve_path_from_repository_root(repository_root, cast(Path, parsed_arguments.request_json))
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    try:
        request_document = _load_json_document(request_path, "simulation request")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        request = SeededTickSimulationRequest.from_mapping(request_document)
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        request_with_runtime_contract = _attach_runtime_hourly_factor_contract(
            request=request,
            calendar_registry=calendar_registry,
        )
        response = run_seeded_tick_event_simulation(request_with_runtime_contract)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                validation_error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                internal_error_id=CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID,
                file_field_path="request_json",
                document_field_path="request_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_simulation_seeded_ticks_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_pricing_vanilla_option_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "pricing execution input")
        execution_input = validate_vanilla_option_pricing_execution_input(execution_input_document)
        run_result = run_vanilla_option_pricing(execution_input)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_PRICING_VANILLA_OPTION_ENTRYPOINT,
                validation_error_id=CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=run_result.to_payload(),
        human_output_factory=lambda: _render_pricing_vanilla_option_human_output(run_result.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_market_data_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]

    try:
        request = _build_market_data_calibration_request_from_arguments(
            parsed_arguments,
            repository_root=repository_root,
        )
        prepared_calibration = prepare_market_data_calibration(request)
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_market_data_command_error(e))

    _write_command_output(
        parsed_arguments,
        json_payload=_build_market_data_calibration_summary_payload(prepared_calibration),
        human_output_factory=lambda: _render_market_data_calibration_human_output(prepared_calibration),
    )

    return int(CliExitCode.SUCCESS)


def _run_calibration_staged_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    execution_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.execution_input_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    try:
        execution_input_document = _load_json_document(execution_input_path, "calibration execution input")
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        execution_input = validate_staged_calibration_execution_input(execution_input_document)
        normalized_execution_input = _override_calibration_execution_mode(
            execution_input=execution_input,
            execution_mode=CalibrationExecutionMode(cast(str, parsed_arguments.execution_mode)),
            disk_checkpoint_directory=disk_checkpoint_directory,
        )
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        response = run_staged_calibration(
            normalized_execution_input,
            calendar_registry=calendar_registry,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                validation_error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                internal_error_id=CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID,
                file_field_path="execution_input_json",
                document_field_path="execution_input_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=response.to_payload(),
        human_output_factory=lambda: _render_calibration_staged_human_output(response.to_payload()),
    )

    return int(CliExitCode.SUCCESS)


def _build_market_data_calibration_request_from_arguments(
    parsed_arguments: Namespace,
    *,
    repository_root: Path,
) -> MarketDataCalibrationRequest:
    tick_data_path = _require_path_argument(
        repository_root,
        cast(Path | None, parsed_arguments.tick_data_path),
        argument_name="tick_data_path",
    )
    session_definition_path = _require_path_argument(
        repository_root,
        cast(Path | None, parsed_arguments.session_definition_path),
        argument_name="session_definition_path",
    )
    disk_checkpoint_directory_argument = cast(Path | None, parsed_arguments.disk_checkpoint_directory)
    disk_checkpoint_directory = (
        _resolve_path_from_repository_root(repository_root, disk_checkpoint_directory_argument)
        if disk_checkpoint_directory_argument is not None
        else None
    )

    option_surface_path_argument = cast(Path | None, parsed_arguments.option_surface_path)
    option_surface_path = (
        _resolve_path_from_repository_root(repository_root, option_surface_path_argument)
        if option_surface_path_argument is not None
        else None
    )
    known_news_path_argument = cast(Path | None, parsed_arguments.known_news_path)
    known_news_path = (
        _resolve_path_from_repository_root(repository_root, known_news_path_argument)
        if known_news_path_argument is not None
        else None
    )

    _reject_orphan_market_data_arguments(
        option_surface_path is not None,
        (
            ("option_surface_format", parsed_arguments.option_surface_format),
            ("option_surface_provenance_label", parsed_arguments.option_surface_provenance_label),
            ("option_quote_input_policy", parsed_arguments.option_quote_input_policy),
            ("option_quote_timestamp_column", parsed_arguments.option_quote_timestamp_column),
            ("option_expiry_column", parsed_arguments.option_expiry_column),
            ("option_strike_column", parsed_arguments.option_strike_column),
            ("option_type_column", parsed_arguments.option_type_column),
            ("option_mid_price_column", parsed_arguments.option_mid_price_column),
            ("option_bid_column", parsed_arguments.option_bid_column),
            ("option_ask_column", parsed_arguments.option_ask_column),
            ("option_implied_volatility_column", parsed_arguments.option_implied_volatility_column),
        ),
        message="option-surface flags require --option-surface-path",
    )
    _reject_orphan_market_data_arguments(
        known_news_path is not None,
        (
            ("known_news_format", parsed_arguments.known_news_format),
            ("known_news_provenance_label", parsed_arguments.known_news_provenance_label),
            ("known_news_timezone", parsed_arguments.known_news_timezone),
            ("known_news_timestamp_column", parsed_arguments.known_news_timestamp_column),
            ("known_news_event_id_column", parsed_arguments.known_news_event_id_column),
        ),
        message="known-news flags require --known-news-path",
    )

    payload: dict[str, Any] = {
        "schema_version": CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
        "run_id": cast(str, parsed_arguments.run_id),
        "tick_data_source": {
            "path": str(tick_data_path),
            "file_format": cast(str, parsed_arguments.tick_data_format),
            "provenance_label": cast(str, parsed_arguments.tick_data_provenance_label),
        },
        "session_configuration": {
            "exchange_code": cast(str, parsed_arguments.exchange_code),
            "session_definition_source": {
                "path": str(session_definition_path),
                "file_format": cast(str, parsed_arguments.session_definition_format),
                "provenance_label": cast(str, parsed_arguments.session_definition_provenance_label),
            },
            "session_timezone": cast(str, parsed_arguments.session_timezone),
        },
        "column_mapping": {
            "tick_data": {
                "timestamp_column": cast(str, parsed_arguments.tick_timestamp_column),
                "price_column": cast(Optional[str], parsed_arguments.tick_price_column),
                "trade_price_column": cast(Optional[str], parsed_arguments.tick_trade_price_column),
                "bid_column": cast(Optional[str], parsed_arguments.tick_bid_column),
                "ask_column": cast(Optional[str], parsed_arguments.tick_ask_column),
                "close_price_column": cast(Optional[str], parsed_arguments.tick_close_price_column),
            },
        },
        "workflow_scope": cast(str, parsed_arguments.workflow_scope),
        "normalization_policy": {
            "timestamp_timezone": cast(str, parsed_arguments.timestamp_timezone),
            "sort_records_by_timestamp": not bool(parsed_arguments.disable_timestamp_sort),
            "duplicate_record_policy": cast(str, parsed_arguments.duplicate_record_policy),
            "tick_price_input_policy": cast(Optional[str], parsed_arguments.tick_price_input_policy),
            "option_quote_input_policy": cast(Optional[str], parsed_arguments.option_quote_input_policy),
        },
        "output_preferences": {
            "execution_mode": cast(str, parsed_arguments.execution_mode),
            "disk_checkpoint_directory": (
                str(disk_checkpoint_directory)
                if disk_checkpoint_directory is not None
                else None
            ),
            "emit_derived_staged_execution_input": bool(parsed_arguments.emit_derived_staged_execution_input),
            "emit_input_provenance_report": not bool(parsed_arguments.omit_input_provenance_report),
        },
    }

    if option_surface_path is not None:
        payload["option_surface_source"] = {
            "path": str(option_surface_path),
            "file_format": cast(Optional[str], parsed_arguments.option_surface_format),
            "provenance_label": cast(Optional[str], parsed_arguments.option_surface_provenance_label),
        }
        payload["column_mapping"]["option_surface"] = {
            "quote_timestamp_column": cast(Optional[str], parsed_arguments.option_quote_timestamp_column),
            "expiry_timestamp_column": cast(Optional[str], parsed_arguments.option_expiry_column),
            "strike_column": cast(Optional[str], parsed_arguments.option_strike_column),
            "option_type_column": cast(Optional[str], parsed_arguments.option_type_column),
            "mid_price_column": cast(Optional[str], parsed_arguments.option_mid_price_column),
            "bid_column": cast(Optional[str], parsed_arguments.option_bid_column),
            "ask_column": cast(Optional[str], parsed_arguments.option_ask_column),
            "implied_volatility_column": cast(Optional[str], parsed_arguments.option_implied_volatility_column),
        }

    if known_news_path is not None:
        payload["known_news_inputs"] = {
            "calendar_source": {
                "path": str(known_news_path),
                "file_format": cast(Optional[str], parsed_arguments.known_news_format),
                "provenance_label": cast(Optional[str], parsed_arguments.known_news_provenance_label),
            },
            "calendar_timezone": cast(Optional[str], parsed_arguments.known_news_timezone),
        }

        if parsed_arguments.known_news_timestamp_column is not None:
            payload["column_mapping"]["known_news"] = {
                "event_timestamp_column": cast(str, parsed_arguments.known_news_timestamp_column),
                "event_identifier_column": cast(Optional[str], parsed_arguments.known_news_event_id_column),
            }

    return validate_market_data_calibration_request(payload)


def _build_market_data_calibration_summary_payload(
    prepared_calibration: PreparedMarketDataCalibration,
) -> dict[str, Any]:
    request = prepared_calibration.request

    return {
        "schema_version": CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION,
        "command": CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
        "request_schema_version": request.schema_version,
        "preparation_schema_version": prepared_calibration.schema_version,
        "run_id": request.run_id,
        "workflow_scope": request.workflow_scope.value,
        "session_context": {
            "exchange_code": request.session_configuration.exchange_code,
            "session_timezone": request.session_configuration.session_timezone,
        },
        "input_files": {
            "tick_data": request.tick_data_source.to_payload(),
            "session_definition": request.session_configuration.session_definition_source.to_payload(),
            "known_news": (
                request.known_news_inputs.calendar_source.to_payload()
                if request.known_news_inputs is not None
                else None
            ),
            "option_surface": (
                request.option_surface_source.to_payload()
                if request.option_surface_source is not None
                else None
            ),
        },
        "column_mapping": request.column_mapping.to_payload(),
        "normalization_policy": request.normalization_policy.to_payload(),
        "output_artifact_locations": {
            "disk_checkpoint_directory": request.output_preferences.disk_checkpoint_directory,
        },
        "output_emission_policy": {
            "execution_mode": request.output_preferences.execution_mode.value,
            "emit_input_provenance_report": request.output_preferences.emit_input_provenance_report,
            "emit_derived_staged_execution_input": request.output_preferences.emit_derived_staged_execution_input,
        },
        "diagnostics_summary": prepared_calibration.diagnostics.to_payload(),
        "provenance_summary": prepared_calibration.provenance.to_payload(),
        "output_schema_metadata": {
            "schema_family": "cli.workflow.command_summary",
            "field_selection_policy": CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY,
            "required_sections": [
                "run_id",
                "workflow_scope",
                "session_context",
                "input_files",
                "column_mapping",
                "normalization_policy",
                "output_artifact_locations",
                "output_emission_policy",
                "diagnostics_summary",
                "provenance_summary",
            ],
            "optional_input_sections": [
                "input_files.known_news",
                "input_files.option_surface",
                "column_mapping.known_news",
                "column_mapping.option_surface",
                "provenance_summary.option_surface_summary",
                "provenance_summary.known_news_summary",
            ],
        },
    }


def _render_market_data_calibration_human_output(
    prepared_calibration: PreparedMarketDataCalibration,
) -> str:
    request = prepared_calibration.request

    selected_stages = ",".join(stage.value for stage in prepared_calibration.diagnostics.selected_stages)
    preprocessing_steps = ",".join(prepared_calibration.diagnostics.executed_preprocessing_steps)

    lines: list[str] = [
        (
            "calibration market-data request prepared: "
            f"run_id={request.run_id} "
            f"workflow_scope={request.workflow_scope.value} "
            f"execution_mode={request.output_preferences.execution_mode.value}"
        ),
        (
            "tick_data: "
            f"path={request.tick_data_source.path} "
            f"format={request.tick_data_source.file_format.value} "
            f"provenance_label={request.tick_data_source.provenance_label}"
        ),
        (
            "session_definition: "
            f"path={request.session_configuration.session_definition_source.path} "
            f"format={request.session_configuration.session_definition_source.file_format.value} "
            f"exchange_code={request.session_configuration.exchange_code} "
            f"session_timezone={request.session_configuration.session_timezone}"
        ),
        (
            "workflow_diagnostics: "
            f"preparation_schema_version={CALIBRATION_MARKET_DATA_PREPARATION_SCHEMA_VERSION} "
            f"selected_stages={selected_stages} "
            f"preprocessing_steps={preprocessing_steps}"
        ),
        (
            "loaded_data_summary: "
            f"tick_records={prepared_calibration.diagnostics.tick_record_count} "
            f"known_news_events={prepared_calibration.diagnostics.known_news_event_count} "
            f"option_surface_records={prepared_calibration.diagnostics.option_surface_record_count}"
        ),
    ]

    if request.known_news_inputs is not None:
        lines.append(
            (
                "known_news: "
                f"path={request.known_news_inputs.calendar_source.path} "
                f"format={request.known_news_inputs.calendar_source.file_format.value} "
                f"provenance_label={request.known_news_inputs.calendar_source.provenance_label}"
            )
        )

    if request.option_surface_source is not None:
        lines.append(
            (
                "option_surface: "
                f"path={request.option_surface_source.path} "
                f"format={request.option_surface_source.file_format.value} "
                f"provenance_label={request.option_surface_source.provenance_label}"
            )
        )

    lines.append(
        (
            "derived_references: "
            f"trade_events={prepared_calibration.provenance.derived_trade_events_data_reference} "
            f"option_surface={prepared_calibration.provenance.derived_option_surface_data_reference or 'none'}"
        )
    )
    lines.append(
        (
            "normalization_policy: "
            f"timestamp_timezone={request.normalization_policy.timestamp_timezone} "
            f"sort_records_by_timestamp={str(request.normalization_policy.sort_records_by_timestamp).lower()} "
            f"duplicate_record_policy={request.normalization_policy.duplicate_record_policy.value} "
            f"tick_price_input_policy={_optional_enum_value(request.normalization_policy.tick_price_input_policy)} "
            f"option_quote_input_policy={_optional_enum_value(request.normalization_policy.option_quote_input_policy)}"
        )
    )
    lines.append(
        (
            "artifact_targets: "
            f"disk_checkpoint_directory={request.output_preferences.disk_checkpoint_directory or 'none'} "
            f"emit_input_provenance_report={str(request.output_preferences.emit_input_provenance_report).lower()} "
            f"emit_derived_staged_execution_input={str(request.output_preferences.emit_derived_staged_execution_input).lower()}"
        )
    )

    return "\n".join(lines) + "\n"


def _render_simulation_seeded_ticks_human_output(response_payload: dict[str, Any]) -> str:
    return (
        "simulation seeded-ticks completed: "
        f"schema_version={response_payload['schema_version']} "
        f"event_count={len(cast(list[dict[str, Any]], response_payload['event_tape']))} "
        f"event_record_schema_version={cast(dict[str, Any], response_payload['metadata'])['event_record_schema_version']}\n"
    )


def _render_pricing_vanilla_option_human_output(run_result_payload: dict[str, Any]) -> str:
    response_payload = cast(dict[str, Any], run_result_payload["response"])
    return (
        "pricing vanilla-option completed: "
        f"schema_version={run_result_payload['schema_version']} "
        f"price={response_payload['price']} "
        f"delta={response_payload['delta']}\n"
    )


def _render_calibration_staged_human_output(response_payload: dict[str, Any]) -> str:
    return (
        "calibration staged completed: "
        f"run_id={response_payload['run_id']} "
        f"execution_mode={response_payload['execution_mode']} "
        f"status={response_payload['status']} "
        f"stage_report_count={len(cast(list[dict[str, Any]], response_payload['stage_reports']))} "
        f"checkpoint_count={len(cast(list[dict[str, Any]], response_payload['emitted_checkpoints']))}\n"
    )


def _run_open_closed_gap_leakage_regression_command(parsed_arguments: Namespace) -> int:
    repository_root = Path(__file__).resolve().parents[2]
    calendar_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_fixture_path),
    )
    gap_behavior_fixture_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.gap_behavior_fixture_path),
    )

    try:
        calendar_fixture_document = _load_json_document(calendar_fixture_path, "calendar fixture")
        gap_behavior_fixture_document = _load_json_document(
            gap_behavior_fixture_path,
            "open closed gap behavior fixture",
        )

        calendar_registry = _build_calendar_registry(calendar_fixture_document)
        gap_behavior_cases = _extract_gap_behavior_cases(gap_behavior_fixture_document)

        report = run_open_closed_gap_leakage_regression_harness(
            calendar_registry=calendar_registry,
            gap_behavior_cases=gap_behavior_cases,
            component_absolute_tolerance=float(parsed_arguments.component_absolute_tolerance),
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(_classify_open_closed_gap_command_error(e))

    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    failed_case_count = cast(dict[str, int], report["summary"])["failed_case_count"]
    if output_mode == CliOutputMode.JSON:
        _write_contract_json(report)
        return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)

    _write_open_closed_gap_human_output(report)
    return int(CliExitCode.SUCCESS if failed_case_count == 0 else CliExitCode.RUNTIME_ERROR)


def _classify_open_closed_gap_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_path",
                message="fixture file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="fixture_document",
                message="fixture file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_OPEN_CLOSED_GAP_ENTRYPOINT,
            message="open closed gap leakage regression command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _classify_market_data_command_error(error: Exception) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, ValueError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
                field_path="command_input",
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT,
            message="market-data calibration command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )


def _attach_runtime_hourly_factor_contract(
    *,
    request: SeededTickSimulationRequest,
    calendar_registry: CalendarRegistry,
) -> SeededTickSimulationRequest:
    range_end_utc = request.start_time_utc + timedelta(hours=request.horizon_interval_count)

    hourly_factor_contract = load_hourly_factor_contract_for_simulation(
        calendar_registry=calendar_registry,
        exchange_code=request.hourly_factor_summary.exchange_code,
        range_start_utc=request.start_time_utc,
        range_end_utc=range_end_utc,
        seasonality_source=HourlySeasonalityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.s_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
        news_intensity_source=HourlyNewsIntensityProviderExplicitSource(
            hourly_factors=request.hourly_factor_summary.nu_h,
            materialization_method="cli.request_hourly_factor_summary.v1",
        ),
    )

    derived_summary = SeededTickSimulationHourlyFactorSummary.from_contract(hourly_factor_contract)
    if derived_summary != request.hourly_factor_summary:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT,
                field_path="request_json.hourly_factor_summary",
                message="request hourly_factor_summary must match the rehydrated hourly factor contract",
                context={
                    "derived_interval_count": derived_summary.interval_count,
                    "request_interval_count": request.hourly_factor_summary.interval_count,
                },
            )
        )

    return SeededTickSimulationRequest.from_runtime_inputs(
        seed=request.seed,
        start_time_utc=request.start_time_utc,
        horizon_interval_count=request.horizon_interval_count,
        kernel_parameters=request.kernel_parameters,
        initial_excitation_state=request.initial_excitation_state,
        hourly_factor_contract=hourly_factor_contract,
        debug_trace_enabled=request.debug_trace_enabled,
        sample_every_nth_transition=request.sample_every_nth_transition,
        max_records_per_chunk=request.max_records_per_chunk,
        state_snapshot_capture=request.state_snapshot_capture,
    )


def _override_calibration_execution_mode(
    *,
    execution_input: StagedCalibrationExecutionInput,
    execution_mode: CalibrationExecutionMode,
    disk_checkpoint_directory: Path | None,
) -> StagedCalibrationExecutionInput:
    if execution_mode == CalibrationExecutionMode.MEMORY and disk_checkpoint_directory is not None:
        raise JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID,
                workflow="cli",
                entrypoint=CLI_CALIBRATION_STAGED_ENTRYPOINT,
                field_path="disk_checkpoint_directory",
                message="--disk-checkpoint-directory requires --execution-mode disk",
            )
        )

    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = execution_input.runtime_resources
    if execution_mode == CalibrationExecutionMode.DISK and disk_checkpoint_directory is not None:
        runtime_resources = CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=str(disk_checkpoint_directory),
        )
    if execution_mode == CalibrationExecutionMode.MEMORY:
        runtime_resources = None

    return validate_staged_calibration_execution_input(
        {
            **execution_input.to_payload(),
            "request": {
                **execution_input.request.to_payload(),
                "execution_mode": execution_mode.value,
            },
            "runtime_resources": None if runtime_resources is None else runtime_resources.to_payload(),
        }
    )


def _classify_workflow_command_error(
    error: Exception,
    *,
    entrypoint: str,
    validation_error_id: str,
    internal_error_id: str,
    file_field_path: str,
    document_field_path: str,
) -> JaniformError:
    if isinstance(error, JaniformError):
        return error

    if isinstance(error, FileNotFoundError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message="input file could not be found",
                context={"path": error.filename},
            )
        )

    if isinstance(error, OSError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=file_field_path,
                message=f"input file could not be read: {error}",
                context={"path": error.filename},
            )
        )

    if isinstance(error, json.JSONDecodeError):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message="input file must contain valid JSON",
                context={
                    "column_number": error.colno,
                    "line_number": error.lineno,
                },
            )
        )

    if isinstance(
        error,
        (
            CalendarInvalidTimestampError,
            CalendarInvalidTimezoneError,
            CalendarMissingExchangeError,
            CalendarRegistryLoadError,
            SeasonalityInputValidationError,
            ValueError,
        ),
    ):
        return JaniformValidationError(
            ContractValidationFailure.from_context(
                error_id=validation_error_id,
                workflow="cli",
                entrypoint=entrypoint,
                field_path=document_field_path,
                message=str(error),
            )
        )

    return JaniformIntegrationError(
        JaniformErrorDetails.from_context(
            error_id=internal_error_id,
            category=JaniformErrorCategory.INTEGRATION,
            workflow="cli",
            entrypoint=entrypoint,
            message="workflow command failed unexpectedly",
            context={"source_exception_type": type(error).__name__},
        )
    )
def _resolve_path_from_repository_root(repository_root: Path, path: Path) -> Path:
    if path.is_absolute():
        return path

    return repository_root / path


def _require_path_argument(
    repository_root: Path,
    path: Path | None,
    *,
    argument_name: str,
) -> Path:
    if path is None:
        raise ValueError(f"--{argument_name.replace('_', '-')} is required")

    return _resolve_path_from_repository_root(repository_root, path)


def _reject_orphan_market_data_arguments(
    has_primary_path: bool,
    optional_arguments: tuple[tuple[str, object], ...],
    *,
    message: str,
) -> None:
    if has_primary_path:
        return

    for _, argument_value in optional_arguments:
        if argument_value is not None:
            raise ValueError(message)


def _load_json_document(path: Path, context: str) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return cast(dict[str, Any], payload)


def _write_contract_json(payload: dict[str, Any]) -> None:
    sys.stdout.write(f"{json.dumps(payload, sort_keys=True)}\n")


def _write_cli_error(error: JaniformError) -> int:
    sys.stderr.write(f"{error}\n")
    return int(_exit_code_for_error(error))


def _exit_code_for_error(error: JaniformError) -> CliExitCode:
    match error.category:
        case JaniformErrorCategory.VALIDATION:
            return CliExitCode.VALIDATION_ERROR
        case JaniformErrorCategory.EXECUTION:
            return CliExitCode.RUNTIME_ERROR
        case JaniformErrorCategory.INTEGRATION:
            return CliExitCode.INTERNAL_ERROR


def _write_command_output(
    parsed_arguments: Namespace,
    *,
    json_payload: dict[str, Any],
    human_output_factory: Callable[[], str],
) -> None:
    output_mode = CliOutputMode(cast(str, parsed_arguments.output))
    match output_mode:
        case CliOutputMode.JSON:
            _write_contract_json(json_payload)
        case CliOutputMode.HUMAN:
            sys.stdout.write(human_output_factory())


def _write_open_closed_gap_human_output(report: dict[str, Any]) -> None:
    summary = cast(dict[str, int], report["summary"])
    behavior_diff_report_summary = cast(
        dict[str, int],
        cast(dict[str, Any], report["behavior_diff_report"])["summary"],
    )
    if summary["failed_case_count"] == 0:
        sys.stdout.write(
            "open closed gap leakage regression passed: "
            f"{summary['passed_case_count']}/{summary['total_case_count']} scenarios, "
            f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}, "
            f"diff_regression_cases={behavior_diff_report_summary['regression_case_count']}\n",
        )
        return

    sys.stderr.write(
        "open closed gap leakage regression failed: "
        f"{summary['failed_case_count']}/{summary['total_case_count']} scenarios, "
        f"semantic_mismatches={summary['total_semantic_mismatch_failure_count']}\n",
    )

    for case_report in cast(list[dict[str, Any]], report["case_reports"]):
        if case_report["status"] != "fail":
            continue

        sys.stderr.write(
            f"scenario_id={case_report['fixture_id']} exchange_code={case_report['exchange_code']} "
            f"semantic_mismatch_failure_count={case_report['semantic_mismatch_failure_count']} "
            f"max_absolute_mismatch={case_report['max_absolute_mismatch']}\n",
        )

        contribution_summary = cast(dict[str, Any], case_report["contribution_summary"])

        sys.stderr.write(
            "  contributions "
            f"activity_expected={contribution_summary['expected_activity_coupling_total']} "
            f"activity_observed={contribution_summary['observed_activity_coupling_total']} "
            f"trading_expected={contribution_summary['expected_trading_return_total']} "
            f"trading_observed={contribution_summary['observed_trading_return_total']} "
            f"open_jump_expected={contribution_summary['expected_open_jump_total']} "
            f"open_jump_observed={contribution_summary['observed_open_jump_total']} "
            f"closed_gap_expected={contribution_summary['expected_closed_gap_total']} "
            f"closed_gap_observed={contribution_summary['observed_closed_gap_total']}\n",
        )

        behavior_diff = cast(dict[str, Any], case_report["behavior_diff"])
        for diff_metric in cast(list[dict[str, Any]], behavior_diff["metrics"]):
            if not cast(bool, diff_metric["regression_detected"]):
                continue

            sys.stderr.write(
                "  diff_regression "
                f"metric_id={diff_metric['metric_id']} "
                f"semantic_bucket={diff_metric['semantic_bucket']} "
                f"expected={diff_metric['expected_value']} "
                f"observed={diff_metric['observed_value']} "
                f"absolute_difference={diff_metric['absolute_difference']} "
                f"absolute_tolerance={diff_metric['absolute_tolerance']}\n",
            )

        for failure in cast(list[dict[str, Any]], case_report["failures"]):
            failure_kind = cast(str, failure["failure_kind"])
            match failure_kind:
                case "semantic_mismatch":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} event_id={failure['event_id']} "
                        f"event_timestamp_utc={failure['event_timestamp_utc']} "
                        f"semantic_bucket={failure['semantic_bucket']} "
                        f"mismatch_kind={failure['mismatch_kind']} "
                        f"absolute_difference={failure['absolute_difference']}\n",
                    )
                case "dispatch_error":
                    sys.stderr.write(
                        f"  failure_kind={failure_kind} error_message={failure['error_message']}\n",
                    )
                case _:
                    sys.stderr.write(f"  failure_kind={failure_kind}\n")


def _optional_enum_value(value: StrEnum | None) -> str:
    return value.value if value is not None else "none"


def _build_calendar_registry(calendar_fixture_document: dict[str, Any]) -> CalendarRegistry:
    return _build_calendar_registry_from_document(calendar_fixture_document)


def _build_calendar_registry_from_document(calendar_registry_document: dict[str, Any]) -> CalendarRegistry:
    calendar_registry_payload: dict[str, Any]
    if "calendar_registry" in calendar_registry_document:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry",
        )
    else:
        calendar_registry_payload = _extract_calendar_registry_payload(
            calendar_registry_document,
            field_path="calendar_registry_document",
        )

    return CalendarRegistry(json.dumps(calendar_registry_payload))


def _extract_calendar_registry_payload(
    calendar_registry_document: dict[str, Any],
    *,
    field_path: str,
) -> dict[str, Any]:
    schema_version = calendar_registry_document.get("schema_version")
    if isinstance(schema_version, str) and schema_version == "calendar.registry.v1":
        return calendar_registry_document

    calendar_registry_payload: object = calendar_registry_document.get("calendar_registry")
    if not isinstance(calendar_registry_payload, dict):
        raise ValueError(f"{field_path} must be a calendar.registry.v1 JSON object")

    return cast(dict[str, Any], calendar_registry_payload)


def _extract_gap_behavior_cases(
    gap_behavior_fixture_document: dict[str, Any],
) -> list[dict[str, Any]]:
    raw_cases: object = gap_behavior_fixture_document.get("cases")
    if not isinstance(raw_cases, list):
        raise ValueError("gap behavior fixture must include list field `cases`")

    parsed_cases: list[dict[str, Any]] = []
    for raw_case in raw_cases:
        if not isinstance(raw_case, dict):
            raise ValueError("gap behavior fixture case must be an object")

        initial_accumulator_state: object = raw_case.get("initial_accumulator_state")
        if not isinstance(initial_accumulator_state, dict):
            raise ValueError("gap behavior case must include object field `initial_accumulator_state`")

        injected_events: object = raw_case.get("injected_events")
        if not isinstance(injected_events, list):
            raise ValueError("gap behavior case must include list field `injected_events`")

        parsed_cases.append(
            {
                "fixture_id": raw_case.get("fixture_id"),
                "exchange_code": raw_case.get("exchange_code"),
                "scenario_class": raw_case.get("scenario_class"),
                "deterministic_replay_metadata": raw_case.get("deterministic_replay_metadata"),
                "issue_traceability": raw_case.get("issue_traceability"),
                "initial_j_t": initial_accumulator_state.get("j_t"),
                "initial_m_t": initial_accumulator_state.get("m_t"),
                "initial_g_t": initial_accumulator_state.get("g_t"),
                "injected_events": [
                    {
                        "event_id": (
                            raw_event.get("event_id")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "timestamp_utc": (
                            raw_event.get("timestamp_utc")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "shared_severity_mark": (
                            raw_event.get("shared_severity_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "news_arrival_component": (
                            raw_event.get("news_arrival_component")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                        "coupled_return_jump_mark": (
                            raw_event.get("coupled_return_jump_mark")
                            if isinstance(raw_event, dict)
                            else None
                        ),
                    }
                    for raw_event in injected_events
                ],
            },
        )

    return parsed_cases


def main() -> None:
    try:
        parsed_arguments = _parse_cli_arguments()
    except JaniformError as e:
        raise SystemExit(_write_cli_error(e)) from e

    handler = parsed_arguments.handler
    if handler is not None:
        raise SystemExit(handler(parsed_arguments))

    parsed_arguments.help_target.print_help()
