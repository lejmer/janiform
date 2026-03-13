from collections.abc import Callable, Mapping
from pathlib import Path
import sys
from typing import Any, Optional, cast

import click

from ._constants import (
    CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
    CLI_CALIBRATION_EXECUTION_MODE_HELP,
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
    CliExitCode,
    CliOutputMode,
    DEFAULT_CALENDAR_FIXTURE_PATH,
    DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH,
)
from .commands.calibration import (
    _run_calibration_market_data_command,
    _run_calibration_staged_command,
)
from .commands.development import (
    _run_deterministic_regression_command,
    _run_open_closed_gap_leakage_regression_command,
)
from .commands.hourly_factors import (
    _run_hourly_factor_build_contract_command,
    _run_hourly_factor_materialize_news_intensity_command,
    _run_hourly_factor_materialize_seasonality_command,
)
from .commands.model_workflows import (
    _run_fit_command,
    _run_model_simulate_command,
)
from .commands.presets import (
    _run_presets_export_command,
    _run_presets_list_command,
    _run_presets_show_command,
)
from .commands.pricing import _run_pricing_vanilla_option_command
from .commands.seasonality import (
    _run_seasonality_diagnostics_command,
    _run_seasonality_preprocess_timestamps_command,
)
from .commands.simulation import (
    _run_simulation_coarse_interval_command,
    _run_simulation_seeded_ticks_command,
)
from .commands.stylized_facts import _run_stylized_facts_horizon_comparison_command
from .configuration import build_click_default_map
from .context import CliCommandContext
from .paths import resolve_repository_root
from .. import __version__

type CliHandler = Callable[..., int]

_UNSET_OPTION_DEFAULT = object()


class _JaniformCommandMixin:
    def __init__(
        self,
        *args: Any,
        command_path: tuple[str, ...] | None = None,
        environment: Mapping[str, str] | None = None,
        **kwargs: Any,
    ) -> None:
        self._command_path = command_path
        self._environment = environment

        super().__init__(*args, **kwargs)

    def make_context(
        self,
        info_name: str | None,
        args: list[str],
        parent: click.Context | None = None,
        **extra: Any,
    ) -> click.Context:
        if self._command_path is not None and not _arguments_target_known_subcommand(self, args):
            command_default_map = build_click_default_map(
                self,
                command_path=self._command_path,
                arguments=args,
                environment=self._environment,
            )
            inherited_default_map = cast(dict[str, Any], extra.get("default_map") or {})
            extra["default_map"] = {**inherited_default_map, **command_default_map}

        return super().make_context(info_name, args, parent=parent, **extra)


class JaniformCommand(_JaniformCommandMixin, click.Command):
    pass


class JaniformGroup(_JaniformCommandMixin, click.Group):
    pass


def _arguments_target_known_subcommand(command: click.Command, arguments: list[str]) -> bool:
    if not isinstance(command, click.Group):
        return False

    subcommand_names = set(command.list_commands(click.Context(command)))
    return any(argument in subcommand_names for argument in arguments)


def _build_app(*, environment: Mapping[str, str] | None = None) -> click.Command:
    return _build_root_group(environment=environment)


def _invoke_application(
    arguments: list[str] | None = None,
    *,
    environment: Mapping[str, str] | None = None,
) -> int:
    effective_arguments = list(sys.argv[1:] if arguments is None else arguments)
    result = _invoke_click_command(_build_app(environment=environment), effective_arguments)
    return int(CliExitCode.SUCCESS if result is None else result)


def _invoke_click_command(command: click.Command, arguments: list[str]) -> int | None:
    try:
        return cast(
            Optional[int],
            command.main(
                args=arguments,
                prog_name="janiform",
                standalone_mode=False,
            ),
        )
    except click.exceptions.Exit as e:
        raise SystemExit(e.exit_code) from e
    except click.ClickException as e:
        e.show()
        raise SystemExit(e.exit_code) from e


def _command_identifier_for_path(command_path: tuple[str, ...]) -> str | None:
    from ._constants import CLI_PRIMARY_COMMAND_IDENTIFIERS

    return CLI_PRIMARY_COMMAND_IDENTIFIERS.get(command_path)


def _command_option(
    flag_name: str,
    *,
    help_text: str,
    choice_options: tuple[str, ...] | None = None,
    default: Any = _UNSET_OPTION_DEFAULT,
    directory: bool = False,
    is_flag: bool = False,
    multiple: bool = False,
    path_type: bool = False,
    required: bool = False,
    value_type: click.ParamType | type[Any] | None = None,
    metavar: str | None = None,
    show_default: bool = False,
) -> click.Option:
    option_type: click.ParamType | type[Any] | None = value_type

    if choice_options is not None:
        option_type = click.Choice(choice_options, case_sensitive=True)
        metavar = metavar or "{" + ",".join(choice_options) + "}"
    elif path_type:
        option_type = click.Path(
            exists=False,
            dir_okay=directory,
            file_okay=not directory,
            path_type=Path,
        )
        metavar = metavar or (CLI_DIRECTORY_METAVAR if directory else CLI_PATH_METAVAR)
    elif option_type is None and not is_flag:
        option_type = str

    option_arguments: dict[str, Any] = {
        "help": help_text,
        "is_flag": is_flag,
        "metavar": metavar or _option_metavar(flag_name),
        "multiple": multiple,
        "required": required,
        "show_default": show_default,
        "type": option_type,
    }

    if default is not _UNSET_OPTION_DEFAULT:
        option_arguments["default"] = default

    return click.Option(
        [flag_name],
        **option_arguments,
    )


def _argument(
    argument_name: str,
    *,
    choice_options: tuple[str, ...] | None = None,
    metavar: str | None = None,
) -> click.Argument:
    argument_type: click.ParamType | type[Any]
    if choice_options is None:
        argument_type = str
    else:
        argument_type = click.Choice(choice_options, case_sensitive=True)
        metavar = metavar or "{" + ",".join(choice_options) + "}"

    return click.Argument(
        [argument_name],
        metavar=metavar or argument_name.upper(),
        type=argument_type,
    )


def _option_metavar(flag_name: str) -> str:
    return flag_name[2:].replace("-", "_").upper()


