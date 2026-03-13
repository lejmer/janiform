from argparse import Namespace
from typing import Any, Optional, cast

from .._constants import (
    CLI_PRESET_LIST_SCHEMA_VERSION,
    CLI_PRESET_SHOW_SCHEMA_VERSION,
    CLI_PRESETS_EXPORT_ENTRYPOINT,
    CLI_PRESETS_EXPORT_INTERNAL_ERROR_ID,
    CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
    CLI_PRESETS_LIST_ENTRYPOINT,
    CLI_PRESETS_LIST_INTERNAL_ERROR_ID,
    CLI_PRESETS_LIST_VALIDATION_ERROR_ID,
    CLI_PRESETS_SHOW_ENTRYPOINT,
    CLI_PRESETS_SHOW_INTERNAL_ERROR_ID,
    CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
    CliExitCode,
)
from ..errors import _classify_generic_cli_command_error
from ..paths import resolve_repository_root
from ..rendering import (
    _render_presets_export_human_output,
    _render_presets_list_human_output,
    _render_presets_show_human_output,
    _write_cli_error,
    _write_command_output,
)
from ...calibration.presets import PresetRegistryScope
from ...errors import JaniformError
from .model_workflows import (
    _build_cli_preset_registry,
    _parse_cli_preset_category,
    _parse_cli_preset_scope,
    _resolve_named_preset_scope,
    _resolve_preset_artifact,
)
from .shared import _require_non_empty_cli_string


def _run_presets_list_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()

    try:
        registry = _build_cli_preset_registry(
            parsed_arguments,
            repository_root=repository_root,
            entrypoint=CLI_PRESETS_LIST_ENTRYPOINT,
            error_id=CLI_PRESETS_LIST_VALIDATION_ERROR_ID,
        )
        category = _parse_cli_preset_category(cast(Optional[str], getattr(parsed_arguments, "category", None)))
        scope = _parse_cli_preset_scope(cast(Optional[str], getattr(parsed_arguments, "scope", None)))
        include_built_in = scope in (None, PresetRegistryScope.BUILT_IN)
        include_user_registered = scope in (None, PresetRegistryScope.USER_REGISTERED)
        entries = registry.list_presets(
            category=category,
            include_built_in=include_built_in,
            include_user_registered=include_user_registered,
        )
        payload = {
            "schema_version": CLI_PRESET_LIST_SCHEMA_VERSION,
            "command": CLI_PRESETS_LIST_ENTRYPOINT,
            "category_filter": None if category is None else category.value,
            "scope_filter": None if scope is None else scope.value,
            "preset_count": len(entries),
            "presets": [entry.to_payload() for entry in entries],
        }
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_generic_cli_command_error(
                e,
                entrypoint=CLI_PRESETS_LIST_ENTRYPOINT,
                validation_error_id=CLI_PRESETS_LIST_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRESETS_LIST_INTERNAL_ERROR_ID,
                file_field_path="user_preset_json",
                document_field_path="user_preset_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_list_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_presets_show_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()

    try:
        registry = _build_cli_preset_registry(
            parsed_arguments,
            repository_root=repository_root,
            entrypoint=CLI_PRESETS_SHOW_ENTRYPOINT,
            error_id=CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
        )
        category = _parse_cli_preset_category(cast(str, parsed_arguments.category))
        preset_name = _require_non_empty_cli_string(
            cast(str, parsed_arguments.preset_name),
            field_path="preset_name",
            error_id=CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
            entrypoint=CLI_PRESETS_SHOW_ENTRYPOINT,
        )
        registry_scope = _resolve_named_preset_scope(
            registry=registry,
            category=category,
            preset_name=preset_name,
            raw_scope=cast(Optional[str], getattr(parsed_arguments, "scope", None)),
        )
        artifact = _resolve_preset_artifact(
            registry=registry,
            category=category,
            preset_name=preset_name,
            registry_scope=registry_scope,
        )
        payload = {
            "schema_version": CLI_PRESET_SHOW_SCHEMA_VERSION,
            "command": CLI_PRESETS_SHOW_ENTRYPOINT,
            "requested_category": category.value,
            "requested_scope": registry_scope.value,
            "preset_name": preset_name,
            "artifact": artifact.to_payload(),
        }
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_generic_cli_command_error(
                e,
                entrypoint=CLI_PRESETS_SHOW_ENTRYPOINT,
                validation_error_id=CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRESETS_SHOW_INTERNAL_ERROR_ID,
                file_field_path="user_preset_json",
                document_field_path="user_preset_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_show_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_presets_export_command(parsed_arguments: Namespace) -> int:
    repository_root = resolve_repository_root()

    try:
        registry = _build_cli_preset_registry(
            parsed_arguments,
            repository_root=repository_root,
            entrypoint=CLI_PRESETS_EXPORT_ENTRYPOINT,
            error_id=CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
        )
        category = _parse_cli_preset_category(cast(str, parsed_arguments.category))
        preset_name = _require_non_empty_cli_string(
            cast(str, parsed_arguments.preset_name),
            field_path="preset_name",
            error_id=CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
            entrypoint=CLI_PRESETS_EXPORT_ENTRYPOINT,
        )
        registry_scope = _resolve_named_preset_scope(
            registry=registry,
            category=category,
            preset_name=preset_name,
            raw_scope=cast(Optional[str], getattr(parsed_arguments, "scope", None)),
        )
        export_payload = registry.export_resolved_preset(
            category=category,
            preset_name=preset_name,
            registry_scope=registry_scope,
        ).to_payload()
        payload = {**export_payload, "command": CLI_PRESETS_EXPORT_ENTRYPOINT}
    except JaniformError as e:
        return _write_cli_error(e)
    except Exception as e:
        return _write_cli_error(
            _classify_generic_cli_command_error(
                e,
                entrypoint=CLI_PRESETS_EXPORT_ENTRYPOINT,
                validation_error_id=CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
                internal_error_id=CLI_PRESETS_EXPORT_INTERNAL_ERROR_ID,
                file_field_path="user_preset_json",
                document_field_path="user_preset_document",
            )
        )

    _write_command_output(
        parsed_arguments,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_export_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_presets_export_command",
    "_run_presets_list_command",
    "_run_presets_show_command",
]
