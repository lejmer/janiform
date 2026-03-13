from argparse import Namespace
from pathlib import Path
from typing import Optional, cast

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
)
from ..documents import _load_json_document
from ..errors import _classify_workflow_command_error
from ..paths import _resolve_path_from_repository_root, resolve_repository_root
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


def _run_hourly_factor_materialize_seasonality_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    materialization_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.materialization_input_json),
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "materialization_input_json"
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document(
            materialization_input_path,
            "hourly seasonality materialization input",
        )
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_SEASONALITY_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
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
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
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
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_seasonality_materialization_payload(materialization)
    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_seasonality_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_materialize_news_intensity_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    materialization_input_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.materialization_input_json),
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        file_field_path = "materialization_input_json"
        document_field_path = "materialization_input_document"
        materialization_input_document = _load_json_document(
            materialization_input_path,
            "hourly news-intensity materialization input",
        )
        document_field_path = "calendar_registry_document"
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_MATERIALIZE_NEWS_INTENSITY_ENTRYPOINT,
        )
        document_field_path = "materialization_input_document"
        settings = _extract_hourly_news_intensity_materialization_input(materialization_input_document)
        materialization = materialize_hourly_news_intensity_factors(
            calendar_registry=calendar_registry,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
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
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_news_intensity_materialization_payload(materialization)
    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_materialize_news_intensity_human_output(
            payload,
        ),
    )
    return int(CliExitCode.SUCCESS)


def _run_hourly_factor_build_contract_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )
    seasonality_explicit_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.seasonality_explicit_source_json,
    )
    seasonality_materialized_source_path_argument = cast(
        Optional[Path], parsed_arguments.seasonality_materialized_source_json
    )
    news_intensity_explicit_source_path_argument = cast(
        Optional[Path],
        parsed_arguments.news_intensity_explicit_source_json,
    )
    news_intensity_materialized_source_path_argument = cast(
        Optional[Path], parsed_arguments.news_intensity_materialized_source_json
    )

    file_field_path = "calendar_registry_json"
    document_field_path = "calendar_registry_document"

    try:
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        range_start_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_start_utc),
            field_path="range_start_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )
        range_end_utc = _parse_utc_datetime_cli_argument(
            cast(str, parsed_arguments.range_end_utc),
            field_path="range_end_utc",
            error_id=CLI_HOURLY_FACTORS_BUILD_CONTRACT_VALIDATION_ERROR_ID,
            entrypoint=CLI_HOURLY_FACTORS_BUILD_CONTRACT_ENTRYPOINT,
        )

        if seasonality_explicit_source_path_argument is not None:
            file_field_path = "seasonality_explicit_source_json"
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document(
                _resolve_path_from_repository_root(
                    repository_root,
                    seasonality_explicit_source_path_argument,
                ),
                "explicit seasonality source",
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    seasonality_source_document,
                    field_prefix="seasonality source document",
                )
            )
        else:
            file_field_path = "seasonality_materialized_source_json"
            document_field_path = "seasonality_source_document"
            seasonality_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, seasonality_materialized_source_path_argument),
                "materialized seasonality source",
            )
            seasonality_hourly_factors, seasonality_materialization_method = (
                _extract_materialized_hourly_seasonality_source_document(
                    seasonality_source_document,
                    exchange_code=cast(str, parsed_arguments.exchange_code),
                    range_start_utc=range_start_utc,
                    range_end_utc=range_end_utc,
                )
            )

        if news_intensity_explicit_source_path_argument is not None:
            file_field_path = "news_intensity_explicit_source_json"
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, news_intensity_explicit_source_path_argument),
                "explicit news-intensity source",
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_explicit_hourly_factor_source_document(
                    news_intensity_source_document,
                    field_prefix="news intensity source document",
                )
            )
        else:
            file_field_path = "news_intensity_materialized_source_json"
            document_field_path = "news_intensity_source_document"
            news_intensity_source_document = _load_json_document(
                _resolve_path_from_repository_root(repository_root, news_intensity_materialized_source_path_argument),
                "materialized news-intensity source",
            )
            news_intensity_hourly_factors, news_intensity_materialization_method = (
                _extract_materialized_hourly_news_intensity_source_document(
                    news_intensity_source_document,
                    exchange_code=cast(str, parsed_arguments.exchange_code),
                    range_start_utc=range_start_utc,
                    range_end_utc=range_end_utc,
                )
            )

        contract = build_hourly_factor_contract(
            calendar_registry=calendar_registry,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            range_start_utc=range_start_utc,
            range_end_utc=range_end_utc,
            interval=cast(str, parsed_arguments.interval),
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
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_hourly_factor_contract_payload(contract)
    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_hourly_factor_build_contract_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_hourly_factor_build_contract_command",
    "_run_hourly_factor_materialize_news_intensity_command",
    "_run_hourly_factor_materialize_seasonality_command",
]
