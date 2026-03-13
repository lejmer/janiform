from pathlib import Path
from typing import Any, Optional, cast

from ..context import CliCommandContext
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


def _run_presets_list_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    user_preset_json: tuple[Path, ...],
    category: Optional[str],
    scope: Optional[str],
    **_: object,
) -> int:
    try:
        registry = _build_cli_preset_registry(
            cli_context=cli_context,
            user_preset_json=user_preset_json,
            entrypoint=CLI_PRESETS_LIST_ENTRYPOINT,
            error_id=CLI_PRESETS_LIST_VALIDATION_ERROR_ID,
        )
        parsed_category = _parse_cli_preset_category(category)
        parsed_scope = _parse_cli_preset_scope(scope)
        include_built_in = parsed_scope in (None, PresetRegistryScope.BUILT_IN)
        include_user_registered = parsed_scope in (None, PresetRegistryScope.USER_REGISTERED)
        entries = registry.list_presets(
            category=parsed_category,
            include_built_in=include_built_in,
            include_user_registered=include_user_registered,
        )
        payload = {
            "schema_version": CLI_PRESET_LIST_SCHEMA_VERSION,
            "command": CLI_PRESETS_LIST_ENTRYPOINT,
            "category_filter": None if parsed_category is None else parsed_category.value,
            "scope_filter": None if parsed_scope is None else parsed_scope.value,
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
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_list_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_presets_show_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    user_preset_json: tuple[Path, ...],
    category: str,
    preset_name: str,
    scope: Optional[str],
    **_: object,
) -> int:
    try:
        registry = _build_cli_preset_registry(
            cli_context=cli_context,
            user_preset_json=user_preset_json,
            entrypoint=CLI_PRESETS_SHOW_ENTRYPOINT,
            error_id=CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
        )
        parsed_category = _parse_cli_preset_category(category)
        normalized_preset_name = _require_non_empty_cli_string(
            preset_name,
            field_path="preset_name",
            error_id=CLI_PRESETS_SHOW_VALIDATION_ERROR_ID,
            entrypoint=CLI_PRESETS_SHOW_ENTRYPOINT,
        )
        registry_scope = _resolve_named_preset_scope(
            registry=registry,
            category=cast(Any, parsed_category),
            preset_name=normalized_preset_name,
            raw_scope=scope,
        )
        artifact = _resolve_preset_artifact(
            registry=registry,
            category=cast(Any, parsed_category),
            preset_name=normalized_preset_name,
            registry_scope=registry_scope,
        )
        payload = {
            "schema_version": CLI_PRESET_SHOW_SCHEMA_VERSION,
            "command": CLI_PRESETS_SHOW_ENTRYPOINT,
            "requested_category": cast(Any, parsed_category).value,
            "requested_scope": registry_scope.value,
            "preset_name": normalized_preset_name,
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
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_show_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


def _run_presets_export_command(
    *,
    cli_context: CliCommandContext,
    output: str,
    user_preset_json: tuple[Path, ...],
    category: str,
    preset_name: str,
    scope: Optional[str],
    **_: object,
) -> int:
    try:
        registry = _build_cli_preset_registry(
            cli_context=cli_context,
            user_preset_json=user_preset_json,
            entrypoint=CLI_PRESETS_EXPORT_ENTRYPOINT,
            error_id=CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
        )
        parsed_category = _parse_cli_preset_category(category)
        normalized_preset_name = _require_non_empty_cli_string(
            preset_name,
            field_path="preset_name",
            error_id=CLI_PRESETS_EXPORT_VALIDATION_ERROR_ID,
            entrypoint=CLI_PRESETS_EXPORT_ENTRYPOINT,
        )
        registry_scope = _resolve_named_preset_scope(
            registry=registry,
            category=cast(Any, parsed_category),
            preset_name=normalized_preset_name,
            raw_scope=scope,
        )
        export_payload = registry.export_resolved_preset(
            category=cast(Any, parsed_category),
            preset_name=normalized_preset_name,
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
        output,
        json_payload=payload,
        human_output_factory=lambda: _render_presets_export_human_output(payload),
    )

    return int(CliExitCode.SUCCESS)


__all__ = [
    "_run_presets_export_command",
    "_run_presets_list_command",
    "_run_presets_show_command",
]
