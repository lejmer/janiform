import math
from datetime import UTC, datetime
from pathlib import Path
from types import SimpleNamespace
from typing import Any, cast

import numpy as np
import pandas
import pyarrow as pa
import pytest
from matplotlib.figure import Figure
from pyarrow import parquet

from janiform._timestamps import datetime_to_utc_microseconds
from janiform.contracts import MeanReversionAnchorModel, MeanReversionFittedParameters
from tools.workbench import plots as workbench_plots
from tools.workbench import load_workbench_artifacts
from tools.workbench.artifacts import WorkbenchArtifacts
from tools.workbench.fit_context import (
    WorkbenchFitContext,
    WorkbenchObservedSimulationComparison,
    WorkbenchPriceSeries,
    WorkbenchSourceTable,
    _candidate_source_paths,
    build_observed_simulation_comparison,
    build_observed_simulation_comparison_for_fit_context,
    load_fit_context,
)
from tools.workbench.plots import create_message_figure, save_all_figures


def test_workbench_comparison_reloads_training_ticks_and_simulates_training_window(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T15:45:00Z,101.0,sell",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T14:30:00Z,102.0,sell",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-run",
        duplicate_record_policy="keep_last",
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    captured_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
                        "trade_price": 99.0,
                        "trade_sign": "buy",
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 99.5,
                        "trade_sign": "buy",
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.5,
                        "trade_sign": "sell",
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
                        "trade_price": 101.5,
                        "trade_sign": "buy",
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            captured_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )

            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    comparison = build_observed_simulation_comparison(artifacts, seed=7)

    assert comparison is not None
    _assert_price_series(
        comparison.observed_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 45, tzinfo=UTC),
        ),
        prices=(102.0, 101.0),
    )
    _assert_price_series(
        comparison.simulated_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        ),
        prices=(99.5, 100.5),
    )
    assert captured_simulations == [
        {
            "seed": 7,
            "start_time_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            "horizon_interval_count": 2,
            "interval": None,
            "silent": True,
        },
        {
            "seed": 7,
            "start_time_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            "horizon_interval_count": 6,
            "interval": "15m",
            "silent": True,
        },
    ]


def test_workbench_comparison_reuses_persisted_cache_for_same_model_seed_and_range(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:30:00Z,101.0",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-cache-hit",
        duplicate_record_policy=None,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    captured_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 99.5,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.5,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            captured_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )
            return _FakeSimulationResult()

    fake_model = _FakeModel()
    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: fake_model,
    )

    artifacts = load_workbench_artifacts(tmp_path)
    first = build_observed_simulation_comparison(artifacts, seed=5)
    second = build_observed_simulation_comparison(artifacts, seed=5)

    assert first is not None
    assert second is not None
    assert len(captured_simulations) == 2
    assert (tmp_path / ".workbench_cache").is_dir()
    _assert_price_series(
        second.simulated_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        ),
        prices=(99.5, 100.5),
    )


def test_workbench_comparison_cache_does_not_apply_to_edited_models(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:30:00Z,101.0",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-cache-miss-for-edited-model",
        duplicate_record_policy=None,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    persisted_model_simulations: list[dict[str, Any]] = []
    edited_model_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 99.5,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.5,
                    },
                ]
            )

    class _PersistedModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            persisted_model_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )
            return _FakeSimulationResult()

    class _EditedModel(_PersistedModel):
        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            edited_model_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )
            return _FakeSimulationResult()

    persisted_model = _PersistedModel()
    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: persisted_model,
    )

    artifacts = load_workbench_artifacts(tmp_path)
    fit_context = load_fit_context(artifacts)

    assert fit_context is not None

    first = build_observed_simulation_comparison(artifacts, seed=3)
    second = build_observed_simulation_comparison_for_fit_context(
        fit_context,
        model=cast(Any, _EditedModel()),
        seed=3,
    )

    assert first is not None
    assert second is not None
    assert len(persisted_model_simulations) == 2
    assert len(edited_model_simulations) == 2


def test_workbench_comparison_uses_canonical_bid_ask_simulation_output_columns(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "datetime,bid_price,ask_price",
                "2026-02-02T14:30:00Z,100.0,100.2",
                "2026-02-02T15:30:00Z,101.0,101.2",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-runtime-columns",
        duplicate_record_policy=None,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "datetime": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "mid_price": 99.5,
                    },
                    {
                        "datetime": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "mid_price": 100.5,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "datetime",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "bid_ask_midpoint",
                "tick_price_columns": {
                    "price_column": None,
                    "trade_sign_column": None,
                    "bid_column": "bid_price",
                    "ask_column": "ask_price",
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del seed, start_time_utc, horizon_interval_count, interval, progress_callback, silent
            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    comparison = build_observed_simulation_comparison(artifacts, seed=5)

    assert comparison is not None
    _assert_price_series(
        comparison.simulated_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        ),
        prices=(99.5, 100.5),
    )


def test_workbench_comparison_replays_exact_hour_endpoints_over_covering_hours(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:00:00Z,100.0,buy",
                "2026-02-02T16:00:00Z,101.0,sell",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-hour-boundary",
        duplicate_record_policy=None,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    captured_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
                        "trade_price": 100.1,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                        "trade_price": 100.5,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
                        "trade_price": 101.1,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            captured_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )

            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    comparison = build_observed_simulation_comparison(artifacts, seed=3)

    assert comparison is not None
    _assert_price_series(
        comparison.simulated_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
        ),
        prices=(100.1, 100.5, 101.1),
    )

    assert captured_simulations == [
        {
            "seed": 3,
            "start_time_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            "horizon_interval_count": 3,
            "interval": None,
            "silent": True,
        },
        {
            "seed": 3,
            "start_time_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            "horizon_interval_count": 9,
            "interval": "15m",
            "silent": True,
        },
    ]


def test_horizon_event_count_distribution_uses_raw_simulated_tick_counts() -> None:
    observed_series = _price_series_from_records(
        (
            ("2026-02-02T14:00:00Z", 100.0),
            ("2026-02-02T14:10:00Z", 100.1),
            ("2026-02-02T14:20:00Z", 100.2),
            ("2026-02-02T14:30:00Z", 100.3),
            ("2026-02-02T14:40:00Z", 100.4),
            ("2026-02-02T14:50:00Z", 100.5),
        )
    )
    simulated_tick_series = _price_series_from_records(
        (
            ("2026-02-02T14:00:00Z", 100.0),
            ("2026-02-02T14:01:00Z", 100.1),
            ("2026-02-02T14:02:00Z", 100.2),
            ("2026-02-02T14:15:00Z", 100.3),
            ("2026-02-02T14:16:00Z", 100.4),
            ("2026-02-02T14:30:00Z", 100.5),
            ("2026-02-02T14:31:00Z", 100.6),
            ("2026-02-02T14:32:00Z", 100.7),
            ("2026-02-02T14:45:00Z", 100.8),
        )
    )
    simulated_seeded_15m_series = _price_series_from_records(
        (
            ("2026-02-02T14:15:00Z", 100.2),
            ("2026-02-02T14:30:00Z", 100.4),
            ("2026-02-02T14:45:00Z", 100.7),
            ("2026-02-02T15:00:00Z", 100.8),
        )
    )

    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(),
        simulation_seed=7,
        observed_series=observed_series,
        observed_comparison_series=observed_series,
        simulated_series=simulated_tick_series,
        simulated_seeded_series_15m=simulated_seeded_15m_series,
        simulated_coarse_series_15m=simulated_seeded_15m_series,
    )
    analysis = workbench_plots._analysis_for_observed_simulation_comparison(comparison)
    axis = _FakeAxis()

    workbench_plots._render_event_count_distribution(
        axis,
        analysis,
        horizon_minutes=(15,),
    )

    assert axis.boxplot_data == [[2, 1, 2, 1], [3, 2, 3, 1]]
    assert axis.xtick_labels == ["Observed 15m", "Simulated 15m"]


