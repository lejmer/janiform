from argparse import _SubParsersAction

from ..commands.model_workflows import (
    _run_fit_and_simulate_command,
    _run_fit_command,
    _run_model_simulate_command,
)
from .shared import (
    _add_model_workflow_fit_arguments,
    _add_model_workflow_simulate_arguments,
)


def _build_fit_parser(subparsers: _SubParsersAction) -> None:
    fit_parser = subparsers.add_parser(
        "fit",
        help="Fit a Janiform model from market data using named presets and session inference.",
        description=(
            "Fit a Janiform model from market data using named presets, explicit overrides, "
            "and automatic session inference when enough evidence is available."
        ),
    )
    fit_parser.set_defaults(handler=_run_fit_command, help_target=fit_parser)

    _add_model_workflow_fit_arguments(fit_parser)


def _build_model_simulate_parser(subparsers: _SubParsersAction) -> None:
    simulate_parser = subparsers.add_parser(
        "simulate",
        help="Simulate from a persisted JaniformModel.",
        description=(
            "Simulate from a persisted JaniformModel without rebuilding runtime contracts "
            "manually."
        ),
    )
    simulate_parser.set_defaults(
        handler=_run_model_simulate_command,
        help_target=simulate_parser,
    )

    _add_model_workflow_simulate_arguments(simulate_parser, require_model_path=True)


def _build_fit_and_simulate_parser(subparsers: _SubParsersAction) -> None:
    fit_and_simulate_parser = subparsers.add_parser(
        "fit-and-simulate",
        help="Fit a model and immediately simulate from it.",
        description=(
            "Fit a model and immediately simulate from it using the same high-level "
            "workflow surface."
        ),
    )
    fit_and_simulate_parser.set_defaults(
        handler=_run_fit_and_simulate_command,
        help_target=fit_and_simulate_parser,
    )

    _add_model_workflow_fit_arguments(fit_and_simulate_parser)
    _add_model_workflow_simulate_arguments(
        fit_and_simulate_parser,
        require_model_path=False,
        include_shared_arguments=False,
    )


__all__ = [
    "_build_fit_and_simulate_parser",
    "_build_fit_parser",
    "_build_model_simulate_parser",
]
