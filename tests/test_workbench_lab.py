import os
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

import pandas
import pytest
from PyQt6.QtCore import Qt
from PyQt6.QtWidgets import QApplication

from tools.workbench.artifacts import load_workbench_artifacts
from tools.workbench.app import WorkbenchWindow
from tools.workbench.fit_context import load_fit_context
from tools.workbench.lab import apply_reevaluation_request
from tools.workbench.parameters import (
    apply_parameter_updates,
    flatten_parameter_tree,
    parse_parameter_value,
)


def test_flatten_parameter_tree_and_apply_updates() -> None:
    leaves = flatten_parameter_tree(
        {
            "hawkes": {
                "fitted_parameters": {
                    "beta": 42.0,
                    "kernel": {
                        "alpha": 0.1,
                    },
                }
            }
        }
    )

    assert [leaf.path for leaf in leaves] == [
        "hawkes.fitted_parameters.beta",
        "hawkes.fitted_parameters.kernel.alpha",
    ]

    updated = apply_parameter_updates(
        {
            "hawkes": {
                "fitted_parameters": {
                    "beta": 42.0,
                    "kernel": {
                        "alpha": 0.1,
                    },
                }
            }
        },
        {
            "hawkes.fitted_parameters.beta": 12.5,
            "hawkes.fitted_parameters.kernel.alpha": 0.25,
        },
    )

    assert updated["hawkes"]["fitted_parameters"]["beta"] == 12.5
    assert updated["hawkes"]["fitted_parameters"]["kernel"]["alpha"] == 0.25


def test_parse_parameter_value_uses_reasonable_scalar_inference() -> None:
    assert parse_parameter_value("12", None) == 12
    assert parse_parameter_value("1.25", None) == 1.25
    assert parse_parameter_value("true", None) is True
    assert parse_parameter_value("alpha", None) == "alpha"


def test_apply_reevaluation_request_updates_loaded_model_parameters(
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

    _write_fit_report(tmp_path / "fit_report.json", tick_data_path=tick_data_path)
    _write_model(tmp_path / "model.json")

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
        run_id = "lab-run"
        workflow_scope = "full_without_option_surface"
        resolved_configuration = {}
        session_resolution = {}
        _execution_input = None
        _stage_report_payloads = ()
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

        def __init__(self) -> None:
            self.parameters = {
                "hawkes": {
                    "fitted_parameters": {
                        "beta": 42.0,
                    }
                }
            }

        @property
        def fitted_parameters(self) -> dict[str, Any]:
            return {
                "hawkes": {
                    "fitted_parameters": {
                        "beta": 42.0,
                    }
                }
            }

        def simulate(self, **_: Any) -> _FakeSimulationResult:
            return _FakeSimulationResult()

    fake_model = _FakeModel()

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: fake_model,
    )

    artifacts = load_workbench_artifacts(tmp_path)
    fit_context = load_fit_context(artifacts)

    assert fit_context is not None

    updated_parameters = apply_reevaluation_request(
        fit_context.model,
        {"hawkes.fitted_parameters.beta": 12.0},
    )

    assert updated_parameters["hawkes"]["fitted_parameters"]["beta"] == 12.0


def test_lab_tab_populates_editable_parameter_rows_when_opened(
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

    _write_fit_report(tmp_path / "fit_report.json", tick_data_path=tick_data_path)
    _write_model(tmp_path / "model.json")

    class _FakeModel:
        run_id = "lab-run"
        workflow_scope = "full_without_option_surface"
        resolved_configuration = {}
        session_resolution = {}
        _execution_input = None
        _stage_report_payloads = ()
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

        @property
        def fitted_parameters(self) -> dict[str, Any]:
            return {
                "hawkes": {
                    "fitted_parameters": {
                        "beta": 42.0,
                    }
                }
            }

    monkeypatch.setattr(
        "tools.workbench.fit_context.JaniformModel.load",
        lambda path: _FakeModel(),
    )

    artifacts = load_workbench_artifacts(tmp_path)
    application = QApplication.instance() or QApplication([])
    window = WorkbenchWindow()

    try:
        window._apply_artifacts(artifacts)

        lab_tab_index = next(
            tab_index
            for tab_index in range(window._root_tabs.count())
            if window._root_tabs.widget(tab_index) is window._lab_container
        )
        window._root_tabs.setCurrentIndex(lab_tab_index)
        application.processEvents()

        assert window._lab_parameter_table.rowCount() == 1

        path_item = window._lab_parameter_table.item(0, 0)
        block_item = window._lab_parameter_table.item(0, 1)
        baseline_item = window._lab_parameter_table.item(0, 2)
        edited_item = window._lab_parameter_table.item(0, 3)

        assert path_item is not None
        assert block_item is not None
        assert baseline_item is not None
        assert edited_item is not None
        assert path_item.text() == "hawkes.fitted_parameters.beta"
        assert block_item.text() == "hawkes"
        assert baseline_item.text() == "42.0"
        assert edited_item.text() == "42.0"
        assert not bool(path_item.flags() & Qt.ItemFlag.ItemIsEditable)
        assert not bool(block_item.flags() & Qt.ItemFlag.ItemIsEditable)
        assert not bool(baseline_item.flags() & Qt.ItemFlag.ItemIsEditable)
        assert bool(edited_item.flags() & Qt.ItemFlag.ItemIsEditable)
        assert window._lab_parameter_table.columnWidth(1) == 120
        assert window._lab_parameter_table.columnWidth(2) == 140
        assert window._lab_parameter_table.columnWidth(3) == 180
    finally:
        window.close()
        application.processEvents()


def _write_fit_report(path: Path, *, tick_data_path: Path) -> None:
    path.write_text(
        """
{
  "run_id": "lab-run",
  "schema_version": "calibration.high_level.fit_report.v1",
  "workflow_scope": "full_without_option_surface",
  "success": true,
  "resolved_configuration": {},
  "session_resolution": {},
  "preparation_diagnostics": {},
  "preparation_provenance": {
    "tick_data_summary": {
      "path": "__TICK_DATA_PATH__",
      "file_format": "csv"
    },
    "normalization_policy": {
      "duplicate_record_policy": null,
      "sort_records_by_timestamp": true
    }
  },
  "staged_response": {
    "run_id": "lab-run",
    "status": "success",
    "execution_mode": "memory",
    "diagnostic_artifact_bundle": {
      "observed_targets": {"targets": []},
      "candidate_frontier": {"frontier_entries": []},
      "coarse_tick_consistency": {"metrics": []},
      "selection_rationale": {"decision": {}},
      "simulation_summary": {"summaries": []},
      "search_surface_manifest": {"surfaces": []},
      "runtime_profile": {"entries": []}
    }
  }
}
        """.replace("__TICK_DATA_PATH__", str(tick_data_path).replace("\\", "\\\\")),
        encoding="utf-8",
    )


def _write_model(path: Path) -> None:
    path.write_text(
        """
{
  "schema_version": "calibration.high_level.model.v1",
  "run_id": "lab-run",
  "workflow_scope": "full_without_option_surface",
  "metadata": {},
  "resolved_configuration": {},
  "session_resolution": {},
  "parameters": {
    "hawkes": {
      "fitted_parameters": {
        "beta": 42.0
      }
    }
  }
}
        """,
        encoding="utf-8",
    )
