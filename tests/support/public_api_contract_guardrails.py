import annotationlib
import hashlib
import inspect
import json
from collections.abc import Mapping
from dataclasses import MISSING, fields, is_dataclass
from difflib import unified_diff
from enum import Enum
from importlib import import_module
from types import ModuleType
from typing import Any, Optional

from janiform.contracts import ContractModel, serialize_deterministic_json

PUBLIC_API_CONTRACT_SNAPSHOT_SCHEMA_VERSION = "python.public_api_contract.snapshot.v1"
PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_SCHEMA_VERSION = "python.public_api_contract.guardrail_manifest.v1"
PUBLIC_API_CONTRACT_APPROVAL_POLICY = "public_api_contract_snapshot_changes_require_explicit_rationale.v1"
PUBLIC_API_SURFACE_DOCUMENT_VERSION = "python.public_api_surface_map.pre_v0_1.control_plane_reset.v1"
PUBLIC_PACKAGE_ROOT_MODULE_NAME = "janiform"
PUBLIC_CONTRACT_MODULE_NAMES: tuple[str, ...] = (
    "janiform.errors",
    "janiform.calendar",
    "janiform.hourly_factors",
    "janiform.hourly_factors.contracts",
    "janiform.contracts",
    "janiform.mean_reversion",
    "janiform.mean_reversion.contracts",
    "janiform.seasonality",
    "janiform.simulation",
    "janiform.simulation.coarse",
    "janiform.simulation.seeded",
    "janiform.simulation.shared",
    "janiform.pricing",
    "janiform.risk",
    "janiform.calibration",
    "janiform.calibration.fit",
    "janiform.calibration.fit.estimator",
    "janiform.calibration.fit.model",
    "janiform.calibration.fit.pricing",
    "janiform.calibration.fit.report",
    "janiform.calibration.market_data",
    "janiform.calibration.market_data.contracts",
    "janiform.calibration.market_data.ingestion",
    "janiform.calibration.market_data.request",
    "janiform.calibration.market_data.translation",
    "janiform.calibration.market_data.workflow",
    "janiform.calibration.presets",
    "janiform.calibration.presets.references",
    "janiform.calibration.presets.registry",
    "janiform.calibration.presets.resolution",
    "janiform.calibration.sessions",
    "janiform.calibration.sessions.contracts",
    "janiform.calibration.staged",
    "janiform.calibration.staged.contracts",
    "janiform.calibration.staged.inputs",
    "janiform.calibration.staged.request",
    "janiform.calibration.staged.results",
    "janiform.calibration.staged.runtime",
    "janiform.cli",
    "janiform.cli.commands.calibration",
    "janiform.cli.commands.simulation",
    "janiform.validation",
    "janiform.validation.contracts",
    "janiform.stylized_facts",
    "janiform.stylized_facts.contracts",
)


class PublicApiContractGuardrailError(ValueError):
    """Raised when the checked-in guardrail manifest is incomplete or inconsistent."""


class PublicApiContractApprovalError(PublicApiContractGuardrailError):
    """Raised when the approval manifest does not approve the checked-in snapshot."""


def build_public_api_contract_snapshot() -> dict[str, Any]:
    package_root = import_module(PUBLIC_PACKAGE_ROOT_MODULE_NAME)
    public_modules = {
        module_name: _build_module_snapshot(import_module(module_name))
        for module_name in PUBLIC_CONTRACT_MODULE_NAMES
    }

    return {
        "schema_version": PUBLIC_API_CONTRACT_SNAPSHOT_SCHEMA_VERSION,
        "public_api_surface_document_version": PUBLIC_API_SURFACE_DOCUMENT_VERSION,
        "package_root": _build_root_snapshot(package_root),
        "public_modules": public_modules,
    }


def serialize_public_api_contract_snapshot(snapshot: Mapping[str, Any]) -> str:
    return serialize_deterministic_json(snapshot)


def compute_public_api_contract_snapshot_sha256(snapshot: Mapping[str, Any]) -> str:
    payload = serialize_public_api_contract_snapshot(snapshot).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def public_api_contract_snapshot_matches_approved(
    current_snapshot: Mapping[str, Any],
    approved_snapshot: Mapping[str, Any],
) -> bool:
    return serialize_public_api_contract_snapshot(current_snapshot) == serialize_public_api_contract_snapshot(
        approved_snapshot
    )


def render_public_api_contract_snapshot_diff(
    approved_snapshot: Mapping[str, Any],
    current_snapshot: Mapping[str, Any],
) -> str:
    approved_lines = json.dumps(approved_snapshot, indent=2, sort_keys=True).splitlines()
    current_lines = json.dumps(current_snapshot, indent=2, sort_keys=True).splitlines()
    return "\n".join(
        unified_diff(
            approved_lines,
            current_lines,
            fromfile="approved_snapshot",
            tofile="current_snapshot",
            lineterm="",
        )
    )


