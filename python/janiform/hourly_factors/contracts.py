"""Public hourly-factor contracts."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime
from typing import Optional

from ..contracts import ContractModel

HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION: str = "seasonality.hourly_factor.contract.v1"
HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION: str = (
    "seasonality.hourly_s_h.materialization.v1"
)
HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION: str = (
    "seasonality.hourly_nu_h.materialization.v1"
)
DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD: str = "explicit_hourly_input.v1"
DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD: str = (
    "known_calendar_anchor_plus_surprise_background_controls.v1"
)
DEFAULT_SEASONALITY_INTERPOLATION_METHOD: str = "previous_observation_carry_forward.v1"
DEFAULT_SEASONALITY_AGGREGATION_METHOD: str = "mean_of_observations.v1"
DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY: str = "global_mean.v1"
DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT: int = 1
HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY: str = (
    "fixed_width_utc_interval_projected_to_exchange_local.v1"
)
HOURLY_FACTOR_SPRING_FORWARD_POLICY: str = "skip_nonexistent_local_hour_labels.v1"
HOURLY_FACTOR_FALL_BACK_POLICY: str = (
    "preserve_repeated_local_hour_labels_with_numeric_utc_offset.v1"
)


@dataclass(frozen=True, slots=True)
class HourlySeasonalityProviderExplicitSource(ContractModel):
    hourly_factors: Sequence[float]
    materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class HourlySeasonalityProviderMaterializedSource(ContractModel):
    source_timestamps_utc: Sequence[datetime]
    source_seasonality_factors: Sequence[float]
    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_hour_minimum_observation_count: int = DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityProviderExplicitSource(ContractModel):
    hourly_factors: Sequence[float]
    materialization_method: str = DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityProviderMaterializedSource(ContractModel):
    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_open_baseline_intensity: float
    surprise_closed_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


type HourlySeasonalityProviderSource = (
    HourlySeasonalityProviderExplicitSource | HourlySeasonalityProviderMaterializedSource
)
type HourlyNewsIntensityProviderSource = (
    HourlyNewsIntensityProviderExplicitSource | HourlyNewsIntensityProviderMaterializedSource
)


@dataclass(frozen=True, slots=True)
class HourlyFactorOutputSchema(ContractModel):
    """Describes the required fields and policies of hourly factor outputs."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_FACTOR_OUTPUT_SCHEMA: HourlyFactorOutputSchema = HourlyFactorOutputSchema(
    schema_version=HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION,
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
        "s_h",
        "known_component_nu_h",
        "surprise_component_nu_h",
        "nu_h",
        "interval_count",
        "first_interval_start_utc_nanos",
        "last_interval_end_utc_nanos",
        "skipped_local_hour_interval_count",
        "repeated_local_hour_interval_count",
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
class HourlySeasonalityMaterializationOutputSchema(ContractModel):
    """Describes required fields and policies of hourly seasonality materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA: HourlySeasonalityMaterializationOutputSchema = (
    HourlySeasonalityMaterializationOutputSchema(
        schema_version=HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION,
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
            "s_h",
            "source_point_count",
            "interval_count",
            "sparse_interval_count",
            "interpolated_interval_count",
            "fallback_interval_count",
            "skipped_local_hour_interval_count",
            "repeated_local_hour_interval_count",
            "interpolation_method",
            "aggregation_method",
            "sparse_fallback_policy",
            "sparse_hour_minimum_observation_count",
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
            "sparse_hour_minimum_observation_count",
            "interval_boundary_policy",
            "interval_alignment_policy",
            "timezone_transition_policy",
            "spring_forward_policy",
            "fall_back_policy",
        ),
    )
)


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityMaterializationOutputSchema(ContractModel):
    """Describes required fields and policies of hourly news-intensity materialization output."""

    schema_version: str
    required_fields: tuple[str, ...]
    interval_boundary_policy: str
    interval_alignment_policy: str
    required_metadata_fields: tuple[str, ...]


HOURLY_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA: HourlyNewsIntensityMaterializationOutputSchema = (
    HourlyNewsIntensityMaterializationOutputSchema(
        schema_version=HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION,
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
            "known_component_nu_h",
            "surprise_component_nu_h",
            "nu_h",
            "interval_count",
            "known_component_non_zero_interval_count",
            "surprise_component_non_zero_interval_count",
            "combined_component_non_zero_interval_count",
            "known_scheduled_arrival_count",
            "surprise_multiplier_override_interval_count",
            "skipped_local_hour_interval_count",
            "repeated_local_hour_interval_count",
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
class HourlySeasonalityMaterializationSettings(ContractModel):
    """Configuration for deterministic materialization from source seasonality samples to hourly `s_h`."""

    interpolation_method: str = DEFAULT_SEASONALITY_INTERPOLATION_METHOD
    aggregation_method: str = DEFAULT_SEASONALITY_AGGREGATION_METHOD
    sparse_fallback_policy: str = DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY
    sparse_hour_minimum_observation_count: int = DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT


@dataclass(frozen=True, slots=True)
class HourlyNewsIntensityMaterializationSettings(ContractModel):
    """Configuration for deterministic known and surprise materialization from `nu_news(t)` to hourly `nu_h`."""

    known_scheduled_event_timestamps_utc: Sequence[datetime]
    surprise_open_baseline_intensity: float
    surprise_closed_baseline_intensity: float
    surprise_deterministic_time_multiplier_by_interval_index: Optional[Mapping[int, float]] = None


__all__ = [
    "DEFAULT_NEWS_INTENSITY_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_AGGREGATION_METHOD",
    "DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD",
    "DEFAULT_SEASONALITY_INTERPOLATION_METHOD",
    "DEFAULT_SEASONALITY_SPARSE_FALLBACK_POLICY",
    "DEFAULT_SPARSE_HOUR_MINIMUM_OBSERVATION_COUNT",
    "HOURLY_FACTOR_FALL_BACK_POLICY",
    "HOURLY_FACTOR_CONTRACT_SCHEMA_VERSION",
    "HOURLY_FACTOR_OUTPUT_SCHEMA",
    "HOURLY_FACTOR_SPRING_FORWARD_POLICY",
    "HOURLY_FACTOR_TIMEZONE_TRANSITION_POLICY",
    "HOURLY_NEWS_INTENSITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "HOURLY_NEWS_INTENSITY_MATERIALIZATION_SCHEMA_VERSION",
    "HOURLY_SEASONALITY_MATERIALIZATION_OUTPUT_SCHEMA",
    "HOURLY_SEASONALITY_MATERIALIZATION_SCHEMA_VERSION",
    "HourlyFactorOutputSchema",
    "HourlyNewsIntensityProviderExplicitSource",
    "HourlyNewsIntensityProviderMaterializedSource",
    "HourlyNewsIntensityProviderSource",
    "HourlyNewsIntensityMaterializationOutputSchema",
    "HourlyNewsIntensityMaterializationSettings",
    "HourlySeasonalityProviderExplicitSource",
    "HourlySeasonalityProviderMaterializedSource",
    "HourlySeasonalityProviderSource",
    "HourlySeasonalityMaterializationOutputSchema",
    "HourlySeasonalityMaterializationSettings",
]
