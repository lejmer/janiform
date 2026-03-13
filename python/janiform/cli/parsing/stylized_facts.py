from argparse import _SubParsersAction

from .._constants import CLI_CALIBRATION_EXECUTION_MODE_CHOICES, CliOutputMode
from ..commands.stylized_facts import _run_stylized_facts_horizon_comparison_command
from ...calibration import CalibrationExecutionMode
from .shared import (
    _add_command_config_argument,
    _add_optional_path_argument,
    _add_output_mode_argument,
    _add_required_json_path_argument,
)


def _build_stylized_facts_parser(subparsers: _SubParsersAction) -> None:
    parser = subparsers.add_parser(
        "stylized-facts",
        help="Stylized-fact reporting workflows.",
        description="Stylized-fact reporting workflows.",
    )
    parser.set_defaults(help_target=parser)

    subcommands = parser.add_subparsers(dest="stylized_facts_command")

    horizon_parser = subcommands.add_parser(
        "horizon-comparison",
        help="Generate a stylized-fact horizon-comparison report bundle.",
        description=(
            "Generate a stylized-fact horizon-comparison report bundle from a metric "
            "catalog, a comparison request, and optional artifact-export settings."
        ),
    )
    horizon_parser.set_defaults(
        handler=_run_stylized_facts_horizon_comparison_command,
        help_target=horizon_parser,
    )

    _add_command_config_argument(horizon_parser)
    _add_output_mode_argument(horizon_parser, default_output_mode=CliOutputMode.JSON)
    _add_required_json_path_argument(
        horizon_parser,
        flag_name="--metric-catalog-json",
        contract_label="a stylized-fact metric catalog JSON object",
    )
    _add_required_json_path_argument(
        horizon_parser,
        flag_name="--comparison-request-json",
        contract_label="a stylized-fact horizon-comparison request JSON object",
    )
    _add_optional_path_argument(
        parser=horizon_parser,
        flag_name="--artifact-export-json",
        help_text=(
            "Optional path to a stylized-fact artifact-export config JSON object. This "
            "payload is passed through the public workflow validation path and requires "
            "--execution-mode disk."
        ),
    )
    horizon_parser.add_argument(
        "--execution-mode",
        choices=CLI_CALIBRATION_EXECUTION_MODE_CHOICES,
        default=CalibrationExecutionMode.MEMORY.value,
        help=(
            "Execution mode passed through to janiform.stylized_facts. Default: memory. "
            "Use disk when artifact export is requested."
        ),
    )


__all__ = ["_build_stylized_facts_parser"]
