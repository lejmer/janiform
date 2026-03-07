import ast
from pathlib import Path

from examples.calibration_workflow import run_example as run_calibration_example
from examples.pricing_workflow import run_example as run_pricing_example
from examples.simulation_workflow import run_example as run_simulation_example

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
EXAMPLES_ROOT = REPOSITORY_ROOT / "examples"
README_PATH = REPOSITORY_ROOT / "README.md"

ALLOWED_JANIFORM_IMPORTS = {
    "janiform.calendar",
    "janiform.calibration",
    "janiform.hourly_factors",
    "janiform.pricing",
    "janiform.simulation",
}


def test_example_files_import_only_public_janiform_modules() -> None:
    for example_path in sorted(EXAMPLES_ROOT.glob("*.py")):
        module_ast = ast.parse(example_path.read_text(encoding="utf-8"), filename=str(example_path))
        for node in ast.walk(module_ast):
            match node:
                case ast.Import():
                    for alias in node.names:
                        _assert_example_import_is_public(example_path, alias.name)
                case ast.ImportFrom():
                    if node.module is not None:
                        _assert_example_import_is_public(example_path, node.module)


def test_simulation_example_runs_with_deterministic_summary() -> None:
    summary = run_simulation_example()

    assert summary == {
        "request_summary": {
            "seed": 17,
            "horizon_interval_count": 3,
            "hourly_factor_interval_count": 3,
        },
        "response_summary": {
            "schema_version": "simulation.seeded_tick_event.path.v1",
            "event_count": 5,
            "first_event": {
                "event_index": 0,
                "side": "sell",
                "state_snapshot": None,
                "timestamp_utc_nanos": 1770044373742285686,
            },
            "last_event": {
                "event_index": 4,
                "side": "sell",
                "state_snapshot": None,
                "timestamp_utc_nanos": 1770051218918372793,
            },
        },
    }


def test_pricing_example_runs_with_deterministic_summary() -> None:
    summary = run_pricing_example()

    assert summary == {
        "request_summary": {
            "interval_delta": 1.0,
            "payoff_type": "call",
            "nu_h_known": 0.01,
            "nu_h_surprise": 0.02,
        },
        "run_result_summary": {
            "schema_version": "pricing.vanilla_option.run_result.v1",
            "price": 3.872311,
            "forward_price": 102.015067,
            "run_diagnostics_included": True,
            "calibration_hooks_included": True,
        },
    }


def test_calibration_example_runs_with_default_and_override_modes(tmp_path: Path) -> None:
    summary = run_calibration_example(disk_checkpoint_directory=tmp_path)

    assert summary == {
        "memory_mode": {
            "request_execution_mode": "memory",
            "response_execution_mode": "memory",
            "status": "success",
            "stage_count": 3,
            "checkpoint_count": 3,
        },
        "disk_mode": {
            "request_execution_mode": "disk",
            "response_execution_mode": "disk",
            "status": "success",
            "stage_count": 3,
            "checkpoint_count": 3,
            "checkpoint_file_count": 10,
        },
    }


def test_readme_references_examples_documentation() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    assert "examples/README.md" in readme_text
    assert "examples/simulation_workflow.py" in readme_text
    assert "examples/pricing_workflow.py" in readme_text
    assert "examples/calibration_workflow.py" in readme_text


def _assert_example_import_is_public(example_path: Path, module_name: str) -> None:
    if module_name.startswith("tests"):
        raise AssertionError(f"{example_path} must not import test-only modules: {module_name}")

    if module_name.startswith("janiform._"):
        raise AssertionError(f"{example_path} must not import private Janiform modules: {module_name}")

    if module_name == "janiform":
        raise AssertionError(f"{example_path} must import task-specific Janiform modules, not {module_name}")

    if module_name.startswith("janiform"):
        if module_name not in ALLOWED_JANIFORM_IMPORTS:
            raise AssertionError(
                f"{example_path} imports non-public or unsupported Janiform module: {module_name}"
            )
