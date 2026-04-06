"""Internal command-config and environment-default resolution for the CLI."""

import json
import os
from collections.abc import Mapping
from pathlib import Path
from typing import Any, Optional

import click

from .constants import (
    CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
    CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
    CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
    CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
    CLI_COMMAND_CONFIG_SCHEMA_VERSION,
    CLI_CONFIG_JSON_ENV_VAR,
    CLI_OUTPUT_ENV_VAR,
    CLI_PRIMARY_COMMAND_IDENTIFIERS,
)
from .documents import load_json_document
from .errors import cli_validation_error
from .paths import resolve_path_from_repository_root, resolve_repository_root


def build_click_default_map(
    command: click.Command,
    *,
    command_path: tuple[str, ...],
    arguments: list[str],
    environment: Optional[Mapping[str, str]] = None,
) -> dict[str, Any]:
    effective_environment = os.environ if environment is None else environment

    command_identifier = CLI_PRIMARY_COMMAND_IDENTIFIERS.get(command_path)
    if command_identifier is None:
        return {}

    repository_root = resolve_repository_root()
    config_path = _resolve_command_config_path(
        command_path=command_path,
        raw_arguments=arguments,
        environment=effective_environment,
        repository_root=repository_root,
    )
    config_defaults = (
        _build_command_config_default_map(
            command,
            command_identifier=command_identifier,
            config_path=config_path,
        )
        if config_path is not None
        else {}
    )
    environment_defaults = _build_environment_default_map(
        command_path=command_path,
        environment=effective_environment,
    )

    return {**config_defaults, **environment_defaults}


def _resolve_command_config_path(
    *,
    command_path: tuple[str, ...],
    raw_arguments: list[str],
    environment: Mapping[str, str],
    repository_root: Path,
) -> Optional[Path]:
    explicit_config_path = _extract_explicit_option_value(
        raw_arguments,
        flag_name="--config-json",
    )

    if explicit_config_path is not None:
        return resolve_path_from_repository_root(repository_root, Path(explicit_config_path))

    environment_config_path = environment.get(CLI_CONFIG_JSON_ENV_VAR)

    if environment_config_path is None:
        return None

    if not environment_config_path.strip():
        _raise_cli_argument_resolution_error(
            field_path=CLI_CONFIG_JSON_ENV_VAR,
            message="environment variable must be non-empty when provided",
        )

    return resolve_path_from_repository_root(repository_root, Path(environment_config_path))


def _extract_explicit_option_value(arguments: list[str], *, flag_name: str) -> Optional[str]:
    explicit_value: Optional[str] = None
    argument_index = 0

    while argument_index < len(arguments):
        current_argument = arguments[argument_index]

        if current_argument.startswith(f"{flag_name}="):
            explicit_value = current_argument.split("=", 1)[1]
            argument_index += 1
            continue

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


def _build_command_config_default_map(
    command: click.Command,
    *,
    command_identifier: str,
    config_path: Path,
) -> dict[str, Any]:
    config_document = _load_cli_command_config_document(config_path)
    configured_command = config_document.get("command")

    if configured_command != command_identifier:
        _raise_cli_argument_resolution_error(
            field_path="command",
            message=f"must be {command_identifier!r} for this command",
            context={"config_path": str(config_path)},
        )

    raw_arguments = config_document.get("arguments")

    if not isinstance(raw_arguments, dict):
        _raise_cli_argument_resolution_error(
            field_path="arguments",
            message="must be an object",
            context={"config_path": str(config_path)},
        )

    parameter_by_flag = _build_click_parameter_map(command)
    default_map: dict[str, Any] = {}

    for raw_flag_name, raw_value in raw_arguments.items():
        if not isinstance(raw_flag_name, str):
            _raise_cli_argument_resolution_error(
                field_path="arguments",
                message="argument keys must be strings",
                context={"config_path": str(config_path)},
            )

        parameter = parameter_by_flag.get(raw_flag_name)

        if parameter is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="is not a supported command-config argument",
                context={"config_path": str(config_path)},
            )

        if parameter.multiple:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="does not support repeated values in command config",
                context={"config_path": str(config_path)},
            )

        if parameter.is_flag:
            if not isinstance(raw_value, bool):
                _raise_cli_argument_resolution_error(
                    field_path=f"arguments.{raw_flag_name}",
                    message="must be a boolean for flag-style arguments",
                    context={"config_path": str(config_path)},
                )

            default_map[parameter.name] = raw_value
            continue

        if isinstance(raw_value, bool) or raw_value is None:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message="must be a scalar value",
                context={"config_path": str(config_path)},
            )

        try:
            converted_value = parameter.type_cast_value(None, raw_value)
        except click.ClickException as e:
            _raise_cli_argument_resolution_error(
                field_path=f"arguments.{raw_flag_name}",
                message=str(e),
                context={"config_path": str(config_path)},
            )

        default_map[parameter.name] = converted_value

    return default_map


def _build_click_parameter_map(command: click.Command) -> dict[str, click.Option]:
    parameters_by_flag: dict[str, click.Option] = {}

    for parameter in command.params:
        if not isinstance(parameter, click.Option):
            continue

        for option_string in parameter.opts:
            if option_string.startswith("--"):
                flag_name = option_string[2:]
                if flag_name != "config-json":
                    parameters_by_flag[flag_name] = parameter

    return parameters_by_flag


def _load_cli_command_config_document(config_path: Path) -> dict[str, Any]:
    try:
        config_document = load_json_document(config_path, "command config")
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


def _build_environment_default_map(
    *,
    command_path: tuple[str, ...],
    environment: Mapping[str, str],
) -> dict[str, Any]:
    environment_default_map: dict[str, Any] = {}
    output_mode = environment.get(CLI_OUTPUT_ENV_VAR)

    if output_mode is not None:
        if not output_mode.strip():
            _raise_cli_argument_resolution_error(
                field_path=CLI_OUTPUT_ENV_VAR,
                message="environment variable must be non-empty when provided",
            )

        environment_default_map["output"] = output_mode

    if command_path in (("fit", "plan"), ("fit", "staged")):
        execution_mode = environment.get(CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR)

        if execution_mode is not None:
            if not execution_mode.strip():
                _raise_cli_argument_resolution_error(
                    field_path=CLI_CALIBRATION_EXECUTION_MODE_ENV_VAR,
                    message="environment variable must be non-empty when provided",
                )

            environment_default_map["execution_mode"] = execution_mode

        checkpoint_directory = environment.get(CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR)

        if checkpoint_directory is not None:
            if not checkpoint_directory.strip():
                _raise_cli_argument_resolution_error(
                    field_path=CLI_CALIBRATION_DISK_CHECKPOINT_DIRECTORY_ENV_VAR,
                    message="environment variable must be non-empty when provided",
                )

            environment_default_map["disk_checkpoint_directory"] = resolve_path_from_repository_root(
                resolve_repository_root(),
                Path(checkpoint_directory),
            )

    return environment_default_map


def _raise_cli_argument_resolution_error(
    *,
    field_path: str,
    message: str,
    context: Optional[dict[str, Any]] = None,
) -> None:
    raise cli_validation_error(
        field_path,
        message,
        error_id=CLI_ARGUMENT_RESOLUTION_VALIDATION_ERROR_ID,
        entrypoint=CLI_ARGUMENT_RESOLUTION_ENTRYPOINT,
        context=context,
    )
