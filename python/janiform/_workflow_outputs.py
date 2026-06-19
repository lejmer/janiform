from __future__ import annotations

import csv
import math
from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import TYPE_CHECKING, Any, Final, Optional, Protocol, cast

import pyarrow
from pyarrow import parquet

from ._native_api import native_module
from ._timestamps import utc_nanos_to_datetime
from .calendar import CalendarRegistry, MarketState

if TYPE_CHECKING:
    from .model import JaniformModel
    from .simulation.coarse import CoarseSimulationPath
    from .simulation.seeded import SeededTickSimulationResponse

DEFAULT_SIMULATION_FILE_FORMAT: Final[str] = "parquet"
DEFAULT_SIMULATION_TIMESTAMP_COLUMN: Final[str] = "timestamp_utc"
DEFAULT_SIMULATION_TIMESTAMP_TIMEZONE: Final[str] = "UTC"
DEFAULT_SIMULATION_PRICE_COLUMN: Final[str] = "price"
DEFAULT_SIMULATION_SIDE_COLUMN: Final[str] = "side"


class SimulationArtifactSink(Protocol):
    def write_records(self, records: list[dict[str, Any]]) -> None: ...

    def close(self) -> None: ...


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


def simulation_output_preferences_from_model(model: JaniformModel) -> SimulationOutputPreferences:
    reference_price = cast(
        dict[str, Any],
        model.metadata.get("simulation_output_preferences", {}),
    ).get("reference_price")
    raw_input_inference = model.metadata.get("input_inference")

    if raw_input_inference is None:
        return SimulationOutputPreferences(
            file_format=DEFAULT_SIMULATION_FILE_FORMAT,
            timestamp_column=DEFAULT_SIMULATION_TIMESTAMP_COLUMN,
            timestamp_timezone=DEFAULT_SIMULATION_TIMESTAMP_TIMEZONE,
            tick_price_input_policy=None,
            price_column=DEFAULT_SIMULATION_PRICE_COLUMN,
            trade_sign_column=None,
            bid_column=None,
            ask_column=None,
            reference_price=1.0 if reference_price is None else reference_price,
        )

    # Deferred because fitted-model runtime modules import this artifact module.
    from .calibration.fit._runtime.input_inference import (
        tick_data_input_inference_from_model_metadata,
    )

    input_inference = tick_data_input_inference_from_model_metadata(model.metadata)

    return SimulationOutputPreferences(
        file_format=input_inference.file_format,
        timestamp_column=input_inference.timestamp_column,
        timestamp_timezone=input_inference.timestamp_timezone,
        tick_price_input_policy=input_inference.tick_price_input_policy,
        price_column=input_inference.price_column,
        trade_sign_column=input_inference.trade_sign_column,
        bid_column=input_inference.bid_column,
        ask_column=input_inference.ask_column,
        reference_price=1.0 if reference_price is None else reference_price,
    )


def simulation_output_price_column(preferences: SimulationOutputPreferences) -> str:
    match preferences.tick_price_input_policy:
        case "bid_ask_midpoint":
            return "mid_price"
        case _:
            return preferences.price_column or "price"


def simulation_output_side_column(preferences: SimulationOutputPreferences) -> str:
    return preferences.trade_sign_column or DEFAULT_SIMULATION_SIDE_COLUMN


def simulation_output_columns(
    preferences: SimulationOutputPreferences,
    *,
    include_side: bool = True,
) -> tuple[str, ...]:
    columns = [preferences.timestamp_column, simulation_output_price_column(preferences)]
    if include_side:
        columns.append(simulation_output_side_column(preferences))

    columns.extend(
        (
            "observed_log_price",
            "anchor_value",
            "pressure_state",
            "deviation_from_anchor",
        )
    )

    return tuple(columns)


