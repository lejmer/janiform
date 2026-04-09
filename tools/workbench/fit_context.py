import csv
import hashlib
import json
import math
from collections.abc import Iterator
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta, tzinfo
from functools import lru_cache
from pathlib import Path, PurePosixPath, PureWindowsPath
from typing import Any, Callable, Optional, cast
from zoneinfo import ZoneInfo

import numpy as np
from numpy.typing import NDArray

from pyarrow import RecordBatch, parquet
from pyarrow import csv as pyarrow_csv

from janiform._timestamps import (
    datetime_to_utc_microseconds,
    parse_timestamp_value,
    utc_microseconds_to_datetime,
)
from janiform.calendar import CalendarRegistry, MarketState
from janiform.calibration.fit import JaniformModel
from janiform.simulation import SimulationProgressEvent

from .artifacts import WorkbenchArtifacts


_DEFAULT_COMPARISON_SIMULATION_SEED = 1
_PARQUET_BATCH_SIZE = 65_536
_CSV_BLOCK_SIZE_BYTES = 1 << 20
_EMPTY_FLOAT64_ARRAY = np.asarray([], dtype=np.float64)
_EMPTY_INT64_ARRAY = np.asarray([], dtype=np.int64)
_WORKBENCH_COMPARISON_RANGE_POLICY = "full_observed_range.v1"
_WORKBENCH_COMPARISON_CACHE_SCHEMA_VERSION = "workbench.observed_simulation_comparison_cache.v1"
_WORKBENCH_COMPARISON_CACHE_DIRECTORY_NAME = ".workbench_cache"
_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT = 5


@dataclass(frozen=True, slots=True)
class WorkbenchPriceSeries:
    timestamp_microseconds_utc: NDArray[np.int64]
    prices: NDArray[np.float64]
    direct_return_timestamps_microseconds_utc: Optional[NDArray[np.int64]] = None
    direct_returns: Optional[NDArray[np.float64]] = None


@dataclass(frozen=True, slots=True)
class WorkbenchSourceTable:
    source_path: Path
    file_format: str
    column_names: tuple[str, ...]
    source_row_count: int


@dataclass(frozen=True, slots=True)
class WorkbenchFitContext:
    model: JaniformModel
    fit_report_path: Path
    tick_data_source: WorkbenchSourceTable
    observed_tick_series: WorkbenchPriceSeries
    observed_range_start_utc: datetime
    observed_range_end_utc: datetime
    option_surface_source: Optional[WorkbenchSourceTable] = None
    known_news_source: Optional[WorkbenchSourceTable] = None


@dataclass(frozen=True, slots=True)
class WorkbenchObservedSimulationComparison:
    fit_context: WorkbenchFitContext
    simulation_seed: int
    observed_series: WorkbenchPriceSeries
    observed_comparison_series: WorkbenchPriceSeries
    simulated_series: WorkbenchPriceSeries
    simulated_seeded_series_15m: WorkbenchPriceSeries
    simulated_coarse_series_15m: WorkbenchPriceSeries


@dataclass(frozen=True, slots=True)
class WorkbenchComparisonBuildProgress:
    detail: str
    completed_units: int
    total_units: int


def load_fit_context(artifacts: WorkbenchArtifacts) -> Optional[WorkbenchFitContext]:
    resolved_paths = _resolve_fit_artifact_paths(artifacts.source_path)
    if resolved_paths is None:
        return None

    model_path, fit_report_path = resolved_paths
    if not model_path.is_file() or not fit_report_path.is_file():
        return None

    model_cache_path, model_cache_timestamp, model_cache_size = _path_cache_identity(model_path)
    fit_report_cache_path, fit_report_cache_timestamp, fit_report_cache_size = _path_cache_identity(
        fit_report_path
    )

    return _load_fit_context_cached(
        model_cache_path,
        model_cache_timestamp,
        model_cache_size,
        fit_report_cache_path,
        fit_report_cache_timestamp,
        fit_report_cache_size,
    )


@lru_cache(maxsize=8)
def _load_fit_context_cached(
    model_path_text: str,
    model_path_timestamp_nanoseconds: int,
    model_path_size_bytes: int,
    fit_report_path_text: str,
    fit_report_path_timestamp_nanoseconds: int,
    fit_report_path_size_bytes: int,
) -> Optional[WorkbenchFitContext]:
    _ = model_path_timestamp_nanoseconds
    _ = model_path_size_bytes
    _ = fit_report_path_timestamp_nanoseconds
    _ = fit_report_path_size_bytes

    model_path = Path(model_path_text)
    fit_report_path = Path(fit_report_path_text)

    report_payload = _load_json_object(fit_report_path)
    preparation_provenance = report_payload.get("preparation_provenance")
    if not isinstance(preparation_provenance, dict):
        return None

    raw_tick_summary = preparation_provenance.get("tick_data_summary")
    if not isinstance(raw_tick_summary, dict):
        return None

    model = JaniformModel.load(model_path)
    tick_data_source = _load_source_table(raw_tick_summary)

    observed_tick_series = _normalize_tick_series(
        source_table=tick_data_source,
        model=model,
        preparation_provenance=preparation_provenance,
    )
    if observed_tick_series.timestamp_microseconds_utc.size == 0:
        return None

    observed_range_start_utc, observed_range_end_utc = _observed_utc_range(
        observed_tick_series
    )

    raw_option_surface_summary = preparation_provenance.get("option_surface_summary")
    raw_known_news_summary = preparation_provenance.get("known_news_summary")

    return WorkbenchFitContext(
        model=model,
        fit_report_path=fit_report_path,
        tick_data_source=tick_data_source,
        observed_tick_series=observed_tick_series,
        observed_range_start_utc=observed_range_start_utc,
        observed_range_end_utc=observed_range_end_utc,
        option_surface_source=(
            _load_source_table(raw_option_surface_summary)
            if isinstance(raw_option_surface_summary, dict)
            else None
        ),
        known_news_source=(
            _load_source_table(raw_known_news_summary)
            if isinstance(raw_known_news_summary, dict)
            else None
        ),
    )


