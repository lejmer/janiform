from collections.abc import Mapping
import sys
from typing import Optional, cast

import click

from .command_runtime import JaniformGroup
from .commands.development import _build_development_group
from .commands.hourly_factors import _build_hourly_factors_group
from .commands.model_workflows import _build_fit_group, _build_simulate_group
from .commands.presets import _build_presets_group
from .commands.pricing import _build_pricing_group
from .commands.seasonality import _build_seasonality_group
from .commands.stylized_facts import _build_stylized_facts_group
from ._constants import CliExitCode
from .. import __version__


def _build_app(*, environment: Mapping[str, str] | None = None) -> click.Command:
    @click.group(
        cls=JaniformGroup,
        command_path=(),
        environment=environment,
        invoke_without_command=True,
        help=(
            "Primary workflows are the fit and simulate commands.\n\n"
            "\b\n"
            "Use the fit command to fit a Janiform model from market data.\n"
            "Use the simulate command to simulate from a persisted Janiform model.\n\n"
            "\b\n"
            "See also:\n"
            "  janiform fit --help\n"
            "  janiform simulate --help"
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
        _build_presets_group,
        _build_seasonality_group,
        _build_hourly_factors_group,
        _build_pricing_group,
        _build_stylized_facts_group,
        _build_development_group,
    ):
        root.add_command(group_builder(environment=environment))

    return root


def _invoke_application(
    arguments: list[str] | None = None,
    *,
    environment: Mapping[str, str] | None = None,
) -> int:
    effective_arguments = list(sys.argv[1:] if arguments is None else arguments)
    result = _invoke_click_command(_build_app(environment=environment), effective_arguments)
    return int(CliExitCode.SUCCESS if result is None else result)


def _invoke_click_command(command: click.Command, arguments: list[str]) -> int | None:
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


__all__ = [
    "_build_app",
    "_invoke_application",
]
