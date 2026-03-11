import json
import os
import sys
from argparse import ArgumentParser, Namespace
from collections.abc import Mapping
from pathlib import Path
from typing import Any, NoReturn, cast

from ._commands import _load_json_document, _resolve_path_from_repository_root
from ._constants import (
    CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
    CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
    CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
    CLI_CALIBRATION_EXECUTION_MODE_HELP,
    CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
    CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
    CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
    CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
    CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
    CLI_COMMAND_CONFIG_HELP,
    CLI_COMMAND_CONFIG_SCHEMA_VERSION,
    CLI_CONFIG_JSON_ENV_VAR,
    CLI_DIRECTORY_METAVAR,
    CLI_OUTPUT_ENV_VAR,
    CLI_OUTPUT_MODE_CHOICES,
    CLI_OUTPUT_MODE_HELP,
    CLI_PATH_METAVAR,
    CLI_PRIMARY_COMMAND_IDENTIFIERS,
    CLI_PRIMARY_COMMAND_TREE,
    CLI_REPOSITORY_RELATIVE_PATH_POLICY,
    CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
    CLI_SEASONALITY_SESSION_FILTER_CHOICES,
    CliOutputMode,
    DEFAULT_CALENDAR_FIXTURE_PATH,
    DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH,
)
from .. import __version__
from ..calibration import CalibrationDuplicateRecordPolicy, CalibrationExecutionMode
from ..seasonality import IntradaySmoothingKernel, SessionFilter
from ..errors import ContractValidationFailure, JaniformValidationError


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


