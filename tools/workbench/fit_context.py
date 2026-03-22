import csv

from dataclasses import dataclass
from datetime import UTC, datetime, timedelta, tzinfo
from pathlib import Path
from typing import Any, Optional, cast
from zoneinfo import ZoneInfo

from pyarrow import parquet

from janiform._timestamps import parse_timestamp_value
from janiform.calibration.fit import JaniformModel

from .artifacts import WorkbenchArtifacts


_DEFAULT_COMPARISON_SIMULATION_SEED = 1


@dataclass(frozen=True, slots=True)
class WorkbenchSourceTable:
    source_path: Path
    file_format: str
    column_names: tuple[str, ...]
    rows: tuple[dict[str, Any], ...]


@dataclass(frozen=True, slots=True)
class WorkbenchFitContext:
    model: JaniformModel
    fit_report_path: Path
    tick_data_source: WorkbenchSourceTable
    observed_tick_rows: tuple[dict[str, Any], ...]
    observed_range_start_utc: datetime
    observed_range_end_utc: datetime
    option_surface_source: Optional[WorkbenchSourceTable] = None
    known_news_source: Optional[WorkbenchSourceTable] = None


@dataclass(frozen=True, slots=True)
class WorkbenchObservedSimulationComparison:
    fit_context: WorkbenchFitContext
    simulation_seed: int
    observed_rows: tuple[dict[str, Any], ...]
    simulated_rows: tuple[dict[str, Any], ...]


def load_fit_context(artifacts: WorkbenchArtifacts) -> Optional[WorkbenchFitContext]:
    resolved_paths = _resolve_fit_artifact_paths(artifacts.source_path)
    if resolved_paths is None:
        return None

    model_path, fit_report_path = resolved_paths
    if not model_path.is_file() or not fit_report_path.is_file():
        return None

    report_payload = _load_json_object(fit_report_path)
    preparation_provenance = report_payload.get("preparation_provenance")
    if not isinstance(preparation_provenance, dict):
        return None

    raw_tick_summary = preparation_provenance.get("tick_data_summary")
    if not isinstance(raw_tick_summary, dict):
        return None

    model = JaniformModel.load(model_path)
    tick_data_source = _load_source_table(raw_tick_summary)
    observed_tick_rows = _normalize_tick_rows(
        raw_rows=tick_data_source.rows,
        model=model,
        preparation_provenance=preparation_provenance,
    )
    if len(observed_tick_rows) == 0:
        return None

    observed_range_start_utc, observed_range_end_utc = _observed_utc_range(
        observed_tick_rows,
        timestamp_column=cast(str, model.metadata["input_inference"]["timestamp_column"]),
    )

    raw_option_surface_summary = preparation_provenance.get("option_surface_summary")
    raw_known_news_summary = preparation_provenance.get("known_news_summary")

    return WorkbenchFitContext(
        model=model,
        fit_report_path=fit_report_path,
        tick_data_source=tick_data_source,
        observed_tick_rows=observed_tick_rows,
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
) -> Optional[WorkbenchObservedSimulationComparison]:
    fit_context = load_fit_context(artifacts)
    if fit_context is None:
        return None

    simulation_start_utc, horizon_interval_count = _simulation_request_window_for_observed_range(
        observed_range_start_utc=fit_context.observed_range_start_utc,
        observed_range_end_utc=fit_context.observed_range_end_utc,
    )
    simulation_result = fit_context.model.simulate(
        seed=seed,
        start_time_utc=simulation_start_utc,
        horizon_interval_count=horizon_interval_count,
        silent=True,
    )
    simulated_rows = _filter_rows_to_timestamp_window(
        rows=tuple(
            cast(list[dict[str, Any]], simulation_result.to_pandas().to_dict(orient="records"))
        ),
        range_start_utc=fit_context.observed_range_start_utc,
        range_end_utc=fit_context.observed_range_end_utc,
    )
    canonical_observed_rows = _canonicalize_comparison_rows(
        rows=fit_context.observed_tick_rows,
        model=fit_context.model,
    )
    canonical_simulated_rows = _canonicalize_comparison_rows(
        rows=simulated_rows,
        model=fit_context.model,
    )

    return WorkbenchObservedSimulationComparison(
        fit_context=fit_context,
        simulation_seed=seed,
        observed_rows=canonical_observed_rows,
        simulated_rows=canonical_simulated_rows,
    )


