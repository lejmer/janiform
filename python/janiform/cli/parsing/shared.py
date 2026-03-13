from argparse import ArgumentParser
from pathlib import Path

from .._constants import (
    CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
    CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
    CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
    CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
    CLI_COMMAND_CONFIG_HELP,
    CLI_DIRECTORY_METAVAR,
    CLI_MODEL_SIMULATION_OUTPUT_CHOICES,
    CLI_OUTPUT_MODE_CHOICES,
    CLI_OUTPUT_MODE_HELP,
    CLI_PATH_METAVAR,
    CLI_PRESET_CATEGORY_CHOICES,
    CLI_PRESET_SCOPE_CHOICES,
    CLI_REPOSITORY_RELATIVE_PATH_POLICY,
    CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
    CLI_SEASONALITY_SESSION_FILTER_CHOICES,
    CliOutputMode,
)
from ...calibration import CalibrationDuplicateRecordPolicy, CalibrationExecutionMode


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


def _add_required_datetime_argument(
    parser: ArgumentParser,
    *,
    flag_name: str,
    help_text: str,
) -> None:
    parser.add_argument(
        flag_name,
        required=True,
        help=f"{help_text} Use an ISO 8601 timezone-aware timestamp.",
    )


def _add_user_preset_json_argument(parser: ArgumentParser) -> None:
    parser.add_argument(
        "--user-preset-json",
        action="append",
        default=[],
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional preset artifact JSON to register in the in-memory user registry before "
            f"command resolution. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )


def _add_preset_scope_argument(
    parser: ArgumentParser,
    *,
    flag_name: str,
    help_text: str,
    include_auto: bool = False,
) -> None:
    parser.add_argument(
        flag_name,
        choices=CLI_PRESET_SCOPE_CHOICES if include_auto else CLI_PRESET_SCOPE_CHOICES[1:],
        default="auto" if include_auto else None,
        help=help_text,
    )


def _add_model_workflow_preset_arguments(parser: ArgumentParser) -> None:
    _add_user_preset_json_argument(parser)

    parser.add_argument(
        "--market",
        help="Optional market-profile preset name used as the high-level workflow baseline.",
    )
    _add_preset_scope_argument(
        parser,
        flag_name="--market-scope",
        help_text="Registry scope for --market. Default: auto.",
        include_auto=True,
    )

    session_group = parser.add_mutually_exclusive_group()
    session_group.add_argument(
        "--session-preset",
        help="Optional session preset name. When omitted, Janiform may infer the session from data.",
    )
    session_group.add_argument(
        "--session-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional explicit session JSON override. Accepts a calendar.registry.v1 document or "
            f"a SessionExplicitConfiguration JSON object. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    _add_preset_scope_argument(
        parser,
        flag_name="--session-scope",
        help_text="Registry scope for --session-preset. Default: auto.",
        include_auto=True,
    )

    known_news_group = parser.add_mutually_exclusive_group()
    known_news_group.add_argument(
        "--known-news",
        help=(
            "Optional known-news preset name. Use `none` to keep the bundled off-by-default "
            "baseline explicit."
        ),
    )
    known_news_group.add_argument(
        "--known-news-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional explicit known-news configuration JSON override using the "
            f"KnownNewsExplicitConfiguration shape. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    _add_preset_scope_argument(
        parser,
        flag_name="--known-news-scope",
        help_text="Registry scope for --known-news. Default: auto.",
        include_auto=True,
    )

    parser.add_argument(
        "--exchange-code",
        help=(
            "Optional exchange-code hint for session inference when neither the selected market "
            "profile nor explicit session input already supplies it."
        ),
    )
    parser.add_argument(
        "--session-timezone",
        help="Optional timezone hint for automatic session inference.",
    )


def _add_model_workflow_fit_arguments(parser: ArgumentParser) -> None:
    _add_command_config_argument(parser)
    _add_output_mode_argument(
        parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_model_workflow_preset_arguments(parser)

    parser.add_argument(
        "--run-id",
        required=True,
        help="Stable run identifier recorded on the fitted model and fit report.",
    )
    parser.add_argument(
        "--workflow-scope",
        required=True,
        choices=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
        help=(
            "High-level calibration scope. Use seasonality_only for activity-only estimation, "
            "full_without_option_surface for full calibration from tick data only, and "
            "full_with_option_surface when option-surface data is supplied."
        ),
    )
    parser.add_argument(
        "--tick-data-path",
        type=Path,
        required=True,
        metavar=CLI_PATH_METAVAR,
        help=f"Path to the input tick-data file. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )
    parser.add_argument(
        "--tick-data-format",
        required=True,
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Tick-data file format.",
    )
    parser.add_argument(
        "--tick-data-provenance-label",
        default="tick_data",
        help="Stable provenance label recorded for the tick-data source. Default: tick_data.",
    )
    parser.add_argument(
        "--tick-timestamp-column",
        required=True,
        help="Tick-data timestamp column name.",
    )
    parser.add_argument(
        "--tick-price-input-policy",
        choices=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
        help="Explicit tick-price normalization policy for non-seasonality workflows.",
    )
    parser.add_argument(
        "--tick-price-column",
        help="Tick-data explicit price column.",
    )
    parser.add_argument(
        "--tick-trade-price-column",
        help="Tick-data trade price column.",
    )
    parser.add_argument(
        "--tick-trade-sign-column",
        help="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
    )
    parser.add_argument(
        "--tick-bid-column",
        help="Tick-data bid column.",
    )
    parser.add_argument(
        "--tick-ask-column",
        help="Tick-data ask column.",
    )
    parser.add_argument(
        "--tick-close-price-column",
        help="Tick-data close price column.",
    )

    parser.add_argument(
        "--timestamp-timezone",
        required=True,
        help="Timezone used to normalize tick-data timestamps before fitting.",
    )
    parser.add_argument(
        "--duplicate-record-policy",
        choices=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
        default=CalibrationDuplicateRecordPolicy.ERROR.value,
        help="Duplicate-record normalization policy recorded in provenance.",
    )
    parser.add_argument(
        "--disable-timestamp-sort",
        action="store_true",
        help="Record that source data should not be re-sorted by timestamp before translation.",
    )

    _add_optional_path_argument(
        parser=parser,
        flag_name="--option-surface-path",
        help_text="Optional path to the option-surface file.",
    )
    parser.add_argument(
        "--option-surface-format",
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Option-surface file format. Default when provided: parquet.",
    )
    parser.add_argument(
        "--option-surface-provenance-label",
        help="Stable provenance label recorded for the option-surface source. Default when provided: option_surface.",
    )
    parser.add_argument(
        "--option-quote-input-policy",
        choices=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
        help="Explicit option-quote normalization policy.",
    )
    parser.add_argument(
        "--option-quote-timestamp-column",
        help="Option-surface quote timestamp column name.",
    )
    parser.add_argument(
        "--option-expiry-column",
        help="Option-surface expiry column name.",
    )
    parser.add_argument(
        "--option-strike-column",
        help="Option-surface strike column name.",
    )
    parser.add_argument(
        "--option-type-column",
        help="Option-surface option type column name.",
    )
    parser.add_argument(
        "--option-mid-price-column",
        help="Option-surface mid-price column name.",
    )
    parser.add_argument(
        "--option-bid-column",
        help="Option-surface bid column name.",
    )
    parser.add_argument(
        "--option-ask-column",
        help="Option-surface ask column name.",
    )

    parser.add_argument(
        "--option-implied-volatility-column",
        help="Option-surface implied volatility column name.",
    )

    parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.MEMORY.value,
        help=(
            "Execution mode recorded in the high-level fit workflow. Default: memory. "
            "Use disk when you want checkpoint artifacts in addition to the fitted model."
        ),
    )
    _add_optional_path_argument(
        parser=parser,
        flag_name="--disk-checkpoint-directory",
        directory=True,
        help_text=(
            "Disk checkpoint directory recorded in output_preferences. Required when "
            "--execution-mode disk is selected."
        ),
    )
    parser.add_argument(
        "--emit-derived-staged-execution-input",
        action="store_true",
        help="Request emission of the derived staged execution input for replay or debugging.",
    )
    parser.add_argument(
        "--omit-input-provenance-report",
        action="store_true",
        help="Disable provenance-report emission in output_preferences.",
    )
    parser.add_argument(
        "--model-output-path",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional path where the fitted JaniformModel should be persisted. "
            f"{CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )


def _add_model_workflow_simulate_arguments(
    parser: ArgumentParser,
    *,
    require_model_path: bool,
    include_shared_arguments: bool = True,
) -> None:
    if include_shared_arguments:
        _add_command_config_argument(parser)
        _add_output_mode_argument(
            parser,
            default_output_mode=CliOutputMode.JSON,
        )

    if require_model_path:
        parser.add_argument(
            "--model-path",
            type=Path,
            required=True,
            metavar=CLI_PATH_METAVAR,
            help=f"Path to a persisted JaniformModel JSON payload. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
        )

    parser.add_argument(
        "--seed",
        required=True,
        type=int,
        help="Simulation random seed.",
    )

    _add_required_datetime_argument(
        parser,
        flag_name="--start-time-utc",
        help_text="Simulation start time in UTC.",
    )

    parser.add_argument(
        "--horizon-interval-count",
        required=True,
        type=int,
        help="Number of simulation intervals to generate.",
    )
    parser.add_argument(
        "--interval",
        default="1h",
        help="Simulation interval such as `1h` or `30m`. Default: 1h.",
    )
    parser.add_argument(
        "--simulation-output",
        choices=CLI_MODEL_SIMULATION_OUTPUT_CHOICES,
        default="coarse",
        help="Model simulation output mode. Default: coarse.",
    )
    parser.add_argument(
        "--debug-trace-enabled",
        action="store_true",
        help="Enable seeded-tick debug tracing when the simulation output includes ticks.",
    )
    parser.add_argument(
        "--sample-every-nth-transition",
        type=int,
        default=1,
        help="Keep every nth transition in the seeded-tick trace. Default: 1.",
    )
    parser.add_argument(
        "--max-records-per-chunk",
        type=int,
        default=1024,
        help="Maximum seeded-tick records emitted per chunk. Default: 1024.",
    )


def _add_hourly_factor_common_arguments(parser: ArgumentParser) -> None:
    _add_command_config_argument(parser)
    _add_output_mode_argument(
        parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used to resolve exchange-local interval labels",
    )
    parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state and interval-factor alignment.",
    )
    parser.add_argument(
        "--interval",
        default="1h",
        help=(
            "Fixed-width UTC interval specification used to derive the target grid, "
            "for example 1h or 15min. Default: 1h."
        ),
    )
    _add_required_datetime_argument(
        parser,
        flag_name="--range-start-utc",
        help_text="Inclusive UTC range start aligned to the first target interval boundary.",
    )
    _add_required_datetime_argument(
        parser,
        flag_name="--range-end-utc",
        help_text="Exclusive UTC range end aligned to the final target interval boundary.",
    )


def _add_market_data_calibration_arguments(parser: ArgumentParser) -> None:
    _add_command_config_argument(parser)
    _add_output_mode_argument(
        parser,
        default_output_mode=CliOutputMode.JSON,
    )

    parser.add_argument(
        "--run-id",
        required=True,
        help="Stable run identifier recorded in the market-data request payload.",
    )
    parser.add_argument(
        "--workflow-scope",
        required=True,
        choices=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
        help=(
            "High-level calibration scope. Use seasonality_only for activity-only estimation, "
            "full_without_option_surface for full calibration from tick data only, and "
            "full_with_option_surface when option-surface data is supplied."
        ),
    )
    parser.add_argument(
        "--tick-data-path",
        type=Path,
        required=True,
        metavar=CLI_PATH_METAVAR,
        help=f"Path to the input tick-data file. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}",
    )
    parser.add_argument(
        "--tick-data-format",
        required=True,
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Tick-data file format.",
    )
    parser.add_argument(
        "--tick-data-provenance-label",
        required=True,
        help="Stable provenance label recorded for the tick-data source.",
    )
    parser.add_argument(
        "--tick-timestamp-column",
        required=True,
        help="Tick-data timestamp column name.",
    )
    parser.add_argument(
        "--tick-price-input-policy",
        choices=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
        help="Explicit tick-price normalization policy for non-seasonality workflows.",
    )
    parser.add_argument(
        "--tick-price-column",
        help="Tick-data explicit price column.",
    )
    parser.add_argument(
        "--tick-trade-price-column",
        help="Tick-data trade price column.",
    )
    parser.add_argument(
        "--tick-trade-sign-column",
        help="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
    )
    parser.add_argument(
        "--tick-bid-column",
        help="Tick-data bid column.",
    )
    parser.add_argument(
        "--tick-ask-column",
        help="Tick-data ask column.",
    )
    parser.add_argument(
        "--tick-close-price-column",
        help="Tick-data close price column.",
    )

    parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state and hourly factors.",
    )
    parser.add_argument(
        "--session-definition-path",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional path to an explicit session-definition file override. "
            f"{CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    parser.add_argument(
        "--session-definition-format",
        choices=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
        help="Explicit session-definition file format when --session-definition-path is provided.",
    )
    parser.add_argument(
        "--session-definition-provenance-label",
        help="Stable provenance label recorded for the explicit session-definition source.",
    )
    parser.add_argument(
        "--session-timezone",
        help="Optional timezone hint for automatic session inference or explicit session overrides.",
    )

    _add_optional_path_argument(
        parser=parser,
        flag_name="--known-news-path",
        help_text="Optional path to the known-news calendar file.",
    )
    parser.add_argument(
        "--known-news-format",
        choices=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
        help="Known-news calendar file format.",
    )
    parser.add_argument(
        "--known-news-provenance-label",
        help="Stable provenance label recorded for the known-news source.",
    )
    parser.add_argument(
        "--known-news-timezone",
        help="Optional timezone for known-news timestamps.",
    )
    parser.add_argument(
        "--known-news-timestamp-column",
        help="Known-news timestamp column name for csv or parquet inputs.",
    )
    parser.add_argument(
        "--known-news-event-id-column",
        help="Optional known-news event identifier column name.",
    )

    _add_optional_path_argument(
        parser=parser,
        flag_name="--option-surface-path",
        help_text="Optional path to the option-surface file.",
    )
    parser.add_argument(
        "--option-surface-format",
        choices=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
        help="Option-surface file format.",
    )
    parser.add_argument(
        "--option-surface-provenance-label",
        help="Stable provenance label recorded for the option-surface source.",
    )
    parser.add_argument(
        "--option-quote-input-policy",
        choices=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
        help="Explicit option-quote normalization policy.",
    )
    parser.add_argument(
        "--option-quote-timestamp-column",
        help="Option-surface quote timestamp column name.",
    )
    parser.add_argument(
        "--option-expiry-column",
        help="Option-surface expiry column name.",
    )
    parser.add_argument(
        "--option-strike-column",
        help="Option-surface strike column name.",
    )
    parser.add_argument(
        "--option-type-column",
        help="Option-surface option type column name.",
    )
    parser.add_argument(
        "--option-mid-price-column",
        help="Option-surface mid-price column name.",
    )
    parser.add_argument(
        "--option-bid-column",
        help="Option-surface bid column name.",
    )
    parser.add_argument(
        "--option-ask-column",
        help="Option-surface ask column name.",
    )

    parser.add_argument(
        "--option-implied-volatility-column",
        help="Option-surface implied volatility column name.",
    )

    parser.add_argument(
        "--timestamp-timezone",
        required=True,
        help="Timezone used to normalize tick-data timestamps before staged translation.",
    )
    parser.add_argument(
        "--duplicate-record-policy",
        choices=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
        default=CalibrationDuplicateRecordPolicy.ERROR.value,
        help="Duplicate-record normalization policy recorded in provenance.",
    )
    parser.add_argument(
        "--disable-timestamp-sort",
        action="store_true",
        help="Record that source data should not be re-sorted by timestamp before translation.",
    )
    parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.DISK.value,
        help=CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
    )
    _add_optional_path_argument(
        parser=parser,
        flag_name="--disk-checkpoint-directory",
        directory=True,
        help_text=(
            "Disk checkpoint directory recorded in output_preferences. Required when "
            "--execution-mode disk is selected."
        ),
    )
    parser.add_argument(
        "--emit-derived-staged-execution-input",
        action="store_true",
        help="Request emission of the derived staged execution input for replay or debugging.",
    )
    parser.add_argument(
        "--omit-input-provenance-report",
        action="store_true",
        help="Disable provenance-report emission in output_preferences.",
    )


