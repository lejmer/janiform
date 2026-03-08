from enum import IntEnum, StrEnum
from typing import Any

from ..calibration import (
    CalibrationDuplicateRecordPolicy,
    CalibrationExecutionMode,
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationTickPriceInputPolicy,
)


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

__all__ = [
    "CLI_ARGUMENT_PRECEDENCE_POLICY",
    "CLI_ARGUMENT_RESOLUTION_ENTRYPOINT",
    "CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID",
    "CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR",
    "CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES",
    "CLI_CALIBRATION_EXECUTION_MODE_CHOICES",
    "CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR",
    "CLI_CALIBRATION_EXECUTION_MODE_HELP",
    "CLI_CALIBRATION_MARKET_DATA_ENTRYPOINT",
    "CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP",
    "CLI_CALIBRATION_MARKET_DATA_INTERNAL_ERROR_ID",
    "CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES",
    "CLI_CALIBRATION_MARKET_DATA_VALIDATION_ERROR_ID",
    "CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES",
    "CLI_CALIBRATION_STAGED_ENTRYPOINT",
    "CLI_CALIBRATION_STAGED_INTERNAL_ERROR_ID",
    "CLI_CALIBRATION_STAGED_VALIDATION_ERROR_ID",
    "CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES",
    "CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES",
    "CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES",
    "CLI_COMMAND_CONFIG_HELP",
    "CLI_COMMAND_CONFIG_SCHEMA_VERSION",
    "CLI_CONFIG_JSON_ENV_VAR",
    "CLI_DIRECTORY_METAVAR",
    "CLI_MARKET_DATA_SUMMARY_FIELD_SELECTION_POLICY",
    "CLI_MARKET_DATA_SUMMARY_SCHEMA_VERSION",
    "CLI_OPEN_CLOSED_GAP_ENTRYPOINT",
    "CLI_OPEN_CLOSED_GAP_INTERNAL_ERROR_ID",
    "CLI_OPEN_CLOSED_GAP_VALIDATION_ERROR_ID",
    "CLI_OUTPUT_ENV_VAR",
    "CLI_OUTPUT_MODE_CHOICES",
    "CLI_OUTPUT_MODE_HELP",
    "CLI_PATH_METAVAR",
    "CLI_PRICING_VANILLA_OPTION_ENTRYPOINT",
    "CLI_PRICING_VANILLA_OPTION_INTERNAL_ERROR_ID",
    "CLI_PRICING_VANILLA_OPTION_VALIDATION_ERROR_ID",
    "CLI_PRIMARY_COMMAND_IDENTIFIERS",
    "CLI_PRIMARY_COMMAND_TREE",
    "CLI_REPOSITORY_RELATIVE_PATH_POLICY",
    "CLI_SIMULATION_SEEDED_TICKS_ENTRYPOINT",
    "CLI_SIMULATION_SEEDED_TICKS_INTERNAL_ERROR_ID",
    "CLI_SIMULATION_SEEDED_TICKS_VALIDATION_ERROR_ID",
    "CliExitCode",
    "CliOutputMode",
    "DEFAULT_CALENDAR_FIXTURE_PATH",
    "DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH",
]