def _simulation_request_window_for_observed_range(
    *,
    observed_range_start_utc: datetime,
    observed_range_end_utc: datetime,
) -> tuple[datetime, int]:
    simulation_start_utc = _floor_to_hour_utc(observed_range_start_utc)
    simulation_end_utc = _end_of_hour_covering_timestamp_utc(observed_range_end_utc)
    horizon_interval_count = max(
        1,
        int((simulation_end_utc - simulation_start_utc) / timedelta(hours=1)),
    )
    return simulation_start_utc, horizon_interval_count


def _filter_rows_to_timestamp_window(
    *,
    rows: tuple[dict[str, Any], ...],
    range_start_utc: datetime,
    range_end_utc: datetime,
) -> tuple[dict[str, Any], ...]:
    if len(rows) == 0:
        return ()

    timestamp_column = _find_timestamp_column(rows[0])
    if timestamp_column is None:
        return rows

    filtered_rows: list[dict[str, Any]] = []
    for row in rows:
        timestamp_value = row.get(timestamp_column)
        if timestamp_value is None:
            continue

        normalized_timestamp = parse_timestamp_value(timestamp_value).astimezone(UTC)
        if range_start_utc <= normalized_timestamp <= range_end_utc:
            filtered_rows.append(row)

    return tuple(filtered_rows)


def _canonicalize_comparison_rows(
    *,
    rows: tuple[dict[str, Any], ...],
    model: JaniformModel,
) -> tuple[dict[str, Any], ...]:
    if len(rows) == 0:
        return ()

    input_inference = cast(dict[str, Any], model.metadata["input_inference"])
    price_columns = cast(dict[str, Any], input_inference["tick_price_columns"])
    timestamp_column = cast(str, input_inference["timestamp_column"])
    tick_price_input_policy = cast(Optional[str], input_inference["tick_price_input_policy"])

    canonical_rows: list[dict[str, Any]] = []
    for row in rows:
        canonical_row = dict(row)
        timestamp_value = _resolve_timestamp_value(row, timestamp_column=timestamp_column)
        if timestamp_value is not None:
            canonical_row["timestamp_utc"] = parse_timestamp_value(timestamp_value).astimezone(UTC)

        price_value = _resolve_price_value(
            row,
            price_column=cast(Optional[str], price_columns["price_column"]),
            bid_column=cast(Optional[str], price_columns["bid_column"]),
            ask_column=cast(Optional[str], price_columns["ask_column"]),
            tick_price_input_policy=tick_price_input_policy,
        )
        if price_value is not None:
            canonical_row["price"] = price_value

        canonical_rows.append(canonical_row)

    return tuple(canonical_rows)


def _resolve_timestamp_value(
    row: dict[str, Any],
    *,
    timestamp_column: str,
) -> Any:
    if "timestamp_utc" in row:
        return row["timestamp_utc"]
    if timestamp_column in row:
        return row[timestamp_column]
    if "time" in row:
        return row["time"]
    if "time_microseconds" in row:
        return row["time_microseconds"]

    return None


def _resolve_price_value(
    row: dict[str, Any],
    *,
    price_column: Optional[str],
    bid_column: Optional[str],
    ask_column: Optional[str],
    tick_price_input_policy: Optional[str],
) -> Optional[float]:
    match tick_price_input_policy:
        case "explicit_price_column":
            effective_price_column = price_column or "price"
            raw_value = row.get(effective_price_column)
            if isinstance(raw_value, int | float):
                return float(raw_value)
        case "bid_ask_midpoint":
            if bid_column is None or ask_column is None:
                return None
            bid_value = row.get(bid_column)
            ask_value = row.get(ask_column)
            if isinstance(bid_value, int | float) and isinstance(ask_value, int | float):
                return (float(bid_value) + float(ask_value)) / 2.0
        case _:
            if isinstance(row.get("price"), int | float):
                return float(cast(float, row["price"]))
            if isinstance(price_column, str) and isinstance(row.get(price_column), int | float):
                return float(cast(float, row[price_column]))

    for candidate_key in ("price", "mid_price", "close", "trade_price"):
        candidate_value = row.get(candidate_key)
        if isinstance(candidate_value, int | float):
            return float(candidate_value)

    return None


def _find_timestamp_column(row: dict[str, Any]) -> Optional[str]:
    return next(
        (
            key
            for key in row
            if "timestamp" in key
        ),
        None,
    )


def _floor_to_hour_utc(timestamp_utc: datetime) -> datetime:
    normalized_timestamp_utc = timestamp_utc.astimezone(UTC)
    return normalized_timestamp_utc.replace(minute=0, second=0, microsecond=0)


