from pathlib import Path
from typing import cast

from ..context import CliCommandContext
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
from ..paths import _resolve_path_from_repository_root
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
    _build_day_of_week_seasonality_settings,
    _build_day_of_year_seasonality_settings,
    _build_intraday_seasonality_settings,
    _build_timestamp_event_preprocessing_payload,
    _extract_event_records,
    _require_non_empty_cli_string,
    _require_positive_integer_cli_argument,
)


def _run_seasonality_preprocess_timestamps_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    event_records_json: Path,
    calendar_registry_json: Path,
    exchange_code: str,
    timestamp_field_name: str,
    session_filter: str,
    sparse_hour_event_threshold: int,
    **_: object,
) -> int:
    event_records_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        event_records_json,
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        calendar_registry_json,
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
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
            timestamp_field_name=timestamp_field_name,
            session_filter=SessionFilter(session_filter),
            sparse_hour_event_threshold=sparse_hour_event_threshold,
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
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_seasonality_preprocess_timestamps_human_output(payload),
    )
    return int(CliExitCode.SUCCESS)


def _run_seasonality_diagnostics_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    event_records_json: Path,
    calendar_registry_json: Path,
    dataset_id: str,
    exchange_code: str,
    timestamp_field_name: str,
    session_filter: str,
    sparse_hour_event_threshold: int,
    intraday_minutes_per_bin: int,
    intraday_smoothing_kernel: str,
    intraday_smoothing_bandwidth_bins: float,
    intraday_minimum_bin_events: int,
    intraday_pseudocount: float,
    day_of_week_minimum_bucket_events: int,
    day_of_week_pseudocount: float,
    day_of_year_minimum_bucket_events: int,
    day_of_year_pseudocount: float,
    estimator_version: str,
    **_: object,
) -> int:
    event_records_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        event_records_json,
    )
    calendar_registry_path = _resolve_path_from_repository_root(
        cli_context.repository_root,
        calendar_registry_json,
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
        normalized_dataset_id = _require_non_empty_cli_string(
            dataset_id,
            field_path="dataset_id",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        normalized_estimator_version = _require_non_empty_cli_string(
            estimator_version,
            field_path="estimator_version",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        validated_sparse_hour_event_threshold = _require_positive_integer_cli_argument(
            sparse_hour_event_threshold,
            field_path="sparse_hour_event_threshold",
            error_id=CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
            entrypoint=CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
        )
        intraday_settings = _build_intraday_seasonality_settings(
            intraday_minutes_per_bin=intraday_minutes_per_bin,
            intraday_smoothing_kernel=intraday_smoothing_kernel,
            intraday_smoothing_bandwidth_bins=intraday_smoothing_bandwidth_bins,
            intraday_minimum_bin_events=intraday_minimum_bin_events,
            intraday_pseudocount=intraday_pseudocount,
        )
        day_of_week_settings = _build_day_of_week_seasonality_settings(
            day_of_week_minimum_bucket_events=day_of_week_minimum_bucket_events,
            day_of_week_pseudocount=day_of_week_pseudocount,
        )
        day_of_year_settings = _build_day_of_year_seasonality_settings(
            day_of_year_minimum_bucket_events=day_of_year_minimum_bucket_events,
            day_of_year_pseudocount=day_of_year_pseudocount,
        )
        document_field_path = "event_records_document"
        preprocessed_events = preprocess_timestamp_events(
            event_records,
            exchange_code=exchange_code,
            calendar_registry=calendar_registry,
            timestamp_field_name=timestamp_field_name,
            session_filter=SessionFilter(session_filter),
            sparse_hour_event_threshold=validated_sparse_hour_event_threshold,
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
            normalized_dataset_id,
            preprocessed_events,
            intraday_estimate,
            day_of_week_estimate,
            day_of_year_estimate,
            composition,
            estimator_version=normalized_estimator_version,
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
        output,
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