def _path_help_text(help_text: str) -> str:
    return f"{help_text} {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"


def _command_config_option() -> click.Option:
    return _command_option(
        "--config-json",
        help_text=_path_help_text(CLI_COMMAND_CONFIG_HELP),
        path_type=True,
    )


def _output_option(*, default_output_mode: CliOutputMode) -> click.Option:
    return _command_option(
        "--output",
        choice_options=CLI_OUTPUT_MODE_CHOICES,
        default=default_output_mode.value,
        help_text=f"{CLI_OUTPUT_MODE_HELP} Default: {default_output_mode.value}.",
        show_default=False,
    )


def _required_json_path_option(
    flag_name: str,
    *,
    contract_label: str,
    semantic_note: str | None = None,
) -> click.Option:
    help_segments: list[str] = [f"Path to {contract_label}.", CLI_REPOSITORY_RELATIVE_PATH_POLICY]
    if semantic_note is not None:
        help_segments.append(semantic_note)

    return _command_option(
        flag_name,
        help_text=" ".join(help_segments),
        path_type=True,
        required=True,
    )


def _optional_path_option(
    flag_name: str,
    *,
    help_text: str,
    directory: bool = False,
    default: Path | None = None,
) -> click.Option:
    return _command_option(
        flag_name,
        default=default,
        directory=directory,
        help_text=_path_help_text(help_text),
        path_type=True,
    )


def _required_datetime_option(
    flag_name: str,
    *,
    help_text: str,
    required: bool = True,
) -> click.Option:
    return _command_option(
        flag_name,
        help_text=f"{help_text} Use an ISO 8601 timezone-aware timestamp.",
        required=required,
    )


def _user_preset_json_option() -> click.Option:
    return click.Option(
        ["--user-preset-json"],
        help=(
            "Optional preset artifact JSON to register in the in-memory user registry before "
            f"command resolution. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
        multiple=True,
        metavar=CLI_PATH_METAVAR,
        type=click.Path(exists=False, dir_okay=False, file_okay=True, path_type=Path),
    )


def _preset_scope_option(
    flag_name: str,
    *,
    help_text: str,
    include_auto: bool = False,
) -> click.Option:
    choices = CLI_PRESET_SCOPE_CHOICES if include_auto else CLI_PRESET_SCOPE_CHOICES[1:]
    return _command_option(
        flag_name,
        choice_options=choices,
        default="auto" if include_auto else None,
        help_text=help_text,
    )


def _model_workflow_fit_options(*, enforce_required: bool = True) -> list[click.Parameter]:
    return [
        _command_config_option(),
        _output_option(default_output_mode=CliOutputMode.JSON),
        _user_preset_json_option(),
        _command_option(
            "--market",
            help_text="Optional market-profile preset name used as the high-level workflow baseline.",
        ),
        _preset_scope_option(
            "--market-scope",
            help_text="Registry scope for --market. Default: auto.",
            include_auto=True,
        ),
        _optional_path_option(
            "--session-json",
            help_text=(
                "Optional explicit session JSON override. Accepts a calendar.registry.v1 document or "
                "a SessionExplicitConfiguration JSON object."
            ),
        ),
        _command_option(
            "--session-preset",
            help_text="Optional session preset name. When omitted, Janiform may infer the session from data.",
        ),
        _preset_scope_option(
            "--session-scope",
            help_text="Registry scope for --session-preset. Default: auto.",
            include_auto=True,
        ),
        _command_option(
            "--known-news",
            help_text=(
                "Optional known-news preset name. Use `none` to keep the bundled off-by-default "
                "baseline explicit."
            ),
        ),
        _optional_path_option(
            "--known-news-json",
            help_text=(
                "Optional explicit known-news configuration JSON override using the "
                "KnownNewsExplicitConfiguration shape."
            ),
        ),
        _preset_scope_option(
            "--known-news-scope",
            help_text="Registry scope for --known-news. Default: auto.",
            include_auto=True,
        ),
        _command_option(
            "--exchange-code",
            help_text=(
                "Optional exchange-code hint for session inference when neither the selected market "
                "profile nor explicit session input already supplies it."
            ),
        ),
        _command_option(
            "--session-timezone",
            help_text="Optional timezone hint for automatic session inference.",
        ),
        _command_option(
            "--run-id",
            help_text="Stable run identifier recorded on the fitted model and fit report.",
            required=enforce_required,
        ),
        _command_option(
            "--workflow-scope",
            choice_options=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
            help_text=(
                "High-level calibration scope. Use seasonality_only for activity-only estimation, "
                "full_without_option_surface for full calibration from tick data only, and "
                "full_with_option_surface when option-surface data is supplied."
            ),
            required=enforce_required,
        ),
        _command_option(
            "--tick-data-path",
            help_text=_path_help_text("Path to the input tick-data file."),
            path_type=True,
            required=enforce_required,
        ),
        _command_option(
            "--tick-data-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Tick-data file format.",
            required=enforce_required,
        ),
        _command_option(
            "--tick-data-provenance-label",
            default="tick_data",
            help_text="Stable provenance label recorded for the tick-data source. Default: tick_data.",
        ),
        _command_option(
            "--tick-timestamp-column",
            help_text="Tick-data timestamp column name.",
            required=enforce_required,
        ),
        _command_option(
            "--tick-price-input-policy",
            choice_options=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
            help_text="Explicit tick-price normalization policy for non-seasonality workflows.",
        ),
        _command_option("--tick-price-column", help_text="Tick-data explicit price column."),
        _command_option("--tick-trade-price-column", help_text="Tick-data trade price column."),
        _command_option(
            "--tick-trade-sign-column",
            help_text="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
        ),
        _command_option("--tick-bid-column", help_text="Tick-data bid column."),
        _command_option("--tick-ask-column", help_text="Tick-data ask column."),
        _command_option("--tick-close-price-column", help_text="Tick-data close price column."),
        _command_option(
            "--timestamp-timezone",
            help_text="Timezone used to normalize tick-data timestamps before fitting.",
            required=enforce_required,
        ),
        _command_option(
            "--duplicate-record-policy",
            choice_options=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
            default="error",
            help_text="Duplicate-record normalization policy recorded in provenance.",
        ),
        _command_option(
            "--disable-timestamp-sort",
            help_text="Record that source data should not be re-sorted by timestamp before translation.",
            is_flag=True,
        ),
        _optional_path_option("--option-surface-path", help_text="Optional path to the option-surface file."),
        _command_option(
            "--option-surface-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Option-surface file format. Default when provided: parquet.",
        ),
        _command_option(
            "--option-surface-provenance-label",
            help_text="Stable provenance label recorded for the option-surface source. Default when provided: option_surface.",
        ),
        _command_option(
            "--option-quote-input-policy",
            choice_options=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
            help_text="Explicit option-quote normalization policy.",
        ),
        _command_option("--option-quote-timestamp-column", help_text="Option-surface quote timestamp column name."),
        _command_option("--option-expiry-column", help_text="Option-surface expiry column name."),
        _command_option("--option-strike-column", help_text="Option-surface strike column name."),
        _command_option("--option-type-column", help_text="Option-surface option type column name."),
        _command_option("--option-mid-price-column", help_text="Option-surface mid-price column name."),
        _command_option("--option-bid-column", help_text="Option-surface bid column name."),
        _command_option("--option-ask-column", help_text="Option-surface ask column name."),
        _command_option(
            "--option-implied-volatility-column",
            help_text="Option-surface implied volatility column name.",
        ),
        _command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="memory",
            help_text=(
                "Execution mode recorded in the high-level fit workflow. Default: memory. "
                "Use disk when you want checkpoint artifacts in addition to the fitted model."
            ),
        ),
        _optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Disk checkpoint directory recorded in output_preferences. Required when "
                "--execution-mode disk is selected."
            ),
        ),
        _command_option(
            "--emit-derived-staged-execution-input",
            help_text="Request emission of the derived staged execution input for replay or debugging.",
            is_flag=True,
        ),
        _command_option(
            "--omit-input-provenance-report",
            help_text="Disable provenance-report emission in output_preferences.",
            is_flag=True,
        ),
        _command_option(
            "--model-output-path",
            help_text=_path_help_text("Optional path where the fitted JaniformModel should be persisted."),
            path_type=True,
        ),
    ]