def validate_public_api_contract_guardrail_manifest(
    manifest: Mapping[str, Any],
    approved_snapshot: Mapping[str, Any],
) -> None:
    schema_version = _require_non_empty_string(manifest, "schema_version")
    if schema_version != PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_SCHEMA_VERSION:
        raise PublicApiContractGuardrailError(
            "guardrail manifest schema_version must be "
            f"{PUBLIC_API_CONTRACT_GUARDRAIL_MANIFEST_SCHEMA_VERSION}",
        )

    approval_policy = _require_non_empty_string(manifest, "approval_policy")
    if approval_policy != PUBLIC_API_CONTRACT_APPROVAL_POLICY:
        raise PublicApiContractGuardrailError(
            "guardrail manifest approval_policy must be "
            f"{PUBLIC_API_CONTRACT_APPROVAL_POLICY}",
        )

    document_version = _require_non_empty_string(
        manifest,
        "public_api_surface_document_version",
    )
    if document_version != PUBLIC_API_SURFACE_DOCUMENT_VERSION:
        raise PublicApiContractGuardrailError(
            "guardrail manifest public_api_surface_document_version must be "
            f"{PUBLIC_API_SURFACE_DOCUMENT_VERSION}",
        )

    approved_snapshot_sha256 = _require_non_empty_string(manifest, "approved_snapshot_sha256")
    expected_snapshot_sha256 = compute_public_api_contract_snapshot_sha256(approved_snapshot)
    if approved_snapshot_sha256 != expected_snapshot_sha256:
        raise PublicApiContractApprovalError(
            "guardrail manifest approved_snapshot_sha256 does not match the checked-in snapshot",
        )

    raw_change_log = manifest.get("change_log")
    if isinstance(raw_change_log, list):
        change_log = raw_change_log
    else:
        raise PublicApiContractGuardrailError("guardrail manifest change_log must be a list")

    if len(change_log) == 0:
        raise PublicApiContractGuardrailError("guardrail manifest change_log must contain at least one entry")

    latest_entry = change_log[-1]
    if not isinstance(latest_entry, Mapping):
        raise PublicApiContractGuardrailError("guardrail manifest latest change_log entry must be an object")

    latest_snapshot_sha256 = _require_non_empty_string(latest_entry, "snapshot_sha256")
    if latest_snapshot_sha256 != approved_snapshot_sha256:
        raise PublicApiContractApprovalError(
            "guardrail manifest latest change_log entry must approve the checked-in snapshot hash",
        )

    _require_non_empty_string(latest_entry, "approved_on")
    _require_non_empty_string(latest_entry, "summary")
    _require_non_empty_string(latest_entry, "rationale")

    updated_surfaces = latest_entry.get("updated_surfaces")
    if updated_surfaces is None:
        return

    if not isinstance(updated_surfaces, list):
        raise PublicApiContractGuardrailError(
            "guardrail manifest latest change_log entry updated_surfaces must be a list when present",
        )

    if len(updated_surfaces) == 0:
        raise PublicApiContractGuardrailError(
            "guardrail manifest latest change_log entry updated_surfaces must contain at least one value when present",
        )

    for surface_name in updated_surfaces:
        if not isinstance(surface_name, str) or not surface_name.strip():
            raise PublicApiContractGuardrailError(
                "guardrail manifest latest change_log entry updated_surfaces must contain non-empty strings",
            )


def _build_root_snapshot(module: ModuleType) -> dict[str, Any]:
    return {
        "module_name": module.__name__,
        "exports": sorted(_load_module_exports(module)),
    }


def _build_module_snapshot(module: ModuleType) -> dict[str, Any]:
    exports = sorted(_load_module_exports(module))
    return {
        "module_name": module.__name__,
        "exports": exports,
        "schema_identifiers": _collect_schema_identifiers(module, exports),
        "symbols": {
            export_name: _build_symbol_snapshot(module, getattr(module, export_name))
            for export_name in exports
        },
    }


def _load_module_exports(module: ModuleType) -> tuple[str, ...]:
    raw_exports = getattr(module, "__all__", None)
    if not isinstance(raw_exports, list):
        raise PublicApiContractGuardrailError(f"{module.__name__}.__all__ must be a list")

    exports: list[str] = []
    for export_name in raw_exports:
        if not isinstance(export_name, str):
            raise PublicApiContractGuardrailError(f"{module.__name__}.__all__ must contain strings")
        exports.append(export_name)

    return tuple(exports)


def _collect_schema_identifiers(module: ModuleType, exports: tuple[str, ...] | list[str]) -> list[dict[str, Any]]:
    exported_names = set(exports)
    identifiers: list[dict[str, Any]] = []
    for name, value in sorted(module.__dict__.items(), key=lambda item: item[0]):
        if not isinstance(value, str):
            continue
        if not name.isupper():
            continue
        if "SCHEMA" not in name and not name.endswith("_VERSION"):
            continue

        identifiers.append(
            {
                "name": name,
                "value": value,
                "public_exported": name in exported_names,
            }
        )

    return identifiers


