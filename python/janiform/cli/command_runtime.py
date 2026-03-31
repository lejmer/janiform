"""Internal Click runtime helpers used by the supported CLI commands."""

from collections.abc import Callable, Mapping
from pathlib import Path
from typing import Any, Optional, cast

import click

from ._constants import (
    CLI_COMMAND_CONFIG_HELP,
    CLI_DIRECTORY_METAVAR,
    CLI_OUTPUT_MODE_CHOICES,
    CLI_OUTPUT_MODE_HELP,
    CLI_PATH_METAVAR,
    CLI_PROGRESS_MODE_CHOICES,
    CLI_PRESET_SCOPE_CHOICES,
    CLI_REPOSITORY_RELATIVE_PATH_POLICY,
    CLI_PRIMARY_COMMAND_IDENTIFIERS,
    CliProgressMode,
    CliOutputMode,
)
from .configuration import build_click_default_map
from .context import CliCommandContext
from .paths import resolve_repository_root

type CliHandler = Callable[..., int]

_UNSET_OPTION_DEFAULT = object()


class _JaniformCommandMixin:
    def __init__(
        self,
        *args,
        command_path: Optional[tuple[str, ...]] = None,
        environment: Optional[Mapping[str, str]] = None,
        **kwargs,
    ) -> None:
        self._command_path = command_path
        self._environment = environment

        super().__init__(*args, **kwargs)

    def make_context(
        self,
        info_name: Optional[str],
        args: list[str],
        parent: Optional[click.Context] = None,
        **extra: Any,
    ) -> click.Context:
        if self._command_path is not None and not _arguments_target_known_subcommand(self, args):
            command_default_map = build_click_default_map(
                self,
                command_path=self._command_path,
                arguments=args,
                environment=self._environment,
            )
            inherited_default_map = cast(dict[str, Any], extra.get("default_map") or {})
            extra["default_map"] = {**inherited_default_map, **command_default_map}

        return super().make_context(info_name, args, parent=parent, **extra)


class JaniformCommand(_JaniformCommandMixin, click.Command):
    pass


class JaniformGroup(_JaniformCommandMixin, click.Group):
    pass


def build_argument(
    argument_name: str,
    *,
    choice_options: Optional[tuple[str, ...]] = None,
    metavar: Optional[str] = None,
) -> click.Argument:
    argument_type: click.ParamType | type[Any]
    if choice_options is None:
        argument_type = str
    else:
        argument_type = click.Choice(choice_options, case_sensitive=True)
        metavar = metavar or "{" + ",".join(choice_options) + "}"

    return click.Argument(
        [argument_name],
        metavar=metavar or argument_name.upper(),
        type=argument_type,
    )


def build_cli_context(ctx: click.Context, *, command_path: tuple[str, ...]) -> CliCommandContext:
    return CliCommandContext(
        command_path=command_path,
        command_identifier=CLI_PRIMARY_COMMAND_IDENTIFIERS.get(command_path),
        default_map=dict(ctx.default_map or {}),
        environment=cast(
            Mapping[str, str],
            ctx.find_root().command._environment
            if isinstance(ctx.find_root().command, _JaniformCommandMixin)
            else {},
        ),
        repository_root=resolve_repository_root(),
    )


def command_config_option() -> click.Option:
    return command_option(
        "--config-json",
        help_text=_path_help_text(CLI_COMMAND_CONFIG_HELP),
        path_type=True,
    )


