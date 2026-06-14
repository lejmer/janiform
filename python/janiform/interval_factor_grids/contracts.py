"""Public interval-factor-grid contracts."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from typing import Final, Optional

from ..contracts import ContractModel

INTERVAL_FACTOR_GRID_SCHEMA_VERSION: Final[str] = "seasonality.interval_factor_grid.contract.v1"
INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: Final[str] = (
    "seasonality.interval_seasonality_factor.materialization.v1"
)
INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION: Final[str] = (
    "seasonality.interval_news_intensity.materialization.v1"
)
DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD: Final[str] = "explicit_interval_grid_input.v1"
DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD: Final[str] = (
    "known_calendar_anchor_plus_surprise_background_controls.v1"
)
DEFAULT_SEASONALITY_INTERPOLATION_METHOD: Final[str] = "previous_observation_carry_forward.v1"
DEFAULT_SEASONALITY_AGGREGATION_METHOD: Final[str] = "mean_of_observations.v1"
DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: Final[str] = "global_mean.v1"
DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT: Final[int] = 1
INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY: Final[str] = (
    "fixed_width_utc_interval_projected_to_exchange_local.v1"
)
INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY: Final[str] = "skip_nonexistent_exchange_local_interval_labels.v1"
INTERVAL_FACTOR_GRID_FALL_BACK_POLICY: Final[str] = (
    "preserve_repeated_exchange_local_interval_labels_with_numeric_utc_offset.v1"
)


@dataclass(frozen=True, slots=True)
class IntervalSeasonalityProviderExplicitSource(ContractModel):
    interval_factors: Sequence[float]
    materialization_method: str = DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class IntervalSeasonalityProviderMaterializedSource(ContractModel):
    source_timestamps_utc: Sequence[datetime]
    source_seasonality_factors: Sequence[float]
    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_interval_minimum_observation_count: int = DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT


@dataclass(frozen=True, slots=True)
class IntervalNewsIntensityProviderExplicitSource(ContractModel):
    interval_factors: Sequence[float]
    materialization_method: str = DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class IntervalNewsIntensityProviderMaterializedSource(ContractModel):
    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_open_baseline_intensity: float
    surprise_closed_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


type IntervalSeasonalityProviderSource = (
    IntervalSeasonalityProviderExplicitSource | IntervalSeasonalityProviderMaterializedSource
)
type IntervalNewsIntensityProviderSource = (
    IntervalNewsIntensityProviderExplicitSource | IntervalNewsIntensityProviderMaterializedSource
)


@dataclass(frozen=True, slots=True)
class IntervalFactorGridOutputSchema(ContractModel):
    """Describes the required fields and policies of interval factor grid outputs."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


INTERVAL_FACTOR_GRID_OUTPUT_SCHEMA: Final[IntervalFactorGridOutputSchema] = IntervalFactorGridOutputSchema(
    schema_version=INTERVAL_FACTOR_GRID_SCHEMA_VERSION,
    required_fields=(
        "schema_version",
        "exchange_code",
        "exchange_timezone_name",
        "request_start_utc_nanos",
        "request_end_utc_nanos",
        "interval_duration_nanos",
        "interval_indices",
        "interval_start_utc_nanos",
        "interval_end_utc_nanos",
        "interval_start_exchange_local",
        "interval_end_exchange_local",
        "seasonality_factor",
        "known_news_intensity",
        "surprise_news_intensity",
        "news_intensity",
        "interval_count",
        "first_interval_start_utc_nanos",
        "last_interval_end_utc_nanos",
        "skipped_exchange_local_interval_count",
        "repeated_exchange_local_interval_count",
        "interval_boundary_policy",
        "interval_alignment_policy",
        "timezone_transition_policy",
        "spring_forward_policy",
        "fall_back_policy",
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
    ),
    interval_boundary_policy="start_inclusive_end_exclusive.v1",
    interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
    required_metadata_fields=(
        "seasonality_materialization_method",
        "news_intensity_materialization_method",
        "interval_boundary_policy",
        "interval_alignment_policy",
        "timezone_transition_policy",
        "spring_forward_policy",
        "fall_back_policy",
    ),
)