def test_workbench_comparison_uses_full_observed_range_without_anchor_clipping() -> None:
    captured_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
                        "trade_price": 100.0,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 17, 0, tzinfo=UTC),
                        "trade_price": 100.5,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 18, 0, tzinfo=UTC),
                        "trade_price": 101.0,
                    },
                ]
            )

    mean_reversion_payload = MeanReversionFittedParameters(
        kappa=0.25,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        parameters = {
            "mean_reversion": {
                "fitted_parameters": mean_reversion_payload,
            }
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            captured_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )
            return _FakeSimulationResult()

    observed_series = _price_series_from_records(
        (
            ("2026-02-02T15:00:00Z", 99.0),
            ("2026-02-02T16:00:00Z", 100.0),
            ("2026-02-02T17:00:00Z", 101.0),
            ("2026-02-02T18:00:00Z", 102.0),
            ("2026-02-02T19:00:00Z", 103.0),
        )
    )
    fit_context = WorkbenchFitContext(
        model=cast(Any, _FakeModel()),
        fit_report_path=Path("/tmp/deterministic-anchor-fit-report.json"),
        tick_data_source=WorkbenchSourceTable(
            source_path=Path("/tmp/source.csv"),
            file_format="csv",
            column_names=("timestamp_utc", "trade_price"),
            source_row_count=5,
        ),
        observed_tick_series=observed_series,
        observed_range_start_utc=datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        observed_range_end_utc=datetime(2026, 2, 2, 19, 0, tzinfo=UTC),
    )

    comparison = build_observed_simulation_comparison_for_fit_context(
        fit_context,
        model=cast(Any, fit_context.model),
        seed=9,
    )

    assert comparison is not None
    _assert_price_series(
        comparison.observed_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 17, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 18, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 19, 0, tzinfo=UTC),
        ),
        prices=(99.0, 100.0, 101.0, 102.0, 103.0),
    )
    _assert_price_series(
        comparison.observed_comparison_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 17, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 18, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 19, 0, tzinfo=UTC),
        ),
        prices=(99.0, 100.0, 101.0, 102.0, 103.0),
    )
    assert captured_simulations == [
        {
            "seed": 9,
            "start_time_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            "horizon_interval_count": 5,
            "interval": None,
            "silent": True,
        },
        {
            "seed": 9,
            "start_time_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            "horizon_interval_count": 17,
            "interval": "15m",
            "silent": True,
        },
    ]


def test_workbench_comparison_uses_full_observed_range_for_large_inputs() -> None:
    captured_simulations: list[dict[str, Any]] = []

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 0, 0, tzinfo=UTC),
                        "trade_price": 100.0,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 9, 0, 0, tzinfo=UTC),
                        "trade_price": 101.0,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> _FakeSimulationResult:
            del progress_callback
            captured_simulations.append(
                {
                    "seed": seed,
                    "start_time_utc": start_time_utc,
                    "horizon_interval_count": horizon_interval_count,
                    "interval": interval,
                    "silent": silent,
                }
            )
            return _FakeSimulationResult()

    observed_records = tuple(
        (
            (
                datetime(2026, 1, 31, 0, 0, tzinfo=UTC)
                + pandas.Timedelta(hours=hours_elapsed)
            )
            .isoformat()
            .replace("+00:00", "Z"),
            100.0 + hours_elapsed * 0.01,
        )
        for hours_elapsed in range(24 * 10 + 1)
    )
    observed_series = _price_series_from_records(observed_records)
    fit_context = WorkbenchFitContext(
        model=cast(Any, _FakeModel()),
        fit_report_path=Path("/tmp/long-range-fit-report.json"),
        tick_data_source=WorkbenchSourceTable(
            source_path=Path("/tmp/source.csv"),
            file_format="csv",
            column_names=("timestamp_utc", "trade_price"),
            source_row_count=len(observed_records),
        ),
        observed_tick_series=observed_series,
        observed_range_start_utc=datetime(2026, 1, 31, 0, 0, tzinfo=UTC),
        observed_range_end_utc=datetime(2026, 2, 10, 0, 0, tzinfo=UTC),
    )

    comparison = build_observed_simulation_comparison_for_fit_context(
        fit_context,
        model=cast(Any, fit_context.model),
        seed=11,
    )

    assert comparison is not None
    assert comparison.observed_comparison_series.timestamp_microseconds_utc.size == len(observed_records)
    assert comparison.observed_comparison_series.prices.size == len(observed_records)
    assert comparison.observed_comparison_series.timestamp_microseconds_utc[0] == (
        datetime_to_utc_microseconds(datetime(2026, 1, 31, 0, 0, tzinfo=UTC))
    )
    assert comparison.observed_comparison_series.timestamp_microseconds_utc[1] == (
        datetime_to_utc_microseconds(datetime(2026, 1, 31, 1, 0, tzinfo=UTC))
    )
    assert comparison.observed_comparison_series.timestamp_microseconds_utc[-1] == (
        datetime_to_utc_microseconds(datetime(2026, 2, 10, 0, 0, tzinfo=UTC))
    )
    assert comparison.observed_comparison_series.prices[0] == 100.0
    assert comparison.observed_comparison_series.prices[1] == 100.01
    assert comparison.observed_comparison_series.prices[-1] == 102.4
    assert captured_simulations == [
        {
            "seed": 11,
            "start_time_utc": datetime(2026, 1, 31, 0, 0, tzinfo=UTC),
            "horizon_interval_count": 241,
            "interval": None,
            "silent": True,
        },
        {
            "seed": 11,
            "start_time_utc": datetime(2026, 1, 31, 0, 0, tzinfo=UTC),
            "horizon_interval_count": 961,
            "interval": "15m",
            "silent": True,
        },
    ]


def test_workbench_fit_context_generates_cross_platform_source_path_candidates() -> None:
    windows_candidates = _candidate_source_paths(
        r"C:\Users\anton\source\repos\janiform\data\AUDCAD_2025.parquet"
    )
    assert Path(
        "/mnt/c/Users/anton/source/repos/janiform/data/AUDCAD_2025.parquet"
    ) in windows_candidates

    mount_candidates = _candidate_source_paths(
        "/mnt/c/Users/anton/source/repos/janiform/data/AUDCAD_2025.parquet"
    )
    assert any(
        str(candidate).lower().startswith("c:")
        and "Users" in str(candidate)
        and "AUDCAD_2025.parquet" in str(candidate)
        for candidate in mount_candidates
    )


def test_parameter_objective_figure_clips_extreme_objective_outliers() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/parameter-objective-test"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "hawkes",
                    "admissible": True,
                    "scalar_tie_break": 0.4,
                    "canonical_coordinates": {"theta": 0.0},
                },
                {
                    "candidate_id": "candidate-002",
                    "stage": "hawkes",
                    "admissible": True,
                    "scalar_tie_break": 0.8,
                    "canonical_coordinates": {"theta": 0.2},
                },
                {
                    "candidate_id": "candidate-003",
                    "stage": "hawkes",
                    "admissible": False,
                    "scalar_tie_break": 1.0,
                    "canonical_coordinates": {"theta": 0.4},
                },
                {
                    "candidate_id": "candidate-004",
                    "stage": "hawkes",
                    "admissible": False,
                    "scalar_tie_break": 120.0,
                    "canonical_coordinates": {"theta": 0.6},
                },
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    figure = workbench_plots.create_parameter_objective_figure_with_columns(
        artifacts,
        column_count=1,
    )
    axis = figure.axes[0]

    assert axis.get_ylim()[1] < 10.0
    assert any("rejected clipped: 1 high" == text.get_text() for text in axis.texts)


