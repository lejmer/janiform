from __future__ import annotations

import csv
import math
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING, Any, Optional, Protocol, cast

import pyarrow as pyarrow
from pyarrow import parquet

from .contracts import serialize_pretty_json
from .calibration.fit.report import JaniformFitReport
from .calendar import CalendarRegistry, MarketState
from .pricing import VanillaOptionPricingRunResult
from .simulation import CoarseSimulationPath, SeededTickSimulationResponse

if TYPE_CHECKING:
    from .calibration.fit.model import JaniformModel


class SimulationArtifactSink(Protocol):
    def write_records(self, records: list[dict[str, Any]]) -> None: ...

    def close(self) -> None: ...


@dataclass(frozen=True, slots=True)
class WorkflowArtifactPaths:
    run_id: str
    run_directory: Path
    primary_output_path: Path
    report_path: Path
    manifest_path: Path


@dataclass(frozen=True, slots=True)
class SimulationOutputPreferences:
    file_format: str
    timestamp_column: str
    timestamp_timezone: str
    tick_price_input_policy: Optional[str]
    price_column: Optional[str]
    trade_sign_column: Optional[str]
    bid_column: Optional[str]
    ask_column: Optional[str]
    reference_price: float


def generate_workflow_run_id(prefix: str) -> str:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    return f"{prefix}_{timestamp}"


def resolve_fit_artifact_paths(
    *,
    run_id: str,
    output_dir: Optional[str | Path],
) -> WorkflowArtifactPaths:
    run_directory = _resolve_run_directory(run_id=run_id, output_dir=output_dir)
    return WorkflowArtifactPaths(
        run_id=run_id,
        run_directory=run_directory,
        primary_output_path=run_directory / "model.json",
        report_path=run_directory / "fit_report.json",
        manifest_path=run_directory / "run.json",
    )


def resolve_simulation_artifact_paths(
    *,
    run_id: str,
    output_path: Optional[str | Path],
    default_file_format: str,
    interval_label: Optional[str],
) -> WorkflowArtifactPaths:
    resolved_output_path = _resolve_output_path(
        run_id=run_id,
        output_path=output_path,
        default_stem="simulation" if interval_label is None else f"simulation_{interval_label}",
        default_file_format=default_file_format,
    )
    run_directory = resolved_output_path.parent

    return WorkflowArtifactPaths(
        run_id=run_id,
        run_directory=run_directory,
        primary_output_path=resolved_output_path,
        report_path=run_directory / "simulation_report.json",
        manifest_path=run_directory / "run.json",
    )


def resolve_pricing_artifact_paths(
    *,
    run_id: str,
    output_path: Optional[str | Path],
) -> WorkflowArtifactPaths:
    resolved_output_path = _resolve_output_path(
        run_id=run_id,
        output_path=output_path,
        default_stem="pricing",
        default_file_format="json",
    )
    run_directory = resolved_output_path.parent

    return WorkflowArtifactPaths(
        run_id=run_id,
        run_directory=run_directory,
        primary_output_path=resolved_output_path,
        report_path=run_directory / "pricing_report.json",
        manifest_path=run_directory / "run.json",
    )


def ensure_artifact_directory(paths: WorkflowArtifactPaths) -> None:
    paths.run_directory.mkdir(parents=True, exist_ok=True)


def write_fit_artifacts(
    *,
    model: JaniformModel,
    fit_report: JaniformFitReport,
    output_dir: str | Path,
) -> WorkflowArtifactPaths:
    paths = resolve_fit_artifact_paths(run_id=model.run_id, output_dir=output_dir)

    ensure_artifact_directory(paths)

    model.save(paths.primary_output_path)
    fit_report.save(paths.report_path)

    write_json_document(
        paths.manifest_path,
        build_run_manifest(
            workflow="fit",
            run_id=paths.run_id,
            primary_output_path=paths.primary_output_path,
            report_path=paths.report_path,
            run_directory=paths.run_directory,
            additional_artifacts={"model_path": str(paths.primary_output_path)},
        ),
    )
    return paths