def _model_workflow_simulate_options(
    *,
    require_model_path: bool,
    include_shared_arguments: bool = True,
    enforce_required: bool = True,
) -> list[click.Parameter]:
    parameters: list[click.Parameter] = []
    if include_shared_arguments:
        parameters.append(_command_config_option())
        parameters.append(_output_option(default_output_mode=CliOutputMode.JSON))

    if require_model_path:
        parameters.append(
            _command_option(
                "--model-path",
                help_text=_path_help_text("Path to a persisted JaniformModel JSON payload."),
                path_type=True,
                required=enforce_required,
            )
        )

    parameters.extend(
        [
            _command_option("--seed", help_text="Simulation random seed.", required=enforce_required, value_type=int),
            _required_datetime_option(
                "--start-time-utc",
                help_text="Simulation start time in UTC.",
                required=enforce_required,
            ),
            _command_option(
                "--horizon-interval-count",
                help_text="Number of simulation intervals to generate.",
                required=enforce_required,
                value_type=int,
            ),
            _command_option(
                "--interval",
                default="1h",
                help_text="Simulation interval such as `1h` or `30m`. Default: 1h.",
            ),
            _command_option(
                "--simulation-output",
                choice_options=CLI_MODEL_SIMULATION_OUTPUT_CHOICES,
                default="coarse",
                help_text="Model simulation output mode. Default: coarse.",
            ),
            _command_option(
                "--debug-trace-enabled",
                help_text="Enable seeded-tick debug tracing when the simulation output includes ticks.",
                is_flag=True,
            ),
            _command_option(
                "--sample-every-nth-transition",
                default=1,
                help_text="Keep every nth transition in the seeded-tick trace. Default: 1.",
                value_type=int,
            ),
            _command_option(
                "--max-records-per-chunk",
                default=1024,
                help_text="Maximum seeded-tick records emitted per chunk. Default: 1024.",
                value_type=int,
            ),
        ]
    )
    return parameters


def _hourly_factor_common_options() -> list[click.Parameter]:
    return [
        _command_config_option(),
        _output_option(default_output_mode=CliOutputMode.JSON),
        _required_json_path_option(
            "--calendar-registry-json",
            contract_label="a calendar registry JSON object used to resolve exchange-local interval labels",
        ),
        _command_option(
            "--exchange-code",
            help_text="Exchange code used to resolve session state and interval-factor alignment.",
            required=True,
        ),
        _command_option(
            "--interval",
            default="1h",
            help_text=(
                "Fixed-width UTC interval specification used to derive the target grid, "
                "for example 1h or 15min. Default: 1h."
            ),
        ),
        _required_datetime_option(
            "--range-start-utc",
            help_text="Inclusive UTC range start aligned to the first target interval boundary.",
        ),
        _required_datetime_option(
            "--range-end-utc",
            help_text="Exclusive UTC range end aligned to the final target interval boundary.",
        ),
    ]


