from argparse import _SubParsersAction

from .._constants import CLI_PRESET_CATEGORY_CHOICES, CliOutputMode
from ..commands.presets import (
    _run_presets_export_command,
    _run_presets_list_command,
    _run_presets_show_command,
)
from .shared import (
    _add_command_config_argument,
    _add_output_mode_argument,
    _add_preset_scope_argument,
    _add_user_preset_json_argument,
)


def _build_presets_parser(subparsers: _SubParsersAction) -> None:
    presets_parser = subparsers.add_parser(
        "presets",
        help="Inspect bundled and user-registered presets.",
        description="Inspect bundled and user-registered presets.",
    )
    presets_parser.set_defaults(help_target=presets_parser)

    presets_subparsers = presets_parser.add_subparsers(dest="presets_command")

    presets_list_parser = presets_subparsers.add_parser(
        "list",
        help="List available presets by category and registry scope.",
        description="List available presets by category and registry scope.",
    )
    presets_list_parser.set_defaults(
        handler=_run_presets_list_command,
        help_target=presets_list_parser,
    )

    _add_command_config_argument(presets_list_parser)
    _add_output_mode_argument(presets_list_parser, default_output_mode=CliOutputMode.JSON)
    _add_user_preset_json_argument(presets_list_parser)
    presets_list_parser.add_argument(
        "--category",
        choices=CLI_PRESET_CATEGORY_CHOICES,
        help="Optional preset category filter.",
    )
    _add_preset_scope_argument(
        presets_list_parser,
        flag_name="--scope",
        help_text="Registry scope filter. Default: auto.",
        include_auto=True,
    )

    for command_name, handler, help_text in (
        ("show", _run_presets_show_command, "Show a single preset artifact."),
        ("export", _run_presets_export_command, "Export a preset with nested dependencies."),
    ):
        command_parser = presets_subparsers.add_parser(
            command_name,
            help=help_text,
            description=help_text,
        )
        command_parser.set_defaults(handler=handler, help_target=command_parser)

        _add_command_config_argument(command_parser)
        _add_output_mode_argument(command_parser, default_output_mode=CliOutputMode.JSON)
        _add_user_preset_json_argument(command_parser)
        command_parser.add_argument(
            "category",
            choices=CLI_PRESET_CATEGORY_CHOICES,
            help="Preset category.",
        )
        command_parser.add_argument("preset_name", help="Preset name to inspect.")
        _add_preset_scope_argument(
            command_parser,
            flag_name="--scope",
            help_text="Registry scope for the requested preset. Default: auto.",
            include_auto=True,
        )


__all__ = ["_build_presets_parser"]