def write_simulation_artifacts(
    *,
    model: JaniformModel,
    simulation_result: CoarseSimulationPath | SeededTickSimulationResponse,
    output_path: str | Path,
    simulation_run_id: str,
    interval_label: Optional[str],
) -> WorkflowArtifactPaths:
    preferences = simulation_output_preferences_from_model(model)

    paths = resolve_simulation_artifact_paths(
        run_id=simulation_run_id,
        output_path=output_path,
        default_file_format=_file_format_for_simulation_path(Path(output_path), preferences.file_format),
        interval_label=interval_label,
    )

    ensure_artifact_directory(paths)

    if isinstance(simulation_result, SeededTickSimulationResponse):
        persist_seeded_tick_simulation(
            simulation_result=simulation_result,
            output_path=paths.primary_output_path,
            preferences=preferences,
        )
        report_payload = build_simulation_report_payload(
            simulation_kind="seeded_tick",
            output_path=paths.primary_output_path,
            diagnostics=simulation_result.diagnostics.to_payload(),
        )
    else:
        persist_coarse_simulation(
            simulation_result=simulation_result,
            output_path=paths.primary_output_path,
            preferences=preferences,
            calendar_registry=CalendarRegistry(model.session_resolution.resolved_session_definition_json),
            exchange_code=model.session_resolution.exchange_code,
        )
        report_payload = build_simulation_report_payload(
            simulation_kind="coarse_interval",
            output_path=paths.primary_output_path,
            diagnostics=simulation_result.diagnostics.to_payload(),
        )

    write_json_document(paths.report_path, report_payload)
    write_json_document(
        paths.manifest_path,
        build_run_manifest(
            workflow="simulate",
            run_id=paths.run_id,
            primary_output_path=paths.primary_output_path,
            report_path=paths.report_path,
            run_directory=paths.run_directory,
            additional_artifacts={"model_run_id": model.run_id},
        ),
    )

    return paths


def write_pricing_artifacts(
    *,
    model: JaniformModel,
    pricing_result: VanillaOptionPricingRunResult,
    output_path: str | Path,
    pricing_run_id: str,
) -> WorkflowArtifactPaths:
    paths = resolve_pricing_artifact_paths(run_id=pricing_run_id, output_path=output_path)

    ensure_artifact_directory(paths)

    pricing_result.save(paths.primary_output_path)

    write_json_document(
        paths.report_path,
        build_pricing_report_payload(output_path=paths.primary_output_path, pricing_result=pricing_result),
    )
    write_json_document(
        paths.manifest_path,
        build_run_manifest(
            workflow="price",
            run_id=paths.run_id,
            primary_output_path=paths.primary_output_path,
            report_path=paths.report_path,
            run_directory=paths.run_directory,
            additional_artifacts={"model_run_id": model.run_id},
        ),
    )

    return paths


def simulation_output_preferences_from_model(model: JaniformModel) -> SimulationOutputPreferences:
    input_inference = cast(dict[str, Any], model.metadata["input_inference"])
    price_columns = cast(dict[str, Any], input_inference["tick_price_columns"])
    reference_price = cast(
        Optional[float],
        cast(dict[str, Any], model.metadata.get("simulation_output_preferences", {})).get("reference_price"),
    )

    return SimulationOutputPreferences(
        file_format=cast(str, input_inference["tick_data_format"]),
        timestamp_column=cast(str, input_inference["timestamp_column"]),
        timestamp_timezone=cast(str, input_inference["timestamp_timezone"]),
        tick_price_input_policy=cast(Optional[str], input_inference["tick_price_input_policy"]),
        price_column=cast(Optional[str], price_columns["price_column"]),
        trade_sign_column=cast(Optional[str], price_columns["trade_sign_column"]),
        bid_column=cast(Optional[str], price_columns["bid_column"]),
        ask_column=cast(Optional[str], price_columns["ask_column"]),
        reference_price=1.0 if reference_price is None else reference_price,
    )


def simulation_output_price_column(preferences: SimulationOutputPreferences) -> str:
    match preferences.tick_price_input_policy:
        case "bid_ask_midpoint":
            return "mid_price"
        case _:
            return preferences.price_column or "price"


def simulation_output_columns(preferences: SimulationOutputPreferences) -> tuple[str, ...]:
    columns = [preferences.timestamp_column, simulation_output_price_column(preferences)]
    if (
        preferences.tick_price_input_policy == "explicit_price_column"
        and preferences.trade_sign_column is not None
    ):
        columns.append(preferences.trade_sign_column)

    return tuple(columns)


def persist_seeded_tick_simulation(
    *,
    simulation_result: SeededTickSimulationResponse,
    output_path: str | Path,
    preferences: SimulationOutputPreferences,
    sink: Optional[SimulationArtifactSink] = None,
) -> None:
    resolved_output_path = Path(output_path)

    effective_sink = sink or _build_simulation_sink(
        output_path=resolved_output_path,
        columns=simulation_output_columns(preferences),
    )

    try:
        record_batch: list[dict[str, Any]] = []
        for record in simulation_result.fine_path:
            timestamp_value = _convert_timestamp_for_output(
                record.timestamp_utc_nanos,
                timezone_name=preferences.timestamp_timezone,
                output_path=resolved_output_path,
            )
            output_row = _seeded_tick_output_row(
                timestamp_value=timestamp_value,
                timestamp_column=preferences.timestamp_column,
                price_value=preferences.reference_price * math.exp(record.observed_log_price),
                side_value=None if record.side is None else record.side.value,
                preferences=preferences,
            )

            record_batch.append(output_row)
            if len(record_batch) >= 256:
                effective_sink.write_records(record_batch)
                record_batch = []

        if len(record_batch) > 0:
            effective_sink.write_records(record_batch)
    finally:
        effective_sink.close()