def _build_simulation_parser(subparsers) -> None:
    from ._commands import (
        _run_simulation_coarse_interval_command,
        _run_simulation_seeded_ticks_command,
    )

    simulation_parser = subparsers.add_parser(
        "simulation",
        help="Simulation workflows.",
        description="Simulation workflows.",
    )
    simulation_parser.set_defaults(help_target=simulation_parser)
    simulation_subparsers = simulation_parser.add_subparsers(dest="simulation_command")

    simulation_coarse_interval_parser = simulation_subparsers.add_parser(
        "coarse-interval",
        help="Direct coarse-interval path simulation workflow.",
        description=(
            "Direct coarse-interval path simulation workflow. Use this command when you want the "
            "canonical coarse simulation path artifact at interval length Delta without replaying "
            "fine-scale seeded ticks."
        ),
    )
    simulation_coarse_interval_parser.set_defaults(
        handler=_run_simulation_coarse_interval_command,
        help_target=simulation_coarse_interval_parser,
    )

    _add_command_config_argument(simulation_coarse_interval_parser)
    _add_output_mode_argument(
        simulation_coarse_interval_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        simulation_coarse_interval_parser,
        flag_name="--request-json",
        contract_label="a coarse-interval simulation request JSON object",
        semantic_note=(
            "Use coarse-interval simulation for direct Delta-spaced paths. Use seeded-ticks when "
            "you need a priced fine path or tick replay."
        ),
    )

    simulation_seeded_ticks_parser = simulation_subparsers.add_parser(
        "seeded-ticks",
        help="Seeded tick priced fine-path simulation workflow.",
        description=(
            "Seeded tick priced fine-path simulation workflow. Use this command when you need the "
            "canonical fine-scale priced path, its derived event replay, or direct news and return "
            "component realization rather than coarse-interval aggregation."
        ),
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


def _build_seasonality_parser(subparsers) -> None:
    from ._commands import (
        _run_seasonality_diagnostics_command,
        _run_seasonality_preprocess_timestamps_command,
    )

    seasonality_parser = subparsers.add_parser(
        "seasonality",
        help="Seasonality preprocessing workflows.",
        description="Seasonality preprocessing workflows.",
    )
    seasonality_parser.set_defaults(help_target=seasonality_parser)
    seasonality_subparsers = seasonality_parser.add_subparsers(dest="seasonality_command")

    diagnostics_parser = seasonality_subparsers.add_parser(
        "diagnostics",
        help="Run end-to-end seasonality diagnostics from raw timestamp events.",
        description=(
            "Run end-to-end seasonality diagnostics from raw timestamp events. "
            "This task-oriented workflow validates timestamp records, applies session filtering, "
            "estimates intraday, day-of-week, and day-of-year components, composes the "
            "deterministic seasonality signal, and emits the canonical diagnostics report."
        ),
    )
    diagnostics_parser.set_defaults(
        handler=_run_seasonality_diagnostics_command,
        help_target=diagnostics_parser,
    )

    _add_command_config_argument(diagnostics_parser)
    _add_output_mode_argument(
        diagnostics_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        diagnostics_parser,
        flag_name="--event-records-json",
        contract_label="an event-record document with a top-level `event_records` list",
        semantic_note=(
            "Each record must include a timezone-aware timestamp string compatible with the "
            "public janiform.seasonality preprocessing contract."
        ),
    )
    _add_required_json_path_argument(
        diagnostics_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used for exchange session classification",
    )

    diagnostics_parser.add_argument(
        "--dataset-id",
        required=True,
        help="Stable dataset identifier recorded in the diagnostics report.",
    )
    diagnostics_parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state for all timestamp events.",
    )
    diagnostics_parser.add_argument(
        "--timestamp-field-name",
        default="timestamp",
        help="Timestamp field name inside each event record. Default: timestamp.",
    )
    diagnostics_parser.add_argument(
        "--session-filter",
        choices=CLI_SEASONALITY_SESSION_FILTER_CHOICES,
        default=SessionFilter.OPEN_ONLY.value,
        help=(
            "Session filter applied after exchange calendar classification. "
            "Default: open_only."
        ),
    )
    diagnostics_parser.add_argument(
        "--sparse-hour-event-threshold",
        type=int,
        default=1,
        help="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-minutes-per-bin",
        type=int,
        default=1,
        help="Intraday estimator bin size in minutes. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-smoothing-kernel",
        choices=CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
        default=IntradaySmoothingKernel.GAUSSIAN.value,
        help="Intraday estimator smoothing kernel. Default: gaussian.",
    )
    diagnostics_parser.add_argument(
        "--intraday-smoothing-bandwidth-bins",
        type=float,
        default=3.0,
        help="Intraday estimator smoothing bandwidth measured in bins. Default: 3.0.",
    )
    diagnostics_parser.add_argument(
        "--intraday-minimum-bin-events",
        type=int,
        default=1,
        help="Minimum events per intraday bin before the bin is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--intraday-pseudocount",
        type=float,
        default=1e-6,
        help="Intraday estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--day-of-week-minimum-bucket-events",
        type=int,
        default=1,
        help="Minimum events per day-of-week bucket before the bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--day-of-week-pseudocount",
        type=float,
        default=1e-6,
        help="Day-of-week estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--day-of-year-minimum-bucket-events",
        type=int,
        default=1,
        help="Minimum events per day-of-year bucket before the bucket is flagged as sparse. Default: 1.",
    )
    diagnostics_parser.add_argument(
        "--day-of-year-pseudocount",
        type=float,
        default=1e-6,
        help="Day-of-year estimator pseudocount added before normalization. Default: 1e-6.",
    )
    diagnostics_parser.add_argument(
        "--estimator-version",
        default="seasonality_estimator.v1",
        help="Estimator version label recorded in reproducibility metadata. Default: seasonality_estimator.v1.",
    )

    preprocess_timestamps_parser = seasonality_subparsers.add_parser(
        "preprocess-timestamps",
        help="Preprocess timestamp events against the exchange calendar registry.",
        description=(
            "Preprocess timestamp events against the exchange calendar registry. "
            "This standalone step validates timestamp records, applies session filtering, "
            "and emits deterministic preprocessing diagnostics for downstream "
            "seasonality or calibration workflows."
        ),
    )
    preprocess_timestamps_parser.set_defaults(
        handler=_run_seasonality_preprocess_timestamps_command,
        help_target=preprocess_timestamps_parser,
    )

    _add_command_config_argument(preprocess_timestamps_parser)
    _add_output_mode_argument(
        preprocess_timestamps_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        preprocess_timestamps_parser,
        flag_name="--event-records-json",
        contract_label="an event-record document with a top-level `event_records` list",
        semantic_note=(
            "Each record must include a timezone-aware timestamp string compatible with the "
            "public janiform.seasonality preprocessing contract."
        ),
    )
    _add_required_json_path_argument(
        preprocess_timestamps_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used for exchange session classification",
    )

    preprocess_timestamps_parser.add_argument(
        "--exchange-code",
        required=True,
        help="Exchange code used to resolve session state for all timestamp events.",
    )
    preprocess_timestamps_parser.add_argument(
        "--timestamp-field-name",
        default="timestamp",
        help="Timestamp field name inside each event record. Default: timestamp.",
    )
    preprocess_timestamps_parser.add_argument(
        "--session-filter",
        choices=CLI_SEASONALITY_SESSION_FILTER_CHOICES,
        default=SessionFilter.OPEN_ONLY.value,
        help=(
            "Session filter applied after exchange calendar classification. "
            "Default: open_only."
        ),
    )
    preprocess_timestamps_parser.add_argument(
        "--sparse-hour-event-threshold",
        type=int,
        default=1,
        help="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1.",
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


def _build_hourly_factors_parser(subparsers) -> None:
    from ._commands import (
        _run_hourly_factor_build_contract_command,
        _run_hourly_factor_materialize_news_intensity_command,
        _run_hourly_factor_materialize_seasonality_command,
    )

    hourly_factors_parser = subparsers.add_parser(
        "hourly-factors",
        help="Hourly factor materialization and contract-building workflows.",
        description="Hourly factor materialization and contract-building workflows.",
    )
    hourly_factors_parser.set_defaults(help_target=hourly_factors_parser)
    hourly_factors_subparsers = hourly_factors_parser.add_subparsers(dest="hourly_factors_command")

    materialize_seasonality_parser = hourly_factors_subparsers.add_parser(
        "materialize-seasonality",
        help="Materialize interval-aligned seasonality factors from timestamped source samples.",
        description=(
            "Materialize interval-aligned seasonality factors from timestamped source samples. "
            "The command preserves the canonical seasonality materialization report shape."
        ),
    )
    materialize_seasonality_parser.set_defaults(
        handler=_run_hourly_factor_materialize_seasonality_command,
        help_target=materialize_seasonality_parser,
    )

    _add_hourly_factor_common_arguments(materialize_seasonality_parser)
    _add_required_json_path_argument(
        materialize_seasonality_parser,
        flag_name="--materialization-input-json",
        contract_label=(
            "an hourly seasonality materialization input document with source timestamps, source factors, "
            "and optional materialization settings"
        ),
    )

    materialize_news_intensity_parser = hourly_factors_subparsers.add_parser(
        "materialize-news-intensity",
        help="Materialize interval-aligned news-intensity factors from known-news and surprise controls.",
        description=(
            "Materialize interval-aligned news-intensity factors from known-news and surprise controls. "
            "The command preserves the canonical news-intensity materialization report shape."
        ),
    )
    materialize_news_intensity_parser.set_defaults(
        handler=_run_hourly_factor_materialize_news_intensity_command,
        help_target=materialize_news_intensity_parser,
    )

    _add_hourly_factor_common_arguments(materialize_news_intensity_parser)
    _add_required_json_path_argument(
        materialize_news_intensity_parser,
        flag_name="--materialization-input-json",
        contract_label=(
            "an hourly news-intensity materialization input document with known-news timestamps, "
            "surprise baseline intensity, and optional interval multipliers"
        ),
    )

    build_contract_parser = hourly_factors_subparsers.add_parser(
        "build-contract",
        help="Build the combined interval-factor contract for downstream simulation or pricing workflows.",
        description=(
            "Build the combined interval-factor contract for downstream simulation or pricing workflows. "
            "Seasonality and news-intensity inputs can come from explicit factor source documents "
            "or from canonical materialization report files produced by Janiform."
        ),
    )
    build_contract_parser.set_defaults(
        handler=_run_hourly_factor_build_contract_command,
        help_target=build_contract_parser,
    )

    _add_hourly_factor_common_arguments(build_contract_parser)

    seasonality_source_group = build_contract_parser.add_mutually_exclusive_group(required=True)
    seasonality_source_group.add_argument(
        "--seasonality-explicit-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to an explicit seasonality source document with `hourly_factors` and optional "
            f"`materialization_method`. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    seasonality_source_group.add_argument(
        "--seasonality-materialized-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to a canonical hourly seasonality materialization report produced by "
            "`hourly-factors materialize-seasonality`. "
            f"{CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )

    news_source_group = build_contract_parser.add_mutually_exclusive_group(required=True)
    news_source_group.add_argument(
        "--news-intensity-explicit-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to an explicit news-intensity source document with `hourly_factors` and optional "
            f"`materialization_method`. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    news_source_group.add_argument(
        "--news-intensity-materialized-source-json",
        type=Path,
        metavar=CLI_PATH_METAVAR,
        help=(
            "Path to a canonical hourly news-intensity materialization report produced by "
            "`hourly-factors materialize-news-intensity`. "
            f"{CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
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
    calibration_market_data_parser.add_argument(
        "--tick-trade-sign-column",
        help="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
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
        metavar=CLI_PATH_METAVAR,
        help=(
            "Optional path to an explicit session-definition file override. "
            f"{CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
    )
    calibration_market_data_parser.add_argument(
        "--session-definition-format",
        choices=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
        help="Explicit session-definition file format when --session-definition-path is provided.",
    )
    calibration_market_data_parser.add_argument(
        "--session-definition-provenance-label",
        help="Stable provenance label recorded for the explicit session-definition source.",
    )
    calibration_market_data_parser.add_argument(
        "--session-timezone",
        help="Optional timezone hint for automatic session inference or explicit session overrides.",
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
    from ._commands import (
        _run_calibration_market_data_command,
        _run_calibration_staged_command,
    )

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
    from ._commands import _run_pricing_vanilla_option_command

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


def _build_stylized_facts_parser(subparsers) -> None:
    from ._commands import _run_stylized_facts_horizon_comparison_command

    stylized_facts_parser = subparsers.add_parser(
        "stylized-facts",
        help="Stylized-fact reporting workflows.",
        description="Stylized-fact reporting workflows.",
    )
    stylized_facts_parser.set_defaults(help_target=stylized_facts_parser)
    stylized_facts_subparsers = stylized_facts_parser.add_subparsers(dest="stylized_facts_command")

    horizon_comparison_parser = stylized_facts_subparsers.add_parser(
        "horizon-comparison",
        help="Generate a stylized-fact horizon-comparison report bundle.",
        description=(
            "Generate a stylized-fact horizon-comparison report bundle from a metric catalog, "
            "a comparison request, and optional artifact-export settings."
        ),
    )
    horizon_comparison_parser.set_defaults(
        handler=_run_stylized_facts_horizon_comparison_command,
        help_target=horizon_comparison_parser,
    )

    _add_command_config_argument(horizon_comparison_parser)
    _add_output_mode_argument(
        horizon_comparison_parser,
        default_output_mode=CliOutputMode.JSON,
    )
    _add_required_json_path_argument(
        horizon_comparison_parser,
        flag_name="--metric-catalog-json",
        contract_label="a stylized-fact metric catalog JSON object",
    )
    _add_required_json_path_argument(
        horizon_comparison_parser,
        flag_name="--comparison-request-json",
        contract_label="a stylized-fact horizon-comparison request JSON object",
    )
    _add_optional_path_argument(
        parser=horizon_comparison_parser,
        flag_name="--artifact-export-json",
        help_text=(
            "Optional path to a stylized-fact artifact-export config JSON object. "
            "This payload is passed through the public workflow validation path and requires "
            "--execution-mode disk."
        ),
    )

    horizon_comparison_parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.MEMORY.value,
        help=(
            "Execution mode passed through to janiform.stylized_facts. Default: memory. "
            "Use disk when artifact export is requested."
        ),
    )


def _build_developer_parser(subparsers) -> None:
    from ._commands import (
        _run_deterministic_regression_command,
        _run_open_closed_gap_leakage_regression_command,
    )

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

    deterministic_regression_parser = regression_subparsers.add_parser(
        "deterministic",
        help="Run deterministic calendar and hourly-factor regression harness.",
        description="Run deterministic calendar and hourly-factor regression harness.",
    )
    deterministic_regression_parser.set_defaults(
        handler=_run_deterministic_regression_command,
        help_target=deterministic_regression_parser,
    )

    _add_output_mode_argument(
        deterministic_regression_parser,
        default_output_mode=CliOutputMode.HUMAN,
    )
    _add_required_json_path_argument(
        deterministic_regression_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object consumed by the deterministic regression harness",
    )
    _add_required_json_path_argument(
        deterministic_regression_parser,
        flag_name="--calendar-cases-json",
        contract_label="a calendar regression case document with a top-level `cases` list",
    )
    _add_required_json_path_argument(
        deterministic_regression_parser,
        flag_name="--seasonality-cases-json",
        contract_label=(
            "an hourly-factor regression case document with a top-level `canonical_output_cases` list"
        ),
    )

    deterministic_regression_parser.add_argument(
        "--seasonality-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for deterministic seasonality factor mismatch detection.",
    )
    deterministic_regression_parser.add_argument(
        "--news-intensity-absolute-tolerance",
        type=float,
        default=1e-12,
        help="Absolute tolerance for deterministic news-intensity mismatch detection.",
    )

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
    _build_seasonality_parser(subparsers)
    _build_hourly_factors_parser(subparsers)
    _build_calibration_parser(subparsers)
    _build_pricing_parser(subparsers)
    _build_stylized_facts_parser(subparsers)
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

    repository_root = Path(__file__).resolve().parents[3]
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

__all__ = [
    '_add_command_config_argument',
    '_add_market_data_calibration_arguments',
    '_add_optional_path_argument',
    '_add_output_mode_argument',
    '_add_required_json_path_argument',
    '_apply_cli_argument_defaults',
    '_build_calibration_parser',
    '_build_command_config_argument_vector',
    '_build_configurable_action_map',
    '_build_developer_parser',
    '_build_hourly_factors_parser',
    '_build_parser',
    '_build_environment_argument_vector',
    '_build_pricing_parser',
    '_build_simulation_parser',
    '_build_stylized_facts_parser',
    '_extract_explicit_option_value',
    '_extract_primary_command_path',
    '_load_cli_command_config_document',
    '_parse_cli_arguments',
    '_raise_cli_argument_resolution_error',
    '_resolve_command_config_path',
    '_resolve_command_parser',
    '_select_long_option_string',
]
