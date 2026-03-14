from typing import cast

import click

from janiform.cli.command_tree import _build_app
from tests.cli_test_support import render_help


def test_cli_top_level_help_lists_primary_and_developer_namespaces() -> None:
    help_output = _render_help([])

    assert "fit" in help_output
    assert "simulate" in help_output
    assert "presets" in help_output
    assert "seasonality" in help_output
    assert "hourly-factors" in help_output
    assert "pricing" in help_output
    assert "stylized-facts" in help_output
    assert "dev" in help_output


def test_fit_help_lists_default_and_explicit_workflows() -> None:
    help_output = _render_help(["fit"])

    assert "plan" in help_output
    assert "staged" in help_output


def test_simulate_help_lists_seeded_tick_workflow() -> None:
    help_output = _render_help(["simulate"])

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


def test_click_app_exposes_the_expected_live_command_tree() -> None:
    root_command = _build_app(environment={})

    assert isinstance(root_command, click.Group)
    assert tuple(root_command.commands) == (
        "fit",
        "simulate",
        "presets",
        "seasonality",
        "hourly-factors",
        "pricing",
        "stylized-facts",
        "dev",
    )

    fit_group = root_command.commands["fit"]
    simulate_group = root_command.commands["simulate"]
    dev_group = root_command.commands["dev"]

    assert isinstance(fit_group, click.Group)
    assert isinstance(simulate_group, click.Group)
    assert isinstance(dev_group, click.Group)
    assert tuple(fit_group.commands) == ("inspect", "plan", "staged")
    assert tuple(simulate_group.commands) == ("coarse-interval", "seeded-ticks")
    assert tuple(cast(click.Group, dev_group.commands["regression"]).commands) == (
        "deterministic",
        "open-closed-gap-leakage",
    )


def _render_help(arguments: list[str]) -> str:
    return render_help(arguments)