def test_parameter_objective_figure_separates_valid_from_invalid_candidates() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/parameter-objective-validity-test"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "hawkes",
                    "valid": True,
                    "admissible": True,
                    "scalar_tie_break": 0.4,
                    "canonical_coordinates": {"theta": 0.0},
                },
                {
                    "candidate_id": "candidate-002",
                    "stage": "hawkes",
                    "valid": True,
                    "admissible": False,
                    "scalar_tie_break": 0.8,
                    "canonical_coordinates": {"theta": 0.2},
                },
                {
                    "candidate_id": "candidate-003",
                    "stage": "hawkes",
                    "valid": False,
                    "admissible": False,
                    "scalar_tie_break": 1.0,
                    "canonical_coordinates": {"theta": 0.4},
                },
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    figure = workbench_plots.create_parameter_objective_figure_with_columns(
        artifacts,
        column_count=1,
    )
    axis = figure.axes[0]
    legend_labels = [
        text.get_text()
        for text in cast(Any, axis.get_legend()).get_texts()
    ]

    assert legend_labels == ["admissible", "valid", "rejected"]


def test_selected_candidate_diagnostics_omits_joint_distance_only_scatter_pairs() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/selected-candidate-diagnostics-test"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "joint_refinement",
                    "evaluation_tier": "tick_joint",
                    "admissible": True,
                    "valid": True,
                    "pareto_rank": 1,
                    "lexicographic_rank": 1,
                    "scalar_tie_break": 0.1,
                    "family_metrics": [
                        {
                            "metric_id": "joint.activity_level",
                            "target_id": "joint.activity_level",
                            "target_family": "activity_level",
                            "normalized_distance": 0.34,
                            "admissible": True,
                            "simulated_value": 0.34,
                            "observed_value": 0.0,
                            "uncertainty_scale": 1.0,
                        },
                        {
                            "metric_id": "joint.return_scale",
                            "target_id": "joint.return_scale",
                            "target_family": "return_scale",
                            "normalized_distance": 0.52,
                            "admissible": True,
                            "simulated_value": 0.52,
                            "observed_value": 0.0,
                            "uncertainty_scale": 1.0,
                        },
                    ],
                    "canonical_coordinates": {"theta": 0.25},
                }
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={
            "decision": {
                "selected_candidate_id": "candidate-001",
                "decisive_metric_ids": [],
            }
        },
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    figure = workbench_plots.create_selected_candidate_diagnostics_figure(artifacts)
    scatter_axis = figure.axes[1]

    assert len(scatter_axis.collections) == 0
    assert any(
        "legacy joint-refinement family-distance scores only"
        in text.get_text()
        for text in scatter_axis.texts
    )


def test_selected_candidate_diagnostics_recovers_joint_pairs_from_stage_report_payload() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/selected-candidate-stage-report-fallback"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload={
            "staged_response": {
                "stage_reports": [
                    {
                        "stage_report_kind": "joint_refinement",
                        "fitted_stage_payload": {
                            "selected_candidate_id": "candidate-001",
                            "candidate_frontier": [
                                {
                                    "candidate_id": "candidate-001",
                                    "admissible": True,
                                    "objective_terms": [
                                        {
                                            "metric_id": "joint.activity_level",
                                            "target_id": "joint.activity_level",
                                            "target_family": "activity_level",
                                            "observed_value": 120.0,
                                            "simulated_value": 118.0,
                                            "uncertainty_scale": 120.0,
                                            "normalized_error": 0.0168,
                                        },
                                        {
                                            "metric_id": "joint.return_scale",
                                            "target_id": "joint.return_scale",
                                            "target_family": "return_scale",
                                            "observed_value": 0.0015,
                                            "simulated_value": 0.0017,
                                            "uncertainty_scale": 0.0015,
                                            "normalized_error": 0.1251,
                                        },
                                    ],
                                }
                            ],
                        },
                    }
                ]
            }
        },
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "joint_refinement",
                    "evaluation_tier": "tick_joint",
                    "admissible": True,
                    "valid": True,
                    "pareto_rank": 1,
                    "lexicographic_rank": 1,
                    "scalar_tie_break": 0.1,
                    "family_metrics": [
                        {
                            "metric_id": "joint.activity_level",
                            "target_id": "joint.activity_level",
                            "target_family": "activity_level",
                            "normalized_distance": 0.34,
                            "admissible": True,
                            "simulated_value": 0.34,
                            "observed_value": 0.0,
                            "uncertainty_scale": 1.0,
                        }
                    ],
                    "canonical_coordinates": {"theta": 0.25},
                }
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={
            "decision": {
                "selected_candidate_id": "candidate-001",
                "decisive_metric_ids": [],
            }
        },
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    figure = workbench_plots.create_selected_candidate_diagnostics_figure(artifacts)
    scatter_axis = figure.axes[1]

    assert len(scatter_axis.collections) == 1
    assert len(scatter_axis.collections[0].get_offsets()) == 2
    assert not any(
        "family-distance scores" in text.get_text() for text in scatter_axis.texts
    )


def test_selected_candidate_diagnostics_uses_symlog_for_mixed_scale_metrics() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/selected-candidate-mixed-scale-metrics"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={
            "frontier_entries": [
                {
                    "candidate_id": "candidate-001",
                    "stage": "joint_refinement",
                    "evaluation_tier": "tick_joint",
                    "admissible": True,
                    "valid": True,
                    "pareto_rank": 1,
                    "lexicographic_rank": 1,
                    "scalar_tie_break": 0.1,
                    "family_metrics": [
                        {
                            "metric_id": "joint.activity_level",
                            "target_id": "joint.activity_level",
                            "target_family": "activity_level",
                            "normalized_distance": 0.03,
                            "admissible": True,
                            "simulated_value": 510000.0,
                            "observed_value": 500000.0,
                            "uncertainty_scale": 500000.0,
                        },
                        {
                            "metric_id": "joint.return_scale",
                            "target_id": "joint.return_scale",
                            "target_family": "return_scale",
                            "normalized_distance": 0.12,
                            "admissible": True,
                            "simulated_value": 0.0017,
                            "observed_value": 0.0015,
                            "uncertainty_scale": 0.0015,
                        },
                        {
                            "metric_id": "joint.return_shape",
                            "target_id": "joint.return_shape",
                            "target_family": "return_asymmetry",
                            "normalized_distance": 0.50,
                            "admissible": True,
                            "simulated_value": 0.05,
                            "observed_value": 0.0,
                            "uncertainty_scale": 0.1,
                        },
                    ],
                    "canonical_coordinates": {"theta": 0.25},
                }
            ]
        },
        coarse_tick_consistency={},
        selection_rationale={
            "decision": {
                "selected_candidate_id": "candidate-001",
                "decisive_metric_ids": [],
            }
        },
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )

    figure = workbench_plots.create_selected_candidate_diagnostics_figure(artifacts)
    scatter_axis = figure.axes[1]

    assert scatter_axis.get_xscale() == "symlog"
    assert scatter_axis.get_yscale() == "symlog"


