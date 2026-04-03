import json
from abc import ABC
from collections.abc import Callable, Mapping, Sequence
from dataclasses import fields, is_dataclass
from datetime import UTC, datetime
from enum import Enum, StrEnum
from math import isfinite, sqrt
from typing import Any, Optional, cast


type ValidationErrorFactory = Callable[[str, str], Exception]


class ContractModel(ABC):
    def to_payload(self) -> dict[str, Any]:
        payload = _to_jsonable(self)
        if not isinstance(payload, dict):
            raise TypeError("contract model payload must be a dictionary")

        return cast(dict[str, Any], payload)

    def to_json(self) -> str:
        return serialize_deterministic_json(self)


def serialize_deterministic_json(value: object) -> str:
    return json.dumps(_to_jsonable(value), sort_keys=True)


def serialize_pretty_json(value: object) -> str:
    return json.dumps(_to_jsonable(value), indent=2, sort_keys=True)


def parse_json_object(
    payload_json: str,
    payload_label: str,
    error_factory: ValidationErrorFactory,
) -> Mapping[str, Any]:
    try:
        parsed_payload = json.loads(payload_json)
    except json.JSONDecodeError as e:
        raise error_factory(payload_label, f"must be valid JSON: {e.msg}") from e

    if not isinstance(parsed_payload, dict):
        raise error_factory(payload_label, "must decode to a JSON object")

    return cast(Mapping[str, Any], parsed_payload)


