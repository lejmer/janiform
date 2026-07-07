"""Shared seasonality-adjacent boundary contracts."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum

from ..contracts import ContractModel

TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION: str = "seasonality.timestamp_event.v1"
PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION: str = "seasonality.timestamp_event.preprocessed.v1"


class SessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


@dataclass(frozen=True, slots=True)
class TimestampEventInputSchema(ContractModel):
    schema_version: str
    required_fields: tuple[str, ...]
    timestamp_field_name: str
    accepted_timestamp_types: tuple[str, ...]
    timezone_requirement: str
    ordering_policy: str


TIMESTAMP_EVENT_INPUT_SCHEMA: TimestampEventInputSchema = TimestampEventInputSchema(
    schema_version=TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION,
    required_fields=("timestamp",),
    timestamp_field_name="timestamp",
    accepted_timestamp_types=("datetime", "RFC3339 / ISO-8601 string"),
    timezone_requirement="timestamp values must include timezone information",
    ordering_policy="output is sorted by normalized UTC timestamp and then input position",
)


__all__ = [
    "PREPROCESSED_TIMESTAMP_EVENT_SCHEMA_VERSION",
    "SessionFilter",
    "TIMESTAMP_EVENT_INPUT_SCHEMA",
    "TIMESTAMP_EVENT_INPUT_SCHEMA_VERSION",
    "TimestampEventInputSchema",
]