def test_workbench_comparison_builds_seeded_15m_series_from_raw_path(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    calendar_registry_json: str,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:15:00Z,101.0,sell",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-raw-seeded-15m",
        duplicate_record_policy=None,
        source_row_count=2,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    reference_price = 100.0
    first_interval_event_utc = datetime(2026, 2, 2, 14, 35, tzinfo=UTC)
    second_interval_event_utc = datetime(2026, 2, 2, 14, 50, tzinfo=UTC)
    third_interval_event_utc = datetime(2026, 2, 2, 15, 5, tzinfo=UTC)

    class _FakeSeededSimulationResult:
        def __init__(self) -> None:
            self.raw_result = SimpleNamespace(
                fine_path=[
                    SimpleNamespace(
                        timestamp_utc_nanos=int(first_interval_event_utc.timestamp() * 1_000_000_000),
                        observed_log_price=0.20,
                    ),
                    SimpleNamespace(
                        timestamp_utc_nanos=int(second_interval_event_utc.timestamp() * 1_000_000_000),
                        observed_log_price=0.25,
                    ),
                    SimpleNamespace(
                        timestamp_utc_nanos=int(third_interval_event_utc.timestamp() * 1_000_000_000),
                        observed_log_price=0.30,
                    ),
                ]
            )

        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 55, tzinfo=UTC),
                        "trade_price": reference_price * np.exp(0.25),
                        "trade_sign": "buy",
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 10, tzinfo=UTC),
                        "trade_price": reference_price * np.exp(0.30),
                        "trade_sign": "sell",
                    },
                ]
            )

    class _FakeCoarseSimulationResult:
        def __init__(self) -> None:
            self.raw_result = SimpleNamespace(
                records=[
                    SimpleNamespace(
                        interval_start_utc_nanos=int(
                            datetime(2026, 2, 2, 14, 30, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        interval_end_utc_nanos=int(
                            datetime(2026, 2, 2, 14, 45, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        return_components=SimpleNamespace(total_return=0.20),
                    ),
                    SimpleNamespace(
                        interval_start_utc_nanos=int(
                            datetime(2026, 2, 2, 14, 45, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        interval_end_utc_nanos=int(
                            datetime(2026, 2, 2, 15, 0, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        return_components=SimpleNamespace(total_return=0.05),
                    ),
                    SimpleNamespace(
                        interval_start_utc_nanos=int(
                            datetime(2026, 2, 2, 15, 0, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        interval_end_utc_nanos=int(
                            datetime(2026, 2, 2, 15, 15, tzinfo=UTC).timestamp() * 1_000_000_000
                        ),
                        return_components=SimpleNamespace(total_return=0.05),
                    ),
                ]
            )

        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records([])

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": reference_price,
            },
        }
        session_resolution = SimpleNamespace(
            resolved_session_definition_json=calendar_registry_json,
            exchange_code="XNYS",
        )

        def simulate(
            self,
            *,
            seed: int,
            start_time_utc: datetime,
            horizon_interval_count: int,
            interval: str | None = None,
            silent: bool,
            progress_callback: Any = None,
        ) -> Any:
            del seed, start_time_utc, horizon_interval_count, progress_callback, silent
            if interval == "15m":
                return _FakeCoarseSimulationResult()

            return _FakeSeededSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    comparison = build_observed_simulation_comparison(artifacts, seed=5)

    assert comparison is not None
    _assert_price_series(
        comparison.simulated_seeded_series_15m,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 45, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 15, tzinfo=UTC),
        ),
        prices=(
            reference_price * float(np.exp(0.20)),
            reference_price * float(np.exp(0.25)),
            reference_price * float(np.exp(0.30)),
        ),
    )


def test_workbench_plot_export_includes_observed_simulated_comparison(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _patch_save_all_figures_for_fast_orchestration(monkeypatch)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price,trade_sign",
                "2026-02-02T14:30:00Z,100.0,buy",
                "2026-02-02T15:30:00Z,101.0,sell",
            ]
        ),
        encoding="utf-8",
    )

    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-plots",
        duplicate_record_policy=None,
    )

    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 100.2,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.8,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    figure_paths = save_all_figures(artifacts, tmp_path / "plots")

    assert "selected_candidate_diagnostics" in figure_paths
    assert "observed_targets" in figure_paths
    assert "observed_simulated_comparison" in figure_paths
    assert "run_progress" in figure_paths
    assert "parameter_objective" in figure_paths
    assert "parameter_search" in figure_paths
    assert "simulation_summary" in figure_paths
    assert "td_sd_return_diagnostics" in figure_paths
    assert "td_sd_horizon_profiles" in figure_paths
    assert "td_sd_mean_reversion_variance_growth" in figure_paths
    assert "td_sd_mean_reversion_future_change" in figure_paths
    assert "candidate_compare" in figure_paths

    assert Path(figure_paths["observed_simulated_comparison"]).is_file()
    assert Path(figure_paths["td_sd_return_diagnostics"]).is_file()
    assert Path(figure_paths["td_sd_horizon_profiles"]).is_file()
    assert Path(figure_paths["td_sd_mean_reversion_variance_growth"]).is_file()
    assert Path(figure_paths["td_sd_mean_reversion_future_change"]).is_file()


def test_mean_reversion_variance_growth_figure_overlays_observed_and_simulated_curves(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        workbench_plots,
        "_mean_reversion_interval_series_for_comparison",
        lambda comparison: (
            comparison.observed_comparison_series,
            comparison.simulated_series,
        ),
    )
    monkeypatch.setattr(
        workbench_plots,
        "_materialize_endogenous_mean_reversion_anchor_values",
        lambda model, *, runtime_log_prices, timestamp_microseconds_utc: (
            np.zeros_like(runtime_log_prices, dtype=np.float64)
        ),
    )

    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/mean-reversion-variance-growth"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(model=_mean_reversion_test_model()),
        simulation_seed=7,
        observed_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.05),
                ("2026-02-02T15:00:00Z", 1.02),
                ("2026-02-02T15:15:00Z", 1.06),
                ("2026-02-02T15:30:00Z", 1.03),
                ("2026-02-02T15:45:00Z", 1.04),
                ("2026-02-02T16:00:00Z", 1.02),
                ("2026-02-02T16:15:00Z", 1.03),
            )
        ),
        observed_comparison_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.05),
                ("2026-02-02T15:00:00Z", 1.02),
                ("2026-02-02T15:15:00Z", 1.06),
                ("2026-02-02T15:30:00Z", 1.03),
                ("2026-02-02T15:45:00Z", 1.04),
                ("2026-02-02T16:00:00Z", 1.02),
                ("2026-02-02T16:15:00Z", 1.03),
            )
        ),
        simulated_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.03),
                ("2026-02-02T15:00:00Z", 1.01),
                ("2026-02-02T15:15:00Z", 1.04),
                ("2026-02-02T15:30:00Z", 1.02),
                ("2026-02-02T15:45:00Z", 1.03),
                ("2026-02-02T16:00:00Z", 1.01),
                ("2026-02-02T16:15:00Z", 1.02),
            )
        ),
        simulated_seeded_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.03),
                ("2026-02-02T15:00:00Z", 1.01),
                ("2026-02-02T15:15:00Z", 1.04),
                ("2026-02-02T15:30:00Z", 1.02),
            )
        ),
        simulated_coarse_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.03),
                ("2026-02-02T15:00:00Z", 1.01),
                ("2026-02-02T15:15:00Z", 1.04),
                ("2026-02-02T15:30:00Z", 1.02),
            )
        ),
    )

    figure = workbench_plots.create_td_sd_mean_reversion_variance_growth_figure(
        artifacts,
        comparison,
    )
    variance_axis = figure.axes[0]
    spread_axis = figure.axes[1]
    summary_axis = figure.axes[2]

    assert len(variance_axis.lines) == 2
    assert variance_axis.get_title() == "Variance Growth of Deviation from Anchor"
    assert len(spread_axis.lines) == 2
    assert spread_axis.get_title() == "Simulated Minus Observed Variance"
    assert summary_axis.get_title() == "Diagnostic Summary"
    assert "horizon count:" in summary_axis.texts[0].get_text()
    assert "max abs variance spread:" in summary_axis.texts[0].get_text()


