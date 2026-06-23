"""Public high-level simulation result adapter."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any, Optional, cast

from .._workflow_outputs import (
    persist_coarse_simulation,
    persist_seeded_tick_simulation,
    simulation_output_columns,
)
from ..calendar import CalendarRegistry
from .coarse import CoarseSimulationPath
from .kinds import SimulationKind
from .seeded import SeededTickSimulationResponse


class _MemorySimulationSink:
    __slots__ = ("rows",)
    rows: list[dict[str, Any]]

    def __init__(self) -> None:
        self.rows: list[dict[str, Any]] = []

    def write_records(self, records: list[dict[str, Any]]) -> None:
        self.rows.extend(records)

    def close(self) -> None:
        return None


@dataclass(slots=True)
class JaniformSimulationResult:
    simulation_kind: SimulationKind
    metadata: dict[str, Any]
    provenance: dict[str, Any]
    diagnostics: dict[str, Any]
    raw_result: CoarseSimulationPath | SeededTickSimulationResponse = field(repr=False)
    _preferences: Optional[Any] = field(repr=False, default=None)
    _calendar_registry: Optional[CalendarRegistry] = field(init=False, repr=False, default=None)
    _exchange_code: Optional[str] = field(init=False, repr=False, default=None)
    _rows_cache: Optional[list[dict[str, Any]]] = field(init=False, repr=False, default=None)

    def to_pandas(self) -> Any:
        import pandas

        rows = self._rows()
        if len(rows) == 0:
            return pandas.DataFrame(columns=self._column_names())

        return pandas.DataFrame.from_records(rows)

    def to_numpy(self) -> Any:
        import numpy

        rows = self._rows()
        if len(rows) == 0:
            return numpy.empty((0, len(self._column_names())), dtype=object)

        column_names = tuple(rows[0].keys())

        return numpy.asarray(
            [tuple(row[column_name] for column_name in column_names) for row in rows],
            dtype=object,
        )

    def to_parquet(self, path: str | Path) -> None:
        output_path = Path(path)
        if self.simulation_kind == SimulationKind.SEEDED_TICK:
            if self._preferences is None:
                raise RuntimeError("seeded simulation exports require output preferences")

            persist_seeded_tick_simulation(
                simulation_result=cast(SeededTickSimulationResponse, self.raw_result),
                output_path=output_path,
                preferences=self._preferences,
            )

            return

        persist_coarse_simulation(
            simulation_result=cast(CoarseSimulationPath, self.raw_result),
            output_path=output_path,
            preferences=self._preferences,
            calendar_registry=self._calendar_registry,
            exchange_code=self._exchange_code,
        )

    def _rows(self) -> list[dict[str, Any]]:
        if self._rows_cache is not None:
            return self._rows_cache

        if self._preferences is None:
            raise RuntimeError("simulation rows require output preferences")

        sink = _MemorySimulationSink()
        if self.simulation_kind == SimulationKind.SEEDED_TICK:
            persist_seeded_tick_simulation(
                simulation_result=cast(SeededTickSimulationResponse, self.raw_result),
                output_path=Path("simulation.parquet"),
                preferences=self._preferences,
                sink=sink,
            )
            rows = sink.rows
        else:
            if self._calendar_registry is None or self._exchange_code is None:
                raise RuntimeError("coarse simulation rows require calendar context")

            persist_coarse_simulation(
                simulation_result=cast(CoarseSimulationPath, self.raw_result),
                output_path=Path("simulation.parquet"),
                preferences=self._preferences,
                sink=sink,
                calendar_registry=self._calendar_registry,
                exchange_code=self._exchange_code,
            )

            rows = sink.rows

        self._rows_cache = rows
        return rows

    def _column_names(self) -> tuple[str, ...]:
        if self.simulation_kind == SimulationKind.COARSE_INTERVAL:
            rows = self._rows()
            return tuple(rows[0].keys()) if rows else (
                "interval_start_utc",
                "interval_end_utc",
                "market_state",
                "total_flow",
                "buy_flow",
                "sell_flow",
                "known_news_intensity",
                "surprise_news_intensity",
                "observed_log_price",
                "anchor_value",
                "market_pressure_state",
                "deviation_from_anchor",
            )

        if self._preferences is None:
            raise RuntimeError("simulation exports require output preferences")

        return simulation_output_columns(self._preferences)


def coarse_simulation_rows(
    simulation_result: CoarseSimulationPath,
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for interval in simulation_result.intervals:
        interval_start_utc = interval.start_time_utc
        rows.append(
            {
                "interval_start_utc": interval_start_utc,
                "interval_end_utc": interval.end_time_utc,
                "market_state": calendar_registry.resolve_market_state(exchange_code, interval_start_utc).value,
                "total_flow": interval.total_flow,
                "buy_flow": interval.buy_flow,
                "sell_flow": interval.sell_flow,
                "known_news_intensity": interval.known_news_intensity,
                "surprise_news_intensity": interval.surprise_news_intensity,
                "observed_log_price": interval.observed_log_price,
                "anchor_value": interval.anchor_value,
                "market_pressure_state": interval.market_pressure_state,
                "deviation_from_anchor": interval.deviation_from_anchor,
            }
        )

    return rows


def normalize_interval_label(interval: str | timedelta) -> str:
    if isinstance(interval, str):
        return interval.replace("/", "_")

    total_seconds = int(interval.total_seconds())
    if total_seconds % 86_400 == 0:
        return f"{total_seconds // 86_400}D"
    if total_seconds % 3_600 == 0:
        return f"{total_seconds // 3_600}h"
    if total_seconds % 60 == 0:
        return f"{total_seconds // 60}min"

    return f"{total_seconds}s"


__all__ = ["JaniformSimulationResult"]
