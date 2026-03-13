from contextlib import redirect_stdout
from io import StringIO

import pytest

import janiform.cli.command_tree as cli_command_tree_module


def test_cli_top_level_help_lists_primary_and_developer_namespaces() -> None:
    help_output = _render_help([])

    assert "presets" in help_output
    assert "fit" in help_output
    assert "simulate" in help_output
    assert "fit-and-simulate" in help_output
    assert "simulation" in help_output
    assert "seasonality" in help_output
    assert "hourly-factors" in help_output
    assert "calibration" in help_output
    assert "pricing" in help_output
    assert "stylized-facts" in help_output
    assert "dev" in help_output


def test_simulation_help_lists_seeded_tick_workflow() -> None:
    help_output = _render_help(["simulation"])

    assert "coarse-interval" in help_output
    assert "seeded-ticks" in help_output


def test_seasonality_help_lists_preprocess_timestamp_workflow() -> None:
    help_output = _render_help(["seasonality"])

    assert "diagnostics" in help_output
    assert "preprocess-timestamps" in help_output


def test_hourly_factors_help_lists_materialization_and_contract_workflows() -> None:
    help_output = _render_help(["hourly-factors"])

    assert "materialize-seasonality" in help_output
    assert "materialize-news-intensity" in help_output
    assert "build-contract" in help_output


def test_calibration_help_lists_staged_workflow() -> None:
    help_output = _render_help(["calibration"])

    assert "market-data" in help_output
    assert "staged" in help_output


def test_pricing_help_lists_vanilla_option_workflow() -> None:
    help_output = _render_help(["pricing"])

    assert "vanilla-option" in help_output


def test_stylized_facts_help_lists_horizon_comparison_workflow() -> None:
    help_output = _render_help(["stylized-facts"])

    assert "horizon-comparison" in help_output


def test_developer_help_lists_regression_namespace() -> None:
    help_output = _render_help(["dev"])

    assert "regression" in help_output


def test_regression_help_lists_developer_regression_commands() -> None:
    help_output = _render_help(["dev", "regression"])

    assert "deterministic" in help_output
    assert "open-closed-gap-leakage" in help_output


def _render_help(arguments: list[str]) -> str:
    parser = cli_command_tree_module._build_parser()
    stdout_buffer = StringIO()

    with pytest.raises(SystemExit) as captured_exit:
        with redirect_stdout(stdout_buffer):
            parser.parse_args([*arguments, "--help"])

    assert captured_exit.value.code == 0
    return stdout_buffer.getvalue()