def _end_of_hour_covering_timestamp_utc(timestamp_utc: datetime) -> datetime:
    return _floor_to_hour_utc(timestamp_utc) + timedelta(hours=1)


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
    import json

    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"expected JSON object at `{path}`")

    return cast(dict[str, Any], payload)


def _load_source_table(raw_summary: dict[str, Any]) -> WorkbenchSourceTable:
    source_path = Path(cast(str, raw_summary["path"])).expanduser().resolve()
    file_format = cast(str, raw_summary["file_format"])
    rows = _load_source_rows(source_path=source_path, file_format=file_format)
    column_names = tuple(rows[0].keys()) if len(rows) > 0 else ()

    return WorkbenchSourceTable(
        source_path=source_path,
        file_format=file_format,
        column_names=column_names,
        rows=tuple(rows),
    )


def _load_source_rows(*, source_path: Path, file_format: str) -> list[dict[str, Any]]:
    match file_format:
        case "csv":
            with source_path.open(encoding="utf-8", newline="") as input_file:
                return [dict(row) for row in csv.DictReader(input_file)]
        case "parquet":
            return cast(list[dict[str, Any]], parquet.read_table(source_path).to_pylist())
        case _:
            raise ValueError(
                f"unsupported workbench source file format `{file_format}` for `{source_path}`"
            )


def _normalize_tick_rows(
    *,
    raw_rows: tuple[dict[str, Any], ...],
    model: JaniformModel,
    preparation_provenance: dict[str, Any],
) -> tuple[dict[str, Any], ...]:
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

    normalized_entries: list[tuple[datetime, int, dict[str, Any]]] = []
    for row_index, raw_row in enumerate(raw_rows):
        if timestamp_column not in raw_row:
            continue

        parsed_timestamp = parse_timestamp_value(raw_row[timestamp_column])
        if parsed_timestamp.tzinfo is None:
            parsed_timestamp = parsed_timestamp.replace(tzinfo=normalized_timezone)

        timestamp_utc = parsed_timestamp.astimezone(UTC)
        output_row: dict[str, Any] = {
            "timestamp_utc": timestamp_utc,
            timestamp_column: parsed_timestamp.astimezone(normalized_timezone),
        }

        match tick_price_input_policy:
            case "explicit_price_column":
                effective_price_column = price_column or "price"
                price_value = raw_row.get(effective_price_column)
                if price_value in (None, ""):
                    continue
                output_row[effective_price_column] = float(price_value)
                if trade_sign_column is not None:
                    output_row[trade_sign_column] = raw_row.get(trade_sign_column)
            case "bid_ask_midpoint":
                bid_column = cast(Optional[str], price_columns["bid_column"])
                ask_column = cast(Optional[str], price_columns["ask_column"])
                if bid_column is None or ask_column is None:
                    continue
                bid_price = raw_row.get(bid_column)
                ask_price = raw_row.get(ask_column)
                if bid_price in (None, "") or ask_price in (None, ""):
                    continue
                output_row["mid_price"] = (float(bid_price) + float(ask_price)) / 2.0
            case _:
                effective_price_column = price_column or "price"
                price_value = raw_row.get(effective_price_column)
                if price_value in (None, ""):
                    continue
                output_row[effective_price_column] = float(price_value)

        normalized_entries.append((timestamp_utc, row_index, output_row))

    if sort_records_by_timestamp:
        normalized_entries.sort(key=lambda entry: (entry[0], entry[1]))

    if duplicate_record_policy in {"keep_first", "keep_last"}:
        deduplicated_entries: dict[datetime, tuple[datetime, int, dict[str, Any]]] = {}
        for entry in normalized_entries:
            if duplicate_record_policy == "keep_first":
                deduplicated_entries.setdefault(entry[0], entry)
            else:
                deduplicated_entries[entry[0]] = entry
        normalized_entries = list(deduplicated_entries.values())

    return tuple(entry[2] for entry in normalized_entries)


def _observed_utc_range(
    observed_tick_rows: tuple[dict[str, Any], ...],
    *,
    timestamp_column: str,
) -> tuple[datetime, datetime]:
    observed_timestamps_utc = [
        cast(datetime, row[timestamp_column]).astimezone(UTC)
        for row in observed_tick_rows
    ]
    return min(observed_timestamps_utc), max(observed_timestamps_utc)


def _resolve_timezone(timezone_name: str) -> tzinfo:
    normalized_timezone_name = timezone_name.strip()
    if normalized_timezone_name.upper() in {"UTC", "Z", "GMT", "UT"}:
        return UTC

    return ZoneInfo(normalized_timezone_name)