def require_field(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Any:
    if field_name not in mapping:
        raise error_factory(field_path, "is required")

    return mapping[field_name]


def require_mapping(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Mapping[str, Any]:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    if not isinstance(raw_value, Mapping):
        raise error_factory(field_path, "must be an object")

    return cast(Mapping[str, Any], raw_value)


def require_boolean(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> bool:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    if not isinstance(raw_value, bool):
        raise error_factory(field_path, "must be a boolean")

    return raw_value


def require_optional_boolean_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[bool]:
    if raw_value is None:
        return None

    if not isinstance(raw_value, bool):
        raise error_factory(field_path, "must be a boolean or null")

    return raw_value


def require_optional_mapping_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[dict[str, Any]]:
    if raw_value is None:
        return None

    if not isinstance(raw_value, Mapping):
        raise error_factory(field_path, "must be an object or null")

    return dict(raw_value)


def require_mapping_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> dict[str, Any]:
    optional_value = require_optional_mapping_value(raw_value, field_path, error_factory)
    if optional_value is None:
        raise error_factory(field_path, "must be an object")

    return optional_value


def require_optional_string_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[str]:
    if raw_value is None:
        return None

    if not isinstance(raw_value, str):
        raise error_factory(field_path, "must be a string or null")

    return raw_value


def require_only_supported_fields(
    mapping: Mapping[str, Any],
    *,
    supported_fields: tuple[str, ...],
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> None:
    supported_field_names = set(supported_fields)
    for field_name in mapping:
        if field_name not in supported_field_names:
            raise error_factory(field_path, f"unsupported field '{field_name}'")


def require_sequence(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Sequence[object]:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    if isinstance(raw_value, str) or not isinstance(raw_value, Sequence):
        raise error_factory(field_path, "must be a sequence")

    return cast(Sequence[object], raw_value)


def require_string(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> str:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    return require_string_value(raw_value, field_path, error_factory)


def require_string_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> str:
    if not isinstance(raw_value, str):
        raise error_factory(field_path, "must be a string")

    return raw_value


def require_str_enum_value[TStrEnum: StrEnum](
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    enum_type: type[TStrEnum],
    allowed_values_message: str,
    include_received_value: bool = False,
) -> TStrEnum:
    if not isinstance(raw_value, str):
        raise error_factory(field_path, "must be a string")

    try:
        return enum_type(raw_value)
    except ValueError as e:
        message = (
            f"{allowed_values_message}, received '{raw_value}'"
            if include_received_value
            else allowed_values_message
        )
        raise error_factory(field_path, message) from e


def require_non_empty_string(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> str:
    value = require_non_empty_string_value(
        require_field(mapping, field_name, field_path, error_factory),
        field_path,
        error_factory,
    )
    return value


def require_non_empty_string_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> str:
    value = require_string_value(raw_value, field_path, error_factory)
    if not value.strip():
        raise error_factory(field_path, "must be a non-empty string")

    return value


def require_string_sequence(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    require_non_empty: bool = False,
    require_non_empty_items: bool = False,
    strip_items: bool = False,
) -> list[str]:
    raw_values = require_sequence(mapping, field_name, field_path, error_factory)

    normalized_values: list[str] = []
    for value_index, raw_value in enumerate(raw_values):
        item_path = f"{field_path}[{value_index}]"
        value = require_string_value(raw_value, item_path, error_factory)
        normalized_value = value.strip() if strip_items else value
        if require_non_empty_items and not normalized_value:
            raise error_factory(item_path, "must be non-empty")

        normalized_values.append(normalized_value)

    if require_non_empty and not normalized_values:
        raise error_factory(field_path, "must include at least one value")

    return normalized_values


def require_non_empty_string_sequence(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    require_non_empty_items: bool = False,
    strip_items: bool = False,
) -> list[str]:
    return require_string_sequence(
        mapping,
        field_name,
        field_path,
        error_factory,
        require_non_empty=True,
        require_non_empty_items=require_non_empty_items,
        strip_items=strip_items,
    )


def require_integer(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    return require_integer_value(raw_value, field_path, error_factory)


def require_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    if not isinstance(raw_value, int) or isinstance(raw_value, bool):
        raise error_factory(field_path, "must be an integer")

    return raw_value


def require_non_negative_integer(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    value = require_integer(mapping, field_name, field_path, error_factory)
    return require_non_negative_integer_value(value, field_path, error_factory)


def require_non_negative_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    value = require_integer_value(raw_value, field_path, error_factory)
    if value < 0:
        raise error_factory(field_path, "must be non-negative")

    return value


def require_positive_integer(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    value = require_integer(mapping, field_name, field_path, error_factory)
    return require_positive_integer_value(value, field_path, error_factory)


def require_positive_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> int:
    value = require_integer_value(raw_value, field_path, error_factory)
    if value <= 0:
        raise error_factory(field_path, "must be strictly positive")

    return value


def require_optional_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[int]:
    if raw_value is None:
        return None

    return require_integer_value(raw_value, field_path, error_factory)


def require_optional_non_negative_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[int]:
    if raw_value is None:
        return None

    return require_non_negative_integer_value(raw_value, field_path, error_factory)


def require_optional_positive_integer_value(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> Optional[int]:
    if raw_value is None:
        return None

    return require_positive_integer_value(raw_value, field_path, error_factory)


def require_finite_scalar(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    number_message: str = "must be a finite number",
) -> float:
    if not isinstance(raw_value, int | float) or isinstance(raw_value, bool):
        raise error_factory(field_path, number_message)

    numeric_value = float(raw_value)
    if not isfinite(numeric_value):
        raise error_factory(field_path, number_message)

    return numeric_value


def require_finite_number(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    number_message: str = "must be a finite number",
) -> float:
    raw_value = require_field(mapping, field_name, field_path, error_factory)
    return require_finite_scalar(
        raw_value,
        field_path,
        error_factory,
        number_message=number_message,
    )


def require_finite_number_sequence(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    require_non_empty: bool = False,
    number_message: str = "must be a finite number",
) -> list[float]:
    raw_values = require_sequence(mapping, field_name, field_path, error_factory)

    normalized_values: list[float] = []
    for value_index, raw_value in enumerate(raw_values):
        normalized_values.append(
            require_finite_scalar(
                raw_value,
                f"{field_path}[{value_index}]",
                error_factory,
                number_message=number_message,
            )
        )

    if require_non_empty and not normalized_values:
        raise error_factory(field_path, "must include at least one value")

    return normalized_values


def require_integer_sequence(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    require_non_empty: bool = False,
) -> list[int]:
    raw_values = require_sequence(mapping, field_name, field_path, error_factory)

    normalized_values: list[int] = []
    for value_index, raw_value in enumerate(raw_values):
        if not isinstance(raw_value, int) or isinstance(raw_value, bool):
            raise error_factory(f"{field_path}[{value_index}]", "must be an integer")

        normalized_values.append(raw_value)

    if require_non_empty and not normalized_values:
        raise error_factory(field_path, "must include at least one value")

    return normalized_values


def require_optional_finite_number(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    default_value: Optional[float] = None,
    number_message: str = "must be a finite number",
) -> Optional[float]:
    if field_name not in mapping:
        return default_value

    return require_optional_finite_scalar(
        mapping[field_name],
        field_path,
        error_factory,
        default_value=default_value,
        number_message=number_message,
    )


def require_optional_finite_scalar(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
    *,
    default_value: Optional[float] = None,
    number_message: str = "must be a finite number",
) -> Optional[float]:
    if raw_value is None:
        return default_value

    return require_finite_scalar(
        raw_value,
        field_path,
        error_factory,
        number_message=number_message,
    )


def require_positive_finite_number(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> float:
    value = require_finite_number(mapping, field_name, field_path, error_factory)
    if value <= 0.0:
        raise error_factory(field_path, "must be strictly positive")

    return value


def require_non_negative_finite_number(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> float:
    value = require_finite_number(mapping, field_name, field_path, error_factory)
    if value < 0.0:
        raise error_factory(field_path, "must be non-negative")

    return value


def require_non_negative_finite_scalar(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> float:
    value = require_finite_scalar(raw_value, field_path, error_factory)
    if value < 0.0:
        raise error_factory(field_path, "must be non-negative")

    return value


def require_positive_finite_scalar(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> float:
    value = require_finite_scalar(raw_value, field_path, error_factory)
    if value <= 0.0:
        raise error_factory(field_path, "must be strictly positive")

    return value


def require_datetime_instance_utc(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> datetime:
    if not isinstance(raw_value, datetime):
        raise error_factory(
            field_path,
            "must be a datetime value with timezone information",
        )

    if raw_value.tzinfo is None:
        raise error_factory(field_path, "must be timezone-aware")

    return raw_value.astimezone(UTC)


def require_datetime_utc(
    raw_value: Any,
    field_path: str,
    error_factory: ValidationErrorFactory,
) -> datetime:
    if isinstance(raw_value, str):
        try:
            normalized_value = raw_value.replace("Z", "+00:00")
            parsed_value = datetime.fromisoformat(normalized_value)
        except ValueError as e:
            raise error_factory(field_path, f"must be a valid ISO 8601 datetime: {e}") from e
    elif isinstance(raw_value, datetime):
        parsed_value = raw_value
    else:
        raise error_factory(field_path, "must be a timezone-aware datetime")

    if parsed_value.tzinfo is None:
        raise error_factory(field_path, "must be timezone-aware")

    return parsed_value.astimezone(UTC)


def compute_hawkes_alpha_over_beta_spectral_radius(
    plus_plus: float,
    plus_minus: float,
    minus_plus: float,
    minus_minus: float,
    beta: float,
) -> float:
    normalized_plus_plus = plus_plus / beta
    normalized_plus_minus = plus_minus / beta
    normalized_minus_plus = minus_plus / beta
    normalized_minus_minus = minus_minus / beta

    trace = normalized_plus_plus + normalized_minus_minus

    determinant = normalized_plus_plus * normalized_minus_minus - normalized_plus_minus * normalized_minus_plus
    discriminant = trace * trace - 4.0 * determinant
    clipped_discriminant = max(discriminant, 0.0)

    largest_eigenvalue = 0.5 * (trace + sqrt(clipped_discriminant))
    smallest_eigenvalue = 0.5 * (trace - sqrt(clipped_discriminant))

    return max(abs(largest_eigenvalue), abs(smallest_eigenvalue))


def validate_exact_string(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: str,
    error_factory: ValidationErrorFactory,
) -> None:
    observed_value = require_string(mapping, field_name, field_path, error_factory)
    if observed_value != expected_value:
        raise error_factory(field_path, f"must be {expected_value}, found {observed_value}")


def validate_exact_integer(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: int,
    error_factory: ValidationErrorFactory,
    *,
    context_suffix: Optional[str] = None,
) -> None:
    observed_value = require_integer(mapping, field_name, field_path, error_factory)
    if observed_value != expected_value:
        suffix = f" ({context_suffix})" if context_suffix is not None else ""
        raise error_factory(field_path, f"must be {expected_value}, found {observed_value}{suffix}")


def validate_exact_optional_integer(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: Optional[int],
    error_factory: ValidationErrorFactory,
) -> None:
    if field_name not in mapping:
        raise error_factory(field_path, "is required")

    raw_value = mapping[field_name]
    if raw_value is None:
        observed_value = None
    elif isinstance(raw_value, int) and not isinstance(raw_value, bool):
        observed_value = raw_value
    else:
        raise error_factory(field_path, "must be an integer or null")

    if observed_value != expected_value:
        raise error_factory(field_path, f"must be {expected_value}, found {observed_value}")


def validate_exact_number(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_value: float,
    error_factory: ValidationErrorFactory,
    *,
    context_suffix: Optional[str] = None,
) -> None:
    observed_value = require_finite_number(mapping, field_name, field_path, error_factory)
    if observed_value != expected_value:
        suffix = f" ({context_suffix})" if context_suffix is not None else ""
        raise error_factory(field_path, f"must be {expected_value}, found {observed_value}{suffix}")


def validate_required_field_list(
    mapping: Mapping[str, Any],
    field_name: str,
    field_path: str,
    expected_field_names: Sequence[str],
    error_factory: ValidationErrorFactory,
) -> None:
    raw_values = require_sequence(mapping, field_name, field_path, error_factory)
    if len(raw_values) != len(expected_field_names):
        raise error_factory(field_path, f"must contain exactly {len(expected_field_names)} entries")

    for field_index, expected_field_name in enumerate(expected_field_names):
        raw_value = raw_values[field_index]
        if not isinstance(raw_value, str):
            raise error_factory(f"{field_path}[{field_index}]", "must be a string")

        if raw_value != expected_field_name:
            raise error_factory(
                f"{field_path}[{field_index}]",
                f"must be {expected_field_name}, found {raw_value}",
            )


def _to_jsonable(value: object) -> Any:
    if value is None or isinstance(value, str | int | float | bool):
        return value

    if isinstance(value, datetime):
        if value.tzinfo is None:
            return value.isoformat()

        return value.astimezone(UTC).isoformat()

    if isinstance(value, Enum):
        return value.value

    if is_dataclass(value):
        return {
            field.name: _to_jsonable(getattr(value, field.name))
            for field in fields(value)
        }

    if isinstance(value, Mapping):
        return {
            str(key): _to_jsonable(mapped_value)
            for key, mapped_value in value.items()
        }

    if isinstance(value, Sequence) and not isinstance(value, str):
        return [_to_jsonable(item) for item in value]

    return value
