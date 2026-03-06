from enum import StrEnum
from typing import Optional

from ..calendar import MarketState


class DeterministicRegressionCaseKind(StrEnum):
    CALENDAR_MARKET_STATE = 'calendar_market_state'
    HOURLY_FACTOR_OUTPUT = 'hourly_factor_output'


class HourlySourceKind(StrEnum):
    EXPLICIT = 'explicit'
    MATERIALIZED = 'materialized'


class HourlyValidationOutcomeKind(StrEnum):
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
    HOURLY_FACTORS = 'hourly_factors'
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


__all__ = [
    "DeterministicRegressionCaseKind",
    "HourlySourceKind",
    "HourlyValidationOutcomeKind",
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
    "parse_str_enum",
]
