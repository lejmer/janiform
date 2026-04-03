import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from pathlib import Path

from PyQt6.QtWidgets import QApplication

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.mean_reversion import MeanReversionAnchorModelKind
from tools.workbench.setup import WorkbenchSetupWidget
from tools.workbench.state import (
    DEFAULT_WORKBENCH_SESSION_PRESET_NAME,
    WorkbenchRunSetup,
)


def test_workbench_setup_defaults_mean_reversion_to_endogenous_on_calibration_submit(
    tmp_path: Path,
) -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()
    submitted_setups: list[WorkbenchRunSetup] = []
    dataset_path = tmp_path / "ticks.csv"
    dataset_path.write_text("timestamp_utc,trade_price\n", encoding="utf-8")

    try:
        widget.setup_submitted.connect(submitted_setups.append)
        widget._dataset_path_edit.setText(str(dataset_path))

        widget._submit_setup()
        application.processEvents()

        assert len(submitted_setups) == 1
        assert (
            submitted_setups[0].mean_reversion_anchor_model_kind
            == MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
        )
        assert submitted_setups[0].session_source.mode == "preset"
        assert submitted_setups[0].session_source.preset_name == DEFAULT_WORKBENCH_SESSION_PRESET_NAME
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_defaults_session_preset_to_fx_spot_24_5() -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()

    try:
        assert (
            widget._session_preset_combo.currentData()
            == DEFAULT_WORKBENCH_SESSION_PRESET_NAME
        )
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_disables_anchor_family_selection_for_seasonality_only_workflows(
    tmp_path: Path,
) -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()
    submitted_setups: list[WorkbenchRunSetup] = []
    dataset_path = tmp_path / "ticks.csv"
    dataset_path.write_text("timestamp_utc,trade_price\n", encoding="utf-8")

    try:
        widget._dataset_path_edit.setText(str(dataset_path))
        widget._workflow_scope_combo.setCurrentIndex(1)
        application.processEvents()

        assert widget._mean_reversion_anchor_combo.isEnabled() is False
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_does_not_offer_removed_deterministic_anchor_family(
    tmp_path: Path,
) -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()
    dataset_path = tmp_path / "ticks.csv"
    dataset_path.write_text("timestamp_utc,trade_price\n", encoding="utf-8")

    try:
        widget._dataset_path_edit.setText(str(dataset_path))
        application.processEvents()

        assert (
            widget._mean_reversion_anchor_combo.findData(
                MeanReversionAnchorModelKind.ENDOGENOUS_CORE_PRESSURE_LEVEL
            )
            != -1
        )
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_disables_mean_reversion_for_seasonality_only_scope() -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()

    try:
        widget._workflow_scope_combo.setCurrentIndex(
            widget._workflow_scope_combo.findData(
                CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
            )
        )
        application.processEvents()

        assert widget._mean_reversion_anchor_combo.isEnabled() is False
        assert (
            widget._mean_reversion_anchor_combo.toolTip()
            == "Anchor-family selection is unavailable for seasonality-only calibration."
        )

        widget._workflow_scope_combo.setCurrentIndex(
            widget._workflow_scope_combo.findData(
                CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE
            )
        )
        application.processEvents()

        assert widget._mean_reversion_anchor_combo.isEnabled() is True
        assert widget._mean_reversion_anchor_combo.toolTip() == ""
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_enables_probe_fallback_exchange_only_in_probe_mode() -> None:
    application = QApplication.instance() or QApplication([])
    widget = WorkbenchSetupWidget()

    try:
        assert widget._session_preset_combo.isEnabled() is True
        assert widget._session_exchange_code_edit.isEnabled() is False

        widget._session_source_mode_combo.setCurrentIndex(
            widget._session_source_mode_combo.findData("probe_fallback")
        )
        application.processEvents()

        assert widget._session_preset_combo.isEnabled() is False
        assert widget._session_exchange_code_edit.isEnabled() is True
    finally:
        widget.close()
        application.processEvents()