def _market_data_calibration_options() -> list[click.Parameter]:
    return [
        _command_config_option(),
        _output_option(default_output_mode=CliOutputMode.JSON),
        _command_option(
            "--run-id",
            help_text="Stable run identifier recorded in the market-data request payload.",
            required=True,
        ),
        _command_option(
            "--workflow-scope",
            choice_options=CLI_CALIBRATION_MARKET_DATA_SCOPE_CHOICES,
            help_text=(
                "High-level calibration scope. Use seasonality_only for activity-only estimation, "
                "full_without_option_surface for full calibration from tick data only, and "
                "full_with_option_surface when option-surface data is supplied."
            ),
            required=True,
        ),
        _command_option(
            "--tick-data-path",
            help_text=_path_help_text("Path to the input tick-data file."),
            path_type=True,
            required=True,
        ),
        _command_option(
            "--tick-data-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Tick-data file format.",
            required=True,
        ),
        _command_option(
            "--tick-data-provenance-label",
            help_text="Stable provenance label recorded for the tick-data source.",
            required=True,
        ),
        _command_option(
            "--tick-timestamp-column",
            help_text="Tick-data timestamp column name.",
            required=True,
        ),
        _command_option(
            "--tick-price-input-policy",
            choice_options=CLI_CALIBRATION_TICK_PRICE_POLICY_CHOICES,
            help_text="Explicit tick-price normalization policy for non-seasonality workflows.",
        ),
        _command_option("--tick-price-column", help_text="Tick-data explicit price column."),
        _command_option("--tick-trade-price-column", help_text="Tick-data trade price column."),
        _command_option(
            "--tick-trade-sign-column",
            help_text="Tick-data trade sign column required for canonical full-model Hawkes calibration.",
        ),
        _command_option("--tick-bid-column", help_text="Tick-data bid column."),
        _command_option("--tick-ask-column", help_text="Tick-data ask column."),
        _command_option("--tick-close-price-column", help_text="Tick-data close price column."),
        _command_option(
            "--exchange-code",
            help_text="Exchange code used to resolve session state and hourly factors.",
            required=True,
        ),
        _optional_path_option(
            "--session-definition-path",
            help_text="Optional path to an explicit session-definition file override.",
        ),
        _command_option(
            "--session-definition-format",
            choice_options=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
            help_text="Explicit session-definition file format when --session-definition-path is provided.",
        ),
        _command_option(
            "--session-definition-provenance-label",
            help_text="Stable provenance label recorded for the explicit session-definition source.",
        ),
        _command_option(
            "--session-timezone",
            help_text="Optional timezone hint for automatic session inference or explicit session overrides.",
        ),
        _optional_path_option("--known-news-path", help_text="Optional path to the known-news calendar file."),
        _command_option(
            "--known-news-format",
            choice_options=CLI_CALIBRATION_STRUCTURED_FILE_FORMAT_CHOICES,
            help_text="Known-news calendar file format.",
        ),
        _command_option(
            "--known-news-provenance-label",
            help_text="Stable provenance label recorded for the known-news source.",
        ),
        _command_option("--known-news-timezone", help_text="Optional timezone for known-news timestamps."),
        _command_option(
            "--known-news-timestamp-column",
            help_text="Known-news timestamp column name for csv or parquet inputs.",
        ),
        _command_option(
            "--known-news-event-id-column",
            help_text="Optional known-news event identifier column name.",
        ),
        _optional_path_option("--option-surface-path", help_text="Optional path to the option-surface file."),
        _command_option(
            "--option-surface-format",
            choice_options=CLI_CALIBRATION_TICK_FILE_FORMAT_CHOICES,
            help_text="Option-surface file format.",
        ),
        _command_option(
            "--option-surface-provenance-label",
            help_text="Stable provenance label recorded for the option-surface source.",
        ),
        _command_option(
            "--option-quote-input-policy",
            choice_options=CLI_CALIBRATION_OPTION_QUOTE_POLICY_CHOICES,
            help_text="Explicit option-quote normalization policy.",
        ),
        _command_option("--option-quote-timestamp-column", help_text="Option-surface quote timestamp column name."),
        _command_option("--option-expiry-column", help_text="Option-surface expiry column name."),
        _command_option("--option-strike-column", help_text="Option-surface strike column name."),
        _command_option("--option-type-column", help_text="Option-surface option type column name."),
        _command_option("--option-mid-price-column", help_text="Option-surface mid-price column name."),
        _command_option("--option-bid-column", help_text="Option-surface bid column name."),
        _command_option("--option-ask-column", help_text="Option-surface ask column name."),
        _command_option(
            "--option-implied-volatility-column",
            help_text="Option-surface implied volatility column name.",
        ),
        _command_option(
            "--timestamp-timezone",
            help_text="Timezone used to normalize tick-data timestamps before staged translation.",
            required=True,
        ),
        _command_option(
            "--duplicate-record-policy",
            choice_options=CLI_CALIBRATION_DUPLICATE_RECORD_POLICY_CHOICES,
            default="error",
            help_text="Duplicate-record normalization policy recorded in provenance.",
        ),
        _command_option(
            "--disable-timestamp-sort",
            help_text="Record that source data should not be re-sorted by timestamp before translation.",
            is_flag=True,
        ),
        _command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="disk",
            help_text=CLI_CALIBRATION_MARKET_DATA_EXECUTION_MODE_HELP,
        ),
        _optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Disk checkpoint directory recorded in output_preferences. Required when "
                "--execution-mode disk is selected."
            ),
        ),
        _command_option(
            "--emit-derived-staged-execution-input",
            help_text="Request emission of the derived staged execution input for replay or debugging.",
            is_flag=True,
        ),
        _command_option(
            "--omit-input-provenance-report",
            help_text="Disable provenance-report emission in output_preferences.",
            is_flag=True,
        ),
    ]


def _staged_calibration_options() -> list[click.Parameter]:
    return [
        _command_config_option(),
        _output_option(default_output_mode=CliOutputMode.JSON),
        _required_json_path_option(
            "--execution-input-json",
            contract_label="a staged calibration execution-input JSON object",
            semantic_note=(
                "The execution input must provide explicit known-news return samples and raw "
                "close-open boundary observations for internal gap detection."
            ),
        ),
        _required_json_path_option(
            "--calendar-registry-json",
            contract_label="a calendar registry JSON object consumed by the staged calibration runner",
        ),
        _command_option(
            "--execution-mode",
            choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
            default="disk",
            help_text=CLI_CALIBRATION_EXECUTION_MODE_HELP,
        ),
        _optional_path_option(
            "--disk-checkpoint-directory",
            directory=True,
            help_text=(
                "Checkpoint directory override. Use only with --execution-mode disk. When "
                "provided, this overrides runtime_resources.disk_checkpoint_directory from "
                "--execution-input-json."
            ),
        ),
    ]


def _wrap_handler(
    handler: CliHandler,
    *,
    command_path: tuple[str, ...],
    exclusivity_groups: tuple[tuple[str, ...], ...] = (),
    required_one_of_groups: tuple[tuple[str, ...], ...] = (),
) -> Callable[..., int]:
    @click.pass_context
    def callback(ctx: click.Context, **kwargs: Any) -> int:
        _validate_option_groups(
            kwargs,
            exclusivity_groups=exclusivity_groups,
            required_one_of_groups=required_one_of_groups,
        )

        cli_context = _build_cli_context(ctx, command_path=command_path)
        return handler(cli_context=cli_context, **kwargs)

    return callback