def _build_symbol_snapshot(module: ModuleType, value: object) -> dict[str, Any]:
    if inspect.isfunction(value) or inspect.isbuiltin(value):
        return {
            "kind": "function",
            "signature": _safe_signature(value),
        }

    if isinstance(value, type):
        if issubclass(value, Enum):
            return {
                "kind": "enum",
                "members": [
                    {
                        "name": member.name,
                        "value": member.value,
                    }
                    for member in value
                ],
            }

        if is_dataclass(value) or issubclass(value, ContractModel):
            return {
                "kind": "contract_model",
                "signature": _contract_model_signature(value),
                "field_names": _contract_model_field_names(value),
            }

        class_kind = "exception_class" if issubclass(value, Exception) else "class"
        class_snapshot: dict[str, Any] = {
            "kind": class_kind,
            "origin_module": value.__module__,
            "qualname": value.__qualname__,
            "base_classes": [
                base_class.__qualname__
                for base_class in value.__mro__[1:]
                if base_class is not object
            ],
        }
        constructor_signature = _safe_signature(value)
        if constructor_signature is not None:
            class_snapshot["constructor_signature"] = constructor_signature

        if value.__module__ == module.__name__:
            public_methods = _public_method_signatures(value)
            if public_methods:
                class_snapshot["public_methods"] = public_methods

        return class_snapshot

    return {
        "kind": "constant",
        "value": _snapshot_constant_value(value),
    }


def _contract_model_field_names(contract_model_type: type[object]) -> list[str]:
    if not is_dataclass(contract_model_type):
        return []

    return [
        field.name
        for field in fields(contract_model_type)
        if not field.name.startswith("_")
    ]


def _contract_model_signature(contract_model_type: type[object]) -> Optional[str]:
    if not is_dataclass(contract_model_type):
        raw_signature = _resolve_signature(contract_model_type)
        if raw_signature is None:
            return None

        return str(raw_signature)

    return _dataclass_contract_model_signature(contract_model_type)


def _dataclass_contract_model_signature(contract_model_type: type[object]) -> Optional[str]:
    try:
        annotation_strings = annotationlib.get_annotations(
            contract_model_type,
            format=annotationlib.Format.STRING,
        )
    except (AttributeError, NameError, TypeError, ValueError):
        annotation_strings = {}

    parameters: list[str] = []
    for field in fields(contract_model_type):
        if field.name.startswith("_"):
            continue

        annotation_text = _format_contract_model_field_annotation(
            annotation_strings.get(field.name),
            field.type,
        )
        parameter_text = f"{field.name}: {annotation_text}"

        if field.default is not MISSING:
            parameter_text += f" = {field.default!r}"
        elif field.default_factory is not MISSING:
            parameter_text += " = <factory>"

        parameters.append(parameter_text)

    return f"({', '.join(parameters)}) -> 'None'"


def _format_contract_model_field_annotation(
    annotation_text: object,
    fallback_annotation: object,
) -> str:
    if isinstance(annotation_text, str) and annotation_text.strip():
        return repr(annotation_text)

    if hasattr(fallback_annotation, "__name__"):
        return repr(getattr(fallback_annotation, "__name__"))

    return repr(str(fallback_annotation))


def _public_method_signatures(class_type: type[object]) -> dict[str, str]:
    method_signatures: dict[str, str] = {}
    for name, value in sorted(class_type.__dict__.items(), key=lambda item: item[0]):
        if name.startswith("_"):
            continue
        if inspect.isfunction(value):
            signature = _safe_signature(value)
            if signature is not None:
                method_signatures[name] = signature

    return method_signatures


def _snapshot_constant_value(value: object) -> Any:
    if isinstance(value, ContractModel):
        return value.to_payload()
    if isinstance(value, Enum):
        return value.value
    if isinstance(value, str | int | float | bool) or value is None:
        return value
    if isinstance(value, list | tuple):
        return [_snapshot_constant_value(item) for item in value]
    if isinstance(value, dict):
        return {
            str(field_name): _snapshot_constant_value(field_value)
            for field_name, field_value in sorted(value.items(), key=lambda item: str(item[0]))
        }
    if is_dataclass(value) and not isinstance(value, type):
        return {
            field_name: _snapshot_constant_value(getattr(value, field_name))
            for field_name in value.__dataclass_fields__
        }

    return repr(value)


def _safe_signature(value: object) -> Optional[str]:
    signature = _resolve_signature(value)
    if signature is None:
        return None

    return str(signature)


def _resolve_signature(value: object) -> Optional[inspect.Signature]:
    try:
        return inspect.signature(
            value,
            eval_str=False,
            annotation_format=inspect.Format.STRING,
        )
    except (NameError, TypeError, ValueError):
        try:
            return inspect.signature(value)
        except (NameError, TypeError, ValueError):
            return None


def _require_non_empty_string(mapping: Mapping[str, Any], field_name: str) -> str:
    value = mapping.get(field_name)
    if not isinstance(value, str) or not value.strip():
        raise PublicApiContractGuardrailError(f"guardrail manifest {field_name} must be a non-empty string")

    return value
