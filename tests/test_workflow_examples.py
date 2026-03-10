import ast
from pathlib import Path

from examples.calibration_workflow import run_example as run_calibration_example
from examples.market_data_calibration_workflow import run_example as run_market_data_calibration_example
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
    "janiform.risk",
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
            "transition_count": 8,
            "event_projection_count": 5,
            "first_transition": {
                "cumulative_log_return": 0.2645866072827854,
                "market_state": "open",
                "news_arrival_component": None,
                "return_components": {
                    "buy_side_return": -0.15584667947115877,
                    "closed_gap_return": 0.0,
                    "open_jump_return": 0.0,
                    "sell_side_return": 0.42043328675394415,
                    "total_return": 0.2645866072827854,
                    "trading_time_return": 0.2645866072827854,
                },
                "side": "sell",
                "state_snapshot": None,
                "timestamp_utc_nanos": 1770044373742285686,
                "transition_index": 0,
                "transition_kind": "sell_trade",
            },
            "last_transition": {
                "cumulative_log_return": -0.2169953075506372,
                "market_state": "open",
                "news_arrival_component": None,
                "return_components": {
                    "buy_side_return": -0.028989504599600237,
                    "closed_gap_return": 0.0,
                    "open_jump_return": 0.0,
                    "sell_side_return": -0.03486925376066073,
                    "total_return": -0.06385875836026098,
                    "trading_time_return": -0.06385875836026098,
                },
                "side": None,
                "state_snapshot": None,
                "timestamp_utc_nanos": 1770051600000000000,
                "transition_index": 7,
                "transition_kind": "interval_boundary_drift",
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


def test_market_data_calibration_example_runs_with_user_facing_preparation_summary(
    tmp_path: Path,
) -> None:
    summary = run_market_data_calibration_example(working_directory=tmp_path)

    assert summary == {
        "tick_csv_primary": {
            "derived_option_surface_data_reference": None,
            "derived_trade_events_data_reference": "market_data://market_data_csv_primary/ticks/primary_tick_feed",
            "executed_preprocessing_steps": [
                "source_loading",
                "column_mapping_validation",
                "timestamp_timezone_normalization",
                "duplicate_record_resolution",
                "timestamp_sorting",
                "session_state_resolution",
                "workflow_scope_selection",
                "known_news_window_filtering",
                "known_news_overlay_derivation",
                "hawkes_candidate_generation",
                "return_components_candidate_generation",
            ],
            "known_news_event_count": 2,
            "material_close_open_gap_count": 1,
            "option_surface_record_count": 0,
            "preparation_schema_version": "calibration.market_data.preparation.v1",
            "prepared_execution_mode": "memory",
            "prepared_stage_count": 3,
            "selected_stages": ["seasonality", "hawkes", "return_components"],
            "tick_data_format": "csv",
            "tick_record_count": 6,
            "workflow_scope": "full_without_option_surface",
        },
        "tick_csv_with_option_surface": {
            "derived_option_surface_data_reference": (
                "market_data://market_data_option_surface_primary/options/options_close"
            ),
            "derived_trade_events_data_reference": (
                "market_data://market_data_option_surface_primary/ticks/primary_tick_feed"
            ),
            "executed_preprocessing_steps": [
                "source_loading",
                "column_mapping_validation",
                "timestamp_timezone_normalization",
                "duplicate_record_resolution",
                "timestamp_sorting",
                "session_state_resolution",
                "workflow_scope_selection",
                "known_news_window_filtering",
                "known_news_overlay_derivation",
                "hawkes_candidate_generation",
                "return_components_candidate_generation",
                "option_surface_quote_normalization",
            ],
            "known_news_event_count": 2,
            "material_close_open_gap_count": 1,
            "option_surface_record_count": 1,
            "preparation_schema_version": "calibration.market_data.preparation.v1",
            "prepared_execution_mode": "memory",
            "prepared_stage_count": 3,
            "selected_stages": ["seasonality", "hawkes", "return_components"],
            "tick_data_format": "csv",
            "tick_record_count": 6,
            "workflow_scope": "full_with_option_surface",
        },
        "tick_parquet_primary": {
            "derived_option_surface_data_reference": None,
            "derived_trade_events_data_reference": "market_data://market_data_parquet_primary/ticks/primary_tick_feed",
            "executed_preprocessing_steps": [
                "source_loading",
                "column_mapping_validation",
                "timestamp_timezone_normalization",
                "duplicate_record_resolution",
                "timestamp_sorting",
                "session_state_resolution",
                "workflow_scope_selection",
                "known_news_window_filtering",
                "known_news_overlay_derivation",
                "hawkes_candidate_generation",
                "return_components_candidate_generation",
            ],
            "known_news_event_count": 2,
            "material_close_open_gap_count": 1,
            "option_surface_record_count": 0,
            "preparation_schema_version": "calibration.market_data.preparation.v1",
            "prepared_execution_mode": "memory",
            "prepared_stage_count": 3,
            "selected_stages": ["seasonality", "hawkes", "return_components"],
            "tick_data_format": "parquet",
            "tick_record_count": 6,
            "workflow_scope": "full_without_option_surface",
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
    assert "examples/market_data_calibration_workflow.py" in readme_text
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
