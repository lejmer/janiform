from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Optional, cast

from ..shared.errors import (
    SIMULATION_ERROR_FACTORY,
    SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
    _simulation_error,
    _simulation_integration_error,
)
from ...contracts import (
    ContractModel,
    require_mapping,
    require_non_negative_finite_number,
    require_non_negative_finite_scalar,
    require_positive_finite_number,
    require_non_negative_integer,
    require_positive_finite_scalar,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)
from ..._timestamps import hours_to_exact_nanoseconds
from ...calendar import CalendarRegistry, CalendarRegistryLoadError


NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION = "news.process.configuration.v1"
COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY = "exclude_environment_specific_metadata.v1"
COARSE_SIMULATION_INTERVAL_TIMING_POLICY = (
    "start_time_utc_anchor_plus_fixed_interval_delta_half_open_utc_boundaries.v1"
)
COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY = "interval_index_and_explicit_utc_boundary_timestamps.v1"
COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY = (
    "factor_row_zero_anchors_to_start_time_utc_and_rows_progress_in_request_interval_order_without_implicit_resampling.v1"
)
COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY = (
    "interval_delta_must_map_to_an_exact_fixed_width_utc_nanosecond_grid.v1"
)
def _prefix_nested_field_path(
    field_path: str,
    *,
    prefix: str,
    strip_prefix: Optional[str] = None,
) -> str:
    normalized_field_path = field_path
    if strip_prefix is not None and normalized_field_path.startswith(strip_prefix):
        normalized_field_path = normalized_field_path.removeprefix(strip_prefix)

    if normalized_field_path == "":
        return prefix

    return f"{prefix}.{normalized_field_path}"


def _interval_delta_to_nanos(interval_delta: float, *, field_path: str) -> int:
    require_positive_finite_scalar(interval_delta, field_path, SIMULATION_ERROR_FACTORY)
    try:
        return hours_to_exact_nanoseconds(interval_delta)
    except ValueError:
        raise _simulation_error(
            field_path,
            "must map to an exact fixed-width UTC nanosecond grid",
        )


def _validate_calendar_registry_artifact_payload(
    calendar_registry_artifact: dict[str, Any] | Any,
    *,
    field_path: str,
) -> dict[str, Any]:
    if not isinstance(calendar_registry_artifact, dict):
        raise _simulation_error(field_path, "must be an object")

    try:
        CalendarRegistry(serialize_deterministic_json(calendar_registry_artifact))
    except CalendarRegistryLoadError as e:
        raise _simulation_error(field_path, str(e)) from e
    except ModuleNotFoundError as e:
        raise _simulation_integration_error(
            "native calendar module is unavailable",
            entrypoint="validate_coarse_simulation_run_request",
            error_id=SIMULATION_INTEGRATION_NATIVE_MODULE_ERROR_ID,
            field_path=field_path,
            context={"module_name": "janiform._native"},
        ) from e

    return cast(dict[str, Any], calendar_registry_artifact)