def _add_staged_calibration_arguments(parser: ArgumentParser) -> None:
    from .._constants import CLI_CALIBRATION_EXECUTION_MODE_HELP

    _add_command_config_argument(parser)
    _add_output_mode_argument(
        parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        parser,
        flag_name="--execution-input-json",
        contract_label="a staged calibration execution-input JSON object",
        semantic_note=(
            "The execution input must provide explicit known-news return samples and raw "
            "close-open boundary observations for internal gap detection."
        ),
    )
    _add_required_json_path_argument(
        parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object consumed by the staged calibration runner",
    )
    parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.DISK.value,
        help=CLI_CALIBRATION_EXECUTION_MODE_HELP,
    )
    _add_optional_path_argument(
        parser=parser,
        flag_name="--disk-checkpoint-directory",
        directory=True,
        help_text=(
            "Checkpoint directory override. Use only with --execution-mode disk. When "
            "provided, this overrides runtime_resources.disk_checkpoint_directory from "
            "--execution-input-json."
        ),
    )


__all__ = [
    "_add_command_config_argument",
    "_add_model_workflow_fit_arguments",
    "_add_model_workflow_simulate_arguments",
    "_add_market_data_calibration_arguments",
    "_add_optional_path_argument",
    "_add_output_mode_argument",
    "_add_required_json_path_argument",
]
