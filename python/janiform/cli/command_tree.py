from collections.abc import Mapping
import sys
from typing import Optional, cast

import click

from .command_runtime import JaniformGroup
from .commands.development import _build_development_group
from .commands.hourly_factors import _build_hourly_factors_group
from .commands.model_workflows import _build_fit_group, _build_simulate_group
from .commands.presets import _build_presets_group
from .commands.pricing import _build_price_command
from .commands.seasonality import _build_seasonality_group
from .commands.stylized_facts import _build_stylized_facts_group
from ._constants import CliExitCode
from .. import __version__


def _build_app(*, environment: Optional[Mapping[str, str]] = None) -> click.Command:
    @click.group(
        cls=JaniformGroup,
        command_path=(),
        environment=environment,
        invoke_without_command=True,
        help=(
            "Primary workflows are the fit, simulate, and price commands.\n\n"
            "\b\n"
            "Use the fit command to fit a Janiform model from market data.\n"
            "Use the simulate command to simulate from a persisted Janiform model.\n\n"
            "Use the price command to price options from a persisted Janiform model.\n\n"
            "\b\n"
            "See also:\n"
            "  janiform fit --help\n"
            "  janiform simulate --help\n"
            "  janiform price --help"
        ),
    )
    @click.version_option(version=__version__)
    @click.pass_context
    def root(ctx: click.Context) -> None:
        if ctx.invoked_subcommand is None:
            click.echo(ctx.get_help())

    for group_builder in (
        _build_fit_group,
        _build_simulate_group,
        _build_price_command,
        _build_presets_group,
        _build_seasonality_group,
        _build_hourly_factors_group,
        _build_stylized_facts_group,
        _build_development_group,
    ):
        root.add_command(group_builder(environment=environment))

    return root


def _invoke_application(
    arguments: Optional[list[str]] = None,
    *,
    environment: Optional[Mapping[str, str]] = None,
) -> int:
    effective_arguments = _rewrite_beginner_fit_arguments(
        list(sys.argv[1:] if arguments is None else arguments)
    )
    result = _invoke_click_command(_build_app(environment=environment), effective_arguments)
    return int(CliExitCode.SUCCESS if result is None else result)


def _invoke_click_command(command: click.Command, arguments: list[str]) -> Optional[int]:
    try:
        return cast(
            Optional[int],
            command.main(
                args=arguments,
                prog_name="janiform",
                standalone_mode=False,
            ),
        )
    except click.exceptions.Exit as e:
        raise SystemExit(e.exit_code) from e
    except click.ClickException as e:
        e.show()
        raise SystemExit(e.exit_code) from e


def _rewrite_beginner_fit_arguments(arguments: list[str]) -> list[str]:
    if len(arguments) < 2 or arguments[0] != "fit":
        return arguments

    if arguments[1] == "inspect":
        if len(arguments) >= 3 and not arguments[2].startswith("-"):
            return [
                "fit",
                "inspect",
                "--tick-data-path",
                arguments[2],
                *arguments[3:],
            ]

        return arguments

    if arguments[1] in {"plan", "staged"} or arguments[1].startswith("-"):
        return arguments

    return [
        "fit",
        "--tick-data-path",
        arguments[1],
        *arguments[2:],
    ]


__all__ = [
    "_build_app",
    "_invoke_application",
]
