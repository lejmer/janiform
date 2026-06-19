from __future__ import annotations

import json
from dataclasses import dataclass, field
from enum import StrEnum
from pathlib import Path
from typing import Any, Final, Optional

from .._native_api import native_module

DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT: Final[int] = 513


class ForecastMeasure(StrEnum):
    PHYSICAL = "physical"
    RISK_NEUTRAL = "risk_neutral"


class ForecastTableFormat(StrEnum):
    PARQUET = "parquet"
    CSV = "csv"
    JSON = "json"


@dataclass(frozen=True, slots=True)
class ForecastTable:
    _rows: Optional[list[dict[str, Any]]] = None
    _path: Optional[Path] = None

    def to_pandas(self) -> Any:
        import pandas

        if self._rows is not None:
            return pandas.DataFrame.from_records(self._rows)

        table_path = self._required_path()
        match table_path.suffix.lower():
            case ".parquet":
                return pandas.read_parquet(table_path)
            case ".csv":
                return pandas.read_csv(table_path)
            case ".json":
                return pandas.read_json(table_path)
            case _:
                raise ValueError(f"unsupported forecast table format `{table_path.suffix}`")

    def to_pyarrow(self) -> Any:
        import pyarrow
        import pyarrow.csv
        import pyarrow.parquet

        if self._rows is not None:
            return pyarrow.Table.from_pylist(self._rows)

        table_path = self._required_path()
        match table_path.suffix.lower():
            case ".parquet":
                return pyarrow.parquet.read_table(table_path)
            case ".csv":
                return pyarrow.csv.read_csv(table_path)
            case ".json":
                return pyarrow.Table.from_pandas(self.to_pandas())
            case _:
                raise ValueError(f"unsupported forecast table format `{table_path.suffix}`")

    def column(self, name: str) -> Any:
        if self._rows is not None:
            return [row[name] for row in self._rows]

        return self.to_pyarrow().column(name).to_pylist()

    def _required_path(self) -> Path:
        if self._path is None:
            raise RuntimeError("forecast table has neither native rows nor a bundle table path")

        return self._path


@dataclass(frozen=True, slots=True)
class ForecastTerminalDistributionTable(ForecastTable):
    quantiles: ForecastTable = field(default_factory=ForecastTable)


@dataclass(frozen=True, slots=True)
class JaniformForecast:
    manifest: dict[str, Any]
    expected_path: ForecastTable
    terminal_distribution: ForecastTerminalDistributionTable
    bundle_path: Optional[Path] = None

    @staticmethod
    def from_native_payload(payload: dict[str, Any]) -> JaniformForecast:
        manifest = _required_mapping(payload, "manifest")
        expected_path_rows = _required_rows(payload, "expected_path")
        terminal_distribution_rows = _required_rows(payload, "terminal_distribution")
        bundle_path_value = payload.get("bundle_path")

        quantile_rows = manifest.get("quantiles")
        if not isinstance(quantile_rows, list):
            raise ValueError("forecast manifest quantiles must be a list")

        return JaniformForecast(
            manifest=manifest,
            expected_path=ForecastTable(_rows=expected_path_rows),
            terminal_distribution=ForecastTerminalDistributionTable(
                _rows=terminal_distribution_rows,
                quantiles=ForecastTable(
                    _rows=_required_row_list(quantile_rows, "manifest.quantiles")
                ),
            ),
            bundle_path=None if bundle_path_value is None else Path(str(bundle_path_value)),
        )

    @staticmethod
    def load(path: str | Path) -> JaniformForecast:
        bundle_path = Path(path)

        raw_manifest = native_module().load_forecast_bundle_manifest_py(str(bundle_path))
        if not isinstance(raw_manifest, dict):
            raise ValueError("native forecast manifest load returned an invalid payload")

        expected_table = _table_path(bundle_path, raw_manifest, "expected_path_table")
        distribution_table = _table_path(bundle_path, raw_manifest, "terminal_distribution_table")

        quantile_rows = raw_manifest.get("quantiles")
        if not isinstance(quantile_rows, list):
            raise ValueError("forecast manifest quantiles must be a list")

        return JaniformForecast(
            manifest=raw_manifest,
            expected_path=ForecastTable(_path=expected_table),
            terminal_distribution=ForecastTerminalDistributionTable(
                _path=distribution_table,
                quantiles=ForecastTable(
                    _rows=_required_row_list(quantile_rows, "manifest.quantiles")
                ),
            ),
            bundle_path=bundle_path,
        )

    def save(
        self,
        path: str | Path,
        table_format: ForecastTableFormat | str = ForecastTableFormat.PARQUET,
    ) -> Path:
        table_format_value = ForecastTableFormat(table_format).value

        payload = {
            "manifest": self.manifest,
            "expected_path": self._table_rows(self.expected_path),
            "terminal_distribution": self._table_rows(self.terminal_distribution),
        }

        raw_output = native_module().save_forecast_bundle_from_payload(
            payload,
            str(Path(path)),
            table_format_value,
        )
        if not isinstance(raw_output, dict) or not isinstance(raw_output.get("path"), str):
            raise ValueError("native forecast save returned an invalid payload")

        return Path(raw_output["path"])

    def _table_rows(self, table: ForecastTable) -> list[dict[str, Any]]:
        if table._rows is not None:
            return table._rows

        raw_rows = json.loads(table.to_pandas().to_json(orient="records"))
        if not isinstance(raw_rows, list):
            raise ValueError("forecast table serialization did not produce row records")

        return _required_row_list(raw_rows, "forecast_table")


def _required_mapping(payload: dict[str, Any], field_name: str) -> dict[str, Any]:
    value = payload.get(field_name)
    if not isinstance(value, dict):
        raise ValueError(f"forecast payload `{field_name}` must be a mapping")

    return value


def _required_rows(payload: dict[str, Any], field_name: str) -> list[dict[str, Any]]:
    value = payload.get(field_name)
    if not isinstance(value, list):
        raise ValueError(f"forecast payload `{field_name}` must be a row list")

    return _required_row_list(value, field_name)


def _required_row_list(rows: list[Any], field_name: str) -> list[dict[str, Any]]:
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError(f"forecast payload `{field_name}` must contain mapping rows")

    return rows


def _table_path(bundle_path: Path, manifest: dict[str, Any], field_name: str) -> Path:
    table_manifest = manifest.get(field_name)
    if not isinstance(table_manifest, dict):
        raise ValueError(f"forecast manifest `{field_name}` must be a mapping")

    file_name = table_manifest.get("file_name")
    if not isinstance(file_name, str):
        raise ValueError(f"forecast manifest `{field_name}.file_name` must be a string")

    return bundle_path / file_name


__all__ = [
    "ForecastMeasure",
    "ForecastTable",
    "ForecastTableFormat",
    "ForecastTerminalDistributionTable",
    "JaniformForecast",
]
