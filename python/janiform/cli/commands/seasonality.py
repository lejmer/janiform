from argparse import Namespace
from pathlib import Path
from typing import cast

from .._constants import (
    CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
    CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_ENTRYPOINT,
    CLI_SEASONALITY_PREPROCESS_TIMESTAMPS_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..documents import _load_json_document
from ..errors import (
    _classify_seasonality_diagnostics_command_error,
    _classify_seasonality_preprocess_command_error,
)
from ..paths import _resolve_path_from_repository_root, resolve_repository_root
from ..rendering import (
    _render_seasonality_diagnostics_human_output,
    _render_seasonality_preprocess_timestamps_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...errors import JaniformError
from ...seasonality import (
    SessionFilter,
    compose_deterministic_seasonality_signal,
    estimate_day_of_week_seasonality,
    estimate_day_of_year_seasonality,
    estimate_intraday_seasonality,
    generate_seasonality_diagnostics_report,
    preprocess_timestamp_events,
)
from .shared import (
    _build_calendar_registry_from_document,
    _build_day_of_week_seasonality_settings_from_arguments,
    _build_day_of_year_seasonality_settings_from_arguments,
    _build_intraday_seasonality_settings_from_arguments,
    _build_timestamp_event_preprocessing_payload,
    _extract_event_records,
    _require_non_empty_cli_string,
    _require_positive_integer_cli_argument,
)


def _run_seasonality_preprocess_timestamps_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    event_records_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.event_records_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    file_field_path = "event_records_json"
    document_field_path = "event_records_document"

    try:
        event_records_document = _load_json_document(event_records_path, "event records")
        event_records = _extract_event_records(event_records_document)
        file_field_path = "calendar_registry_json"
        document_field_path = "calendar_registry_document"
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "event_records_document"
        preprocessed_events = preprocess_timestamp_events(
            event_records,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            calendar_registry=calendar_registry,
            timestamp_field_name=cast(str, parsed_arguments.timestamp_field_name),
            session_filter=SessionFilter(cast(str, parsed_arguments.session_filter)),
            sparse_hour_event_threshold=int(parsed_arguments.sparse_hour_event_threshold),
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_seasonality_preprocess_command_error(
                e,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    payload = _build_timestamp_event_preprocessing_payload(preprocessed_events)
    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_seasonality_preprocess_timestamps_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _run_seasonality_diagnostics_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()
    event_records_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.event_records_json),
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        repository_root,
        cast(Path, parsed_arguments.calendar_registry_json),
    )

    file_field_path = "event_records_json"
    document_field_path = "event_records_document"

    try:
        event_records_document = _load_json_document(event_records_path, "event records")
        event_records = _extract_event_records(event_records_document)
        file_field_path = "calendar_registry_json"
        document_field_path = "calendar_registry_document"
        calendar_registry_document = _load_json_document(calendar_registry_path, "calendar registry")
        calendar_registry = _build_calendar_registry_from_document(calendar_registry_document)
        document_field_path = "command_input"
        dataset_id = _require_non_empty_cli_string(
            cast(str, parsed_arguments.dataset_id),
            field_path="dataset_id",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        estimator_version = _require_non_empty_cli_string(
            cast(str, parsed_arguments.estimator_version),
            field_path="estimator_version",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        sparse_hour_event_threshold = _require_positive_integer_cli_argument(
            int(parsed_arguments.sparse_hour_event_threshold),
            field_path="sparse_hour_event_threshold",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        intraday_settings = _build_intraday_seasonality_settings_from_arguments(parsed_arguments)
        day_of_week_settings = _build_day_of_week_seasonality_settings_from_arguments(parsed_arguments)
        day_of_year_settings = _build_day_of_year_seasonality_settings_from_arguments(parsed_arguments)
        document_field_path = "event_records_document"
        preprocessed_events = preprocess_timestamp_events(
            event_records,
            exchange_code=cast(str, parsed_arguments.exchange_code),
            calendar_registry=calendar_registry,
            timestamp_field_name=cast(str, parsed_arguments.timestamp_field_name),
            session_filter=SessionFilter(cast(str, parsed_arguments.session_filter)),
            sparse_hour_event_threshold=sparse_hour_event_threshold,
        )
        document_field_path = "command_input"
        intraday_estimate = estimate_intraday_seasonality(preprocessed_events, settings=intraday_settings)
        day_of_week_estimate = estimate_day_of_week_seasonality(preprocessed_events, settings=day_of_week_settings)
        day_of_year_estimate = estimate_day_of_year_seasonality(preprocessed_events, settings=day_of_year_settings)
        composition = compose_deterministic_seasonality_signal(
            preprocessed_events,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
        )
        report = generate_seasonality_diagnostics_report(
            dataset_id,
            preprocessed_events,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
            composition,
            estimator_version=estimator_version,
        )
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_seasonality_diagnostics_command_error(
                e,
                file_field_path=file_field_path,
                document_field_path=document_field_path,
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=report,
        human_output_factory=lambda: _render_seasonality_diagnostics_human_output(
            report,
            preprocessed_events=preprocessed_events,
            intraday_estimate=intraday_estimate,
            day_of_week_estimate=day_of_week_estimate,
            day_of_year_estimate=day_of_year_estimate,
        ),
    )
    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_seasonality_diagnostics_command",
    "_run_seasonality_preprocess_timestamps_command",
]
