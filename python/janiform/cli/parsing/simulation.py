from argparse import _SubParsersAction

from .._constants import CliOutputMode
from ..commands.simulation import (
    _run_simulation_coarse_interval_command,
    _run_simulation_seeded_ticks_command,
)
from .shared import (
    _add_command_config_argument,
    _add_output_mode_argument,
    _add_required_json_path_argument,
)


def _build_simulation_parser(subparsers: _SubParsersAction) -> None:
    simulation_parser = subparsers.add_parser(
        "simulation",
        help="Advanced explicit simulation contract workflows.",
        description="Advanced explicit simulation contract workflows.",
    )
    simulation_parser.set_defaults(help_target=simulation_parser)

    simulation_subparsers = simulation_parser.add_subparsers(dest="simulation_command")

    coarse_parser = simulation_subparsers.add_parser(
        "coarse-interval",
        help="Direct coarse-interval path simulation workflow.",
        description=(
            "Direct coarse-interval path simulation workflow. Use this command when you "
            "want the canonical coarse simulation path artifact at interval length Delta "
            "without replaying fine-scale seeded ticks."
        ),
    )
    coarse_parser.set_defaults(
        handler=_run_simulation_coarse_interval_command,
        help_target=coarse_parser,
    )

    _add_command_config_argument(coarse_parser)
    _add_output_mode_argument(coarse_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        coarse_parser,
        flag_name="--request-json",
        contract_label="a coarse-interval simulation request JSON object",
        semantic_note=(
            "Use coarse-interval simulation for direct Delta-spaced paths. Use "
            "seeded-ticks when you need a priced fine path or tick replay."
        ),
    )

    seeded_parser = simulation_subparsers.add_parser(
        "seeded-ticks",
        help="Seeded tick priced fine-path simulation workflow.",
        description=(
            "Seeded tick priced fine-path simulation workflow. Use this command when you "
            "need the canonical fine-scale priced path, its derived event replay, or "
            "direct news and return component realization rather than coarse-interval "
            "aggregation."
        ),
    )
    seeded_parser.set_defaults(
        handler=_run_simulation_seeded_ticks_command,
        help_target=seeded_parser,
    )

    _add_command_config_argument(seeded_parser)
    _add_output_mode_argument(seeded_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        seeded_parser,
        flag_name="--request-json",
        contract_label="a seeded-tick simulation request JSON object",
        semantic_note="The request summary must preserve open-jump versus closed-gap hourly factors.",
    )
    _add_required_json_path_argument(
        seeded_parser,
        flag_name="--calendar-registry-json",
        contract_label="a calendar registry JSON object used to rehydrate hourly factors",
    )


__all__ = ["_build_simulation_parser"]
