from argparse import ArgumentParser

from .calibration import _build_calibration_parser
from .configuration import _parse_cli_arguments as _parse_cli_arguments_with_parser
from .development import _build_developer_parser
from .hourly_factors import _build_hourly_factors_parser
from .model_workflows import (
    _build_fit_and_simulate_parser,
    _build_fit_parser,
    _build_model_simulate_parser,
)
from .presets import _build_presets_parser
from .pricing import _build_pricing_parser
from .seasonality import _build_seasonality_parser
from .simulation import _build_simulation_parser
from .stylized_facts import _build_stylized_facts_parser
from ... import __version__


def _build_parser() -> ArgumentParser:
    parser: ArgumentParser = ArgumentParser(
        prog="janiform",
        description="Command line interface for Janiform workflows and developer utilities.",
    )
    parser.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    parser.set_defaults(command=None, handler=None, help_target=parser)

    subparsers = parser.add_subparsers(dest="command")

    _build_presets_parser(subparsers)
    _build_fit_parser(subparsers)
    _build_model_simulate_parser(subparsers)
    _build_fit_and_simulate_parser(subparsers)
    _build_simulation_parser(subparsers)
    _build_seasonality_parser(subparsers)
    _build_hourly_factors_parser(subparsers)
    _build_calibration_parser(subparsers)
    _build_pricing_parser(subparsers)
    _build_stylized_facts_parser(subparsers)
    _build_developer_parser(subparsers)

    return parser


def _parse_cli_arguments(
    arguments: list[str] | None = None,
    *,
    environment: dict[str, str] | None = None,
):
    parser = _build_parser()
    return _parse_cli_arguments_with_parser(parser, arguments, environment=environment)


__all__ = [
    "_build_parser",
    "_parse_cli_arguments",
]