def command_option(
    flag_name: str,
    *,
    help_text: str,
    choice_options: Optional[tuple[str, ...]] = None,
    default: Any = _UNSET_OPTION_DEFAULT,
    directory: bool = False,
    is_flag: bool = False,
    multiple: bool = False,
    path_type: bool = False,
    required: bool = False,
    value_type: Optional[click.ParamType | type[Any]] = None,
    metavar: Optional[str] = None,
    show_default: bool = False,
) -> click.Option:
    option_type: Optional[click.ParamType | type[Any]] = value_type

    if choice_options is not None:
        option_type = click.Choice(choice_options, case_sensitive=True)
        metavar = metavar or "{" + ",".join(choice_options) + "}"
    elif path_type:
        option_type = click.Path(
            exists=False,
            dir_okay=directory,
            file_okay=not directory,
            path_type=Path,
        )
        metavar = metavar or (CLI_DIRECTORY_METAVAR if directory else CLI_PATH_METAVAR)
    elif option_type is None and not is_flag:
        option_type = str

    option_arguments: dict[str, Any] = {
        "help": help_text,
        "is_flag": is_flag,
        "metavar": metavar or _option_metavar(flag_name),
        "multiple": multiple,
        "required": required,
        "show_default": show_default,
        "type": option_type,
    }

    if default is not _UNSET_OPTION_DEFAULT:
        option_arguments["default"] = default

    return click.Option([flag_name], **option_arguments)


def group_help_callback(ctx: click.Context) -> None:
    if ctx.invoked_subcommand is None:
        click.echo(ctx.get_help())


def invoke_command_handler(
    ctx: click.Context,
    *,
    handler: CliHandler,
    command_path: tuple[str, ...],
    command_arguments: dict[str, Any],
    exclusivity_groups: tuple[tuple[str, ...], ...] = (),
    required_one_of_groups: tuple[tuple[str, ...], ...] = (),
) -> int:
    _validate_option_groups(
        command_arguments,
        exclusivity_groups=exclusivity_groups,
        required_one_of_groups=required_one_of_groups,
    )

    cli_context = build_cli_context(ctx, command_path=command_path)
    return handler(cli_context=cli_context, **command_arguments)


def invoke_group_default_handler(
    ctx: click.Context,
    *,
    handler: CliHandler,
    command_path: tuple[str, ...],
    command_arguments: dict[str, Any],
    required_option_names: tuple[str, ...],
    exclusivity_groups: tuple[tuple[str, ...], ...] = (),
    required_one_of_groups: tuple[tuple[str, ...], ...] = (),
) -> Optional[int]:
    if ctx.invoked_subcommand is not None:
        return None

    _validate_required_options(
        ctx,
        values=command_arguments,
        required_option_names=required_option_names,
    )
    _validate_option_groups(
        command_arguments,
        exclusivity_groups=exclusivity_groups,
        required_one_of_groups=required_one_of_groups,
    )

    cli_context = build_cli_context(ctx, command_path=command_path)
    return handler(cli_context=cli_context, **command_arguments)


def optional_path_option(
    flag_name: str,
    *,
    help_text: str,
    directory: bool = False,
    default: Optional[Path] = None,
) -> click.Option:
    return command_option(
        flag_name,
        default=default,
        directory=directory,
        help_text=_path_help_text(help_text),
        path_type=True,
    )


def output_option(*, default_output_mode: CliOutputMode) -> click.Option:
    return command_option(
        "--output",
        choice_options=CLI_OUTPUT_MODE_CHOICES,
        default=default_output_mode.value,
        help_text=f"{CLI_OUTPUT_MODE_HELP} Default: {default_output_mode.value}.",
        show_default=False,
    )


def progress_option(*, default_progress_mode: CliProgressMode) -> click.Option:
    return command_option(
        "--progress",
        choice_options=CLI_PROGRESS_MODE_CHOICES,
        default=default_progress_mode.value,
        help_text=(
            "Progress rendering mode. Use human for an operator-oriented spinner and determinate "
            "status line, jsonl for structured progress events on stderr, none to disable "
            "progress output, or auto to follow the command output mode."
        ),
        show_default=False,
    )


