from argparse import _SubParsersAction

from .._constants import CliOutputMode
from ..commands.pricing import _run_pricing_vanilla_option_command
from .shared import (
    _add_command_config_argument,
    _add_output_mode_argument,
    _add_required_json_path_argument,
)


def _build_pricing_parser(subparsers: _SubParsersAction) -> None:
    pricing_parser = subparsers.add_parser(
        "pricing",
        help="Pricing workflows.",
        description="Pricing workflows.",
    )
    pricing_parser.set_defaults(help_target=pricing_parser)

    pricing_subparsers = pricing_parser.add_subparsers(dest="pricing_command")

    vanilla_parser = pricing_subparsers.add_parser(
        "vanilla-option",
        help="Vanilla option pricing workflow.",
        description="Vanilla option pricing workflow.",
    )
    vanilla_parser.set_defaults(
        handler=_run_pricing_vanilla_option_command,
        help_target=vanilla_parser,
    )

    _add_command_config_argument(vanilla_parser)
    _add_output_mode_argument(vanilla_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        vanilla_parser,
        flag_name="--execution-input-json",
        contract_label="a vanilla-option pricing execution-input JSON object",
        semantic_note="Interval factors keep known and surprise news intensity separate.",
    )


__all__ = ["_build_pricing_parser"]
