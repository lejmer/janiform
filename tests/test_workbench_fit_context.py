from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import pandas
import pytest

from tools.workbench import load_workbench_artifacts
from tools.workbench.fit_context import build_observed_simulation_comparison
from tools.workbench.plots import save_all_figures


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
        ) -> _FakeSimulationResult:
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
    assert comparison.observed_rows == (
        {
            "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            "trade_price": 102.0,
            "trade_sign": "sell",
            "price": 102.0,
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 45, tzinfo=UTC),
            "trade_price": 101.0,
            "trade_sign": "sell",
            "price": 101.0,
        },
    )
    assert comparison.simulated_rows == (
        {
            "timestamp_utc": datetime(2026, 2, 2, 14, 30, tzinfo=UTC),
            "trade_price": 99.5,
            "trade_sign": "buy",
            "price": 99.5,
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 30, tzinfo=UTC),
            "trade_price": 100.5,
            "trade_sign": "sell",
            "price": 100.5,
        },
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
        ) -> _FakeSimulationResult:
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
    assert comparison.simulated_rows == (
        {
            "timestamp_utc": datetime(2026, 2, 2, 14, 0, tzinfo=UTC),
            "trade_price": 100.1,
            "price": 100.1,
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 15, 0, tzinfo=UTC),
            "trade_price": 100.5,
            "price": 100.5,
        },
        {
            "timestamp_utc": datetime(2026, 2, 2, 16, 0, tzinfo=UTC),
            "trade_price": 101.1,
            "price": 101.1,
        },
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


def test_workbench_plot_export_includes_observed_simulated_comparison(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
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
    assert "simulation_summary" in figure_paths

    assert Path(figure_paths["observed_simulated_comparison"]).is_file()


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
    figure_paths = save_all_figures(artifacts, tmp_path / "plots-time")

    assert Path(figure_paths["observed_simulated_comparison"]).is_file()


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
    figure_paths = save_all_figures(artifacts, tmp_path / "plots-time-microseconds")

    assert Path(figure_paths["observed_simulated_comparison"]).is_file()


def _write_fit_report(
    path: Path,
    *,
    tick_data_path: Path,
    run_id: str,
    duplicate_record_policy: str | None,
) -> None:
    import json

    payload = {
        "run_id": run_id,
        "preparation_provenance": {
            "tick_data_summary": {
                "path": str(tick_data_path),
                "file_format": "csv",
                "provenance_label": "tick_data",
                "source_row_count": 3,
                "retained_row_count": 3,
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
