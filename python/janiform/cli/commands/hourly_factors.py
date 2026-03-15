from collections.abc import Mapping
from pathlib import Path
from typing import Optional

import click

from .shared import (
    _build_calendar_registry_from_document,
    _build_hourly_factor_contract_payload,
    _build_hourly_news_intensity_materialization_payload,
    _build_hourly_seasonality_materialization_payload,
    _extract_explicit_hourly_factor_source_document,
    _extract_hourly_news_intensity_materialization_input,
    _extract_hourly_seasonality_materialization_input,
    _extract_materialized_hourly_news_intensity_source_document,
    _extract_materialized_hourly_seasonality_source_document,
    _parse_utc_datetime_cli_argument,
)
from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_config_option,
    command_option,
    group_help_callback,
    invoke_command_handler,
    optional_path_option,
    output_option,
    required_datetime_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_INTERNAL_ERROR_ID,
    CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
    CliExitCode,
    CliOutputMode,
)
from ..documents import _load_json_document_for_cli_field
from ..errors import _classify_workflow_command_error
from ..rendering import (
    _render_hourly_factor_build_contract_human_output,
    _render_hourly_factor_materialize_news_intensity_human_output,
    _render_hourly_factor_materialize_seasonality_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...errors import JaniformError
from ...hourly_factors import (
    build_hourly_factor_contract,
    materialize_hourly_news_intensity_factors,
    materialize_hourly_seasonality_factors,
)


def _run_hourly_factor_materialize_seasonality_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    calendar_registry_json: Path,
    materialization_input_json: Path,
    range_start_utc: str,
    range_end_utc: str,
    exchange_code: str,
    interval: str,
    **_,
) -> int:
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=materialization_input_json,
            context="hourly seasonality materialization input",
            field_path="materialization_input_json",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        parsed_range_start_utc = _parse_utc_datetime_cli_argument(
            range_start_utc,
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        parsed_range_end_utc = _parse_utc_datetime_cli_argument(
            range_end_utc,
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        document_field_path = "materialization_input_document"
        source_timestamps_utc, source_seasonality_factors, settings = _extract_hourly_seasonality_materialization_input(
            materialization_input_document
        )
        materialization = materialize_hourly_seasonality_factors(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=parsed_range_start_utc,
            range_end_utc=parsed_range_end_utc,
            interval=interval,
            source_timestamps_utc=source_timestamps_utc,
            source_seasonality_factors=source_seasonality_factors,
            settings=settings,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_INTERNAL_ERROR_ID,
                file_field_path="calendar_registry_json",
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_seasonality_materialization_payload(materialization)

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_seasonality_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_materialize_news_intensity_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    calendar_registry_json: Path,
    materialization_input_json: Path,
    range_start_utc: str,
    range_end_utc: str,
    exchange_code: str,
    interval: str,
    **_,
) -> int:
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=materialization_input_json,
            context="hourly news-intensity materialization input",
            field_path="materialization_input_json",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        parsed_range_start_utc = _parse_utc_datetime_cli_argument(
            range_start_utc,
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        parsed_range_end_utc = _parse_utc_datetime_cli_argument(
            range_end_utc,
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        document_field_path = "materialization_input_document"
        settings = _extract_hourly_news_intensity_materialization_input(materialization_input_document)
        materialization = materialize_hourly_news_intensity_factors(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=parsed_range_start_utc,
            range_end_utc=parsed_range_end_utc,
            interval=interval,
            settings=settings,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_INTERNAL_ERROR_ID,
                file_field_path="calendar_registry_json",
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_news_intensity_materialization_payload(materialization)

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_news_intensity_human_output(
            payload,
        ),
    )

    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_build_contract_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    calendar_registry_json: Path,
    range_start_utc: str,
    range_end_utc: str,
    exchange_code: str,
    interval: str,
    seasonality_explicit_source_json: Optional[Path],
    seasonality_materialized_source_json: Optional[Path],
    news_intensity_explicit_source_json: Optional[Path],
    news_intensity_materialized_source_json: Optional[Path],
    **_,
) -> int:
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document_for_cli_field(
            repository_root=cli_context.repository_root,
            path=calendar_registry_json,
            context="calendar registry",
            field_path="calendar_registry_json",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        parsed_range_start_utc = _parse_utc_datetime_cli_argument(
            range_start_utc,
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )
        parsed_range_end_utc = _parse_utc_datetime_cli_argument(
            range_end_utc,
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )

        if seasonality_explicit_source_json is not None:
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document_for_cli_field(
                repository_root=cli_context.repository_root,
                path=seasonality_explicit_source_json,
                context="explicit seasonality source",
                field_path="seasonality_explicit_source_json",
                error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    seasonality_source_document,
                    field_prefix="seasonality source document",
                )
            )
        else:
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document_for_cli_field(
                repository_root=cli_context.repository_root,
                path=seasonality_materialized_source_json,
                context="materialized seasonality source",
                field_path="seasonality_materialized_source_json",
                error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_materialized_hourly_seasonality_source_document(
                    seasonality_source_document,
                    exchange_code=exchange_code,
                    range_start_utc=parsed_range_start_utc,
                    range_end_utc=parsed_range_end_utc,
                )
            )

        if news_intensity_explicit_source_json is not None:
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document_for_cli_field(
                repository_root=cli_context.repository_root,
                path=news_intensity_explicit_source_json,
                context="explicit news-intensity source",
                field_path="news_intensity_explicit_source_json",
                error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    news_intensity_source_document,
                    field_prefix="news intensity source document",
                )
            )
        else:
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document_for_cli_field(
                repository_root=cli_context.repository_root,
                path=news_intensity_materialized_source_json,
                context="materialized news-intensity source",
                field_path="news_intensity_materialized_source_json",
                error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_materialized_hourly_news_intensity_source_document(
                    news_intensity_source_document,
                    exchange_code=exchange_code,
                    range_start_utc=parsed_range_start_utc,
                    range_end_utc=parsed_range_end_utc,
                )
            )

        contract = build_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=parsed_range_start_utc,
            range_end_utc=parsed_range_end_utc,
            interval=interval,
            seasonality_hourly_factors=seasonality_hourly_factors,
            news_intensity_hourly_factors=news_intensity_hourly_factors,
            seasonality_materialization_method=seasonality_materialization_method,
            news_intensity_materialization_method=news_intensity_materialization_method,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_workflow_command_error(
                e,
                entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
                validation_error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_INTERNAL_ERROR_ID,
                file_field_path="calendar_registry_json",
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_factor_contract_payload(contract)

    _write_command_output(
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_build_contract_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _build_hourly_factors_group(*, environment: Optional[Mapping[str, str]] = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("hourly-factors",),
        environment=environment,
        help="Hourly factor materialization and contract-building workflows.",
        invoke_without_command=True,
    )
    @click.pass_context
    def hourly_factors(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @hourly_factors.command(
        "materialize-seasonality",
        cls=JaniformCommand,
        command_path=("hourly-factors", "materialize-seasonality"),
        environment=environment,
        help=(
            "Materialize interval-aligned seasonality factors from timestamped source "
            "samples. The command preserves the canonical seasonality materialization "
            "report shape."
        ),
        params=[
            *_hourly_factor_common_options(),
            required_json_path_option(
                "--materialization-input-json",
                contract_label=(
                    "an hourly seasonality materialization input document with source "
                    "timestamps, source factors, and optional materialization settings"
                ),
            ),
        ],
    )
    @click.pass_context
    def materialize_seasonality(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_hourly_factor_materialize_seasonality_command,
            command_path=("hourly-factors", "materialize-seasonality"),
            command_arguments=dict(command_arguments),
        )

    @hourly_factors.command(
        "materialize-news-intensity",
        cls=JaniformCommand,
        command_path=("hourly-factors", "materialize-news-intensity"),
        environment=environment,
        help=(
            "Materialize interval-aligned news-intensity factors from known-news and "
            "surprise controls. The command preserves the canonical news-intensity "
            "materialization report shape."
        ),
        params=[
            *_hourly_factor_common_options(),
            required_json_path_option(
                "--materialization-input-json",
                contract_label=(
                    "an hourly news-intensity materialization input document with known-news "
                    "timestamps, surprise baseline intensity, and optional interval multipliers"
                ),
            ),
        ],
    )
    @click.pass_context
    def materialize_news_intensity(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_hourly_factor_materialize_news_intensity_command,
            command_path=("hourly-factors", "materialize-news-intensity"),
            command_arguments=dict(command_arguments),
        )

    @hourly_factors.command(
        "build-contract",
        cls=JaniformCommand,
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
            optional_path_option(
                "--seasonality-explicit-source-json",
                help_text=(
                    "Path to an explicit seasonality source document with `hourly_factors` and "
                    "optional `materialization_method`."
                ),
            ),
            optional_path_option(
                "--seasonality-materialized-source-json",
                help_text=(
                    "Path to a canonical hourly seasonality materialization report produced by "
                    "`hourly-factors materialize-seasonality`."
                ),
            ),
            optional_path_option(
                "--news-intensity-explicit-source-json",
                help_text=(
                    "Path to an explicit news-intensity source document with `hourly_factors` "
                    "and optional `materialization_method`."
                ),
            ),
            optional_path_option(
                "--news-intensity-materialized-source-json",
                help_text=(
                    "Path to a canonical hourly news-intensity materialization report produced "
                    "by `hourly-factors materialize-news-intensity`."
                ),
            ),
        ],
    )
    @click.pass_context
    def build_contract(ctx: click.Context, **command_arguments) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_hourly_factor_build_contract_command,
            command_path=("hourly-factors", "build-contract"),
            command_arguments=dict(command_arguments),
            required_one_of_groups=(
                ("seasonality_explicit_source_json", "seasonality_materialized_source_json"),
                ("news_intensity_explicit_source_json", "news_intensity_materialized_source_json"),
            ),
        )

    return hourly_factors


def _hourly_factor_common_options() -> list[click.Parameter]:
    return [
        command_config_option(),
        output_option(default_output_mode=CliOutputMode.JSON),
        required_json_path_option(
            "--calendar-registry-json",
            contract_label="a calendar registry JSON object used to resolve exchange-local interval labels",
        ),
        command_option(
            "--exchange-code",
            help_text="Exchange code used to resolve session state and interval-factor alignment.",
            required=True,
        ),
        command_option(
            "--interval",
            default="1h",
            help_text=(
                "Fixed-width UTC interval specification used to derive the target grid, "
                "for example 1h or 15min. Default: 1h."
            ),
        ),
        required_datetime_option(
            "--range-start-utc",
            help_text="Inclusive UTC range start aligned to the first target interval boundary.",
        ),
        required_datetime_option(
            "--range-end-utc",
            help_text="Exclusive UTC range end aligned to the final target interval boundary.",
        ),
    ]


__all__ = []
