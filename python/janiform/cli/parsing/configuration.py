import json
import os
import sys
from argparse import ArgumentParser, Namespace
from collections.abc import Mapping
from pathlib import Path
from typing import Any, NoReturn, cast

from .._constants import (
    CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
    CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
    CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
    CLI_COMMAND_CONFIG_SCHEMA_VERSION,
    CLI_CONFIG_JSON_ENV_VAR,
    CLI_OUTPUT_ENV_VAR,
    CLI_PRIMARY_COMMAND_IDENTIFIERS,
    CLI_PRIMARY_COMMAND_TREE,
)
from ..documents import _load_json_document
from ..errors import _cli_validation_error
from ..paths import _resolve_path_from_repository_root, resolve_repository_root


def _parse_cli_arguments(
    parser: ArgumentParser,
    arguments: list[str] | None = None,
    *,
    environment: Mapping[str, str] | None = None,
) -> Namespace:
    raw_arguments = list(sys.argv[1:] if arguments is None else arguments)

    resolved_arguments = _apply_cli_argument_defaults(
        parser,
        raw_arguments,
        environment=os.environ if environment is None else environment,
    )

    return parser.parse_args(resolved_arguments)


def _apply_cli_argument_defaults(
    parser: ArgumentParser,
    raw_arguments: list[str],
    *,
    environment: Mapping[str, str],
) -> list[str]:
    command_path = _extract_primary_command_path(raw_arguments)

    if command_path not in CLI_PRIMARY_COMMAND_IDENTIFIERS:
        return raw_arguments

    command_parser = _resolve_command_parser(parser, command_path)

    if command_parser is None:
        return raw_arguments

    repository_root = resolve_repository_root()
    config_path = _resolve_command_config_path(
        command_path=command_path,
        raw_arguments=raw_arguments,
        environment=environment,
        repository_root=repository_root,
    )
    config_arguments = (
        _build_command_config_argument_vector(
            command_parser,
            command_path=command_path,
            config_path=config_path,
        )
        if config_path is not None
        else []
    )
    environment_arguments = _build_environment_argument_vector(
        command_path=command_path,
        environment=environment,
    )

    if not config_arguments and not environment_arguments:
        return raw_arguments

    return [
        *raw_arguments[: len(command_path)],
        *config_arguments,
        *environment_arguments,
        *raw_arguments[len(command_path) :],
    ]


def _extract_primary_command_path(raw_arguments: list[str]) -> tuple[str, ...]:
    command_path: list[str] = []
    subtree: dict[str, dict[str, Any]] = CLI_PRIMARY_COMMAND_TREE

    for raw_argument in raw_arguments:
        if raw_argument.startswith("-") or raw_argument not in subtree:
            break

        command_path.append(raw_argument)
        subtree = subtree[raw_argument]

    return tuple(command_path)


def _resolve_command_parser(
    parser: ArgumentParser,
    command_path: tuple[str, ...],
) -> ArgumentParser | None:
    current_parser = parser

    for command_segment in command_path:
        next_parser: ArgumentParser | None = None

        for action in current_parser._actions:
            action_choices = getattr(action, "choices", None)

            if isinstance(action_choices, dict) and command_segment in action_choices:
                next_parser = cast(ArgumentParser, action_choices[command_segment])
                break

        if next_parser is None:
            return None

        current_parser = next_parser

    return current_parser


def _resolve_command_config_path(
    *,
    command_path: tuple[str, ...],
    raw_arguments: list[str],
    environment: Mapping[str, str],
    repository_root: Path,
) -> Path | None:
    explicit_config_path = _extract_explicit_option_value(
        raw_arguments[len(command_path) :],
        flag_name="--config-json",
    )

    if explicit_config_path is not None:
        return _resolve_path_from_repository_root(repository_root, Path(explicit_config_path))

    environment_config_path = environment.get(CLI_CONFIG_JSON_ENV_VAR)

    if environment_config_path is None:
        return None

    if not environment_config_path.strip():
        _raise_cli_argument_resolution_error(
            field_path=CLI_CONFIG_JSON_ENV_VAR,
            message="environment variable must be non-empty when provided",
        )

    return _resolve_path_from_repository_root(repository_root, Path(environment_config_path))


def _extract_explicit_option_value(arguments: list[str], *, flag_name: str) -> str | None:
    explicit_value: str | None = None
    argument_index = 0

    while argument_index < len(arguments):
        current_argument = arguments[argument_index]

        if current_argument != flag_name:
            argument_index += 1
            continue

        next_index = argument_index + 1

        if next_index >= len(arguments):
            _raise_cli_argument_resolution_error(
                field_path=flag_name.lstrip("-").replace("-", "_"),
                message="flag requires a value",
            )

        explicit_value = arguments[next_index]
        argument_index += 2

    return explicit_value


