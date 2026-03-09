from datetime import UTC, datetime

_EPOCH = datetime(1970, 1, 1, tzinfo=UTC)


def datetime_to_utc_nanos(timestamp_utc: datetime) -> int:
    normalized_timestamp_utc = timestamp_utc.astimezone(UTC)
    delta = normalized_timestamp_utc - _EPOCH

    return (
        ((delta.days * 24 * 60 * 60) + delta.seconds) * 1_000_000_000
        + delta.microseconds * 1_000
    )


__all__ = ["datetime_to_utc_nanos"]
