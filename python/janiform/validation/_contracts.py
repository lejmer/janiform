from collections.abc import Mapping
from typing import Any

from ..contracts.base import (
    require_boolean as _require_boolean,
    require_field as _require_field,
    require_finite_number_sequence,
    require_finite_scalar,
    require_mapping as _require_mapping,
    require_non_empty_string as _require_non_empty_string,
    require_non_empty_string_sequence,
    require_non_negative_finite_scalar,
    require_non_negative_integer as _require_non_negative_integer,
    require_string_sequence,
)
from ._errors import VALIDATION_ERROR_FACTORY


def require_field(parent: Mapping[str, Any], field_name: str, context: str) -> Any:
    return _require_field(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_mapping(parent: Mapping[str, Any], field_name: str, context: str) -> Mapping[str, Any]:
    return _require_mapping(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_non_empty_string(parent: Mapping[str, Any], field_name: str, context: str) -> str:
    return _require_non_empty_string(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_non_empty_string_list(parent: Mapping[str, Any], field_name: str, context: str) -> list[str]:
    return require_non_empty_string_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty_items=True,
        strip_items=True,
    )


def require_string_list(
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
    *,
    require_non_empty: bool,
) -> list[str]:
    return require_string_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty=require_non_empty,
        require_non_empty_items=True,
        strip_items=True,
    )


def require_float_list(parent: Mapping[str, Any], field_name: str, context: str) -> list[float]:
    return require_finite_number_sequence(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
        require_non_empty=True,
    )


def require_non_negative_integer(parent: Mapping[str, Any], field_name: str, context: str) -> int:
    return _require_non_negative_integer(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def require_boolean(parent: Mapping[str, Any], field_name: str, context: str) -> bool:
    return _require_boolean(
        parent,
        field_name,
        f"{context}.{field_name}",
        VALIDATION_ERROR_FACTORY,
    )


def validate_finite_float(raw_value: Any, context: str) -> float:
    return require_finite_scalar(
        raw_value,
        context,
        VALIDATION_ERROR_FACTORY,
    )


def validate_finite_non_negative_float(raw_value: Any, context: str) -> float:
    return require_non_negative_finite_scalar(
        raw_value,
        context,
        VALIDATION_ERROR_FACTORY,
    )
