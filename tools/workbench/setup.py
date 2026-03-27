from pathlib import Path
from typing import Optional

from PyQt6.QtCore import pyqtSignal
from PyQt6.QtWidgets import (
    QCheckBox,
    QComboBox,
    QFileDialog,
    QFormLayout,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)

from janiform.calibration.fit.input_resolution import (
    HighLevelFitInference,
    infer_high_level_fit_inputs,
)
from janiform.calibration.market_data.contracts import (
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationTickPriceInputPolicy,
)

from .state import WorkbenchRunSetup


class WorkbenchSetupWidget(QWidget):
    setup_submitted = pyqtSignal(object)
    open_existing_requested = pyqtSignal()

    def __init__(self) -> None:
        super().__init__()

        self._latest_inference: Optional[HighLevelFitInference] = None
        self._dataset_path_edit = QLineEdit()
        self._exchange_code_edit = QLineEdit("FXSPOT")
        self._output_directory_edit = QLineEdit(str((Path.cwd() / ".janiform").resolve()))
        self._workflow_scope_combo = QComboBox()
        self._mean_reversion_checkbox = QCheckBox("Enable mean reversion and anchoring")
        self._file_format_combo = QComboBox()
        self._timestamp_column_combo = QComboBox()
        self._price_policy_combo = QComboBox()
        self._price_column_combo = QComboBox()
        self._bid_column_combo = QComboBox()
        self._ask_column_combo = QComboBox()
        self._status_label = QLabel("Select a dataset to infer calibration inputs.")
        self._inference_view = QTextEdit()
        self._preferred_mean_reversion_enabled: bool = True
        self._build_ui()

    def _build_ui(self) -> None:
        root_layout = QVBoxLayout(self)
        form_layout = QFormLayout()

        dataset_row = QWidget()
        dataset_layout = QHBoxLayout(dataset_row)
        dataset_layout.setContentsMargins(0, 0, 0, 0)
        dataset_layout.addWidget(self._dataset_path_edit)
        browse_button = QPushButton("Browse")
        browse_button.clicked.connect(self._choose_dataset_path)
        dataset_layout.addWidget(browse_button)
        infer_button = QPushButton("Infer")
        infer_button.clicked.connect(self._infer_inputs)
        dataset_layout.addWidget(infer_button)
        form_layout.addRow("Dataset", dataset_row)

        self._file_format_combo.addItem("Auto", None)
        self._file_format_combo.addItem("Parquet", CalibrationInputFileFormat.PARQUET)
        self._file_format_combo.addItem("CSV", CalibrationInputFileFormat.CSV)
        form_layout.addRow("File Format", self._file_format_combo)

        self._workflow_scope_combo.addItem(
            "Full Without Option Surface",
            CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        )
        self._workflow_scope_combo.addItem(
            "Seasonality Only",
            CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        )
        self._workflow_scope_combo.addItem(
            "Full With Option Surface",
            CalibrationMarketDataWorkflowScope.FULL_WITH_OPTION_SURFACE,
        )

        form_layout.addRow("Workflow Scope", self._workflow_scope_combo)

        self._mean_reversion_checkbox.setChecked(True)
        self._mean_reversion_checkbox.toggled.connect(self._on_mean_reversion_toggled)
        self._workflow_scope_combo.currentIndexChanged.connect(self._on_workflow_scope_changed)

        form_layout.addRow("Mean Reversion", self._mean_reversion_checkbox)
        form_layout.addRow("Exchange Code", self._exchange_code_edit)

        output_row = QWidget()
        output_layout = QHBoxLayout(output_row)
        output_layout.setContentsMargins(0, 0, 0, 0)
        output_layout.addWidget(self._output_directory_edit)
        output_browse_button = QPushButton("Browse")
        output_browse_button.clicked.connect(self._choose_output_directory)
        output_layout.addWidget(output_browse_button)

        form_layout.addRow("Output Dir", output_row)
        form_layout.addRow("Timestamp Column", self._timestamp_column_combo)

        self._price_policy_combo.addItem("Auto", None)
        self._price_policy_combo.addItem(
            "Explicit Price Column",
            CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN,
        )
        self._price_policy_combo.addItem(
            "Bid/Ask Midpoint",
            CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT,
        )

        form_layout.addRow("Price Policy", self._price_policy_combo)
        form_layout.addRow("Price Column", self._price_column_combo)
        form_layout.addRow("Bid Column", self._bid_column_combo)
        form_layout.addRow("Ask Column", self._ask_column_combo)

        root_layout.addLayout(form_layout)
        self._status_label.setWordWrap(True)
        root_layout.addWidget(self._status_label)

        self._inference_view.setReadOnly(True)
        root_layout.addWidget(self._inference_view)

        action_row = QWidget()
        action_layout = QHBoxLayout(action_row)
        action_layout.setContentsMargins(0, 0, 0, 0)
        open_existing_button = QPushButton("Open Existing Run")
        open_existing_button.clicked.connect(self.open_existing_requested.emit)
        action_layout.addWidget(open_existing_button)
        calibrate_button = QPushButton("Calibrate")
        calibrate_button.clicked.connect(self._submit_setup)
        action_layout.addWidget(calibrate_button)
        root_layout.addWidget(action_row)

        self._on_workflow_scope_changed()

    def set_dataset_path(self, path: Path) -> None:
        self._dataset_path_edit.setText(str(path))
        self._infer_inputs()

    def _choose_dataset_path(self) -> None:
        selected_path, _ = QFileDialog.getOpenFileName(
            self,
            "Select dataset",
            str(Path.cwd()),
            "Market data (*.parquet *.pq *.csv);;All files (*)",
        )
        if not selected_path:
            return

        self._dataset_path_edit.setText(selected_path)
        self._infer_inputs()

    def _choose_output_directory(self) -> None:
        selected_directory = QFileDialog.getExistingDirectory(
            self,
            "Select output directory",
            self._output_directory_edit.text() or str(Path.cwd()),
        )
        if selected_directory:
            self._output_directory_edit.setText(selected_directory)

    def _infer_inputs(self) -> None:
        dataset_path_text = self._dataset_path_edit.text().strip()
        if not dataset_path_text:
            self._status_label.setText("Select a dataset to infer calibration inputs.")
            return

        try:
            inference = infer_high_level_fit_inputs(
                tick_data_path=Path(dataset_path_text).expanduser().resolve(),
                tick_data_format=self._file_format_combo.currentData(),
                tick_data_provenance_label="tick_data",
                workflow_scope=self._workflow_scope_combo.currentData(),
                run_id=None,
                column_overrides=None,
                normalization_overrides=None,
                market_profile_configuration=None,
            )
        except Exception as error:
            self._latest_inference = None
            self._status_label.setText(f"Inference failed: {error}")
            self._inference_view.setPlainText(str(error))
            return

        self._latest_inference = inference

        self._populate_column_combo(
            self._timestamp_column_combo,
            inference.available_columns,
            inference.tick_data_column_mapping.timestamp_column,
        )
        self._populate_column_combo(
            self._price_column_combo,
            inference.available_columns,
            inference.tick_data_column_mapping.price_column,
        )
        self._populate_column_combo(
            self._bid_column_combo,
            inference.available_columns,
            inference.tick_data_column_mapping.bid_column,
        )
        self._populate_column_combo(
            self._ask_column_combo,
            inference.available_columns,
            inference.tick_data_column_mapping.ask_column,
        )

        inferred_price_policy = inference.normalization_policy.tick_price_input_policy

        self._set_combo_data(self._price_policy_combo, inferred_price_policy)
        self._status_label.setText(
            f"Inferred run_id `{inference.run_id}` from {len(inference.available_columns)} columns."
        )
        self._inference_view.setPlainText(
            "\n".join(
                (
                    f"Run ID: {inference.run_id}",
                    f"Available columns: {', '.join(inference.available_columns)}",
                    f"Timestamp candidates: {', '.join(inference.timestamp_candidates)}",
                    f"Price candidates: {', '.join(inference.price_candidates)}",
                    f"Bid candidates: {', '.join(inference.bid_candidates)}",
                    f"Ask candidates: {', '.join(inference.ask_candidates)}",
                    f"Timestamp timezone: {inference.normalization_policy.timestamp_timezone}",
                    f"Timestamp timezone source: {inference.timestamp_timezone_source}",
                )
            )
        )

    def _submit_setup(self) -> None:
        dataset_path = Path(self._dataset_path_edit.text().strip()).expanduser()
        if not dataset_path.exists():
            self._status_label.setText("The selected dataset path does not exist.")
            return

        run_setup = WorkbenchRunSetup(
            dataset_path=dataset_path.resolve(),
            exchange_code=self._exchange_code_edit.text().strip() or "FXSPOT",
            workflow_scope=self._workflow_scope_combo.currentData(),
            output_directory=Path(self._output_directory_edit.text().strip()).expanduser().resolve(),
            mean_reversion_enabled=self._mean_reversion_checkbox.isChecked(),
            file_format=self._file_format_combo.currentData(),
            timestamp_column=self._selected_combo_value(self._timestamp_column_combo),
            tick_price_input_policy=self._price_policy_combo.currentData(),
            price_column=self._selected_combo_value(self._price_column_combo),
            bid_column=self._selected_combo_value(self._bid_column_combo),
            ask_column=self._selected_combo_value(self._ask_column_combo),
            run_id=None if self._latest_inference is None else self._latest_inference.run_id,
        )

        self.setup_submitted.emit(run_setup)

    def _populate_column_combo(
        self,
        combo_box: QComboBox,
        columns: tuple[str, ...],
        selected_value: Optional[str],
    ) -> None:
        combo_box.clear()
        combo_box.addItem("", None)
        for column in columns:
            combo_box.addItem(column, column)
        self._set_combo_data(combo_box, selected_value)

    @staticmethod
    def _set_combo_data(combo_box: QComboBox, target_value: object) -> None:
        for index in range(combo_box.count()):
            if combo_box.itemData(index) == target_value:
                combo_box.setCurrentIndex(index)
                return

        combo_box.setCurrentIndex(0)

    @staticmethod
    def _selected_combo_value(combo_box: QComboBox) -> Optional[str]:
        selected_value = combo_box.currentData()
        if isinstance(selected_value, str) and selected_value:
            return selected_value

        return None

    def _on_mean_reversion_toggled(self, checked: bool) -> None:
        if self._mean_reversion_checkbox.isEnabled():
            self._preferred_mean_reversion_enabled = checked

    def _on_workflow_scope_changed(self) -> None:
        workflow_scope = self._workflow_scope_combo.currentData()
        mean_reversion_supported = (
            workflow_scope != CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY
        )

        self._mean_reversion_checkbox.setEnabled(mean_reversion_supported)

        if mean_reversion_supported:
            self._mean_reversion_checkbox.setChecked(self._preferred_mean_reversion_enabled)
            self._mean_reversion_checkbox.setToolTip("")
            return

        self._mean_reversion_checkbox.setChecked(False)
        self._mean_reversion_checkbox.setToolTip(
            "Mean reversion and anchoring are unavailable for seasonality-only calibration."
        )


__all__ = ["WorkbenchSetupWidget"]
