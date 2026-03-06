import math
from collections.abc import Mapping, Sequence
from enum import StrEnum
from typing import Any, Optional, cast

from .enum import enum_values, parse_str_enum
from ..seasonality import SeasonalityInputValidationError


def require_string_list(
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
    *,
    require_non_empty: bool,
) -> list[str]:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, Sequence) or isinstance(field_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a sequence of strings",
        )

    normalized_values: list[str] = []
    for value_index, raw_value in enumerate(field_value):
        if not isinstance(raw_value, str):
            raise SeasonalityInputValidationError(
                f"{context}.{field_name}[{value_index}] must be a string",
            )

        normalized_value: str = raw_value.strip()
        if not normalized_value:
            raise SeasonalityInputValidationError(
                f"{context}.{field_name}[{value_index}] must be non-empty",
            )

        normalized_values.append(normalized_value)

    if require_non_empty and not normalized_values:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must include at least one value",
        )

    return normalized_values


def require_enum_member[T: StrEnum](
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
    enum_type: type[T],
    *,
    error_message: Optional[str] = None,
) -> T:
    value: str = require_non_empty_string(parent, field_name, context)
    return parse_enum_member(value, enum_type, f"{context}.{field_name}", error_message=error_message)


def require_enum_members_list[T: StrEnum](
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
    enum_type: type[T],
    *,
    error_message: Optional[str] = None,
) -> list[T]:
    values: list[str] = require_non_empty_string_list(parent, field_name, context)
    return [
        parse_enum_member(
            value,
            enum_type,
            f"{context}.{field_name}[{value_index}]",
            error_message=error_message,
        )
        for value_index, value in enumerate(values)
    ]


def parse_enum_member[T: StrEnum](
    value: str,
    enum_type: type[T],
    context: str,
    *,
    error_message: Optional[str] = None,
) -> T:
    member: Optional[T] = parse_str_enum(enum_type, value)
    if member is None:
        if error_message is not None:
            raise SeasonalityInputValidationError(error_message.format(value=value))

        raise SeasonalityInputValidationError(
            f"{context} must be one of {list(enum_values(enum_type))}, found `{value}`",
        )

    return member


def require_mapping(parent: Mapping[str, Any], field_name: str, context: str) -> Mapping[str, Any]:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, Mapping):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a mapping",
        )

    return cast(Mapping[str, Any], field_value)


def require_non_empty_string(parent: Mapping[str, Any], field_name: str, context: str) -> str:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, str):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a string",
        )

    normalized_value: str = field_value.strip()
    if not normalized_value:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be non-empty",
        )

    return normalized_value


def require_non_empty_string_list(
    parent: Mapping[str, Any],
    field_name: str,
    context: str,
) -> list[str]:
    return require_string_list(parent, field_name, context, require_non_empty=True)


def require_float_list(parent: Mapping[str, Any], field_name: str, context: str) -> list[float]:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, Sequence) or isinstance(field_value, str | bytes):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a sequence of finite numbers",
        )

    normalized_values: list[float] = []
    for value_index, raw_value in enumerate(field_value):
        normalized_values.append(
            validate_finite_float(raw_value, f"{context}.{field_name}[{value_index}]"),
        )

    if not normalized_values:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must include at least one value",
        )

    return normalized_values


def require_non_negative_integer(parent: Mapping[str, Any], field_name: str, context: str) -> int:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, int) or isinstance(field_value, bool):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be an integer",
        )

    if field_value < 0:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be non-negative",
        )

    return field_value


def require_boolean(parent: Mapping[str, Any], field_name: str, context: str) -> bool:
    field_value: object = require_field(parent, field_name, context)
    if not isinstance(field_value, bool):
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} must be a boolean",
        )

    return field_value


def require_field(parent: Mapping[str, Any], field_name: str, context: str) -> object:
    if field_name not in parent:
        raise SeasonalityInputValidationError(
            f"{context}.{field_name} is required",
        )

    return parent[field_name]


def validate_finite_non_negative_float(raw_value: object, context: str) -> float:
    validated_value: float = validate_finite_float(raw_value, context)
    if validated_value < 0.0:
        raise SeasonalityInputValidationError(f"{context} must be non-negative")

    return validated_value


def validate_finite_float(raw_value: object, context: str) -> float:
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise SeasonalityInputValidationError(f"{context} must be a finite number")

    validated_value: float = float(raw_value)
    if not math.isfinite(validated_value):
        raise SeasonalityInputValidationError(f"{context} must be a finite number")

    return validated_value
