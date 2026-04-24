from datetime import UTC, datetime

from janiform._timestamps import datetime_to_utc_nanos


REFERENCE_PRICING_TIMESTAMP_UTC = datetime(2024, 2, 2, 14, 0, 0, tzinfo=UTC)
REFERENCE_PRICING_TIMESTAMP_UTC_NANOS = datetime_to_utc_nanos(REFERENCE_PRICING_TIMESTAMP_UTC)
