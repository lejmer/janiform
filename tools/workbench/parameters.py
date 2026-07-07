from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class WorkbenchParameterLeaf:
    path: str
    block_id: str
    field_name: str
    value: Any


def flatten_parameter_tree(parameters: dict[str, Any]) -> tuple[WorkbenchParameterLeaf, ...]:
    leaves: list[WorkbenchParameterLeaf] = []
    _append_parameter_leaves(parameters, prefix="", leaves=leaves)
    leaves.sort(key=lambda leaf: leaf.path)

    return tuple(leaves)


def apply_parameter_updates(
    parameters: dict[str, Any],
    updates: dict[str, Any],
) -> dict[str, Any]:
    updated_parameters = _deep_copy_mapping(parameters)
    for path, updated_value in updates.items():
        _assign_parameter_value(updated_parameters, path=path, value=updated_value)

    return updated_parameters


def parse_parameter_value(text: str, baseline_value: Any) -> Any:
    normalized_text = text.strip()
    if normalized_text == "":
        return None

    match baseline_value:
        case bool():
            return normalized_text.lower() in {"1", "true", "yes", "on"}
        case int():
            return int(normalized_text)
        case float():
            return float(normalized_text)
        case None:
            lowered_text = normalized_text.lower()
            if lowered_text in {"none", "null"}:
                return None
            if lowered_text in {"true", "false"}:
                return lowered_text == "true"

            try:
                if any(character in normalized_text for character in (".", "e", "E")):
                    return float(normalized_text)

                return int(normalized_text)
            except ValueError:
                return normalized_text
        case _:
            return normalized_text


def _append_parameter_leaves(
    payload: Any,
    *,
    prefix: str,
    leaves: list[WorkbenchParameterLeaf],
) -> None:
    if isinstance(payload, dict):
        for key, value in payload.items():
            if not isinstance(key, str):
                continue

            field_path = f"{prefix}.{key}" if prefix else key
            _append_parameter_leaves(value, prefix=field_path, leaves=leaves)

        return

    if isinstance(payload, list):
        for index, value in enumerate(payload):
            field_path = f"{prefix}[{index}]"
            _append_parameter_leaves(value, prefix=field_path, leaves=leaves)

        return

    block_id, _, field_name = prefix.partition(".")

    leaves.append(
        WorkbenchParameterLeaf(
            path=prefix,
            block_id=block_id or "root",
            field_name=field_name or prefix,
            value=payload,
        )
    )


def _deep_copy_mapping(payload: dict[str, Any]) -> dict[str, Any]:
    copied: dict[str, Any] = {}

    for key, value in payload.items():
        if isinstance(value, dict):
            copied[key] = _deep_copy_mapping(value)
        elif isinstance(value, list):
            copied[key] = _deep_copy_list(value)
        else:
            copied[key] = value

    return copied


def _deep_copy_list(payload: list[Any]) -> list[Any]:
    copied: list[Any] = []

    for value in payload:
        if isinstance(value, dict):
            copied.append(_deep_copy_mapping(value))
        elif isinstance(value, list):
            copied.append(_deep_copy_list(value))
        else:
            copied.append(value)

    return copied


def _assign_parameter_value(payload: dict[str, Any], *, path: str, value: Any) -> None:
    normalized_path = path.replace("[", ".[").split(".")
    cursor: dict[str, Any] = payload
    for path_segment in normalized_path[:-1]:
        if path_segment == "":
            continue

        if path_segment.startswith("[") and path_segment.endswith("]"):
            index = int(path_segment[1:-1])
            cursor = cursor[index]
        else:
            cursor = cursor[path_segment]

    final_segment = normalized_path[-1]
    if final_segment.startswith("[") and final_segment.endswith("]"):
        index = int(final_segment[1:-1])
        cursor[index] = value
        return

    cursor[final_segment] = value


__all__ = [
    "WorkbenchParameterLeaf",
    "apply_parameter_updates",
    "flatten_parameter_tree",
    "parse_parameter_value",
]