def _wrap_group_default_handler(
    handler: CliHandler,
    *,
    command_path: tuple[str, ...],
    required_option_names: tuple[str, ...],
    exclusivity_groups: tuple[tuple[str, ...], ...] = (),
    required_one_of_groups: tuple[tuple[str, ...], ...] = (),
) -> Callable[..., int | None]:
    @click.pass_context
    def callback(ctx: click.Context, **kwargs: Any) -> int | None:
        if ctx.invoked_subcommand is not None:
            return None

        _validate_required_options(
            ctx,
            values=kwargs,
            required_option_names=required_option_names,
        )
        _validate_option_groups(
            kwargs,
            exclusivity_groups=exclusivity_groups,
            required_one_of_groups=required_one_of_groups,
        )

        cli_context = _build_cli_context(ctx, command_path=command_path)
        return handler(cli_context=cli_context, **kwargs)

    return callback


def _build_cli_context(ctx: click.Context, *, command_path: tuple[str, ...]) -> CliCommandContext:
    return CliCommandContext(
        command_path=command_path,
        command_identifier=_command_identifier_for_path(command_path),
        default_map=dict(ctx.default_map or {}),
        environment=cast(
            Mapping[str, str],
            ctx.find_root().command._environment
            if isinstance(ctx.find_root().command, _JaniformCommandMixin)
            else {},
        ),
        repository_root=resolve_repository_root(),
    )


def _validate_option_groups(
    values: dict[str, Any],
    *,
    exclusivity_groups: tuple[tuple[str, ...], ...],
    required_one_of_groups: tuple[tuple[str, ...], ...],
) -> None:
    for group in exclusivity_groups:
        selected_options = [option_name for option_name in group if values.get(option_name) not in (None, False, [], ())]
        if len(selected_options) > 1:
            option_flags = ", ".join("--" + option_name.replace("_", "-") for option_name in group)
            raise click.UsageError(f"options are mutually exclusive: {option_flags}")

    for group in required_one_of_groups:
        selected_options = [option_name for option_name in group if values.get(option_name) not in (None, False, [], ())]
        if len(selected_options) != 1:
            option_flags = ", ".join("--" + option_name.replace("_", "-") for option_name in group)
            raise click.UsageError(
                f"exactly one of {option_flags} is required",
            )


def _validate_required_options(
    ctx: click.Context,
    *,
    values: dict[str, Any],
    required_option_names: tuple[str, ...],
) -> None:
    for option_name in required_option_names:
        if values.get(option_name) not in (None, [], ()):
            continue

        parameter = _find_option_parameter(ctx.command, option_name)
        if parameter is None:
            raise click.UsageError(f"missing required option --{option_name.replace('_', '-')}")

        raise click.MissingParameter(ctx=ctx, param=parameter)


def _find_option_parameter(command: click.Command, option_name: str) -> click.Option | None:
    for parameter in command.params:
        if isinstance(parameter, click.Option) and parameter.name == option_name:
            return parameter

    return None


def _build_root_group(*, environment: Mapping[str, str] | None = None) -> click.Command:
    @click.group(
        cls=JaniformGroup,
        command_path=(),
        environment=environment,
        invoke_without_command=True,
        help=(
            "Primary workflows are the fit and simulate commands.\n\n"
            "\b\n"
            "Use the fit command to fit a Janiform model from market data.\n"
            "Use the simulate command to simulate from a persisted Janiform model.\n\n"
            "\b\n"
            "See also:\n"
            "  janiform fit --help\n"
            "  janiform simulate --help"
        ),
    )
    @click.version_option(version=__version__)
    @click.pass_context
    def root(ctx: click.Context) -> None:
        if ctx.invoked_subcommand is None:
            click.echo(ctx.get_help())

    _attach_current_tree(root, environment=environment)
    return root


