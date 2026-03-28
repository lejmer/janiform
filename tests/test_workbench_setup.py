import os

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from pathlib import Path

from PyQt6.QtWidgets import QApplication

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.contracts import MeanReversionAnchorModelKind
from tools.workbench.setup import WorkbenchSetupWidget
from tools.workbench.state import WorkbenchRunSetup


def test_workbench_setup_defaults_mean_reversion_to_enabled_on_calibration_submit(
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
        assert submitted_setups[0].mean_reversion_enabled is True
        assert (
            submitted_setups[0].mean_reversion_anchor_model_kind
            == MeanReversionAnchorModelKind.CONSTANT
        )
    finally:
        widget.close()
        application.processEvents()


def test_workbench_setup_allows_disabling_mean_reversion_before_submit(
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
        widget._mean_reversion_checkbox.setChecked(False)

        widget._submit_setup()
        application.processEvents()

        assert len(submitted_setups) == 1
        assert submitted_setups[0].mean_reversion_enabled is False
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
                MeanReversionAnchorModelKind.ENDOGENOUS_CORE_EXPECTATION_ONE_INTERVAL
            )
            == -1
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

        assert widget._mean_reversion_checkbox.isEnabled() is False
        assert widget._mean_reversion_checkbox.isChecked() is False
        assert widget._mean_reversion_anchor_combo.isEnabled() is False

        widget._workflow_scope_combo.setCurrentIndex(
            widget._workflow_scope_combo.findData(
                CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE
            )
        )
        application.processEvents()

        assert widget._mean_reversion_checkbox.isEnabled() is True
        assert widget._mean_reversion_checkbox.isChecked() is True
        assert widget._mean_reversion_anchor_combo.isEnabled() is True
    finally:
        widget.close()
        application.processEvents()