@dataclass(frozen=True, slots=True)
class DoubleExponentialBaseShockConfiguration(ContractModel):
    probability_positive: float
    eta_plus: float
    eta_minus: float

    def __post_init__(self) -> None:
        require_positive_finite_scalar(self.eta_plus, "eta_plus", SIMULATION_ERROR_FACTORY)
        require_positive_finite_scalar(self.eta_minus, "eta_minus", SIMULATION_ERROR_FACTORY)
        require_positive_finite_scalar(
            self.probability_positive,
            "probability_positive",
            SIMULATION_ERROR_FACTORY,
        )
        if self.probability_positive >= 1.0:
            raise _simulation_error("probability_positive", "must be strictly less than 1.0")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> DoubleExponentialBaseShockConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return DoubleExponentialBaseShockConfiguration(
            probability_positive=require_positive_finite_number(
                raw_payload,
                "probability_positive",
                f"{field_path}.probability_positive",
                SIMULATION_ERROR_FACTORY,
            ),
            eta_plus=require_positive_finite_number(
                raw_payload,
                "eta_plus",
                f"{field_path}.eta_plus",
                SIMULATION_ERROR_FACTORY,
            ),
            eta_minus=require_positive_finite_number(
                raw_payload,
                "eta_minus",
                f"{field_path}.eta_minus",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CoupledGapMarkConfiguration(ContractModel):
    sigma_g: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(self.sigma_g, "sigma_g", SIMULATION_ERROR_FACTORY)

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CoupledGapMarkConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return CoupledGapMarkConfiguration(
            sigma_g=require_non_negative_finite_number(
                raw_payload,
                "sigma_g",
                f"{field_path}.sigma_g",
                SIMULATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class SharedNewsMarkLawConfiguration(ContractModel):
    gamma_severity_marks: Any
    double_exponential_base_shock: DoubleExponentialBaseShockConfiguration
    coupled_gap_marks: CoupledGapMarkConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SharedNewsMarkLawConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        from ...pricing import GammaSeverityMarkConfiguration, VanillaOptionPricingValidationError

        try:
            return SharedNewsMarkLawConfiguration(
                gamma_severity_marks=GammaSeverityMarkConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "gamma_severity_marks",
                        f"{field_path}.gamma_severity_marks",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.gamma_severity_marks",
                ),
                double_exponential_base_shock=DoubleExponentialBaseShockConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "double_exponential_base_shock",
                        f"{field_path}.double_exponential_base_shock",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.double_exponential_base_shock",
                ),
                coupled_gap_marks=CoupledGapMarkConfiguration.from_mapping(
                    require_mapping(
                        raw_payload,
                        "coupled_gap_marks",
                        f"{field_path}.coupled_gap_marks",
                        SIMULATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.coupled_gap_marks",
                ),
            )
        except VanillaOptionPricingValidationError as e:
            raise _simulation_error(e.failure.field_path, e.failure.message) from e


@dataclass(frozen=True, slots=True)
class KnownNewsArrivalConfiguration(ContractModel):
    scheduled_event_timestamps_utc_nanos: tuple[int, ...]

    def __post_init__(self) -> None:
        previous_timestamp_utc_nanos: Optional[int] = None
        for timestamp_index, timestamp_utc_nanos in enumerate(self.scheduled_event_timestamps_utc_nanos):
            if previous_timestamp_utc_nanos is not None and timestamp_utc_nanos <= previous_timestamp_utc_nanos:
                raise _simulation_error(
                    f"scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    "must be strictly greater than the previous timestamp",
                )

            previous_timestamp_utc_nanos = timestamp_utc_nanos

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> KnownNewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        scheduled_event_timestamps_utc_nanos = require_sequence(
            raw_payload,
            "scheduled_event_timestamps_utc_nanos",
            f"{field_path}.scheduled_event_timestamps_utc_nanos",
            SIMULATION_ERROR_FACTORY,
        )

        return KnownNewsArrivalConfiguration(
            scheduled_event_timestamps_utc_nanos=tuple(
                require_non_negative_integer(
                    {"value": raw_timestamp_utc_nanos},
                    "value",
                    f"{field_path}.scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    SIMULATION_ERROR_FACTORY,
                )
                for timestamp_index, raw_timestamp_utc_nanos in enumerate(scheduled_event_timestamps_utc_nanos)
            )
        )


@dataclass(frozen=True, slots=True)
class SurpriseNewsArrivalConfiguration(ContractModel):
    baseline_intensity: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(
            self.baseline_intensity,
            "baseline_intensity",
            SIMULATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> SurpriseNewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return SurpriseNewsArrivalConfiguration(
            baseline_intensity=require_non_negative_finite_number(
                raw_payload,
                "baseline_intensity",
                f"{field_path}.baseline_intensity",
                SIMULATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class NewsArrivalConfiguration(ContractModel):
    known: KnownNewsArrivalConfiguration
    surprise: SurpriseNewsArrivalConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> NewsArrivalConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return NewsArrivalConfiguration(
            known=KnownNewsArrivalConfiguration.from_mapping(
                require_mapping(raw_payload, "known", f"{field_path}.known", SIMULATION_ERROR_FACTORY),
                field_path=f"{field_path}.known",
            ),
            surprise=SurpriseNewsArrivalConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "surprise",
                    f"{field_path}.surprise",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.surprise",
            ),
        )


@dataclass(frozen=True, slots=True)
class NewsComponentMarkScaleConfiguration(ContractModel):
    activity_coupling_scale: float
    return_jump_scale: float

    def __post_init__(self) -> None:
        require_non_negative_finite_scalar(
            self.activity_coupling_scale,
            "activity_coupling_scale",
            SIMULATION_ERROR_FACTORY,
        )
        require_non_negative_finite_scalar(
            self.return_jump_scale,
            "return_jump_scale",
            SIMULATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> NewsComponentMarkScaleConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        return NewsComponentMarkScaleConfiguration(
            activity_coupling_scale=require_non_negative_finite_number(
                raw_payload,
                "activity_coupling_scale",
                f"{field_path}.activity_coupling_scale",
                SIMULATION_ERROR_FACTORY,
            ),
            return_jump_scale=require_non_negative_finite_number(
                raw_payload,
                "return_jump_scale",
                f"{field_path}.return_jump_scale",
                SIMULATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class NewsProcessConfiguration(ContractModel):
    schema_version: str
    arrivals: NewsArrivalConfiguration
    shared_mark_law: SharedNewsMarkLawConfiguration
    known_component_mark_scales: NewsComponentMarkScaleConfiguration
    surprise_component_mark_scales: NewsComponentMarkScaleConfiguration

    def __post_init__(self) -> None:
        if self.schema_version != NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION:
            raise _simulation_error(
                "schema_version",
                f"must be {NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION}, found {self.schema_version}",
            )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> NewsProcessConfiguration:
        if not isinstance(raw_payload, dict):
            raise _simulation_error("news_process_configuration", "must be an object")

        return NewsProcessConfiguration(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                "news_process_configuration.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            arrivals=NewsArrivalConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "arrivals",
                    "news_process_configuration.arrivals",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.arrivals",
            ),
            shared_mark_law=SharedNewsMarkLawConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "shared_mark_law",
                    "news_process_configuration.shared_mark_law",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.shared_mark_law",
            ),
            known_component_mark_scales=NewsComponentMarkScaleConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_component_mark_scales",
                    "news_process_configuration.known_component_mark_scales",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.known_component_mark_scales",
            ),
            surprise_component_mark_scales=NewsComponentMarkScaleConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "surprise_component_mark_scales",
                    "news_process_configuration.surprise_component_mark_scales",
                    SIMULATION_ERROR_FACTORY,
                ),
                field_path="news_process_configuration.surprise_component_mark_scales",
            ),
        )


__all__ = [
    "COARSE_SIMULATION_COMPATIBLE_INTERVAL_GRID_POLICY",
    "COARSE_SIMULATION_ENVIRONMENT_METADATA_POLICY",
    "COARSE_SIMULATION_INTERVAL_FACTOR_ALIGNMENT_POLICY",
    "COARSE_SIMULATION_INTERVAL_IDENTIFIER_POLICY",
    "COARSE_SIMULATION_INTERVAL_TIMING_POLICY",
    "NEWS_PROCESS_CONFIGURATION_SCHEMA_VERSION",
    "CoupledGapMarkConfiguration",
    "DoubleExponentialBaseShockConfiguration",
    "KnownNewsArrivalConfiguration",
    "NewsArrivalConfiguration",
    "NewsComponentMarkScaleConfiguration",
    "NewsProcessConfiguration",
    "SharedNewsMarkLawConfiguration",
    "SurpriseNewsArrivalConfiguration",
]
