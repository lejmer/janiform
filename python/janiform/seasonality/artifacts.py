"""Timestamp preprocessing artifacts for calendar-aligned event data."""

from __future__ import annotations

from typing import Optional, cast


class TimestampEventPreprocessingResult:
    """Thin Python wrapper over the Rust preprocessing output object."""

    def __init__(self, native_result: object) -> None:
        self._native_result: object = native_result

    @property
    def schema_version(self) -> str:
        return cast(str, self._native_result.schema_version)

    @property
    def input_schema_version(self) -> str:
        return cast(str, self._native_result.input_schema_version)

    @property
    def exchange_code(self) -> str:
        return cast(str, self._native_result.exchange_code)

    @property
    def exchange_timezone_name(self) -> str:
        return cast(str, self._native_result.exchange_timezone_name)

    @property
    def source_record_positions(self) -> list[int]:
        return cast(list[int], self._native_result.source_record_positions)

    @property
    def timestamp_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.timestamp_utc_nanos)

    @property
    def exchange_local_minute_of_day(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_minute_of_day)

    @property
    def exchange_local_day_of_week(self) -> bytes:
        return cast(bytes, self._native_result.exchange_local_day_of_week)

    @property
    def exchange_local_day_of_year(self) -> list[int]:
        return cast(list[int], self._native_result.exchange_local_day_of_year)

    @property
    def market_state_codes(self) -> bytes:
        return cast(bytes, self._native_result.market_state_codes)

    @property
    def missing_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.missing_hour_bucket_utc_nanos)

    @property
    def sparse_hour_bucket_utc_nanos(self) -> list[int]:
        return cast(list[int], self._native_result.sparse_hour_bucket_utc_nanos)

    @property
    def input_record_count(self) -> int:
        return cast(int, self._native_result.input_record_count)

    @property
    def validated_record_count(self) -> int:
        return cast(int, self._native_result.validated_record_count)

    @property
    def accepted_record_count(self) -> int:
        return cast(int, self._native_result.accepted_record_count)

    @property
    def filtered_record_count(self) -> int:
        return cast(int, self._native_result.filtered_record_count)

    @property
    def first_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.first_event_timestamp_utc_nanos)

    @property
    def last_event_timestamp_utc_nanos(self) -> Optional[int]:
        return cast(Optional[int], self._native_result.last_event_timestamp_utc_nanos)

    @property
    def hour_bucket_count(self) -> int:
        return cast(int, self._native_result.hour_bucket_count)

    @property
    def missing_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.missing_hour_bucket_count)

    @property
    def sparse_hour_bucket_count(self) -> int:
        return cast(int, self._native_result.sparse_hour_bucket_count)


__all__ = ["TimestampEventPreprocessingResult"]
