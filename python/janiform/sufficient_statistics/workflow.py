"""Public workflow entrypoints for sufficient-statistics contracts."""

from __future__ import annotations

from typing import Any

from ..contracts import parse_json_object, serialize_deterministic_json
from .contracts import SufficientStatisticsPath, SufficientStatisticsRunRequest
from .errors import SUFFICIENT_STATISTICS_ERROR_FACTORY


def validate_sufficient_statistics_run_request(
    raw_request: dict[str, Any] | Any,
) -> SufficientStatisticsRunRequest:
    return SufficientStatisticsRunRequest.from_mapping(raw_request)


def validate_sufficient_statistics_path(
    raw_path: dict[str, Any] | Any,
) -> SufficientStatisticsPath:
    return SufficientStatisticsPath.from_mapping(raw_path)


def serialize_sufficient_statistics_run_request(
    request: SufficientStatisticsRunRequest,
) -> str:
    return serialize_deterministic_json(request.to_payload())


def deserialize_sufficient_statistics_run_request(
    payload_json: str,
) -> SufficientStatisticsRunRequest:
    raw_payload = parse_json_object(payload_json, "sufficient-statistics run request", SUFFICIENT_STATISTICS_ERROR_FACTORY)
    return validate_sufficient_statistics_run_request(dict(raw_payload))


def serialize_sufficient_statistics_path(path: SufficientStatisticsPath) -> str:
    return serialize_deterministic_json(path)


def deserialize_sufficient_statistics_path(payload_json: str) -> SufficientStatisticsPath:
    raw_payload = parse_json_object(payload_json, "sufficient-statistics path", SUFFICIENT_STATISTICS_ERROR_FACTORY)
    return validate_sufficient_statistics_path(dict(raw_payload))