def test_mean_reversion_series_analysis_materializes_endogenous_anchor_values(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        workbench_plots,
        "_materialize_endogenous_mean_reversion_anchor_values",
        lambda model, *, runtime_log_prices, timestamp_microseconds_utc: (
            np.asarray(
                [
                    float(runtime_log_prices[0]),
                    float(runtime_log_prices[1]) - 0.1,
                    float(runtime_log_prices[2]) - 0.2,
                ],
                dtype=np.float64,
            )
        ),
    )

    analysis = workbench_plots._build_mean_reversion_series_analysis(
        SimpleNamespace(
            metadata={"simulation_output_preferences": {"reference_price": 100.0}},
            parameters={
                "mean_reversion": {
                    "fitted_parameters": MeanReversionFittedParameters(
                        kappa=0.4,
                        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
                    ).to_payload()
                }
            },
        ),
        _price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 100.0),
                ("2026-02-02T14:45:00Z", 110.0),
                ("2026-02-02T15:00:00Z", 121.0),
            )
        ),
    )

    assert analysis is not None
    assert analysis.anchor_values == pytest.approx(np.log([1.0, 1.1, 1.21]) - [0.0, 0.1, 0.2])
    assert analysis.deviations_from_anchor == pytest.approx([0.0, 0.1, 0.2])


def test_summary_statistics_rows_use_observed_tick_returns_instead_of_prices(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    empty_bucket_series = workbench_plots.WorkbenchBucketSeriesAnalysis(
        bucket_timestamps_utc=(),
        close_prices=np.asarray([], dtype=np.float64),
        returns=np.asarray([], dtype=np.float64),
        event_counts=np.asarray([], dtype=np.int64),
    )
    analysis = workbench_plots.WorkbenchObservedSimulationAnalysis(
        observed=workbench_plots.WorkbenchSeriesAnalysis(
            timestamp_microseconds_utc=np.asarray([1, 2, 3], dtype=np.int64),
            prices=np.asarray([100.0, 200.0, 400.0], dtype=np.float64),
            normalized_prices=np.asarray([1.0, 2.0, 4.0], dtype=np.float64),
            returns=np.asarray([0.01, -0.02], dtype=np.float64),
            hourly_event_counts={},
            hourly_activity_profile=np.asarray([], dtype=np.float64),
            hourly_absolute_return_profile=np.asarray([], dtype=np.float64),
            bucket_series_by_horizon={
                15: workbench_plots.WorkbenchBucketSeriesAnalysis(
                    bucket_timestamps_utc=(),
                    close_prices=np.asarray([], dtype=np.float64),
                    returns=np.asarray([0.03], dtype=np.float64),
                    event_counts=np.asarray([], dtype=np.int64),
                )
            },
            rolling_activity_points=np.empty((0, 2), dtype=np.float64),
        ),
        simulated=workbench_plots.WorkbenchSeriesAnalysis(
            timestamp_microseconds_utc=np.asarray([1, 2, 3], dtype=np.int64),
            prices=np.asarray([100.0, 101.0, 102.0], dtype=np.float64),
            normalized_prices=np.asarray([1.0, 1.01, 1.02], dtype=np.float64),
            returns=np.asarray([0.04, 0.05], dtype=np.float64),
            hourly_event_counts={},
            hourly_activity_profile=np.asarray([], dtype=np.float64),
            hourly_absolute_return_profile=np.asarray([], dtype=np.float64),
            bucket_series_by_horizon={15: empty_bucket_series},
            rolling_activity_points=np.empty((0, 2), dtype=np.float64),
        ),
        simulated_seeded_15m=workbench_plots.WorkbenchSeriesAnalysis(
            timestamp_microseconds_utc=np.asarray([1, 2], dtype=np.int64),
            prices=np.asarray([1.0, 1.1], dtype=np.float64),
            normalized_prices=np.asarray([1.0, 1.1], dtype=np.float64),
            returns=np.asarray([0.06], dtype=np.float64),
            hourly_event_counts={},
            hourly_activity_profile=np.asarray([], dtype=np.float64),
            hourly_absolute_return_profile=np.asarray([], dtype=np.float64),
            bucket_series_by_horizon={15: empty_bucket_series},
            rolling_activity_points=np.empty((0, 2), dtype=np.float64),
        ),
        simulated_coarse_15m=workbench_plots.WorkbenchSeriesAnalysis(
            timestamp_microseconds_utc=np.asarray([1, 2], dtype=np.int64),
            prices=np.asarray([1.0, 1.1], dtype=np.float64),
            normalized_prices=np.asarray([1.0, 1.1], dtype=np.float64),
            returns=np.asarray([0.07], dtype=np.float64),
            hourly_event_counts={},
            hourly_activity_profile=np.asarray([], dtype=np.float64),
            hourly_absolute_return_profile=np.asarray([], dtype=np.float64),
            bucket_series_by_horizon={15: empty_bucket_series},
            rolling_activity_points=np.empty((0, 2), dtype=np.float64),
        ),
        available_horizon_minutes=(15,),
    )

    monkeypatch.setattr(
        workbench_plots,
        "_observed_simulation_analysis_for_plot",
        lambda artifacts, comparison: analysis,
    )

    rows = workbench_plots.summary_statistics_rows(
        WorkbenchArtifacts(
            source_path=Path("/tmp/summary-statistics-rows"),
            source_kind="run_directory",
            metadata={},
            fit_report_payload=None,
            model_payload=None,
            observed_targets={},
            candidate_frontier={},
            coarse_tick_consistency={},
            selection_rationale={},
            simulation_summary={},
            search_surface_manifest={},
            runtime_profile={},
        ),
        cast(Any, object()),
    )

    assert rows[0]["series"] == "Observed tick returns"
    assert rows[0]["count"] == 2
    assert rows[0]["mean"] == pytest.approx(-0.005)


def test_observed_simulated_price_figure_uses_full_observed_path_when_comparison_is_clipped() -> None:
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/observed-simulated-full-observed-path"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(model=_mean_reversion_test_model()),
        simulation_seed=3,
        observed_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 100.0),
                ("2026-02-02T14:45:00Z", 101.0),
                ("2026-02-02T15:00:00Z", 102.0),
            )
        ),
        observed_comparison_series=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 201.0),
                ("2026-02-02T15:00:00Z", 202.0),
            )
        ),
        simulated_series=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 150.0),
                ("2026-02-02T15:00:00Z", 151.0),
            )
        ),
        simulated_seeded_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 150.0),
                ("2026-02-02T15:00:00Z", 151.0),
            )
        ),
        simulated_coarse_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 150.0),
                ("2026-02-02T15:00:00Z", 151.0),
            )
        ),
    )

    figure = workbench_plots.create_observed_simulated_figure(artifacts, comparison)

    price_axis = figure.axes[0]
    observed_line = price_axis.lines[0]
    simulated_line = price_axis.lines[1]

    assert list(observed_line.get_ydata()) == [100.0, 101.0, 102.0]
    assert list(simulated_line.get_ydata()) == [150.0, 151.0]

    normalized_axis = figure.axes[1]
    observed_rebased_line = normalized_axis.lines[0]
    simulated_rebased_line = normalized_axis.lines[1]

    assert np.allclose(observed_rebased_line.get_ydata(), [100.0, 101.0, 102.0])
    assert np.allclose(simulated_rebased_line.get_ydata(), [100.0, 100.0 * 151.0 / 150.0])


