"""Public workflow entrypoints for sufficient-statistics risk contracts."""

from __future__ import annotations

from typing import Any

from ..contracts import parse_json_object, serialize_deterministic_json
from .contracts import SufficientStatisticsRiskPath, SufficientStatisticsRiskRunRequest
from .errors import RISK_ERROR_FACTORY


def validate_sufficient_statistics_risk_run_request(
    raw_request: dict[str, Any] | Any,
) -> SufficientStatisticsRiskRunRequest:
    return SufficientStatisticsRiskRunRequest.from_mapping(raw_request)


def validate_sufficient_statistics_risk_path(
    raw_path: dict[str, Any] | Any,
) -> SufficientStatisticsRiskPath:
    return SufficientStatisticsRiskPath.from_mapping(raw_path)


def serialize_sufficient_statistics_risk_run_request(
    request: SufficientStatisticsRiskRunRequest,
) -> str:
    return serialize_deterministic_json(request.to_payload())


def deserialize_sufficient_statistics_risk_run_request(
    payload_json: str,
) -> SufficientStatisticsRiskRunRequest:
    raw_payload = parse_json_object(payload_json, "risk run request", RISK_ERROR_FACTORY)
    return validate_sufficient_statistics_risk_run_request(dict(raw_payload))


def serialize_sufficient_statistics_risk_path(path: SufficientStatisticsRiskPath) -> str:
    return serialize_deterministic_json(path)


def deserialize_sufficient_statistics_risk_path(payload_json: str) -> SufficientStatisticsRiskPath:
    raw_payload = parse_json_object(payload_json, "risk path", RISK_ERROR_FACTORY)
    return validate_sufficient_statistics_risk_path(dict(raw_payload))