def _build_command_config_argument_vector(
    command_parser: ArgumentParser,
    *,
    command_path: tuple[str, ...],
    config_path: Path,
) -> list[str]:
    config_document = _load_cli_command_config_document(config_path)
    expected_command = CLI_PRIMARY_COMMAND_IDENTIFIERS[command_path]
    configured_command = config_document.get("command")

    if configured_command != expected_command:
        _raise_cli_argument_resolution_error(
            field_path="command",
            message=f"must be {expected_command!r} for this command",
            context={"config_path": str(config_path)},
        )

    raw_arguments = config_document.get("arguments")

    if not isinstance(raw_arguments, dict):
        _raise_cli_argument_resolution_error(
            field_path="arguments",
            message="must be an object",
            context={"config_path": str(config_path)},
        )

    action_by_flag = _build_configurable_action_map(command_parser)
    argument_vector: list[str] = []

    for raw_flag_name, raw_value in raw_arguments.items():
        if not isinstance(raw_flag_name, str):
            _raise_cli_argument_resolution_error(
                field_path="arguments",
                message="argument keys must be strings",
                context={"config_path": str(config_path)},
            )

        action = action_by_flag.get(raw_flag_name)

        if action is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="is not a supported command-config argument",
                context={"config_path": str(config_path)},
            )

        option_string = _select_long_option_string(
            action,
            field_path=f"arguments.{raw_flag_name}",
        )

        if action.nargs == 0:
            if not isinstance(raw_value, bool):
                _raise_cli_argument_resolution_error(
                    field_path=f"arguments.{raw_flag_name}",
                    message="must be a boolean for flag-style arguments",
                    context={"config_path": str(config_path)},
                )

            if raw_value:
                argument_vector.append(option_string)

            continue

        if isinstance(raw_value, bool) or raw_value is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="must be a scalar value",
                context={"config_path": str(config_path)},
            )

        argument_vector.extend((option_string, str(raw_value)))

    return argument_vector


def _load_cli_command_config_document(config_path: Path) -> dict[str, Any]:
    try:
        config_document = _load_json_document(config_path, "command config")
    except FileNotFoundError:
        _raise_cli_argument_resolution_error(
            field_path="config_json",
            message="command config file could not be found",
            context={"path": str(config_path)},
        )
    except OSError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_json",
            message=f"command config file could not be read: {e}",
            context={"path": str(config_path)},
        )
    except json.JSONDecodeError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_document",
            message="command config file must contain valid JSON",
            context={
                "path": str(config_path),
                "line_number": e.lineno,
                "column_number": e.colno,
            },
        )
    except ValueError as e:
        _raise_cli_argument_resolution_error(
            field_path="config_document",
            message=str(e),
            context={"path": str(config_path)},
        )

    schema_version = config_document.get("schema_version")

    if schema_version != CLI_COMMAND_CONFIG_SCHEMA_VERSION:
        _raise_cli_argument_resolution_error(
            field_path="schema_version",
            message=f"must be {CLI_COMMAND_CONFIG_SCHEMA_VERSION!r}",
            context={"config_path": str(config_path)},
        )

    return config_document


def _build_configurable_action_map(command_parser: ArgumentParser) -> dict[str, Any]:
    action_by_flag: dict[str, Any] = {}

    for action in command_parser._actions:
        for option_string in cast(list[str], action.option_strings):
            if option_string.startswith("--"):
                flag_name = option_string[2:]

                if flag_name != "config-json":
                    action_by_flag[flag_name] = action

    return action_by_flag


def _select_long_option_string(action: Any, *, field_path: str) -> str:
    for option_string in cast(list[str], action.option_strings):
        if option_string.startswith("--"):
            return option_string

    _raise_cli_argument_resolution_error(
        field_path=field_path,
        message="is not configurable through command config",
    )


def _build_environment_argument_vector(
    *,
    command_path: tuple[str, ...],
    environment: Mapping[str, str],
) -> list[str]:
    environment_argument_vector: list[str] = []
    output_mode = environment.get(CLI_OUTPUT_ENV_VAR)

    if output_mode is not None:
        if not output_mode.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_OUTPUT_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )

        environment_argument_vector.extend(("--output", output_mode))

    if command_path[:1] != ("calibration",):
        return environment_argument_vector

    execution_mode = environment.get(CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR)

    if execution_mode is not None:
        if not execution_mode.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )

        environment_argument_vector.extend(("--execution-mode", execution_mode))

    disk_checkpoint_directory = environment.get(
        CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
    )

    if disk_checkpoint_directory is not None:
        if not disk_checkpoint_directory.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )

        environment_argument_vector.extend(
            ("--disk-checkpoint-directory", disk_checkpoint_directory),
        )

    return environment_argument_vector


def _raise_cli_argument_resolution_error(
    *,
    field_path: str,
    message: str,
    context: dict[str, Any] | None = None,
) -> NoReturn:
    raise _cli_validation_error(
        field_path,
        message,
        error_id=CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
        entrypoint=CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
        context=context,
    )


__all__ = [
    "_apply_cli_argument_defaults",
    "_build_command_config_argument_vector",
    "_build_configurable_action_map",
    "_build_environment_argument_vector",
    "_extract_explicit_option_value",
    "_extract_primary_command_path",
    "_load_cli_command_config_document",
    "_parse_cli_arguments",
    "_resolve_command_config_path",
    "_resolve_command_parser",
]
