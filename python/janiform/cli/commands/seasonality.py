from collections.abc import Mapping
from pathlib import Path

import click

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
from ..command_runtime import (
    JaniformCommand,
    JaniformGroup,
    command_config_option,
    command_option,
    group_help_callback,
    invoke_command_handler,
    output_option,
    required_json_path_option,
)
from ..context import CliCommandContext
from .._constants import (
    CLI_SEASONALITY_DIAGNOSTICS_ENTRYPOINT,
    CLI_SEASONALITY_DIAGNOSTICS_VALIDATION_ERROR_ID,
    CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES,
    CLI_SEASONALITY_SESSION_FILTER_CHOICES,
    CliExitCode,
    CliOutputMode,
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


def _build_seasonality_group(*, environment: Mapping[str, str] | None = None) -> click.Group:
    @click.group(
        cls=JaniformGroup,
        command_path=("seasonality",),
        environment=environment,
        help="Seasonality preprocessing workflows.",
        invoke_without_command=True,
    )
    @click.pass_context
    def seasonality(ctx: click.Context) -> None:
        group_help_callback(ctx)

    @seasonality.command(
        "diagnostics",
        cls=JaniformCommand,
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
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option(
                "--event-records-json",
                contract_label="an event-record document with a top-level `event_records` list",
                semantic_note=(
                    "Each record must include a timezone-aware timestamp string compatible with "
                    "the public janiform.seasonality preprocessing contract."
                ),
            ),
            required_json_path_option(
                "--calendar-registry-json",
                contract_label="a calendar registry JSON object used for exchange session classification",
            ),
            command_option("--dataset-id", help_text="Stable dataset identifier recorded in the diagnostics report.", required=True),
            command_option("--exchange-code", help_text="Exchange code used to resolve session state for all timestamp events.", required=True),
            command_option("--timestamp-field-name", default="timestamp", help_text="Timestamp field name inside each event record. Default: timestamp."),
            command_option("--session-filter", choice_options=CLI_SEASONALITY_SESSION_FILTER_CHOICES, default="open_only", help_text="Session filter applied after exchange calendar classification. Default: open_only."),
            command_option("--sparse-hour-event-threshold", default=1, value_type=int, help_text="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1."),
            command_option("--intraday-minutes-per-bin", default=1, value_type=int, help_text="Intraday estimator bin size in minutes. Default: 1."),
            command_option("--intraday-smoothing-kernel", choice_options=CLI_SEASONALITY_INTRADAY_SMOOTHING_KERNEL_CHOICES, default="gaussian", help_text="Intraday estimator smoothing kernel. Default: gaussian."),
            command_option("--intraday-smoothing-bandwidth-bins", default=3.0, value_type=float, help_text="Intraday estimator smoothing bandwidth measured in bins. Default: 3.0."),
            command_option("--intraday-minimum-bin-events", default=1, value_type=int, help_text="Minimum events per intraday bin before the bin is flagged as sparse. Default: 1."),
            command_option("--intraday-pseudocount", default=1e-6, value_type=float, help_text="Intraday estimator pseudocount added before normalization. Default: 1e-6."),
            command_option("--day-of-week-minimum-bucket-events", default=1, value_type=int, help_text="Minimum events per day-of-week bucket before the bucket is flagged as sparse. Default: 1."),
            command_option("--day-of-week-pseudocount", default=1e-6, value_type=float, help_text="Day-of-week estimator pseudocount added before normalization. Default: 1e-6."),
            command_option("--day-of-year-minimum-bucket-events", default=1, value_type=int, help_text="Minimum events per day-of-year bucket before the bucket is flagged as sparse. Default: 1."),
            command_option("--day-of-year-pseudocount", default=1e-6, value_type=float, help_text="Day-of-year estimator pseudocount added before normalization. Default: 1e-6."),
            command_option("--estimator-version", default="seasonality_estimator.v1", help_text="Estimator version label recorded in reproducibility metadata. Default: seasonality_estimator.v1."),
        ],
    )
    @click.pass_context
    def diagnostics(ctx: click.Context, **command_arguments: object) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_seasonality_diagnostics_command,
            command_path=("seasonality", "diagnostics"),
            command_arguments=dict(command_arguments),
        )

    @seasonality.command(
        "preprocess-timestamps",
        cls=JaniformCommand,
        command_path=("seasonality", "preprocess-timestamps"),
        environment=environment,
        help=(
            "Preprocess timestamp events against the exchange calendar registry. This "
            "standalone step validates timestamp records, applies session filtering, and "
            "emits deterministic preprocessing diagnostics for downstream seasonality "
            "or calibration workflows."
        ),
        params=[
            command_config_option(),
            output_option(default_output_mode=CliOutputMode.JSON),
            required_json_path_option(
                "--event-records-json",
                contract_label="an event-record document with a top-level `event_records` list",
                semantic_note=(
                    "Each record must include a timezone-aware timestamp string compatible with "
                    "the public janiform.seasonality preprocessing contract."
                ),
            ),
            required_json_path_option(
                "--calendar-registry-json",
                contract_label="a calendar registry JSON object used for exchange session classification",
            ),
            command_option("--exchange-code", help_text="Exchange code used to resolve session state for all timestamp events.", required=True),
            command_option("--timestamp-field-name", default="timestamp", help_text="Timestamp field name inside each event record. Default: timestamp."),
            command_option("--session-filter", choice_options=CLI_SEASONALITY_SESSION_FILTER_CHOICES, default="open_only", help_text="Session filter applied after exchange calendar classification. Default: open_only."),
            command_option("--sparse-hour-event-threshold", default=1, value_type=int, help_text="Minimum accepted events per hour before a bucket is flagged as sparse. Default: 1."),
        ],
    )
    @click.pass_context
    def preprocess_timestamps(ctx: click.Context, **command_arguments: object) -> int:
        return invoke_command_handler(
            ctx,
            handler=_run_seasonality_preprocess_timestamps_command,
            command_path=("seasonality", "preprocess-timestamps"),
            command_arguments=dict(command_arguments),
        )

    return seasonality


__all__ = [
    "_run_seasonality_diagnostics_command",
    "_run_seasonality_preprocess_timestamps_command",
]