def build_observed_simulation_comparison(
    artifacts: WorkbenchArtifacts,
    *,
    seed: int = _DEFAULT_COMPARISON_SIMULATION_SEED,
    progress_callback: Optional[Callable[[WorkbenchComparisonBuildProgress], None]] = None,
) -> Optional[WorkbenchObservedSimulationComparison]:
    fit_context = load_fit_context(artifacts)
    if fit_context is None:
        return None

    return build_observed_simulation_comparison_for_fit_context(
        fit_context,
        model=fit_context.model,
        seed=seed,
        progress_callback=progress_callback,
    )


def build_observed_simulation_comparison_for_fit_context(
    fit_context: WorkbenchFitContext,
    *,
    model: JaniformModel,
    seed: int = _DEFAULT_COMPARISON_SIMULATION_SEED,
    progress_callback: Optional[Callable[[WorkbenchComparisonBuildProgress], None]] = None,
) -> Optional[WorkbenchObservedSimulationComparison]:
    comparison_range = _comparison_range_for_model(
        model=model,
        observed_range_start_utc=fit_context.observed_range_start_utc,
        observed_range_end_utc=fit_context.observed_range_end_utc,
    )
    if comparison_range is None:
        return None

    comparison_range_start_utc, comparison_range_end_utc = comparison_range
    observed_series = _clip_price_series_to_range(
        fit_context.observed_tick_series,
        range_start_utc=comparison_range_start_utc,
        range_end_utc=comparison_range_end_utc,
    )
    if observed_series.timestamp_microseconds_utc.size == 0:
        return None

    _emit_comparison_build_progress(
        progress_callback,
        detail="checking cached simulated comparison",
        completed_units=0,
        total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
    )
    cached_comparison_series = _load_cached_simulated_comparison_series(
        fit_context=fit_context,
        model=model,
        seed=seed,
        range_start_utc=comparison_range_start_utc,
        range_end_utc=comparison_range_end_utc,
    )
    if cached_comparison_series is not None:
        _emit_comparison_build_progress(
            progress_callback,
            detail="loaded cached simulated comparison",
            completed_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
            total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
        )
        simulated_series, seeded_simulated_series_15m, coarse_simulated_series = cached_comparison_series
        return WorkbenchObservedSimulationComparison(
            fit_context=fit_context,
            simulation_seed=seed,
            observed_series=fit_context.observed_tick_series,
            observed_comparison_series=observed_series,
            simulated_series=simulated_series,
            simulated_seeded_series_15m=seeded_simulated_series_15m,
            simulated_coarse_series_15m=coarse_simulated_series,
        )

    simulation_start_utc, horizon_interval_count = _simulation_request_window_for_observed_range(
        observed_range_start_utc=comparison_range_start_utc,
        observed_range_end_utc=comparison_range_end_utc,
    )

    _emit_comparison_build_progress(
        progress_callback,
        detail="simulating seeded ticks",
        completed_units=1,
        total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
    )
    def emit_seeded_simulation_progress(event: SimulationProgressEvent) -> None:
        _emit_comparison_build_progress(
            progress_callback,
            detail=event.detail,
            completed_units=event.completed_units,
            total_units=event.total_units,
        )

    simulation_result = model.simulate(
        seed=seed,
        start_time_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        silent=True,
        progress_callback=emit_seeded_simulation_progress,
    )

    _emit_comparison_build_progress(
        progress_callback,
        detail="materializing simulated tick series",
        completed_units=2,
        total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
    )
    simulated_series = _series_from_simulation_result(
        simulation_result=simulation_result,
        model=model,
        range_start_utc=comparison_range_start_utc,
        range_end_utc=comparison_range_end_utc,
    )

    coarse_simulation_start_utc, coarse_horizon_interval_count = _simulation_request_window_for_interval(
        observed_range_start_utc=comparison_range_start_utc,
        observed_range_end_utc=comparison_range_end_utc,
        interval=timedelta(minutes=15),
    )

    _emit_comparison_build_progress(
        progress_callback,
        detail="simulating coarse 15 minute intervals",
        completed_units=3,
        total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
    )
    def emit_coarse_simulation_progress(event: SimulationProgressEvent) -> None:
        _emit_comparison_build_progress(
            progress_callback,
            detail=event.detail,
            completed_units=event.completed_units,
            total_units=event.total_units,
        )

    coarse_simulation_result = model.simulate(
        seed=seed,
        start_time_utc=coarse_simulation_start_utc,
        horizon_interval_count=coarse_horizon_interval_count,
        interval="15m",
        silent=True,
        progress_callback=emit_coarse_simulation_progress,
    )

    session_resolution = getattr(model, "session_resolution", None)
    if session_resolution is not None:
        calendar_registry = CalendarRegistry(session_resolution.resolved_session_definition_json)
        exchange_code = session_resolution.exchange_code
        reference_price = _simulation_reference_price(model)

        _emit_comparison_build_progress(
            progress_callback,
            detail="assembling simulated comparison series",
            completed_units=4,
            total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
        )
        seeded_simulated_series_15m = _series_from_seeded_raw_result(
            simulation_result=simulation_result,
            reference_price=reference_price,
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            interval_start_utc=coarse_simulation_start_utc,
            horizon_interval_count=coarse_horizon_interval_count,
            interval=timedelta(minutes=15),
            range_start_utc=comparison_range_start_utc,
            range_end_utc=comparison_range_end_utc,
        )
        coarse_simulated_series = _series_from_coarse_raw_result(
            simulation_result=coarse_simulation_result,
            reference_price=reference_price,
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            range_start_utc=comparison_range_start_utc,
            range_end_utc=comparison_range_end_utc,
        )
    else:
        _emit_comparison_build_progress(
            progress_callback,
            detail="assembling simulated comparison series",
            completed_units=4,
            total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
        )
        seeded_simulated_series_15m = _series_from_simulation_result(
            simulation_result=simulation_result,
            model=model,
            range_start_utc=comparison_range_start_utc,
            range_end_utc=comparison_range_end_utc,
        )
        coarse_simulated_series = _series_from_simulation_result(
            simulation_result=coarse_simulation_result,
            model=model,
            range_start_utc=comparison_range_start_utc,
            range_end_utc=comparison_range_end_utc,
        )

    _write_cached_simulated_comparison_series(
        fit_context=fit_context,
        model=model,
        seed=seed,
        range_start_utc=comparison_range_start_utc,
        range_end_utc=comparison_range_end_utc,
        simulated_series=simulated_series,
        seeded_simulated_series_15m=seeded_simulated_series_15m,
        coarse_simulated_series=coarse_simulated_series,
    )
    _emit_comparison_build_progress(
        progress_callback,
        detail="simulated comparison ready",
        completed_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
        total_units=_WORKBENCH_COMPARISON_BUILD_PHASE_COUNT,
    )
    return WorkbenchObservedSimulationComparison(
        fit_context=fit_context,
        simulation_seed=seed,
        observed_series=fit_context.observed_tick_series,
        observed_comparison_series=observed_series,
        simulated_series=simulated_series,
        simulated_seeded_series_15m=seeded_simulated_series_15m,
        simulated_coarse_series_15m=coarse_simulated_series,
    )