def preset_scope_option(
    flag_name: str,
    *,
    help_text: str,
    include_auto: bool = False,
) -> click.Option:
    choices = CLI_PRESET_SCOPE_CHOICES if include_auto else CLI_PRESET_SCOPE_CHOICES[1:]
    return command_option(
        flag_name,
        choice_options=choices,
        default="auto" if include_auto else None,
        help_text=help_text,
    )


def required_datetime_option(
    flag_name: str,
    *,
    help_text: str,
    required: bool = True,
) -> click.Option:
    return command_option(
        flag_name,
        help_text=f"{help_text} Use an ISO 8601 timezone-aware timestamp.",
        required=required,
    )


def required_json_path_option(
    flag_name: str,
    *,
    contract_label: str,
    semantic_note: Optional[str] = None,
) -> click.Option:
    help_segments: list[str] = [f"Path to {contract_label}.", CLI_REPOSITORY_RELATIVE_PATH_POLICY]
    if semantic_note is not None:
        help_segments.append(semantic_note)

    return command_option(
        flag_name,
        help_text=" ".join(help_segments),
        path_type=True,
        required=True,
    )


def user_preset_json_option() -> click.Option:
    return click.Option(
        ["--user-preset-json"],
        help=(
            "Optional preset artifact JSON to register in the in-memory user registry before "
            f"command resolution. {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"
        ),
        multiple=True,
        metavar=CLI_PATH_METAVAR,
        type=click.Path(exists=False, dir_okay=False, file_okay=True, path_type=Path),
    )


def _arguments_target_known_subcommand(command: click.Command, arguments: list[str]) -> bool:
    if not isinstance(command, click.Group):
        return False

    subcommand_names = set(command.list_commands(click.Context(command)))
    return any(argument in subcommand_names for argument in arguments)


def _find_option_parameter(command: click.Command, option_name: str) -> Optional[click.Option]:
    for parameter in command.params:
        if isinstance(parameter, click.Option) and parameter.name == option_name:
            return parameter

    return None


def _option_metavar(flag_name: str) -> str:
    return flag_name[2:].replace("-", "_").upper()


def _path_help_text(help_text: str) -> str:
    return f"{help_text} {CLI_REPOSITORY_RELATIVE_PATH_POLICY}"


def _validate_option_groups(
    values: dict[str, Any],
    *,
    exclusivity_groups: tuple[tuple[str, ...], ...],
    required_one_of_groups: tuple[tuple[str, ...], ...],
) -> None:
    for group in exclusivity_groups:
        selected_options = [option_name for option_name in group if values.get(option_name) not in (None, False, [], ())]
        if len(selected_options) > 1:
            option_flags = ", ".join("--" + option_name.replace("_", "-") for option_name in group)
            raise click.UsageError(f"options are mutually exclusive: {option_flags}")

    for group in required_one_of_groups:
        selected_options = [option_name for option_name in group if values.get(option_name) not in (None, False, [], ())]
        if len(selected_options) != 1:
            option_flags = ", ".join("--" + option_name.replace("_", "-") for option_name in group)
            raise click.UsageError(f"exactly one of {option_flags} is required")


def _validate_required_options(
    ctx: click.Context,
    *,
    values: dict[str, Any],
    required_option_names: tuple[str, ...],
) -> None:
    for option_name in required_option_names:
        if values.get(option_name) not in (None, [], ()):
            continue

        parameter = _find_option_parameter(ctx.command, option_name)
        if parameter is None:
            raise click.UsageError(f"missing required option --{option_name.replace('_', '-')}")

        raise click.MissingParameter(ctx=ctx, param=parameter)


__all__ = [
    "CliHandler",
    "JaniformCommand",
    "JaniformGroup",
    "build_argument",
    "build_cli_context",
    "command_config_option",
    "command_option",
    "group_help_callback",
    "invoke_command_handler",
    "invoke_group_default_handler",
    "optional_path_option",
    "output_option",
    "preset_scope_option",
    "required_datetime_option",
    "required_json_path_option",
    "user_preset_json_option",
]