def persist_seeded_tick_simulation(
    *,
    simulation_result: SeededTickSimulationResponse,
    output_path: str | Path,
    preferences: SimulationOutputPreferences,
    sink: Optional[SimulationArtifactSink] = None,
) -> None:
    from .simulation.seeded import SeededTickSimulationResponse

    resolved_output_path = Path(output_path)
    side_column = simulation_output_side_column(preferences)
    if sink is None:
        native_result = (
            simulation_result._native_result
            if isinstance(simulation_result, SeededTickSimulationResponse)
            else None
        )
        if native_result is not None:
            native_module().export_seeded_tick_simulation_artifact(
                native_result,
                str(resolved_output_path),
                preferences.reference_price,
                preferences.timestamp_column,
                simulation_output_price_column(preferences),
                side_column,
            )

            return

    effective_sink = sink or _build_simulation_sink(
        output_path=resolved_output_path,
        columns=simulation_output_columns(preferences),
    )

    try:
        record_batch: list[dict[str, Any]] = []
        for record in simulation_result.fine_path:
            timestamp_value = _convert_timestamp_for_output(
                record.observed_timestamp_utc_nanos,
                timezone_name=preferences.timestamp_timezone,
                output_path=resolved_output_path,
            )
            output_row = _seeded_tick_output_row(
                timestamp_value=timestamp_value,
                timestamp_column=preferences.timestamp_column,
                price_value=preferences.reference_price * math.exp(record.observed_log_price),
                side_value=None if record.side is None else record.side.value,
                observed_log_price=record.observed_log_price,
                anchor_value=record.anchor_value,
                pressure_state=record.pressure_state,
                deviation_from_anchor=record.deviation_from_anchor,
                preferences=preferences,
                side_column=side_column,
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

    if sink is None:
        if calendar_registry is None or exchange_code is None:
            raise RuntimeError("coarse simulation file export requires calendar context")

        native_module().export_coarse_simulation_artifact(
            simulation_result.to_payload(),
            str(resolved_output_path),
            preferences.reference_price,
            preferences.timestamp_column,
            simulation_output_price_column(preferences),
            calendar_registry._native_registry,
            exchange_code,
        )

        return

    effective_sink = sink or _build_simulation_sink(
        output_path=resolved_output_path,
        columns=simulation_output_columns(preferences, include_side=False),
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
                    observed_log_price=record.observed_log_price_end,
                    anchor_value=record.anchor_value_end,
                    pressure_state=record.pressure_state_end,
                    deviation_from_anchor=record.deviation_from_anchor_end,
                    preferences=preferences,
                    side_column=None,
                )
            )
            if len(record_batch) >= 256:
                effective_sink.write_records(record_batch)
                record_batch = []

        if len(record_batch) > 0:
            effective_sink.write_records(record_batch)
    finally:
        effective_sink.close()


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
    observed_log_price: float,
    anchor_value: float,
    pressure_state: float,
    deviation_from_anchor: float,
    preferences: SimulationOutputPreferences,
    side_column: Optional[str],
) -> dict[str, Any]:
    output_row: dict[str, Any] = {
        timestamp_column: timestamp_value,
        simulation_output_price_column(preferences): price_value,
    }

    if side_column is not None:
        output_row[side_column] = side_value

    output_row["observed_log_price"] = observed_log_price
    output_row["anchor_value"] = anchor_value
    output_row["pressure_state"] = pressure_state
    output_row["deviation_from_anchor"] = deviation_from_anchor

    return output_row


def _convert_timestamp_for_output(
    timestamp_utc_nanos: int,
    *,
    timezone_name: str,
    output_path: Path,
) -> datetime | str:
    timestamp_utc = utc_nanos_to_datetime(timestamp_utc_nanos)
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
            utc_nanos_to_datetime(interval_start_utc_nanos),
        )
        == MarketState.OPEN
    )


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
    "generate_workflow_run_id",
    "persist_coarse_simulation",
    "persist_seeded_tick_simulation",
    "simulation_output_columns",
    "simulation_output_price_column",
    "simulation_output_preferences_from_model",
    "simulation_output_side_column",
]
