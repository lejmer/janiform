from datetime import UTC, datetime, timedelta
from math import isfinite


_COMMON_TIMESTAMP_FORMATS: tuple[str, ...] = (
    "%d.%m.%Y %H:%M:%S.%f%z",
    "%d.%m.%Y %H:%M:%S.%f %z",
    "%d.%m.%Y %H:%M:%S%z",
    "%d.%m.%Y %H:%M:%S %z",
    "%d.%m.%Y %H:%M%z",
    "%d.%m.%Y %H:%M %z",
    "%d.%m.%Y %H:%M:%S.%f",
    "%d.%m.%Y %H:%M:%S",
    "%d.%m.%Y %H:%M",
    "%Y/%m/%d %H:%M:%S.%f%z",
    "%Y/%m/%d %H:%M:%S.%f %z",
    "%Y/%m/%d %H:%M:%S%z",
    "%Y/%m/%d %H:%M:%S %z",
    "%Y/%m/%d %H:%M%z",
    "%Y/%m/%d %H:%M %z",
    "%Y/%m/%d %H:%M:%S.%f",
    "%Y/%m/%d %H:%M:%S",
    "%Y/%m/%d %H:%M",
    "%Y%m%d %H:%M:%S.%f",
    "%Y%m%d %H:%M:%S",
    "%Y%m%d %H%M%S.%f",
    "%Y%m%d %H%M%S",
    "%Y%m%dT%H:%M:%S.%f",
    "%Y%m%dT%H:%M:%S",
    "%Y%m%dT%H%M%S.%f",
    "%Y%m%dT%H%M%S",
    "%Y%m%d%H%M%S.%f",
    "%Y%m%d%H%M%S",
)
_UNIX_EPOCH_UTC = datetime(1970, 1, 1, tzinfo=UTC)


def datetime_to_utc_nanos(value: datetime) -> int:
    if value.tzinfo is None:
        raise ValueError("datetime must be timezone-aware")

    utc_value = value.astimezone(UTC)
    delta = utc_value - _UNIX_EPOCH_UTC
    return (
        delta.days * 86_400_000_000_000
        + delta.seconds * 1_000_000_000
        + delta.microseconds * 1_000
    )


def parse_timestamp_value(raw_value: object) -> datetime:
    if isinstance(raw_value, datetime):
        return raw_value
    if isinstance(raw_value, bool):
        raise TypeError("boolean values are not valid timestamps")
    if isinstance(raw_value, int | float):
        return _parse_unix_epoch_value(raw_value)
    if isinstance(raw_value, str):
        stripped_value = raw_value.strip()
        if not stripped_value:
            raise ValueError("timestamp value cannot be blank")

        parsed_numeric_value = _try_parse_numeric_timestamp_text(stripped_value)
        if parsed_numeric_value is not None:
            return parsed_numeric_value

        return _parse_timestamp_text(stripped_value)

    raise TypeError(f"unsupported timestamp value type: {type(raw_value).__name__}")


def _parse_timestamp_text(raw_value: str) -> datetime:
    normalized_value = raw_value.replace(" UTC", "+00:00").replace(" GMT", "+00:00")
    if normalized_value.endswith("Z"):
        normalized_value = f"{normalized_value[:-1]}+00:00"

    try:
        return datetime.fromisoformat(normalized_value)
    except ValueError:
        pass

    for format_string in _COMMON_TIMESTAMP_FORMATS:
        try:
            return datetime.strptime(normalized_value, format_string)
        except ValueError:
            continue

    raise ValueError(f"unsupported timestamp format: {raw_value}")


def _try_parse_numeric_timestamp_text(raw_value: str) -> datetime | None:
    unsigned_value = raw_value.lstrip("+-")
    if unsigned_value.isdigit():
        digit_count = len(unsigned_value)
        if digit_count not in {9, 10, 12, 13, 15, 16, 18, 19}:
            return None

    try:
        if any(character in raw_value for character in (".", "e", "E")):
            parsed_numeric_value = float(raw_value)
        else:
            parsed_numeric_value = int(raw_value)
    except ValueError:
        return None

    return _parse_unix_epoch_value(parsed_numeric_value)


def _parse_unix_epoch_value(raw_value: int | float) -> datetime:
    if not isfinite(raw_value):
        raise ValueError("timestamp value must be finite")

    absolute_value = abs(raw_value)
    if absolute_value >= 100_000_000_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000_000_000)
    if absolute_value >= 100_000_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000_000)
    if absolute_value >= 100_000_000_000:
        return _from_epoch_fraction(raw_value, 1_000)

    return _from_epoch_fraction(raw_value, 1)


def _from_epoch_fraction(raw_value: int | float, divisor: int) -> datetime:
    if isinstance(raw_value, int):
        whole_seconds, fractional_units = divmod(raw_value, divisor)
        microseconds = (fractional_units * 1_000_000) // divisor
        try:
            return datetime.fromtimestamp(whole_seconds, tz=UTC) + timedelta(
                microseconds=microseconds,
            )
        except (OverflowError, OSError, ValueError) as e:
            raise ValueError(f"timestamp value is outside the supported range: {raw_value}") from e

    try:
        return datetime.fromtimestamp(raw_value / divisor, tz=UTC)
    except (OverflowError, OSError, ValueError) as e:
        raise ValueError(f"timestamp value is outside the supported range: {raw_value}") from e