@dataclass(frozen=True, slots=True)
class IntervalSeasonalityMaterializationOutputSchema(ContractModel):
    """Describes required fields and policies of interval seasonality materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


INTERVAL_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA: Final[IntervalSeasonalityMaterializationOutputSchema] = (
    IntervalSeasonalityMaterializationOutputSchema(
        schema_version=INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
        required_fields=(
            "schema_version",
            "exchange_code",
            "exchange_timezone_name",
            "request_start_utc_nanos",
            "request_end_utc_nanos",
            "interval_duration_nanos",
            "interval_indices",
            "interval_start_utc_nanos",
            "interval_end_utc_nanos",
            "interval_start_exchange_local",
            "interval_end_exchange_local",
            "seasonality_factor",
            "source_point_count",
            "interval_count",
            "sparse_interval_count",
            "interpolated_interval_count",
            "fallback_interval_count",
            "skipped_exchange_local_interval_count",
            "repeated_exchange_local_interval_count",
            "interpolation_method",
            "aggregation_method",
            "sparse_fallback_policy",
            "sparse_interval_minimum_observation_count",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
        interval_boundary_policy="start_inclusive_end_exclusive.v1",
        interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
        required_metadata_fields=(
            "interpolation_method",
            "aggregation_method",
            "sparse_fallback_policy",
            "sparse_interval_minimum_observation_count",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
    )
)


@dataclass(frozen=True, slots=True)
class IntervalNewsIntensityMaterializationOutputSchema(ContractModel):
    """Describes required fields and policies of interval news-intensity materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


INTERVAL_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA: Final[IntervalNewsIntensityMaterializationOutputSchema] = (
    IntervalNewsIntensityMaterializationOutputSchema(
        schema_version=INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
        required_fields=(
            "schema_version",
            "exchange_code",
            "exchange_timezone_name",
            "request_start_utc_nanos",
            "request_end_utc_nanos",
            "interval_duration_nanos",
            "interval_indices",
            "interval_start_utc_nanos",
            "interval_end_utc_nanos",
            "interval_start_exchange_local",
            "interval_end_exchange_local",
            "known_news_intensity",
            "surprise_news_intensity",
            "news_intensity",
            "interval_count",
            "known_component_non_zero_interval_count",
            "surprise_component_non_zero_interval_count",
            "combined_component_non_zero_interval_count",
            "known_scheduled_arrival_count",
            "surprise_multiplier_override_interval_count",
            "skipped_exchange_local_interval_count",
            "repeated_exchange_local_interval_count",
            "materialization_method",
            "component_aggregation_policy",
            "arrival_intensity_policy",
            "known_news_calendar_anchor_policy",
            "surprise_news_intensity_control_policy",
            "known_news_calendar_anchor_count",
            "surprise_open_baseline_intensity",
            "surprise_closed_baseline_intensity",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
        interval_boundary_policy="start_inclusive_end_exclusive.v1",
        interval_alignment_policy="request_start_anchored_fixed_width_utc_interval_grid.v1",
        required_metadata_fields=(
            "materialization_method",
            "component_aggregation_policy",
            "arrival_intensity_policy",
            "known_news_calendar_anchor_policy",
            "surprise_news_intensity_control_policy",
            "known_news_calendar_anchor_count",
            "surprise_open_baseline_intensity",
            "surprise_closed_baseline_intensity",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
    )
)


@dataclass(frozen=True, slots=True)
class IntervalSeasonalityMaterializationSettings(ContractModel):
    """Configuration for deterministic materialization from source seasonality samples to interval seasonality."""

    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_interval_minimum_observation_count: int = DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT


@dataclass(frozen=True, slots=True)
class IntervalNewsIntensityMaterializationSettings(ContractModel):
    """Configuration for deterministic known and surprise materialization from `nu_news(t)` to interval news intensity."""

    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_open_baseline_intensity: float
    surprise_closed_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


__all__ = [
    "DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_AGGREGATION_METHOD",
    "DEFAULT_INTERVAL_FACTOR_GRID_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_INTERPOLATION_METHOD",
    "DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY",
    "DEFAULT_SPARSE_INTERVAL_MINIMUM_OBSERVATION_COUNT",
    "INTERVAL_FACTOR_GRID_FALL_BACK_POLICY",
    "INTERVAL_FACTOR_GRID_SCHEMA_VERSION",
    "INTERVAL_FACTOR_GRID_OUTPUT_SCHEMA",
    "INTERVAL_FACTOR_GRID_SPRING_FORWARD_POLICY",
    "INTERVAL_FACTOR_GRID_TIMEZONE_TRANSITION_POLICY",
    "INTERVAL_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "INTERVAL_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION",
    "INTERVAL_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "INTERVAL_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION",
    "IntervalFactorGridOutputSchema",
    "IntervalNewsIntensityProviderExplicitSource",
    "IntervalNewsIntensityProviderMaterializedSource",
    "IntervalNewsIntensityProviderSource",
    "IntervalNewsIntensityMaterializationOutputSchema",
    "IntervalNewsIntensityMaterializationSettings",
    "IntervalSeasonalityProviderExplicitSource",
    "IntervalSeasonalityProviderMaterializedSource",
    "IntervalSeasonalityProviderSource",
    "IntervalSeasonalityMaterializationOutputSchema",
    "IntervalSeasonalityMaterializationSettings",
]
