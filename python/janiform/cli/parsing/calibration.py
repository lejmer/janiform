from argparse import _SubParsersAction

from ..commands.calibration import (
    _run_calibration_market_data_command,
    _run_calibration_staged_command,
)
from .shared import _add_market_data_calibration_arguments, _add_staged_calibration_arguments


def _build_calibration_parser(subparsers: _SubParsersAction) -> None:
    calibration_parser = subparsers.add_parser(
        "calibration",
        help="Advanced calibration request and staged execution workflows.",
        description="Advanced calibration request and staged execution workflows.",
    )
    calibration_parser.set_defaults(help_target=calibration_parser)

    calibration_subparsers = calibration_parser.add_subparsers(dest="calibration_command")

    market_data_parser = calibration_subparsers.add_parser(
        "market-data",
        help="Market-data calibration request workflow.",
        description=(
            "Market-data calibration request workflow. This command validates file-driven "
            "inputs and emits a user-facing market-data calibration summary with "
            "preprocessing diagnostics and provenance."
        ),
    )
    market_data_parser.set_defaults(
        handler=_run_calibration_market_data_command,
        help_target=market_data_parser,
    )

    _add_market_data_calibration_arguments(market_data_parser)

    staged_parser = calibration_subparsers.add_parser(
        "staged",
        help="Staged calibration workflow.",
        description="Staged calibration workflow.",
    )
    staged_parser.set_defaults(
        handler=_run_calibration_staged_command,
        help_target=staged_parser,
    )

    _add_staged_calibration_arguments(staged_parser)


__all__ = ["_build_calibration_parser"]