def test_mean_reversion_future_change_figure_renders_endogenous_mean_reversion_profiles(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        workbench_plots,
        "_mean_reversion_interval_series_for_comparison",
        lambda comparison: (
            comparison.observed_comparison_series,
            comparison.simulated_series,
        ),
    )
    monkeypatch.setattr(
        workbench_plots,
        "_materialize_endogenous_mean_reversion_anchor_values",
        lambda model, *, runtime_log_prices, timestamp_microseconds_utc: (
            np.zeros_like(runtime_log_prices, dtype=np.float64)
        ),
    )

    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/mean-reversion-future-change"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload=None,
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(model=_mean_reversion_test_model()),
        simulation_seed=11,
        observed_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 0.96),
                ("2026-02-02T15:00:00Z", 0.98),
                ("2026-02-02T15:15:00Z", 1.01),
                ("2026-02-02T15:30:00Z", 1.04),
                ("2026-02-02T15:45:00Z", 1.02),
                ("2026-02-02T16:00:00Z", 1.00),
                ("2026-02-02T16:15:00Z", 0.99),
                ("2026-02-02T16:30:00Z", 1.01),
                ("2026-02-02T16:45:00Z", 1.03),
                ("2026-02-02T17:00:00Z", 1.00),
                ("2026-02-02T17:15:00Z", 0.98),
                ("2026-02-02T17:30:00Z", 1.00),
                ("2026-02-02T17:45:00Z", 1.02),
                ("2026-02-02T18:00:00Z", 1.01),
                ("2026-02-02T18:15:00Z", 0.99),
            )
        ),
        observed_comparison_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 0.96),
                ("2026-02-02T15:00:00Z", 0.98),
                ("2026-02-02T15:15:00Z", 1.01),
                ("2026-02-02T15:30:00Z", 1.04),
                ("2026-02-02T15:45:00Z", 1.02),
                ("2026-02-02T16:00:00Z", 1.00),
                ("2026-02-02T16:15:00Z", 0.99),
                ("2026-02-02T16:30:00Z", 1.01),
                ("2026-02-02T16:45:00Z", 1.03),
                ("2026-02-02T17:00:00Z", 1.00),
                ("2026-02-02T17:15:00Z", 0.98),
                ("2026-02-02T17:30:00Z", 1.00),
                ("2026-02-02T17:45:00Z", 1.02),
                ("2026-02-02T18:00:00Z", 1.01),
                ("2026-02-02T18:15:00Z", 0.99),
            )
        ),
        simulated_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 0.98),
                ("2026-02-02T15:00:00Z", 0.99),
                ("2026-02-02T15:15:00Z", 1.01),
                ("2026-02-02T15:30:00Z", 1.02),
                ("2026-02-02T15:45:00Z", 1.01),
                ("2026-02-02T16:00:00Z", 1.00),
                ("2026-02-02T16:15:00Z", 0.99),
                ("2026-02-02T16:30:00Z", 1.00),
                ("2026-02-02T16:45:00Z", 1.01),
                ("2026-02-02T17:00:00Z", 1.00),
                ("2026-02-02T17:15:00Z", 0.99),
                ("2026-02-02T17:30:00Z", 1.00),
                ("2026-02-02T17:45:00Z", 1.01),
                ("2026-02-02T18:00:00Z", 1.00),
                ("2026-02-02T18:15:00Z", 0.99),
            )
        ),
        simulated_seeded_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 0.98),
                ("2026-02-02T15:00:00Z", 0.99),
                ("2026-02-02T15:15:00Z", 1.01),
                ("2026-02-02T15:30:00Z", 1.02),
            )
        ),
        simulated_coarse_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 0.98),
                ("2026-02-02T15:00:00Z", 0.99),
                ("2026-02-02T15:15:00Z", 1.01),
                ("2026-02-02T15:30:00Z", 1.02),
            )
        ),
    )

    figure = workbench_plots.create_td_sd_mean_reversion_future_change_figure(
        artifacts,
        comparison,
    )
    conditional_axis = figure.axes[0]
    summary_axis = figure.axes[1]

    assert len(conditional_axis.lines) == 4
    assert conditional_axis.get_title() == "Conditional Future Change vs Deviation"
    assert summary_axis.get_title() == "Diagnostic Summary"
    assert "selected horizon:" in summary_axis.texts[0].get_text()


def test_mean_reversion_variance_growth_figure_reports_endogenous_diagnostics_summary() -> None:
    diagnostics_payload = _endogenous_mean_reversion_diagnostics_payload()
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/mean-reversion-disabled-variance"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload={
            "staged_response": {
                "stage_reports": [
                    {
                        "stage_report_kind": "mean_reversion",
                        "fitted_stage_payload": {
                            "fitted_parameters": _endogenous_mean_reversion_fitted_parameters_payload(),
                            "diagnostics": diagnostics_payload,
                        },
                    }
                ]
            }
        },
        model_payload={
            "parameters": {
                "mean_reversion": {
                    "fitted_parameters": _endogenous_mean_reversion_fitted_parameters_payload(),
                    "diagnostics": diagnostics_payload,
                }
            }
        },
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(model=_diagnostic_only_mean_reversion_test_model(diagnostics_payload)),
        simulation_seed=5,
        observed_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.01),
                ("2026-02-02T15:00:00Z", 1.02),
            )
        ),
        observed_comparison_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.01),
                ("2026-02-02T15:00:00Z", 1.02),
            )
        ),
        simulated_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
        simulated_seeded_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
        simulated_coarse_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
    )

    figure = workbench_plots.create_td_sd_mean_reversion_variance_growth_figure(
        artifacts,
        comparison,
    )

    assert figure.axes[0].texts[0].get_text().startswith(
        "Mean-reversion anchor diagnostics are unavailable because the loaded model does not expose enough runtime inputs"
    )
    assert "stage decision: endogenous_anchor_runtime_projection_required.v1" in figure.axes[2].texts[0].get_text()
    assert "source observations: 4" in figure.axes[2].texts[0].get_text()


def test_mean_reversion_future_change_figure_reports_endogenous_diagnostics_summary() -> None:
    diagnostics_payload = _endogenous_mean_reversion_diagnostics_payload()
    artifacts = WorkbenchArtifacts(
        source_path=Path("/tmp/mean-reversion-disabled-future-change"),
        source_kind="run_directory",
        metadata={},
        fit_report_payload=None,
        model_payload={
            "parameters": {
                "mean_reversion": {
                    "fitted_parameters": _endogenous_mean_reversion_fitted_parameters_payload(),
                    "diagnostics": diagnostics_payload,
                }
            }
        },
        observed_targets={},
        candidate_frontier={},
        coarse_tick_consistency={},
        selection_rationale={},
        simulation_summary={},
        search_surface_manifest={},
        runtime_profile={},
    )
    comparison = WorkbenchObservedSimulationComparison(
        fit_context=SimpleNamespace(model=_diagnostic_only_mean_reversion_test_model(diagnostics_payload)),
        simulation_seed=13,
        observed_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.01),
                ("2026-02-02T15:00:00Z", 1.02),
            )
        ),
        observed_comparison_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.01),
                ("2026-02-02T15:00:00Z", 1.02),
            )
        ),
        simulated_series=_price_series_from_records(
            (
                ("2026-02-02T14:30:00Z", 1.00),
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
        simulated_seeded_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
        simulated_coarse_series_15m=_price_series_from_records(
            (
                ("2026-02-02T14:45:00Z", 1.00),
                ("2026-02-02T15:00:00Z", 1.01),
            )
        ),
    )

    figure = workbench_plots.create_td_sd_mean_reversion_future_change_figure(
        artifacts,
        comparison,
    )

    assert figure.axes[0].texts[0].get_text().startswith(
        "Mean-reversion anchor diagnostics are unavailable because the loaded model does not expose enough runtime inputs"
    )
    assert "retained observations: 0" in figure.axes[1].texts[0].get_text()
    assert "coverage ratio: 0" in figure.axes[1].texts[0].get_text()


def test_workbench_fit_context_reads_parquet_ticks_with_column_batches(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.parquet"
    parquet.write_table(
        pa.table(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                ],
                "trade_price": [100.0, 101.0],
                "trade_sign": ["buy", "sell"],
            }
        ),
        tick_data_path,
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-parquet",
        duplicate_record_policy=None,
        file_format="parquet",
        source_row_count=2,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "parquet",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
        }

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    fit_context = load_fit_context(artifacts)

    assert fit_context is not None
    _assert_price_series(
        fit_context.observed_tick_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
        ),
        prices=(100.0, 101.0),
    )