def persist_coarse_simulation(
    *,
    simulation_result: CoarseSimulationPath,
    output_path: str | Path,
    preferences: SimulationOutputPreferences,
    sink: Optional[SimulationArtifactSink] = None,
    calendar_registry: Optional[CalendarRegistry] = None,
    exchange_code: Optional[str] = None,
) -> None:
    resolved_output_path = Path(output_path)

    if (calendar_registry is None) != (exchange_code is None):
        raise ValueError("calendar_registry and exchange_code must be provided together")

    effective_sink = sink or _build_simulation_sink(
        output_path=resolved_output_path,
        columns=simulation_output_columns(preferences),
    )

    try:
        record_batch: list[dict[str, Any]] = []
        for record in simulation_result.records:
            if not _coarse_interval_is_open(
                interval_start_utc_nanos=record.interval_start_utc_nanos,
                calendar_registry=calendar_registry,
                exchange_code=exchange_code,
            ):
                continue

            record_batch.append(
                _seeded_tick_output_row(
                    timestamp_value=_convert_timestamp_for_output(
                        record.interval_end_utc_nanos,
                        timezone_name=preferences.timestamp_timezone,
                        output_path=resolved_output_path,
                    ),
                    timestamp_column=preferences.timestamp_column,
                    price_value=preferences.reference_price * math.exp(record.observed_log_price_end),
                    side_value=None,
                    preferences=preferences,
                )
            )
            if len(record_batch) >= 256:
                effective_sink.write_records(record_batch)
                record_batch = []

        if len(record_batch) > 0:
            effective_sink.write_records(record_batch)
    finally:
        effective_sink.close()


def build_run_manifest(
    *,
    workflow: str,
    run_id: str,
    primary_output_path: Path,
    report_path: Path,
    run_directory: Path,
    additional_artifacts: Optional[dict[str, Any]] = None,
) -> dict[str, Any]:
    payload = {
        "schema_version": "janiform.run_manifest.v1",
        "workflow": workflow,
        "run_id": run_id,
        "created_at_utc": datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "run_directory": str(run_directory),
        "primary_output_path": str(primary_output_path),
        "report_path": str(report_path),
    }

    if additional_artifacts is not None:
        payload["artifacts"] = additional_artifacts

    return payload


def build_simulation_report_payload(
    *,
    simulation_kind: str,
    output_path: Path,
    diagnostics: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema_version": "janiform.workflow_report.v1",
        "workflow": "simulate",
        "simulation_kind": simulation_kind,
        "created_at_utc": datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "primary_output_path": str(output_path),
        "diagnostics": diagnostics,
    }


def build_pricing_report_payload(
    *,
    output_path: Path,
    pricing_result: VanillaOptionPricingRunResult,
) -> dict[str, Any]:
    return {
        "schema_version": "janiform.workflow_report.v1",
        "workflow": "price",
        "created_at_utc": datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "primary_output_path": str(output_path),
        "response": pricing_result.response.to_payload(),
        "run_diagnostics": None if pricing_result.run_diagnostics is None else pricing_result.run_diagnostics.to_payload(),
    }


def write_json_document(path: str | Path, payload: dict[str, Any]) -> None:
    Path(path).write_text(serialize_pretty_json(payload), encoding="utf-8")


def _resolve_run_directory(
    *,
    run_id: str,
    output_dir: Optional[str | Path],
) -> Path:
    if output_dir is None:
        return Path.cwd() / ".janiform" / run_id

    return Path(output_dir) / run_id


def _resolve_output_path(
    *,
    run_id: str,
    output_path: Optional[str | Path],
    default_stem: str,
    default_file_format: str,
) -> Path:
    if output_path is None:
        run_directory = Path.cwd() / ".janiform" / run_id
        return run_directory / f"{default_stem}.{default_file_format}"

    resolved_output_path = Path(output_path)
    if resolved_output_path.suffix == "":
        return resolved_output_path.with_suffix(f".{default_file_format}")

    return resolved_output_path