def _emit_comparison_build_progress(
    progress_callback: Optional[Callable[[WorkbenchComparisonBuildProgress], None]],
    *,
    detail: str,
    completed_units: int,
    total_units: int,
) -> None:
    if progress_callback is None:
        return

    progress_callback(
        WorkbenchComparisonBuildProgress(
            detail=detail,
            completed_units=completed_units,
            total_units=total_units,
        )
    )


def _comparison_range_for_model(
    *,
    model: JaniformModel,
    observed_range_start_utc: datetime,
    observed_range_end_utc: datetime,
) -> Optional[tuple[datetime, datetime]]:
    _ = model

    return observed_range_start_utc, observed_range_end_utc


def _clip_price_series_to_range(
    series: WorkbenchPriceSeries,
    *,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> WorkbenchPriceSeries:
    range_start_microseconds_utc = datetime_to_utc_microseconds(range_start_utc)
    range_end_microseconds_utc = datetime_to_utc_microseconds(range_end_utc)
    in_range_mask = (
        (series.timestamp_microseconds_utc >= range_start_microseconds_utc)
        & (series.timestamp_microseconds_utc <= range_end_microseconds_utc)
    )

    direct_return_timestamps_microseconds_utc: Optional[NDArray[np.int64]] = None
    direct_returns: Optional[NDArray[np.float64]] = None
    if (
        series.direct_return_timestamps_microseconds_utc is not None
        and series.direct_returns is not None
    ):
        direct_in_range_mask = (
            (series.direct_return_timestamps_microseconds_utc >= range_start_microseconds_utc)
            & (series.direct_return_timestamps_microseconds_utc <= range_end_microseconds_utc)
        )
        direct_return_timestamps_microseconds_utc = (
            series.direct_return_timestamps_microseconds_utc[direct_in_range_mask]
        )
        direct_returns = series.direct_returns[direct_in_range_mask]

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=series.timestamp_microseconds_utc[in_range_mask],
        prices=series.prices[in_range_mask],
        direct_return_timestamps_microseconds_utc=direct_return_timestamps_microseconds_utc,
        direct_returns=direct_returns,
    )


def _simulation_request_window_for_observed_range(
    *,
    observed_range_start_utc: datetime,
    observed_range_end_utc: datetime,
) -> tuple[datetime, int]:
    return _simulation_request_window_for_interval(
        observed_range_start_utc=observed_range_start_utc,
        observed_range_end_utc=observed_range_end_utc,
        interval=timedelta(hours=1),
    )


def _simulation_request_window_for_interval(
    *,
    observed_range_start_utc: datetime,
    observed_range_end_utc: datetime,
    interval: timedelta,
) -> tuple[datetime, int]:
    normalized_start_utc = observed_range_start_utc.astimezone(UTC)
    normalized_end_utc = observed_range_end_utc.astimezone(UTC)

    interval_seconds = interval.total_seconds()
    start_seconds = normalized_start_utc.timestamp()
    end_seconds = normalized_end_utc.timestamp()

    simulation_start_utc = datetime.fromtimestamp(
        math.floor(start_seconds / interval_seconds) * interval_seconds,
        tz=UTC,
    )
    simulation_end_utc = datetime.fromtimestamp(
        (
            math.ceil(end_seconds / interval_seconds) * interval_seconds
            if (end_seconds % interval_seconds) != 0.0
            else end_seconds + interval_seconds
        ),
        tz=UTC,
    )

    horizon_interval_count = max(
        1,
        int((simulation_end_utc - simulation_start_utc) / interval),
    )

    return simulation_start_utc, horizon_interval_count