def test_workbench_fit_context_keeps_last_duplicate_across_parquet_batch_boundaries(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks-duplicates.parquet"
    parquet.write_table(
        pa.table(
            {
                "timestamp_utc": [
                    datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                    datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
                ],
                "trade_price": [100.0, 101.0, 102.0, 103.0],
                "trade_sign": ["buy", "sell", "buy", "sell"],
            }
        ),
        tick_data_path,
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-batch-boundary",
        duplicate_record_policy="keep_last",
        file_format="parquet",
        source_row_count=4,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "parquet",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": "trade_sign",
                    "bid_column": None,
                    "ask_column": None,
                },
            },
        }

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )
    monkeypatch.setattr("tools.workbench.fit_context._PARQUET_BATCH_SIZE", 2)

    artifacts = load_workbench_artifacts(tmp_path)
    fit_context = load_fit_context(artifacts)

    assert fit_context is not None
    _assert_price_series(
        fit_context.observed_tick_series,
        timestamps_utc=(
            datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
        ),
        prices=(100.0, 102.0, 103.0),
    )


def test_workbench_plot_export_builds_observed_simulation_comparison_once(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _patch_save_all_figures_for_fast_orchestration(monkeypatch)

    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:30:00Z,101.0",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-export-cache",
        duplicate_record_policy=None,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    comparison_build_count = 0

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 100.2,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.8,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
        }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    def _fake_build_observed_simulation_comparison(*args: Any, **kwargs: Any) -> Any:
        nonlocal comparison_build_count
        comparison_build_count += 1
        return build_observed_simulation_comparison(*args, **kwargs)

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )
    monkeypatch.setattr(
        "tools.workbench.plots.build_observed_simulation_comparison",
        _fake_build_observed_simulation_comparison,
    )

    artifacts = load_workbench_artifacts(tmp_path)
    figure_paths = save_all_figures(artifacts, tmp_path / "plots-cached")

    assert comparison_build_count == 1
    assert Path(figure_paths["observed_simulated_comparison"]).is_file()
    assert Path(figure_paths["td_sd_summary_statistics"]).is_file()


def test_workbench_plot_export_accepts_mismatched_return_series_lengths(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "timestamp_utc,trade_price",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T14:31:00Z,100.5",
                "2026-02-02T14:32:00Z,101.0",
                "2026-02-02T14:33:00Z,101.5",
            ]
        ),
        encoding="utf-8",
    )
    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-mismatched-returns",
        duplicate_record_policy=None,
        source_row_count=4,
    )
    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "trade_price": 100.2,
                    },
                    {
                        "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "trade_price": 100.8,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "timestamp_utc",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "trade_price",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
        }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    output_paths = _save_selected_figures(
        artifacts,
        tmp_path / "plots-mismatched-returns",
        {
            "observed_simulated_comparison": workbench_plots.create_observed_simulated_figure,
            "td_sd_return_distributions": workbench_plots.create_td_sd_return_distribution_figure,
        },
    )

    assert Path(output_paths["observed_simulated_comparison"]).is_file()
    assert Path(output_paths["td_sd_return_distributions"]).is_file()


