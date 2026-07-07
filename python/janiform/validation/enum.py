from enum import StrEnum
from typing import Any, Optional

from ..contracts.base import (
    require_non_empty_string,
    require_non_empty_string_sequence,
)
from ..calendar import MarketState
from ._errors import VALIDATION_ERROR_FACTORY, validation_error


class DeterministicRegressionCaseKind(StrEnum):
    CALENDAR_MARKET_STATE = 'calendar_market_state'
    INTERVAL_FACTOR_GRID_OUTPUT = 'interval_factor_grid_output'


class IntervalFactorGridSourceKind(StrEnum):
    EXPLICIT = 'explicit'
    MATERIALIZED = 'materialized'


class IntervalFactorGridValidationOutcomeKind(StrEnum):
    SUCCESS = 'success'
    FAILURE = 'failure'


class OpenClosedGapFailureKind(StrEnum):
    DISPATCH_ERROR = 'dispatch_error'
    SEMANTIC_MISMATCH = 'semantic_mismatch'


class OpenClosedGapScenarioClass(StrEnum):
    KNOWN_ONLY = 'known_only'
    SURPRISE_ONLY = 'surprise_only'
    MIXED_NEWS = 'mixed_news'


class NewsArrivalComponent(StrEnum):
    KNOWN = 'known'
    SURPRISE = 'surprise'


class SemanticBucket(StrEnum):
    TRADING_RETURN = 'trading_return'
    OPEN_JUMP = 'open_jump'
    CLOSED_GAP = 'closed_gap'
    ACTIVITY_COUPLING = 'activity_coupling'


class RegressionStatus(StrEnum):
    PASS = 'pass'
    FAIL = 'fail'


class ValidationFixtureDomain(StrEnum):
    CALENDAR = 'calendar'
    SEASONALITY = 'seasonality'
    INTERVAL_FACTOR_GRIDS = 'interval_factor_grids'
    GAP_BEHAVIOR = 'gap_behavior'
    SCALE_CONSISTENCY = 'scale_consistency'
    STYLIZED_FACTS = 'stylized_facts'


class StylizedFactDiagnosticKind(StrEnum):
    ACTIVITY_CLUSTERING = 'activity_clustering'
    TAIL_SKEW = 'tail_skew'
    VOLATILITY_PERSISTENCE = 'volatility_persistence'
    GAP_BEHAVIOR = 'gap_behavior'


class StylizedFactExpectedMetricStatus(StrEnum):
    AVAILABLE = 'available'
    INSUFFICIENT_DATA = 'insufficient_data'


class StylizedFactTolerancePolicyKind(StrEnum):
    DETERMINISTIC_EXACT_MATCH = 'deterministic_exact_match'
    BOOTSTRAP_CONFIDENCE_INTERVAL = 'bootstrap_confidence_interval'


class ScaleConsistencyDatasetRole(StrEnum):
    COARSE = 'coarse'
    FINE = 'fine'


class ScaleConsistencyDecisionReasonCode(StrEnum):
    ABSOLUTE_DELTA_THRESHOLD_EXCEEDED = 'absolute_delta_threshold_exceeded'
    METRIC_COMPUTATION_MISSING = 'metric_computation_missing'
    ENGINE_BENCHMARK_PREREQUISITE_FAILED = 'engine_benchmark_prerequisite_failed'


class ScaleConsistencyDeltaKind(StrEnum):
    SIGNED_DIFFERENCE = 'signed_difference'
    ABSOLUTE_DISTANCE = 'absolute_distance'


class ScaleConsistencyDriftCategory(StrEnum):
    RETURNS = 'returns'
    VOLATILITY = 'volatility'
    EVENT_DERIVED = 'event_derived'


class ScaleConsistencyFailureReasonCode(StrEnum):
    INSUFFICIENT_RETURN_OBSERVATIONS = 'insufficient_return_observations'
    INSUFFICIENT_LAGGED_OBSERVATIONS = 'insufficient_lagged_observations'
    MISSING_OPEN_JUMP_OBSERVATIONS = 'missing_open_jump_observations'
    MISSING_CLOSED_GAP_OBSERVATIONS = 'missing_closed_gap_observations'


class ScaleConsistencyMetricCategory(StrEnum):
    RETURNS = 'returns'
    VOLATILITY = 'volatility'
    EVENTS = 'events'


class ScaleConsistencyMetricComputationStatus(StrEnum):
    COMPUTED = 'computed'
    MISSING = 'missing'


class SeededTickSide(StrEnum):
    BUY = 'buy'
    SELL = 'sell'


def enum_values[T: StrEnum](enum_type: type[T]) -> tuple[str, ...]:
    return tuple(member.value for member in enum_type)


def parse_str_enum[T: StrEnum](enum_type: type[T], value: str) -> Optional[T]:
    for member in enum_type:
        if member.value == value:
            return member

    return None


def parse_enum_member[T: StrEnum](
    value: str,
    enum_type: type[T],
    field_path: str,
    *,
    error_message: Optional[str] = None,
) -> T:
    member = parse_str_enum(enum_type, value)
    if member is not None:
        return member

    if error_message is not None:
        raise validation_error(field_path, error_message.format(value=value))

    raise validation_error(
        field_path,
        f"must be one of {list(enum_values(enum_type))}, found `{value}`",
    )


def require_enum_member[T: StrEnum](
    mapping: dict[str, Any] | Any,
    field_name: str,
    context: str,
    enum_type: type[T],
    *,
    error_message: Optional[str] = None,
) -> T:
    field_path = f"{context}.{field_name}"
    value = require_non_empty_string(
        mapping,
        field_name,
        field_path,
        VALIDATION_ERROR_FACTORY,
    )
    return parse_enum_member(
        value,
        enum_type,
        field_path,
        error_message=error_message,
    )


def require_enum_members_list[T: StrEnum](
    mapping: dict[str, Any] | Any,
    field_name: str,
    context: str,
    enum_type: type[T],
    *,
    error_message: Optional[str] = None,
) -> list[T]:
    field_path = f"{context}.{field_name}"
    values = require_non_empty_string_sequence(
        mapping,
        field_name,
        field_path,
        VALIDATION_ERROR_FACTORY,
        require_non_empty_items=True,
        strip_items=True,
    )
    return [
        parse_enum_member(
            value,
            enum_type,
            f"{field_path}[{value_index}]",
            error_message=error_message,
        )
        for value_index, value in enumerate(values)
    ]


__all__ = [
    "DeterministicRegressionCaseKind",
    "IntervalFactorGridSourceKind",
    "IntervalFactorGridValidationOutcomeKind",
    "MarketState",
    "NewsArrivalComponent",
    "OpenClosedGapFailureKind",
    "OpenClosedGapScenarioClass",
    "RegressionStatus",
    "ScaleConsistencyDatasetRole",
    "ScaleConsistencyDecisionReasonCode",
    "ScaleConsistencyDeltaKind",
    "ScaleConsistencyDriftCategory",
    "ScaleConsistencyFailureReasonCode",
    "ScaleConsistencyMetricCategory",
    "ScaleConsistencyMetricComputationStatus",
    "SeededTickSide",
    "SemanticBucket",
    "StylizedFactDiagnosticKind",
    "StylizedFactExpectedMetricStatus",
    "StylizedFactTolerancePolicyKind",
    "ValidationFixtureDomain",
    "enum_values",
    "parse_enum_member",
    "parse_str_enum",
    "require_enum_member",
    "require_enum_members_list",
]