def _load_cached_simulated_comparison_series(
    *,
    fit_context: WorkbenchFitContext,
    model: JaniformModel,
    seed: int,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> Optional[tuple[WorkbenchPriceSeries, WorkbenchPriceSeries, WorkbenchPriceSeries]]:
    cache_path = _simulated_comparison_cache_path(
        fit_context=fit_context,
        model=model,
        seed=seed,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
    )
    if cache_path is None or not cache_path.is_file():
        return None

    try:
        with np.load(cache_path, allow_pickle=False) as cached_payload:
            cached_schema_version = str(cached_payload["schema_version"].item())
            if cached_schema_version != _WORKBENCH_COMPARISON_CACHE_SCHEMA_VERSION:
                return None

            return (
                _price_series_from_cached_payload(cached_payload, "simulated"),
                _price_series_from_cached_payload(cached_payload, "simulated_seeded_15m"),
                _price_series_from_cached_payload(cached_payload, "simulated_coarse_15m"),
            )
    except (KeyError, OSError, ValueError):
        return None


def _write_cached_simulated_comparison_series(
    *,
    fit_context: WorkbenchFitContext,
    model: JaniformModel,
    seed: int,
    range_start_utc: datetime,
    range_end_utc: datetime,
    simulated_series: WorkbenchPriceSeries,
    seeded_simulated_series_15m: WorkbenchPriceSeries,
    coarse_simulated_series: WorkbenchPriceSeries,
) -> None:
    cache_path = _simulated_comparison_cache_path(
        fit_context=fit_context,
        model=model,
        seed=seed,
        range_start_utc=range_start_utc,
        range_end_utc=range_end_utc,
    )
    if cache_path is None:
        return

    cache_path.parent.mkdir(parents=True, exist_ok=True)
    temporary_cache_path = cache_path.with_name(f"{cache_path.name}.tmp.npz")

    np.savez_compressed(
        temporary_cache_path,
        schema_version=np.asarray(_WORKBENCH_COMPARISON_CACHE_SCHEMA_VERSION),
        **_cached_payload_for_price_series("simulated", simulated_series),
        **_cached_payload_for_price_series("simulated_seeded_15m", seeded_simulated_series_15m),
        **_cached_payload_for_price_series("simulated_coarse_15m", coarse_simulated_series),
    )
    temporary_cache_path.replace(cache_path)


def _simulated_comparison_cache_path(
    *,
    fit_context: WorkbenchFitContext,
    model: JaniformModel,
    seed: int,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> Optional[Path]:
    if model is not fit_context.model:
        return None

    fit_report_path = fit_context.fit_report_path
    model_path = fit_report_path.with_name("model.json")
    if not fit_report_path.is_file() or not model_path.is_file():
        return None

    cache_identity = {
        "schema_version": _WORKBENCH_COMPARISON_CACHE_SCHEMA_VERSION,
        "seed": seed,
        "range_start_utc": range_start_utc.isoformat(),
        "range_end_utc": range_end_utc.isoformat(),
        "model_path": str(model_path.resolve()),
        "model_mtime_ns": model_path.stat().st_mtime_ns,
        "model_size_bytes": model_path.stat().st_size,
        "fit_report_path": str(fit_report_path.resolve()),
        "fit_report_mtime_ns": fit_report_path.stat().st_mtime_ns,
        "fit_report_size_bytes": fit_report_path.stat().st_size,
    }
    cache_key = hashlib.sha256(
        json.dumps(cache_identity, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()
    return (
        fit_report_path.parent
        / _WORKBENCH_COMPARISON_CACHE_DIRECTORY_NAME
        / f"observed_simulation_comparison_{cache_key}.npz"
    )


def _cached_payload_for_price_series(
    prefix: str,
    series: WorkbenchPriceSeries,
) -> dict[str, NDArray[Any]]:
    has_direct_returns = (
        series.direct_return_timestamps_microseconds_utc is not None
        and series.direct_returns is not None
    )
    return {
        f"{prefix}_timestamp_microseconds_utc": series.timestamp_microseconds_utc,
        f"{prefix}_prices": series.prices,
        f"{prefix}_has_direct_returns": np.asarray(1 if has_direct_returns else 0, dtype=np.int8),
        f"{prefix}_direct_return_timestamps_microseconds_utc": (
            _EMPTY_INT64_ARRAY
            if series.direct_return_timestamps_microseconds_utc is None
            else series.direct_return_timestamps_microseconds_utc
        ),
        f"{prefix}_direct_returns": (
            _EMPTY_FLOAT64_ARRAY
            if series.direct_returns is None
            else series.direct_returns
        ),
    }


def _price_series_from_cached_payload(
    cached_payload: Any,
    prefix: str,
) -> WorkbenchPriceSeries:
    has_direct_returns = bool(int(cached_payload[f"{prefix}_has_direct_returns"].item()))
    direct_return_timestamps_microseconds_utc: Optional[NDArray[np.int64]] = None
    direct_returns: Optional[NDArray[np.float64]] = None
    if has_direct_returns:
        direct_return_timestamps_microseconds_utc = np.asarray(
            cached_payload[f"{prefix}_direct_return_timestamps_microseconds_utc"],
            dtype=np.int64,
        )
        direct_returns = np.asarray(
            cached_payload[f"{prefix}_direct_returns"],
            dtype=np.float64,
        )

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=np.asarray(
            cached_payload[f"{prefix}_timestamp_microseconds_utc"],
            dtype=np.int64,
        ),
        prices=np.asarray(
            cached_payload[f"{prefix}_prices"],
            dtype=np.float64,
        ),
        direct_return_timestamps_microseconds_utc=direct_return_timestamps_microseconds_utc,
        direct_returns=direct_returns,
    )


def _resolve_fit_artifact_paths(source_path: Path) -> Optional[tuple[Path, Path]]:
    candidate_directories: list[Path] = []
    if source_path.is_dir():
        candidate_directories.append(source_path)
    else:
        candidate_directories.append(source_path.parent)
        if source_path.parent.name == "diagnostics":
            candidate_directories.append(source_path.parent.parent)

    for candidate_directory in candidate_directories:
        model_path = candidate_directory / "model.json"
        fit_report_path = candidate_directory / "fit_report.json"

        if model_path.is_file() and fit_report_path.is_file():
            return model_path, fit_report_path

    if source_path.is_file() and source_path.name == "fit_report.json":
        model_path = source_path.with_name("model.json")
        if model_path.is_file():
            return model_path, source_path

    if source_path.is_file() and source_path.name == "model.json":
        fit_report_path = source_path.with_name("fit_report.json")
        if fit_report_path.is_file():
            return source_path, fit_report_path

    return None


def _load_json_object(path: Path) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object at `{path}`")

    return cast(dict[str, Any], payload)


def _load_source_table(raw_summary: dict[str, Any]) -> WorkbenchSourceTable:
    source_path = _resolve_source_path(cast(str, raw_summary["path"]))
    file_format = cast(str, raw_summary["file_format"])

    return WorkbenchSourceTable(
        source_path=source_path,
        file_format=file_format,
        column_names=_load_source_column_names(source_path=source_path, file_format=file_format),
        source_row_count=_load_source_row_count(
            source_path=source_path,
            file_format=file_format,
            raw_summary=raw_summary,
        ),
    )


def _resolve_source_path(raw_source_path: str) -> Path:
    candidate_paths = _candidate_source_paths(raw_source_path)

    for candidate_path in candidate_paths:
        expanded_candidate_path = candidate_path.expanduser()
        if expanded_candidate_path.exists():
            return expanded_candidate_path.resolve()

    return candidate_paths[0].expanduser().resolve()


def _candidate_source_paths(raw_source_path: str) -> tuple[Path, ...]:
    candidate_paths = [Path(raw_source_path).expanduser()]

    translated_windows_mount_path = _translate_windows_source_path_to_mount(raw_source_path)
    if translated_windows_mount_path is not None:
        candidate_paths.append(translated_windows_mount_path)

    translated_windows_drive_path = _translate_mount_source_path_to_windows(raw_source_path)
    if translated_windows_drive_path is not None:
        candidate_paths.append(translated_windows_drive_path)

    unique_candidate_paths: list[Path] = []
    seen_candidate_texts: set[str] = set()
    for candidate_path in candidate_paths:
        candidate_text = str(candidate_path)
        if candidate_text in seen_candidate_texts:
            continue
        seen_candidate_texts.add(candidate_text)
        unique_candidate_paths.append(candidate_path)

    return tuple(unique_candidate_paths)


def _translate_windows_source_path_to_mount(raw_source_path: str) -> Optional[Path]:
    windows_path = PureWindowsPath(raw_source_path)
    drive_letter = windows_path.drive.removesuffix(":")
    if len(drive_letter) != 1 or windows_path.root == "":
        return None

    return Path("/mnt", drive_letter.lower(), *windows_path.parts[1:])


def _translate_mount_source_path_to_windows(raw_source_path: str) -> Optional[Path]:
    posix_path = PurePosixPath(raw_source_path)
    if (
        len(posix_path.parts) < 4
        or posix_path.parts[0] != "/"
        or posix_path.parts[1] != "mnt"
        or len(posix_path.parts[2]) != 1
        or not posix_path.parts[2].isalpha()
    ):
        return None

    return Path(PureWindowsPath(f"{posix_path.parts[2].upper()}:\\", *posix_path.parts[3:]))


def _load_source_column_names(*, source_path: Path, file_format: str) -> tuple[str, ...]:
    match file_format:
        case "csv":
            with source_path.open(encoding="utf-8", newline="") as input_file:
                header_row = next(csv.reader(input_file), [])

            return tuple(header_row)
        case "parquet":
            return tuple(parquet.ParquetFile(source_path).schema_arrow.names)
        case _:
            raise ValueError(
                f"unsupported workbench source file format `{file_format}` for `{source_path}`"
            )


def _load_source_row_count(
    *,
    source_path: Path,
    file_format: str,
    raw_summary: dict[str, Any],
) -> int:
    summary_row_count = raw_summary.get("source_row_count")
    if isinstance(summary_row_count, int):
        return summary_row_count

    match file_format:
        case "csv":
            reader = pyarrow_csv.open_csv(
                source_path,
                read_options=pyarrow_csv.ReadOptions(block_size=_CSV_BLOCK_SIZE_BYTES),
            )
            return sum(batch.num_rows for batch in reader)
        case "parquet":
            metadata = parquet.ParquetFile(source_path).metadata
            return 0 if metadata is None else metadata.num_rows
        case _:
            raise ValueError(
                f"unsupported workbench source file format `{file_format}` for `{source_path}`"
            )


def _normalize_tick_series(
    *,
    source_table: WorkbenchSourceTable,
    model: JaniformModel,
    preparation_provenance: dict[str, Any],
) -> WorkbenchPriceSeries:
    input_inference = cast(dict[str, Any], model.metadata["input_inference"])
    price_columns = cast(dict[str, Any], input_inference["tick_price_columns"])

    normalization_policy = cast(
        dict[str, Any],
        preparation_provenance["normalization_policy"],
    )

    timestamp_column = cast(str, input_inference["timestamp_column"])
    timezone_name = cast(str, input_inference["timestamp_timezone"])
    tick_price_input_policy = cast(Optional[str], input_inference["tick_price_input_policy"])
    price_column = cast(Optional[str], price_columns["price_column"])
    trade_sign_column = cast(Optional[str], price_columns["trade_sign_column"])
    duplicate_record_policy = cast(Optional[str], normalization_policy.get("duplicate_record_policy"))
    sort_records_by_timestamp = bool(normalization_policy.get("sort_records_by_timestamp", True))

    normalized_timezone = _resolve_timezone(timezone_name)
    required_columns = _required_tick_columns(
        timestamp_column=timestamp_column,
        tick_price_input_policy=tick_price_input_policy,
        price_column=price_column,
        bid_column=cast(Optional[str], price_columns["bid_column"]),
        ask_column=cast(Optional[str], price_columns["ask_column"]),
        trade_sign_column=trade_sign_column,
    )
    available_columns = tuple(
        column_name
        for column_name in required_columns
        if column_name in source_table.column_names
    )
    if timestamp_column not in available_columns:
        return _empty_price_series()

    timestamp_chunks: list[NDArray[np.int64]] = []
    price_chunks: list[NDArray[np.float64]] = []
    source_row_index_chunks: list[NDArray[np.int64]] = []
    source_row_offset = 0

    for batch in _iter_source_batches(
        source_path=source_table.source_path,
        file_format=source_table.file_format,
        included_columns=available_columns,
    ):
        batch_columns = _batch_columns_to_numpy(batch)
        batch_row_count = batch.num_rows

        timestamp_microseconds_utc = _parse_timestamp_column(
            batch_columns[timestamp_column],
            normalized_timezone=normalized_timezone,
        )
        price_values, valid_price_mask = _resolve_price_values(
            batch_columns,
            price_column=price_column,
            bid_column=cast(Optional[str], price_columns["bid_column"]),
            ask_column=cast(Optional[str], price_columns["ask_column"]),
            tick_price_input_policy=tick_price_input_policy,
        )

        valid_row_mask = valid_price_mask
        if not np.any(valid_row_mask):
            source_row_offset += batch_row_count
            continue

        source_row_indexes = source_row_offset + np.arange(batch_row_count, dtype=np.int64)
        timestamp_chunks.append(timestamp_microseconds_utc[valid_row_mask])
        price_chunks.append(price_values[valid_row_mask])
        source_row_index_chunks.append(source_row_indexes[valid_row_mask])
        source_row_offset += batch_row_count

    if len(timestamp_chunks) == 0:
        return _empty_price_series()

    timestamp_microseconds_utc = np.concatenate(timestamp_chunks)
    prices = np.concatenate(price_chunks)
    source_row_indexes = np.concatenate(source_row_index_chunks)

    if sort_records_by_timestamp or duplicate_record_policy in {"keep_first", "keep_last"}:
        sort_order = np.lexsort((source_row_indexes, timestamp_microseconds_utc))
        timestamp_microseconds_utc = timestamp_microseconds_utc[sort_order]
        prices = prices[sort_order]

    if duplicate_record_policy in {"keep_first", "keep_last"}:
        deduplication_mask = np.ones(timestamp_microseconds_utc.size, dtype=bool)
        if duplicate_record_policy == "keep_first":
            deduplication_mask[1:] = (
                timestamp_microseconds_utc[1:] != timestamp_microseconds_utc[:-1]
            )
        else:
            deduplication_mask[:-1] = (
                timestamp_microseconds_utc[:-1] != timestamp_microseconds_utc[1:]
            )

        timestamp_microseconds_utc = timestamp_microseconds_utc[deduplication_mask]
        prices = prices[deduplication_mask]

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=timestamp_microseconds_utc,
        prices=prices,
    )


def _required_tick_columns(
    *,
    timestamp_column: str,
    tick_price_input_policy: Optional[str],
    price_column: Optional[str],
    bid_column: Optional[str],
    ask_column: Optional[str],
    trade_sign_column: Optional[str],
) -> tuple[str, ...]:
    columns: list[str] = [timestamp_column]

    match tick_price_input_policy:
        case "explicit_price_column":
            if price_column is not None:
                columns.append(price_column)
            else:
                columns.append("price")
        case "bid_ask_midpoint":
            if bid_column is not None:
                columns.append(bid_column)
            if ask_column is not None:
                columns.append(ask_column)
        case _:
            columns.extend(
                [
                    value
                    for value in (
                        price_column,
                        "price",
                        "mid_price",
                        "close",
                        "trade_price",
                    )
                    if value is not None
                ]
            )

    if trade_sign_column is not None:
        columns.append(trade_sign_column)

    return tuple(dict.fromkeys(columns))


def _iter_source_batches(
    *,
    source_path: Path,
    file_format: str,
    included_columns: tuple[str, ...],
) -> Iterator[RecordBatch]:
    match file_format:
        case "csv":
            reader = pyarrow_csv.open_csv(
                source_path,
                read_options=pyarrow_csv.ReadOptions(block_size=_CSV_BLOCK_SIZE_BYTES),
                convert_options=pyarrow_csv.ConvertOptions(
                    include_columns=list(included_columns),
                ),
            )
            yield from reader
        case "parquet":
            parquet_file = parquet.ParquetFile(source_path)
            for batch in parquet_file.iter_batches(
                batch_size=_PARQUET_BATCH_SIZE,
                columns=list(included_columns),
            ):
                yield batch
        case _:
            raise ValueError(
                f"unsupported workbench source file format `{file_format}` for `{source_path}`"
            )


def _batch_columns_to_numpy(batch: RecordBatch) -> dict[str, NDArray[Any]]:
    return {
        column_name: cast(
            NDArray[Any],
            batch.column(column_index).to_numpy(zero_copy_only=False),
        )
        for column_index, column_name in enumerate(batch.schema.names)
    }


def _series_from_simulation_result(
    *,
    simulation_result: Any,
    model: JaniformModel,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> WorkbenchPriceSeries:
    frame = simulation_result.to_pandas()
    if len(frame.index) == 0:
        return _empty_price_series()

    from janiform._workflow_outputs import simulation_output_preferences_from_model

    preferences = simulation_output_preferences_from_model(model)

    timestamp_values = _timestamp_values_from_simulation_frame(
        frame=frame,
        preferences=preferences,
    )
    price_values = _price_values_from_simulation_frame(
        frame=frame,
        preferences=preferences,
    )

    if timestamp_values is None or price_values is None:
        return _empty_price_series()

    normalized_prices, valid_price_mask = price_values

    timestamp_microseconds_utc = _parse_timestamp_column(
        timestamp_values,
        normalized_timezone=UTC,
    )

    range_start_microseconds_utc = datetime_to_utc_microseconds(range_start_utc)
    range_end_microseconds_utc = datetime_to_utc_microseconds(range_end_utc)

    in_range_mask = (
        (timestamp_microseconds_utc >= range_start_microseconds_utc)
        & (timestamp_microseconds_utc <= range_end_microseconds_utc)
        & valid_price_mask
    )

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=timestamp_microseconds_utc[in_range_mask],
        prices=normalized_prices[in_range_mask],
        direct_return_timestamps_microseconds_utc=_direct_return_timestamps_from_simulation_result(
            simulation_result
        ),
        direct_returns=_direct_returns_from_simulation_result(simulation_result),
    )


def _series_from_seeded_raw_result(
    *,
    simulation_result: Any,
    reference_price: float,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    interval_start_utc: datetime,
    horizon_interval_count: int,
    interval: timedelta,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> WorkbenchPriceSeries:
    raw_result = getattr(simulation_result, "raw_result", None)
    fine_path = getattr(raw_result, "fine_path", None)
    if fine_path is None:
        return _empty_price_series()

    fine_path_records = list(fine_path)
    if len(fine_path_records) == 0:
        return _empty_price_series()

    interval_microseconds = int(interval.total_seconds() * 1_000_000)
    next_record_index = 0
    latest_observed_log_price = 0.0
    timestamp_microseconds_utc: list[int] = []
    prices: list[float] = []

    for interval_index in range(horizon_interval_count):
        current_interval_start_utc = interval_start_utc + interval * interval_index
        current_interval_end_utc = current_interval_start_utc + interval
        current_interval_end_utc_nanos = _datetime_to_utc_nanos(current_interval_end_utc)

        while next_record_index < len(fine_path_records):
            record = fine_path_records[next_record_index]
            if int(record.timestamp_utc_nanos) > current_interval_end_utc_nanos:
                break

            latest_observed_log_price = float(record.observed_log_price)
            next_record_index += 1

        if not _interval_is_open(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            interval_start_utc=current_interval_start_utc,
        ):
            continue

        interval_end_microseconds_utc = datetime_to_utc_microseconds(current_interval_end_utc)
        if (
            interval_end_microseconds_utc < datetime_to_utc_microseconds(range_start_utc)
            or interval_end_microseconds_utc > datetime_to_utc_microseconds(range_end_utc)
        ):
            continue

        timestamp_microseconds_utc.append(interval_end_microseconds_utc)
        prices.append(reference_price * math.exp(latest_observed_log_price))

    if len(timestamp_microseconds_utc) == 0:
        return _empty_price_series()

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=np.asarray(timestamp_microseconds_utc, dtype=np.int64),
        prices=np.asarray(prices, dtype=np.float64),
    )


def _series_from_coarse_raw_result(
    *,
    simulation_result: Any,
    reference_price: float,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> WorkbenchPriceSeries:
    raw_result = getattr(simulation_result, "raw_result", None)
    records = getattr(raw_result, "records", None)
    if records is None:
        return _empty_price_series()

    range_start_microseconds_utc = datetime_to_utc_microseconds(range_start_utc)
    range_end_microseconds_utc = datetime_to_utc_microseconds(range_end_utc)
    cumulative_log_return = 0.0
    timestamp_microseconds_utc: list[int] = []
    prices: list[float] = []

    for record in records:
        cumulative_log_return += float(record.return_components.total_return)
        interval_start_utc = utc_microseconds_to_datetime(int(record.interval_start_utc_nanos // 1_000))
        if not _interval_is_open(
            calendar_registry=calendar_registry,
            exchange_code=exchange_code,
            interval_start_utc=interval_start_utc,
        ):
            continue

        interval_end_microseconds_utc = int(record.interval_end_utc_nanos // 1_000)
        if (
            interval_end_microseconds_utc < range_start_microseconds_utc
            or interval_end_microseconds_utc > range_end_microseconds_utc
        ):
            continue

        timestamp_microseconds_utc.append(interval_end_microseconds_utc)
        prices.append(reference_price * math.exp(cumulative_log_return))

    if len(timestamp_microseconds_utc) == 0:
        return _empty_price_series()

    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=np.asarray(timestamp_microseconds_utc, dtype=np.int64),
        prices=np.asarray(prices, dtype=np.float64),
    )


def _timestamp_values_from_simulation_frame(
    *,
    frame: Any,
    preferences: Any,
) -> Optional[NDArray[Any]]:
    timestamp_column = preferences.timestamp_column
    if timestamp_column in frame.columns:
        return cast(
            NDArray[Any],
            frame[timestamp_column].to_numpy(dtype=object, copy=False),
        )

    return None


def _price_values_from_simulation_frame(
    *,
    frame: Any,
    preferences: Any,
) -> Optional[tuple[NDArray[np.float64], NDArray[np.bool_]]]:
    from janiform._workflow_outputs import simulation_output_price_column

    price_column = simulation_output_price_column(preferences)
    if price_column not in frame.columns:
        return None

    return _parse_numeric_column(
        cast(
            NDArray[Any],
            frame[price_column].to_numpy(dtype=object, copy=False),
        )
    )


def _resolve_price_values(
    columns: dict[str, NDArray[Any]],
    *,
    price_column: Optional[str],
    bid_column: Optional[str],
    ask_column: Optional[str],
    tick_price_input_policy: Optional[str],
) -> tuple[NDArray[np.float64], NDArray[np.bool_]]:
    row_count = len(next(iter(columns.values()))) if columns else 0

    match tick_price_input_policy:
        case "explicit_price_column":
            effective_price_column = price_column or "price"
            if effective_price_column not in columns:
                return _empty_numeric_resolution(row_count)
            return _parse_numeric_column(columns[effective_price_column])
        case "bid_ask_midpoint":
            if bid_column is None or ask_column is None:
                return _empty_numeric_resolution(row_count)
            if bid_column not in columns or ask_column not in columns:
                return _empty_numeric_resolution(row_count)

            bid_values, valid_bid_mask = _parse_numeric_column(columns[bid_column])
            ask_values, valid_ask_mask = _parse_numeric_column(columns[ask_column])

            valid_mask = valid_bid_mask & valid_ask_mask

            midpoint_values = np.full(row_count, np.nan, dtype=np.float64)
            midpoint_values[valid_mask] = (bid_values[valid_mask] + ask_values[valid_mask]) / 2.0

            return midpoint_values, valid_mask
        case _:
            resolved_values = np.full(row_count, np.nan, dtype=np.float64)
            resolved_mask = np.zeros(row_count, dtype=bool)

            for candidate_key in _candidate_price_resolution_columns(price_column):
                if candidate_key not in columns:
                    continue

                candidate_values, candidate_mask = _parse_numeric_column(columns[candidate_key])
                fill_mask = candidate_mask & ~resolved_mask

                resolved_values[fill_mask] = candidate_values[fill_mask]
                resolved_mask[fill_mask] = True

            return resolved_values, resolved_mask


def _candidate_price_resolution_columns(price_column: Optional[str]) -> tuple[str, ...]:
    return tuple(
        dict.fromkeys(
            value
            for value in (price_column, "price", "mid_price", "close", "trade_price")
            if value is not None
        )
    )


def _simulation_reference_price(model: JaniformModel) -> float:
    raw_preferences = cast(dict[str, Any], model.metadata.get("simulation_output_preferences", {}))
    raw_reference_price = raw_preferences.get("reference_price")
    if isinstance(raw_reference_price, int | float) and not isinstance(raw_reference_price, bool):
        return float(raw_reference_price)

    return 1.0


def _interval_is_open(
    *,
    calendar_registry: CalendarRegistry,
    exchange_code: str,
    interval_start_utc: datetime,
) -> bool:
    return calendar_registry.resolve_market_state(exchange_code, interval_start_utc) == MarketState.OPEN


def _datetime_to_utc_nanos(value: datetime) -> int:
    normalized_value = value.astimezone(UTC)
    return int(normalized_value.timestamp() * 1_000_000_000)


def _observed_utc_range(
    observed_tick_series: WorkbenchPriceSeries,
) -> tuple[datetime, datetime]:
    return (
        utc_microseconds_to_datetime(int(observed_tick_series.timestamp_microseconds_utc[0])),
        utc_microseconds_to_datetime(int(observed_tick_series.timestamp_microseconds_utc[-1])),
    )


def _resolve_timezone(timezone_name: str) -> tzinfo:
    normalized_timezone_name = timezone_name.strip()
    if normalized_timezone_name.upper() in {"UTC", "Z", "GMT", "UT"}:
        return UTC

    return ZoneInfo(normalized_timezone_name)


def _path_cache_identity(path: Path) -> tuple[str, int, int]:
    path_stat = path.stat()
    return str(path), path_stat.st_mtime_ns, path_stat.st_size


def _empty_price_series() -> WorkbenchPriceSeries:
    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=_EMPTY_INT64_ARRAY.copy(),
        prices=_EMPTY_FLOAT64_ARRAY.copy(),
    )


def _direct_return_timestamps_from_simulation_result(
    simulation_result: Any,
) -> Optional[NDArray[np.int64]]:
    fine_path_records = _trade_fine_path_records(simulation_result)
    if fine_path_records is None:
        return None

    return np.asarray(
        [int(record.timestamp_utc_nanos // 1_000) for record in fine_path_records],
        dtype=np.int64,
    )


def _direct_returns_from_simulation_result(
    simulation_result: Any,
) -> Optional[NDArray[np.float64]]:
    fine_path_records = _trade_fine_path_records(simulation_result)
    if fine_path_records is None:
        return None

    return np.asarray(
        [
            float(record.return_components.total_return)
            for record in fine_path_records
        ],
        dtype=np.float64,
    )


def _trade_fine_path_records(simulation_result: Any) -> Optional[list[Any]]:
    raw_result = getattr(simulation_result, "raw_result", None)
    fine_path = getattr(raw_result, "fine_path", None)
    if fine_path is None:
        return None

    trade_records = [
        record
        for record in fine_path
        if _is_trade_transition(getattr(record, "transition_kind", None))
    ]
    if len(trade_records) == 0:
        return None

    return trade_records


def _is_trade_transition(transition_kind: Any) -> bool:
    if transition_kind is None:
        return False

    normalized_label = str(getattr(transition_kind, "name", transition_kind)).strip().lower()
    return normalized_label in {"buy_trade", "sell_trade"}


def _empty_numeric_resolution(row_count: int) -> tuple[NDArray[np.float64], NDArray[np.bool_]]:
    return (
        np.full(row_count, np.nan, dtype=np.float64),
        np.zeros(row_count, dtype=bool),
    )


def _parse_timestamp_column(
    raw_values: NDArray[Any],
    *,
    normalized_timezone: tzinfo,
) -> NDArray[np.int64]:
    timestamp_microseconds_utc = np.empty(len(raw_values), dtype=np.int64)
    for row_index, raw_value in enumerate(raw_values):
        if isinstance(raw_value, np.generic):
            raw_value = raw_value.item()

        parsed_timestamp = parse_timestamp_value(raw_value)
        if parsed_timestamp.tzinfo is None:
            parsed_timestamp = parsed_timestamp.replace(tzinfo=normalized_timezone)

        timestamp_microseconds_utc[row_index] = datetime_to_utc_microseconds(
            parsed_timestamp.astimezone(UTC)
        )

    return timestamp_microseconds_utc


def _parse_numeric_column(
    raw_values: NDArray[Any],
) -> tuple[NDArray[np.float64], NDArray[np.bool_]]:
    numeric_values = np.full(len(raw_values), np.nan, dtype=np.float64)
    valid_mask = np.zeros(len(raw_values), dtype=bool)
    for row_index, raw_value in enumerate(raw_values):
        parsed_value = _parse_numeric_value(raw_value)
        if parsed_value is None:
            continue

        numeric_values[row_index] = parsed_value
        valid_mask[row_index] = True

    return numeric_values, valid_mask


def _parse_numeric_value(raw_value: Any) -> Optional[float]:
    if isinstance(raw_value, (int, float, np.integer, np.floating)):
        return float(raw_value)
    if isinstance(raw_value, str) and raw_value != "":
        return float(raw_value)

    return None