def _attach_current_tree(root: click.Group, *, environment: Mapping[str, str] | None = None) -> None:
    root.add_command(
        JaniformGroup(
            "fit",
            callback=_wrap_group_default_handler(
                _run_fit_command,
                command_path=("fit",),
                required_option_names=(
                    "run_id",
                    "workflow_scope",
                    "tick_data_path",
                    "tick_data_format",
                    "tick_timestamp_column",
                    "timestamp_timezone",
                ),
                exclusivity_groups=(("session_preset", "session_json"), ("known_news", "known_news_json")),
            ),
            command_path=("fit",),
            environment=environment,
            help=(
                "Fit a Janiform model from market data.\n\n"
                "Run `janiform fit` with the options below for the default end-to-end fitting "
                "workflow. Use `fit plan` to inspect the derived fit plan before execution, and "
                "use `fit staged` only when you need direct control over the staged execution boundary."
            ),
            invoke_without_command=True,
            params=_model_workflow_fit_options(enforce_required=False),
        )
    )
    fit_group = cast(click.Group, root.get_command(click.Context(root), "fit"))
    fit_group.add_command(
        JaniformCommand(
            "plan",
            callback=_wrap_handler(_run_calibration_market_data_command, command_path=("fit", "plan")),
            command_path=("fit", "plan"),
            environment=environment,
            help=(
                "Validate market-data inputs, resolve presets and provenance, and build the "
                "derived fit plan without running the staged fit."
            ),
            params=_market_data_calibration_options(),
        )
    )
    fit_group.add_command(
        JaniformCommand(
            "staged",
            callback=_wrap_handler(_run_calibration_staged_command, command_path=("fit", "staged")),
            command_path=("fit", "staged"),
            environment=environment,
            help="Run the explicit staged fit workflow from a prepared execution-input contract.",
            params=_staged_calibration_options(),
        )
    )

    root.add_command(
        JaniformGroup(
            "simulate",
            callback=_wrap_group_default_handler(
                _run_model_simulate_command,
                command_path=("simulate",),
                required_option_names=("model_path", "seed", "start_time_utc", "horizon_interval_count"),
            ),
            command_path=("simulate",),
            environment=environment,
            help=(
                "Simulate from a persisted Janiform model.\n\n"
                "Run `janiform simulate` with the options below for the default model-driven "
                "simulation workflow. Use `simulate coarse-interval` or `simulate seeded-ticks` "
                "when you need direct control over those explicit workflow contracts."
            ),
            invoke_without_command=True,
            params=_model_workflow_simulate_options(
                require_model_path=True,
                enforce_required=False,
            ),
        )
    )
    simulate_group = cast(click.Group, root.get_command(click.Context(root), "simulate"))
    simulate_group.add_command(
        JaniformCommand(
            "coarse-interval",
            callback=_wrap_handler(_run_simulation_coarse_interval_command, command_path=("simulate", "coarse-interval")),
            command_path=("simulate", "coarse-interval"),
            environment=environment,
            help=(
                "Run the explicit coarse-interval simulation workflow when you want the canonical "
                "coarse path artifact without replaying fine-scale seeded ticks."
            ),
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option(
                    "--request-json",
                    contract_label="a coarse-interval simulation request JSON object",
                    semantic_note=(
                        "Use coarse-interval simulation for direct Delta-spaced paths. Use "
                        "seeded-ticks when you need a priced fine path or tick replay."
                    ),
                ),
            ],
        )
    )
    simulate_group.add_command(
        JaniformCommand(
            "seeded-ticks",
            callback=_wrap_handler(_run_simulation_seeded_ticks_command, command_path=("simulate", "seeded-ticks")),
            command_path=("simulate", "seeded-ticks"),
            environment=environment,
            help=(
                "Run the explicit seeded-tick simulation workflow when you need the fine-scale "
                "priced path, event replay, or direct component realizations."
            ),
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option(
                    "--request-json",
                    contract_label="a seeded-tick simulation request JSON object",
                    semantic_note="The request summary must preserve open-jump versus closed-gap hourly factors.",
                ),
                _required_json_path_option(
                    "--calendar-registry-json",
                    contract_label="a calendar registry JSON object used to rehydrate hourly factors",
                ),
            ],
        )
    )

    root.add_command(
        JaniformGroup(
            "presets",
            command_path=("presets",),
            environment=environment,
            help="Inspect bundled and user-registered presets.",
            invoke_without_command=True,
            callback=_group_help_callback,
        )
    )
    presets_group = cast(click.Group, root.get_command(click.Context(root), "presets"))
    presets_group.add_command(
        JaniformCommand(
            "list",
            callback=_wrap_handler(_run_presets_list_command, command_path=("presets", "list")),
            command_path=("presets", "list"),
            environment=environment,
            help="List available presets by category and registry scope.",
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _user_preset_json_option(),
                _command_option("--category", choice_options=CLI_PRESET_CATEGORY_CHOICES, help_text="Optional preset category filter."),
                _preset_scope_option("--scope", help_text="Registry scope filter. Default: auto.", include_auto=True),
            ],
        )
    )
    for command_name, handler, help_text in (
        ("show", _run_presets_show_command, "Show a single preset artifact."),
        ("export", _run_presets_export_command, "Export a preset with nested dependencies."),
    ):
        presets_group.add_command(
            JaniformCommand(
                command_name,
                callback=_wrap_handler(handler, command_path=("presets", command_name)),
                command_path=("presets", command_name),
                environment=environment,
                help=help_text,
                params=[
                    _command_config_option(),
                    _output_option(default_output_mode=CliOutputMode.JSON),
                    _user_preset_json_option(),
                    _argument("category", choice_options=CLI_PRESET_CATEGORY_CHOICES),
                    _argument("preset_name", metavar="PRESET_NAME"),
                    _preset_scope_option(
                        "--scope",
                        help_text="Registry scope for the requested preset. Default: auto.",
                        include_auto=True,
                    ),
                ],
            )
        )

    root.add_command(
        JaniformGroup(
            "seasonality",
            callback=_group_help_callback,
            command_path=("seasonality",),
            environment=environment,
            help="Seasonality preprocessing workflows.",
            invoke_without_command=True,
        )
    )
    seasonality_group = cast(click.Group, root.get_command(click.Context(root), "seasonality"))
    seasonality_group.add_command(
        JaniformCommand(
            "diagnostics",
            callback=_wrap_handler(_run_seasonality_diagnostics_command, command_path=("seasonality", "diagnostics")),
            command_path=("seasonality", "diagnostics"),
            environment=environment,
            help=(
                "Run end-to-end seasonality diagnostics from raw timestamp events. This "
                "task-oriented workflow validates timestamp records, applies session "
                "filtering, estimates intraday, day-of-week, and day-of-year components, "
                "composes the deterministic seasonality signal, and emits the canonical "
                "diagnostics report."
            ),
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option(
                    "--event-records-json",
                    contract_label="an event-record document with a top-level `event_records` list",
                    semantic_note=(
                        "Each record must include a timezone-aware timestamp string compatible with "
                        "the public janiform.seasonality preprocessing contract."
                    ),
                ),
                _required_json_path_option(
                    "--calendar-registry-json",
                    contract_label="a calendar registry JSON object used for exchange session classification",
                ),
                _command_option("--dataset-id", help_text="Stable dataset identifier recorded in the diagnostics report.", required=True),
                _command_option("--exchange-code", help_text="Exchange code used to resolve session state for all timestamp events.", required=True),
                _command_option("--timestamp-field-name", default="timestamp", help_text="Timestamp field name inside each event record. Default: timestamp."),
                _command_option("--session-filter", choice_options=CLI_SEASONALITY_SESSION_FILTER_CHOICES, default="open_only", help_text="Session filter applied after exchange calendar classification. Default: open_only."),
                _command_option("--sparse-hour-event-threshold", default=1, value_type=int, help_text="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1."),
                _command_option("--intraday-minutes-per-bin", default=1, value_type=int, help_text="Intraday estimator bin size in minutes. Default: 1."),
                _command_option("--intraday-smoothing-kernel", choice_options=CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES, default="gaussian", help_text="Intraday estimator smoothing kernel. Default: gaussian."),
                _command_option("--intraday-smoothing-bandwidth-bins", default=3.0, value_type=float, help_text="Intraday estimator smoothing bandwidth measured in bins. Default: 3.0."),
                _command_option("--intraday-minimum-bin-events", default=1, value_type=int, help_text="Minimum events per intraday bin before the bin is flagged as sparse. Default: 1."),
                _command_option("--intraday-pseudocount", default=1e-6, value_type=float, help_text="Intraday estimator pseudocount added before normalization. Default: 1e-6."),
                _command_option("--day-of-week-minimum-bucket-events", default=1, value_type=int, help_text="Minimum events per day-of-week bucket before the bucket is flagged as sparse. Default: 1."),
                _command_option("--day-of-week-pseudocount", default=1e-6, value_type=float, help_text="Day-of-week estimator pseudocount added before normalization. Default: 1e-6."),
                _command_option("--day-of-year-minimum-bucket-events", default=1, value_type=int, help_text="Minimum events per day-of-year bucket before the bucket is flagged as sparse. Default: 1."),
                _command_option("--day-of-year-pseudocount", default=1e-6, value_type=float, help_text="Day-of-year estimator pseudocount added before normalization. Default: 1e-6."),
                _command_option("--estimator-version", default="seasonality_estimator.v1", help_text="Estimator version label recorded in reproducibility metadata. Default: seasonality_estimator.v1."),
            ],
        )
    )
    seasonality_group.add_command(
        JaniformCommand(
            "preprocess-timestamps",
            callback=_wrap_handler(_run_seasonality_preprocess_timestamps_command, command_path=("seasonality", "preprocess-timestamps")),
            command_path=("seasonality", "preprocess-timestamps"),
            environment=environment,
            help=(
                "Preprocess timestamp events against the exchange calendar registry. This "
                "standalone step validates timestamp records, applies session filtering, and "
                "emits deterministic preprocessing diagnostics for downstream seasonality "
                "or calibration workflows."
            ),
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option(
                    "--event-records-json",
                    contract_label="an event-record document with a top-level `event_records` list",
                    semantic_note=(
                        "Each record must include a timezone-aware timestamp string compatible with "
                        "the public janiform.seasonality preprocessing contract."
                    ),
                ),
                _required_json_path_option(
                    "--calendar-registry-json",
                    contract_label="a calendar registry JSON object used for exchange session classification",
                ),
                _command_option("--exchange-code", help_text="Exchange code used to resolve session state for all timestamp events.", required=True),
                _command_option("--timestamp-field-name", default="timestamp", help_text="Timestamp field name inside each event record. Default: timestamp."),
                _command_option("--session-filter", choice_options=CLI_SEASONALITY_SESSION_FILTER_CHOICES, default="open_only", help_text="Session filter applied after exchange calendar classification. Default: open_only."),
                _command_option("--sparse-hour-event-threshold", default=1, value_type=int, help_text="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1."),
            ],
        )
    )

    root.add_command(
        JaniformGroup(
            "hourly-factors",
            callback=_group_help_callback,
            command_path=("hourly-factors",),
            environment=environment,
            help="Hourly factor materialization and contract-building workflows.",
            invoke_without_command=True,
        )
    )
    hourly_group = cast(click.Group, root.get_command(click.Context(root), "hourly-factors"))
    hourly_group.add_command(
        JaniformCommand(
            "materialize-seasonality",
            callback=_wrap_handler(_run_hourly_factor_materialize_seasonality_command, command_path=("hourly-factors", "materialize-seasonality")),
            command_path=("hourly-factors", "materialize-seasonality"),
            environment=environment,
            help=(
                "Materialize interval-aligned seasonality factors from timestamped source "
                "samples. The command preserves the canonical seasonality materialization "
                "report shape."
            ),
            params=[
                *_hourly_factor_common_options(),
                _required_json_path_option(
                    "--materialization-input-json",
                    contract_label=(
                        "an hourly seasonality materialization input document with source "
                        "timestamps, source factors, and optional materialization settings"
                    ),
                ),
            ],
        )
    )
    hourly_group.add_command(
        JaniformCommand(
            "materialize-news-intensity",
            callback=_wrap_handler(_run_hourly_factor_materialize_news_intensity_command, command_path=("hourly-factors", "materialize-news-intensity")),
            command_path=("hourly-factors", "materialize-news-intensity"),
            environment=environment,
            help=(
                "Materialize interval-aligned news-intensity factors from known-news and "
                "surprise controls. The command preserves the canonical news-intensity "
                "materialization report shape."
            ),
            params=[
                *_hourly_factor_common_options(),
                _required_json_path_option(
                    "--materialization-input-json",
                    contract_label=(
                        "an hourly news-intensity materialization input document with known-news "
                        "timestamps, surprise baseline intensity, and optional interval multipliers"
                    ),
                ),
            ],
        )
    )
    hourly_group.add_command(
        JaniformCommand(
            "build-contract",
            callback=_wrap_handler(
                _run_hourly_factor_build_contract_command,
                command_path=("hourly-factors", "build-contract"),
                required_one_of_groups=(
                    ("seasonality_explicit_source_json", "seasonality_materialized_source_json"),
                    ("news_intensity_explicit_source_json", "news_intensity_materialized_source_json"),
                ),
            ),
            command_path=("hourly-factors", "build-contract"),
            environment=environment,
            help=(
                "Build the combined interval-factor contract for downstream simulation or "
                "pricing workflows. Seasonality and news-intensity inputs can come from "
                "explicit factor source documents or from canonical materialization report "
                "files produced by Janiform."
            ),
            params=[
                *_hourly_factor_common_options(),
                _optional_path_option(
                    "--seasonality-explicit-source-json",
                    help_text=(
                        "Path to an explicit seasonality source document with `hourly_factors` and "
                        "optional `materialization_method`."
                    ),
                ),
                _optional_path_option(
                    "--seasonality-materialized-source-json",
                    help_text=(
                        "Path to a canonical hourly seasonality materialization report produced by "
                        "`hourly-factors materialize-seasonality`."
                    ),
                ),
                _optional_path_option(
                    "--news-intensity-explicit-source-json",
                    help_text=(
                        "Path to an explicit news-intensity source document with `hourly_factors` "
                        "and optional `materialization_method`."
                    ),
                ),
                _optional_path_option(
                    "--news-intensity-materialized-source-json",
                    help_text=(
                        "Path to a canonical hourly news-intensity materialization report produced "
                        "by `hourly-factors materialize-news-intensity`."
                    ),
                ),
            ],
        )
    )

    root.add_command(
        JaniformGroup(
            "pricing",
            callback=_group_help_callback,
            command_path=("pricing",),
            environment=environment,
            help="Pricing workflows.",
            invoke_without_command=True,
        )
    )
    pricing_group = cast(click.Group, root.get_command(click.Context(root), "pricing"))
    pricing_group.add_command(
        JaniformCommand(
            "vanilla-option",
            callback=_wrap_handler(_run_pricing_vanilla_option_command, command_path=("pricing", "vanilla-option")),
            command_path=("pricing", "vanilla-option"),
            environment=environment,
            help="Vanilla option pricing workflow.",
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option(
                    "--execution-input-json",
                    contract_label="a vanilla-option pricing execution-input JSON object",
                    semantic_note="Interval factors keep known and surprise news intensity separate.",
                ),
            ],
        )
    )

    root.add_command(
        JaniformGroup(
            "stylized-facts",
            callback=_group_help_callback,
            command_path=("stylized-facts",),
            environment=environment,
            help="Stylized-fact reporting workflows.",
            invoke_without_command=True,
        )
    )
    stylized_group = cast(click.Group, root.get_command(click.Context(root), "stylized-facts"))
    stylized_group.add_command(
        JaniformCommand(
            "horizon-comparison",
            callback=_wrap_handler(_run_stylized_facts_horizon_comparison_command, command_path=("stylized-facts", "horizon-comparison")),
            command_path=("stylized-facts", "horizon-comparison"),
            environment=environment,
            help=(
                "Generate a stylized-fact horizon-comparison report bundle from a metric "
                "catalog, a comparison request, and optional artifact-export settings."
            ),
            params=[
                _command_config_option(),
                _output_option(default_output_mode=CliOutputMode.JSON),
                _required_json_path_option("--metric-catalog-json", contract_label="a stylized-fact metric catalog JSON object"),
                _required_json_path_option("--comparison-request-json", contract_label="a stylized-fact horizon-comparison request JSON object"),
                _optional_path_option(
                    "--artifact-export-json",
                    help_text=(
                        "Optional path to a stylized-fact artifact-export config JSON object. This "
                        "payload is passed through the public workflow validation path and requires "
                        "--execution-mode disk."
                    ),
                ),
                _command_option(
                    "--execution-mode",
                    choice_options=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
                    default="memory",
                    help_text=(
                        "Execution mode passed through to janiform.stylized_facts. Default: memory. "
                        "Use disk when artifact export is requested."
                    ),
                ),
            ],
        )
    )

    root.add_command(
        JaniformGroup(
            "dev",
            callback=_group_help_callback,
            command_path=("dev",),
            environment=environment,
            help="Developer-only utilities.",
            invoke_without_command=True,
        )
    )
    dev_group = cast(click.Group, root.get_command(click.Context(root), "dev"))
    dev_group.add_command(
        JaniformGroup(
            "regression",
            callback=_group_help_callback,
            command_path=("dev", "regression"),
            environment=environment,
            help="Regression validation suites.",
            invoke_without_command=True,
        )
    )
    regression_group = cast(click.Group, dev_group.get_command(click.Context(dev_group), "regression"))
    regression_group.add_command(
        JaniformCommand(
            "deterministic",
            callback=_wrap_handler(_run_deterministic_regression_command, command_path=("dev", "regression", "deterministic")),
            command_path=("dev", "regression", "deterministic"),
            environment=environment,
            help="Run deterministic calendar and hourly-factor regression harness.",
            params=[
                _output_option(default_output_mode=CliOutputMode.HUMAN),
                _required_json_path_option(
                    "--calendar-registry-json",
                    contract_label="a calendar registry JSON object consumed by the deterministic regression harness",
                ),
                _required_json_path_option(
                    "--calendar-cases-json",
                    contract_label="a calendar regression case document with a top-level `cases` list",
                ),
                _required_json_path_option(
                    "--seasonality-cases-json",
                    contract_label="an hourly-factor regression case document with a top-level `canonical_output_cases` list",
                ),
                _command_option(
                    "--seasonality-absolute-tolerance",
                    default=1e-12,
                    value_type=float,
                    help_text="Absolute tolerance for deterministic seasonality factor mismatch detection.",
                ),
                _command_option(
                    "--news-intensity-absolute-tolerance",
                    default=1e-12,
                    value_type=float,
                    help_text="Absolute tolerance for deterministic news-intensity mismatch detection.",
                ),
            ],
        )
    )
    regression_group.add_command(
        JaniformCommand(
            "open-closed-gap-leakage",
            callback=_wrap_handler(_run_open_closed_gap_leakage_regression_command, command_path=("dev", "regression", "open-closed-gap-leakage")),
            command_path=("dev", "regression", "open-closed-gap-leakage"),
            environment=environment,
            help="Run open and closed gap leakage regression harness.",
            params=[
                _output_option(default_output_mode=CliOutputMode.HUMAN),
                _optional_path_option(
                    "--calendar-fixture-path",
                    default=Path(DEFAULT_CALENDAR_FIXTURE_PATH),
                    help_text="Path to the calendar regression fixture JSON.",
                ),
                _optional_path_option(
                    "--gap-behavior-fixture-path",
                    default=Path(DEFAULT_GAP_BEHAVIOR_FIXTURE_PATH),
                    help_text="Path to the open and closed gap behavior fixture JSON.",
                ),
                _command_option(
                    "--component-absolute-tolerance",
                    default=1e-12,
                    value_type=float,
                    help_text="Absolute tolerance for component-level semantic mismatch detection.",
                ),
            ],
        )
    )


@click.pass_context
def _group_help_callback(ctx: click.Context) -> None:
    if ctx.invoked_subcommand is None:
        click.echo(ctx.get_help())


__all__ = [
    "_build_app",
    "_invoke_application",
]