def test_workbench_plot_export_accepts_time_as_timestamp_key(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "time,close",
                "2026-02-02T14:30:00Z,100.0",
                "2026-02-02T15:30:00Z,101.0",
            ]
        ),
        encoding="utf-8",
    )

    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-time-key",
        duplicate_record_policy=None,
    )

    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "time": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
                        "close": 100.2,
                    },
                    {
                        "time": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
                        "close": 100.8,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "time",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "close",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    output_paths = _save_selected_figures(
        artifacts,
        tmp_path / "plots-time",
        {
            "observed_simulated_comparison": workbench_plots.create_observed_simulated_figure,
            "td_sd_return_distributions": workbench_plots.create_td_sd_return_distribution_figure,
        },
    )

    assert Path(output_paths["observed_simulated_comparison"]).is_file()
    assert Path(output_paths["td_sd_return_distributions"]).is_file()


def test_workbench_plot_export_accepts_time_microseconds_timestamp_key(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tick_data_path = tmp_path / "ticks.csv"
    tick_data_path.write_text(
        "\n".join(
            [
                "time_microseconds,close",
                "1706884200000000,100.0",
                "1706887800000000,101.0",
            ]
        ),
        encoding="utf-8",
    )

    _write_fit_report(
        tmp_path / "fit_report.json",
        tick_data_path=tick_data_path,
        run_id="fit-context-time-microseconds-key",
        duplicate_record_policy=None,
    )

    (tmp_path / "model.json").write_text("{}", encoding="utf-8")

    class _FakeSimulationResult:
        def to_pandas(self) -> Any:
            return pandas.DataFrame.from_records(
                [
                    {
                        "timestamp_utc": datetime(2024, 2, 2, 14, 30, tzinfo=UTC),
                        "close": 100.2,
                    },
                    {
                        "timestamp_utc": datetime(2024, 2, 2, 15, 30, tzinfo=UTC),
                        "close": 100.8,
                    },
                ]
            )

    class _FakeModel:
        metadata = {
            "input_inference": {
                "tick_data_format": "csv",
                "timestamp_column": "time_microseconds",
                "timestamp_timezone": "UTC",
                "tick_price_input_policy": "explicit_price_column",
                "tick_price_columns": {
                    "price_column": "close",
                    "trade_sign_column": None,
                    "bid_column": None,
                    "ask_column": None,
                },
            },
            "simulation_output_preferences": {
                "reference_price": 100.0,
            },
        }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    output_paths = _save_selected_figures(
        artifacts,
        tmp_path / "plots-time-microseconds",
        {
            "observed_simulated_comparison": workbench_plots.create_observed_simulated_figure,
            "td_sd_return_distributions": workbench_plots.create_td_sd_return_distribution_figure,
        },
    )

    assert Path(output_paths["observed_simulated_comparison"]).is_file()
    assert Path(output_paths["td_sd_return_distributions"]).is_file()


def _write_fit_report(
    path: Path,
    *,
    tick_data_path: Path,
    run_id: str,
    duplicate_record_policy: str | None,
    file_format: str = "csv",
    source_row_count: int = 3,
) -> None:
    import json

    payload = {
        "run_id": run_id,
        "preparation_provenance": {
            "tick_data_summary": {
                "path": str(tick_data_path),
                "file_format": file_format,
                "provenance_label": "tick_data",
                "source_row_count": source_row_count,
                "retained_row_count": source_row_count,
                "dropped_duplicate_row_count": 0,
                "sort_records_by_timestamp": True,
                "applied_timestamp_timezone": "UTC",
                "canonical_value_field": "trade_price",
                "value_mapping_policy": "explicit_price_column",
            },
            "option_surface_summary": None,
            "known_news_summary": None,
            "normalization_policy": {
                "timestamp_timezone": "UTC",
                "sort_records_by_timestamp": True,
                "duplicate_record_policy": duplicate_record_policy,
            },
        },
        "staged_response": {
            "run_id": run_id,
            "status": "success",
            "execution_mode": "memory",
            "diagnostic_artifact_bundle": {
                "observed_targets": {
                    "schema_version": "calibration.diagnostics.observed_targets.v1",
                    "target_builder_version": "test",
                    "targets": [],
                },
                "candidate_frontier": {
                    "schema_version": "calibration.diagnostics.candidate_frontier.v1",
                    "frontier_entries": [],
                },
                "coarse_tick_consistency": {
                    "schema_version": "calibration.diagnostics.coarse_tick_consistency.v1",
                    "metrics": [],
                },
                "selection_rationale": {
                    "schema_version": "calibration.diagnostics.selection_rationale.v1",
                    "decision": {
                        "selection_stage": "joint_refinement",
                        "selected_candidate_id": "candidate-ctx",
                        "selection_policy": "test",
                        "validity_screened_candidate_count": 0,
                        "admissible_candidate_count": 0,
                        "pareto_frontier_count": 0,
                        "tie_break_candidate_count": 0,
                        "decisive_metric_ids": [],
                        "rationale": "test",
                    },
                },
                "simulation_summary": {
                    "schema_version": "calibration.diagnostics.simulation_summary.v1",
                    "summaries": [],
                },
                "search_surface_manifest": {
                    "schema_version": "calibration.diagnostics.search_surface_manifest.v1",
                    "surfaces": [
                        {
                            "surface_id": "joint_refinement.frontier.weighted_objective",
                            "x_label": "lexicographic_rank",
                            "x_value": 1.0,
                            "objective_value": 0.5,
                            "admissible": True,
                        }
                    ],
                },
                "runtime_profile": {
                    "schema_version": "calibration.diagnostics.runtime_profile.v1",
                    "cpu_count": 1,
                    "entries": [],
                },
            },
        },
    }

    path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")


def _assert_price_series(
    series: Any,
    *,
    timestamps_utc: tuple[datetime, ...],
    prices: tuple[float, ...],
) -> None:
    assert np.array_equal(
        series.timestamp_microseconds_utc,
        np.asarray(
            [datetime_to_utc_microseconds(timestamp_utc) for timestamp_utc in timestamps_utc],
            dtype=np.int64,
        ),
    )
    assert np.array_equal(series.prices, np.asarray(prices, dtype=np.float64))


def _price_series_from_records(
    records: tuple[tuple[str, float], ...],
) -> WorkbenchPriceSeries:
    return WorkbenchPriceSeries(
        timestamp_microseconds_utc=np.asarray(
            [
                datetime_to_utc_microseconds(datetime.fromisoformat(timestamp_text.replace("Z", "+00:00")))
                for timestamp_text, _ in records
            ],
            dtype=np.int64,
        ),
        prices=np.asarray([price for _, price in records], dtype=np.float64),
    )


def _mean_reversion_test_model() -> Any:
    return SimpleNamespace(
        metadata={"simulation_output_preferences": {"reference_price": 1.0}},
        parameters={
            "mean_reversion": {
                "fitted_parameters": MeanReversionFittedParameters(
                    kappa=0.4,
                    anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
                ).to_payload()
            }
        }
    )


def _diagnostic_only_mean_reversion_test_model(diagnostics_payload: dict[str, Any]) -> Any:
    return SimpleNamespace(
        metadata={"simulation_output_preferences": {"reference_price": 1.0}},
        parameters={
            "mean_reversion": {
                "fitted_parameters": _endogenous_mean_reversion_fitted_parameters_payload(),
                "diagnostics": diagnostics_payload,
            }
        }
    )


def _endogenous_mean_reversion_diagnostics_payload() -> dict[str, Any]:
    return {
        "chosen_anchor_family": "endogenous_core_pressure_level",
        "selected_candidate_family": "random_walk_null",
        "calibration_grid": {
            "calibration_grid_kind": "synthetic_daily_close_grid.v1",
            "calibration_grid_interval_hours": 24.0,
            "source_observation_count": 4,
            "retained_observation_count": 0,
            "coverage_ratio": 0.0,
        },
        "anchor_parameter_estimates": {
            "anchor_model_kind": "endogenous_core_pressure_level",
            "reference_interval_hours": 24.0,
            "pressure_persistence_per_reference_interval": 0.0,
            "pressure_level_gain": 0.0,
        },
        "anchor_origin_level": None,
        "phi_reference": 1.0,
        "kappa": 0.0,
        "residual_sum_of_squares": 0.0,
        "quasi_log_likelihood": None,
        "information_criterion_bic": None,
        "model_selection_policy": "gaussian_quasi_likelihood_bic_with_endogenous_core_pressure_level_candidate_family_profile.v1",
        "deviation_sample_count": 0,
        "regression_pair_count": 0,
        "least_squares_rank": 0,
        "evaluated_candidates": [
            {
                "candidate_family": "random_walk_null",
                "anchor_model": {
                    "anchor_model_kind": "endogenous_core_pressure_level",
                    "reference_interval_hours": 24.0,
                    "pressure_persistence_per_reference_interval": 0.0,
                    "pressure_level_gain": 0.0,
                },
                "anchor_origin_level": None,
                "phi_reference": 1.0,
                "kappa": 0.0,
                "residual_sum_of_squares": 0.0,
                "quasi_log_likelihood": None,
                "information_criterion_bic": None,
                "parameter_count": 0,
                "least_squares_rank": 0,
                "deviation_sample_count": 0,
                "regression_pair_count": 0,
                "core_expected_increments": [],
                "pressure_state_path": [],
                "boundary_anchor_values": [],
                "deviations": [],
                "residuals": [],
                "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
            }
        ],
        "stage_decision_reason": "endogenous_anchor_runtime_projection_required.v1",
    }


def _endogenous_mean_reversion_fitted_parameters_payload() -> dict[str, Any]:
    return MeanReversionFittedParameters(
        kappa=0.0,
        anchor_model=MeanReversionAnchorModel.zero_gain_endogenous_core_pressure_level(),
    ).to_payload()


class _FakeAxis:
    def __init__(self) -> None:
        self.boxplot_data: list[list[int]] = []
        self.xtick_labels: list[str] = []
        self.transAxes = object()

    def boxplot(self, data: list[list[int]], **_: Any) -> None:
        self.boxplot_data = data

    def set_xticks(self, ticks: list[float]) -> None:
        del ticks

    def set_xticklabels(self, labels: list[str], rotation: int = 0) -> None:
        del rotation
        self.xtick_labels = labels

    def set_title(self, title: str) -> None:
        del title

    def set_ylabel(self, y_label: str) -> None:
        del y_label

    def grid(self, *args: Any, **kwargs: Any) -> None:
        del args
        del kwargs

    def text(self, *args: Any, **kwargs: Any) -> None:
        del args
        del kwargs

    def set_yticks(self, ticks: list[float]) -> None:
        del ticks


def _patch_save_all_figures_for_fast_orchestration(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    builder_names = (
        "create_selected_candidate_diagnostics_figure",
        "create_observed_targets_figure",
        "create_frontier_figure",
        "create_run_progress_figure",
        "create_runtime_figure",
        "create_parameter_objective_figure",
        "create_parameter_search_figure",
        "create_consistency_figure",
        "create_simulation_summary_figure",
        "create_search_surface_figure",
        "create_observed_simulated_figure",
        "create_td_sd_return_distribution_figure",
        "create_td_sd_return_diagnostics_figure",
        "create_td_sd_hourly_profile_figure",
        "create_td_sd_horizon_profile_figure",
        "create_td_sd_mean_reversion_variance_growth_figure",
        "create_td_sd_mean_reversion_future_change_figure",
        "create_td_sd_autocorrelation_figure",
        "create_td_sd_summary_statistics_figure",
        "create_candidate_compare_figure",
    )

    def _lightweight_builder(*args: Any, **kwargs: Any) -> Figure:
        del args
        del kwargs
        return create_message_figure("Test", "placeholder")

    def _fast_savefig(self: Figure, filename: Any, *args: Any, **kwargs: Any) -> None:
        del self
        del args
        del kwargs
        Path(filename).write_bytes(b"test")

    for builder_name in builder_names:
        monkeypatch.setattr(
            f"tools.workbench.plots.{builder_name}",
            _lightweight_builder,
        )

    monkeypatch.setattr(Figure, "savefig", _fast_savefig)


def _save_selected_figures(
    artifacts: Any,
    output_directory: Path,
    builders_by_name: dict[str, Any],
) -> dict[str, str]:
    output_directory.mkdir(parents=True, exist_ok=True)
    output_paths: dict[str, str] = {}

    for figure_name, builder in builders_by_name.items():
        figure = builder(artifacts)
        output_path = output_directory / f"{figure_name}.png"
        figure.savefig(output_path, dpi=160)
        output_paths[figure_name] = str(output_path)

    return output_paths