def _file_format_for_simulation_path(output_path: Path, default_file_format: str) -> str:
    if output_path.suffix == "":
        return default_file_format

    suffix = output_path.suffix.lower().lstrip(".")
    if suffix not in {"csv", "parquet"}:
        raise ValueError("simulation output_path must end in .csv or .parquet")

    return suffix


def _seeded_tick_output_row(
    *,
    timestamp_value: datetime | str,
    timestamp_column: str,
    price_value: float,
    side_value: Optional[str],
    preferences: SimulationOutputPreferences,
) -> dict[str, Any]:
    output_row: dict[str, Any] = {
        timestamp_column: timestamp_value,
        simulation_output_price_column(preferences): price_value,
    }

    if (
        preferences.tick_price_input_policy == "explicit_price_column"
        and preferences.trade_sign_column is not None
    ):
        output_row[preferences.trade_sign_column] = side_value

    return output_row


def _convert_timestamp_for_output(
    timestamp_utc_nanos: int,
    *,
    timezone_name: str,
    output_path: Path,
) -> datetime | str:
    timestamp_utc = _datetime_from_utc_nanos(timestamp_utc_nanos)
    normalized_timestamp = timestamp_utc.astimezone(_zoneinfo(timezone_name))
    if output_path.suffix.lower() == ".csv":
        return normalized_timestamp.isoformat().replace("+00:00", "Z")

    return normalized_timestamp


def _coarse_interval_is_open(
    *,
    interval_start_utc_nanos: int,
    calendar_registry: Optional[CalendarRegistry],
    exchange_code: Optional[str],
) -> bool:
    if calendar_registry is None:
        return True

    return (
        calendar_registry.resolve_market_state(
            cast(str, exchange_code),
            _datetime_from_utc_nanos(interval_start_utc_nanos),
        )
        == MarketState.OPEN
    )


def _datetime_from_utc_nanos(timestamp_utc_nanos: int) -> datetime:
    seconds, nanos_remainder = divmod(timestamp_utc_nanos, 1_000_000_000)
    return datetime.fromtimestamp(seconds, tz=UTC) + timedelta(microseconds=nanos_remainder / 1000.0)


def _zoneinfo(timezone_name: str):
    from zoneinfo import ZoneInfo

    normalized_timezone = timezone_name.strip()
    if normalized_timezone.upper() in {"UTC", "Z", "GMT", "UT"}:
        return UTC

    return ZoneInfo(normalized_timezone)


class _CsvSimulationSink:
    def __init__(self, *, output_path: Path, columns: tuple[str, ...]) -> None:
        self._output_file = output_path.open("w", encoding="utf-8", newline="")
        self._writer = csv.DictWriter(self._output_file, fieldnames=list(columns))
        self._writer.writeheader()

    def write_records(self, records: list[dict[str, Any]]) -> None:
        self._writer.writerows(records)

    def close(self) -> None:
        self._output_file.close()


class _ParquetSimulationSink:
    def __init__(self, *, output_path: Path, columns: tuple[str, ...]) -> None:
        self._output_path = output_path
        self._columns = columns
        self._writer: Optional[parquet.ParquetWriter] = None

    def write_records(self, records: list[dict[str, Any]]) -> None:
        table = pyarrow.Table.from_pylist(records)
        if self._writer is None:
            self._writer = parquet.ParquetWriter(self._output_path, table.schema)

        self._writer.write_table(table)

    def close(self) -> None:
        if self._writer is not None:
            self._writer.close()


def _build_simulation_sink(
    *,
    output_path: Path,
    columns: tuple[str, ...],
) -> SimulationArtifactSink:
    suffix = output_path.suffix.lower()
    if suffix == ".csv":
        return _CsvSimulationSink(output_path=output_path, columns=columns)
    if suffix == ".parquet":
        return _ParquetSimulationSink(output_path=output_path, columns=columns)

    raise ValueError("simulation outputs must use .csv or .parquet")


__all__ = [
    "SimulationArtifactSink",
    "SimulationOutputPreferences",
    "WorkflowArtifactPaths",
    "build_pricing_report_payload",
    "build_run_manifest",
    "build_simulation_report_payload",
    "ensure_artifact_directory",
    "generate_workflow_run_id",
    "persist_coarse_simulation",
    "persist_seeded_tick_simulation",
    "resolve_fit_artifact_paths",
    "resolve_pricing_artifact_paths",
    "resolve_simulation_artifact_paths",
    "simulation_output_columns",
    "simulation_output_price_column",
    "simulation_output_preferences_from_model",
    "write_fit_artifacts",
    "write_json_document",
    "write_pricing_artifacts",
    "write_simulation_artifacts",
]
