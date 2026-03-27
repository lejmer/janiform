from pathlib import Path

from janiform.calibration.market_data.contracts import CalibrationMarketDataWorkflowScope
from janiform.contracts import MeanReversionAnchorModelKind
from tools.workbench.execution import _mean_reversion_configuration_from_setup
from tools.workbench.state import WorkbenchRunSetup


def test_workbench_execution_enables_constant_anchor_mean_reversion_by_default() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
    )

    configuration = _mean_reversion_configuration_from_setup(run_setup)

    assert configuration is not None
    assert configuration.anchor_model_kind == MeanReversionAnchorModelKind.CONSTANT
    assert configuration.calibration_grid_interval_hours is None


def test_workbench_execution_omits_mean_reversion_when_user_turns_it_off() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.FULL_WITHOUT_OPTION_SURFACE,
        output_directory=Path("output"),
        mean_reversion_enabled=False,
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None


def test_workbench_execution_omits_mean_reversion_for_seasonality_only_runs() -> None:
    run_setup = WorkbenchRunSetup(
        dataset_path=Path("ticks.csv"),
        exchange_code="FXSPOT",
        workflow_scope=CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY,
        output_directory=Path("output"),
        mean_reversion_enabled=True,
    )

    assert _mean_reversion_configuration_from_setup(run_setup) is None
