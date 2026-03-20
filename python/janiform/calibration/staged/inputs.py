from dataclasses import dataclass
from enum import StrEnum
from typing import TYPE_CHECKING, Any, Optional

from .contracts import (
    CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION,
    CalibrationExecutionMode,
    CalibrationStage,
    CalibrationTradeSign,
)
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error, _require_bool
from ..._contracts import (
    ContractModel,
    parse_json_object,
    require_finite_number,
    require_integer,
    require_mapping,
    require_non_empty_string,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    require_string,
    serialize_deterministic_json,
)

if TYPE_CHECKING:
    from .request import StagedCalibrationRequest
    from ...pricing import GammaSeverityMarkConfiguration


def _parse_gamma_severity_mark_configuration(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> Any:
    from ...pricing import GammaSeverityMarkConfiguration

    return GammaSeverityMarkConfiguration.from_mapping(raw_payload, field_path=field_path)


class CalibrationTimestampEventSessionFilter(StrEnum):
    ALL = 'all'
    OPEN_ONLY = 'open_only'
    CLOSED_ONLY = 'closed_only'


class CalibrationIntradaySmoothingKernel(StrEnum):
    GAUSSIAN = 'gaussian'
    UNIFORM = 'uniform'


def _require_timestamp_utc_nanos(raw_payload: dict[str, Any], field_name: str, field_path: str) -> int:
    return require_integer(raw_payload, field_name, field_path, CALIBRATION_ERROR_FACTORY)


@dataclass(frozen=True, slots=True)
class CalibrationTimestampEventRecord(ContractModel):
    source_record_position: int
    timestamp_utc_nanos: int

    def __post_init__(self) -> None:
        if self.source_record_position < 0:
            raise _calibration_error("source_record_position", "must be non-negative")

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationTimestampEventRecord:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationTimestampEventRecord(
            source_record_position=require_non_negative_integer(
                raw_payload,
                "source_record_position",
                f"{field_path}.source_record_position",
                CALIBRATION_ERROR_FACTORY,
            ),
            timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "timestamp_utc_nanos",
                f"{field_path}.timestamp_utc_nanos",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationScheduledEventOverlay(ContractModel):
    overlay_id: str
    start_timestamp_utc_nanos: int
    end_timestamp_utc_nanos: int
    log_adjustment: float
    precedence: int
    enabled: bool
    source_label: str

    def __post_init__(self) -> None:
        if not self.overlay_id.strip():
            raise _calibration_error("overlay_id", "must be a non-empty string")
        if not self.source_label.strip():
            raise _calibration_error("source_label", "must be a non-empty string")
        if self.precedence < 0:
            raise _calibration_error("precedence", "must be non-negative")

        require_finite_number(
            {"log_adjustment": self.log_adjustment},
            "log_adjustment",
            "log_adjustment",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationScheduledEventOverlay:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationScheduledEventOverlay(
            overlay_id=require_non_empty_string(
                raw_payload,
                "overlay_id",
                f"{field_path}.overlay_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            start_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "start_timestamp_utc_nanos",
                f"{field_path}.start_timestamp_utc_nanos",
            ),
            end_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "end_timestamp_utc_nanos",
                f"{field_path}.end_timestamp_utc_nanos",
            ),
            log_adjustment=require_finite_number(
                raw_payload,
                "log_adjustment",
                f"{field_path}.log_adjustment",
                CALIBRATION_ERROR_FACTORY,
            ),
            precedence=require_non_negative_integer(
                raw_payload,
                "precedence",
                f"{field_path}.precedence",
                CALIBRATION_ERROR_FACTORY,
            ),
            enabled=_require_bool(raw_payload, "enabled", f"{field_path}.enabled"),
            source_label=require_non_empty_string(
                raw_payload,
                "source_label",
                f"{field_path}.source_label",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationIntradaySeasonalityEstimatorSettings(ContractModel):
    minutes_per_bin: int
    smoothing_kernel: CalibrationIntradaySmoothingKernel
    smoothing_bandwidth_bins: float
    minimum_bin_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minutes_per_bin <= 0:
            raise _calibration_error("minutes_per_bin", "must be strictly positive")
        if self.minimum_bin_events <= 0:
            raise _calibration_error("minimum_bin_events", "must be strictly positive")
        if self.smoothing_bandwidth_bins <= 0.0:
            raise _calibration_error("smoothing_bandwidth_bins", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationIntradaySeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationIntradaySeasonalityEstimatorSettings(
            minutes_per_bin=require_positive_integer(
                raw_payload,
                "minutes_per_bin",
                f"{field_path}.minutes_per_bin",
                CALIBRATION_ERROR_FACTORY,
            ),
            smoothing_kernel=CalibrationIntradaySmoothingKernel(
                require_string(
                    raw_payload,
                    "smoothing_kernel",
                    f"{field_path}.smoothing_kernel",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            smoothing_bandwidth_bins=require_positive_finite_number(
                raw_payload,
                "smoothing_bandwidth_bins",
                f"{field_path}.smoothing_bandwidth_bins",
                CALIBRATION_ERROR_FACTORY,
            ),
            minimum_bin_events=require_positive_integer(
                raw_payload,
                "minimum_bin_events",
                f"{field_path}.minimum_bin_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDayOfWeekSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minimum_bucket_events <= 0:
            raise _calibration_error("minimum_bucket_events", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDayOfWeekSeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDayOfWeekSeasonalityEstimatorSettings(
            minimum_bucket_events=require_positive_integer(
                raw_payload,
                "minimum_bucket_events",
                f"{field_path}.minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDayOfYearSeasonalityEstimatorSettings(ContractModel):
    minimum_bucket_events: int
    pseudocount: float

    def __post_init__(self) -> None:
        if self.minimum_bucket_events <= 0:
            raise _calibration_error("minimum_bucket_events", "must be strictly positive")
        if self.pseudocount <= 0.0:
            raise _calibration_error("pseudocount", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDayOfYearSeasonalityEstimatorSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDayOfYearSeasonalityEstimatorSettings(
            minimum_bucket_events=require_positive_integer(
                raw_payload,
                "minimum_bucket_events",
                f"{field_path}.minimum_bucket_events",
                CALIBRATION_ERROR_FACTORY,
            ),
            pseudocount=require_positive_finite_number(
                raw_payload,
                "pseudocount",
                f"{field_path}.pseudocount",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationSeasonalityStageExecutionSettings(ContractModel):
    session_filter: CalibrationTimestampEventSessionFilter
    sparse_hour_event_threshold: int
    intraday_settings: CalibrationIntradaySeasonalityEstimatorSettings
    day_of_week_settings: CalibrationDayOfWeekSeasonalityEstimatorSettings
    day_of_year_settings: CalibrationDayOfYearSeasonalityEstimatorSettings

    def __post_init__(self) -> None:
        if self.sparse_hour_event_threshold < 0:
            raise _calibration_error("sparse_hour_event_threshold", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSeasonalityStageExecutionSettings:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationSeasonalityStageExecutionSettings(
            session_filter=CalibrationTimestampEventSessionFilter(
                require_string(
                    raw_payload,
                    "session_filter",
                    f"{field_path}.session_filter",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            sparse_hour_event_threshold=require_non_negative_integer(
                raw_payload,
                "sparse_hour_event_threshold",
                f"{field_path}.sparse_hour_event_threshold",
                CALIBRATION_ERROR_FACTORY,
            ),
            intraday_settings=CalibrationIntradaySeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "intraday_settings",
                    f"{field_path}.intraday_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.intraday_settings",
            ),
            day_of_week_settings=CalibrationDayOfWeekSeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "day_of_week_settings",
                    f"{field_path}.day_of_week_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.day_of_week_settings",
            ),
            day_of_year_settings=CalibrationDayOfYearSeasonalityEstimatorSettings.from_mapping(
                require_mapping(
                    raw_payload,
                    "day_of_year_settings",
                    f"{field_path}.day_of_year_settings",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.day_of_year_settings",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationSeasonalityStageExecutionInput(ContractModel):
    timestamp_event_records: tuple[CalibrationTimestampEventRecord, ...]
    scheduled_event_overlays: tuple[CalibrationScheduledEventOverlay, ...] = ()
    settings: Optional[CalibrationSeasonalityStageExecutionSettings] = None
    dataset_id_override: Optional[str] = None
    estimator_version_override: Optional[str] = None

    def __post_init__(self) -> None:
        if len(self.timestamp_event_records) == 0:
            raise _calibration_error("timestamp_event_records", "must include at least one event record")
        if self.dataset_id_override is not None and not self.dataset_id_override.strip():
            raise _calibration_error("dataset_id_override", "must be non-empty when provided")
        if self.estimator_version_override is not None and not self.estimator_version_override.strip():
            raise _calibration_error("estimator_version_override", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSeasonalityStageExecutionInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_records = require_sequence(
            raw_payload,
            "timestamp_event_records",
            f"{field_path}.timestamp_event_records",
            CALIBRATION_ERROR_FACTORY,
        )
        raw_overlays = raw_payload.get("scheduled_event_overlays", [])
        if isinstance(raw_overlays, str) or not isinstance(raw_overlays, list):
            raise _calibration_error(f"{field_path}.scheduled_event_overlays", "must be a sequence")

        dataset_id_override = raw_payload.get("dataset_id_override")
        estimator_version_override = raw_payload.get("estimator_version_override")
        if dataset_id_override is not None and not isinstance(dataset_id_override, str):
            raise _calibration_error(f"{field_path}.dataset_id_override", "must be a string when provided")
        if estimator_version_override is not None and not isinstance(estimator_version_override, str):
            raise _calibration_error(
                f"{field_path}.estimator_version_override",
                "must be a string when provided",
            )

        return CalibrationSeasonalityStageExecutionInput(
            timestamp_event_records=tuple(
                CalibrationTimestampEventRecord.from_mapping(
                    raw_record,
                    field_path=f"{field_path}.timestamp_event_records[{record_index}]",
                )
                for record_index, raw_record in enumerate(raw_records)
            ),
            scheduled_event_overlays=tuple(
                CalibrationScheduledEventOverlay.from_mapping(
                    raw_overlay,
                    field_path=f"{field_path}.scheduled_event_overlays[{overlay_index}]",
                )
                for overlay_index, raw_overlay in enumerate(raw_overlays)
            ),
            settings=(
                CalibrationSeasonalityStageExecutionSettings.from_mapping(
                    require_mapping(
                        raw_payload,
                        "settings",
                        f"{field_path}.settings",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.settings",
                )
                if "settings" in raw_payload and raw_payload["settings"] is not None
                else None
            ),
            dataset_id_override=dataset_id_override,
            estimator_version_override=estimator_version_override,
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredAlphaParameterization(ContractModel):
    a_self: float
    a_cross: float
    delta_self: float
    delta_cross: float

    def __post_init__(self) -> None:
        if self.a_self <= 0.0:
            raise _calibration_error("a_self", "must be strictly positive")
        if self.a_cross < 0.0:
            raise _calibration_error("a_cross", "must be non-negative")
        if self.delta_self < 0.0:
            raise _calibration_error("delta_self", "must be non-negative")
        if self.delta_cross < 0.0:
            raise _calibration_error("delta_cross", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredAlphaParameterization:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredAlphaParameterization(
            a_self=require_positive_finite_number(
                raw_payload,
                "a_self",
                f"{field_path}.a_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            a_cross=require_non_negative_finite_number(
                raw_payload,
                "a_cross",
                f"{field_path}.a_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_self=require_non_negative_finite_number(
                raw_payload,
                "delta_self",
                f"{field_path}.delta_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_cross=require_non_negative_finite_number(
                raw_payload,
                "delta_cross",
                f"{field_path}.delta_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredRhoParameterization(ContractModel):
    r_self: float
    r_cross: float
    delta_self_r: float
    delta_cross_r: float

    def __post_init__(self) -> None:
        if self.r_self <= 0.0:
            raise _calibration_error("r_self", "must be strictly positive")
        if self.r_cross < 0.0:
            raise _calibration_error("r_cross", "must be non-negative")
        if self.delta_self_r < 0.0:
            raise _calibration_error("delta_self_r", "must be non-negative")
        if self.delta_cross_r < 0.0:
            raise _calibration_error("delta_cross_r", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredRhoParameterization:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredRhoParameterization(
            r_self=require_positive_finite_number(
                raw_payload,
                "r_self",
                f"{field_path}.r_self",
                CALIBRATION_ERROR_FACTORY,
            ),
            r_cross=require_non_negative_finite_number(
                raw_payload,
                "r_cross",
                f"{field_path}.r_cross",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_self_r=require_non_negative_finite_number(
                raw_payload,
                "delta_self_r",
                f"{field_path}.delta_self_r",
                CALIBRATION_ERROR_FACTORY,
            ),
            delta_cross_r=require_non_negative_finite_number(
                raw_payload,
                "delta_cross_r",
                f"{field_path}.delta_cross_r",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStructuredRhoSelfDominancePolicy(ContractModel):
    minimum_self_to_cross_ratio: float

    def __post_init__(self) -> None:
        if self.minimum_self_to_cross_ratio < 0.0:
            raise _calibration_error("minimum_self_to_cross_ratio", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStructuredRhoSelfDominancePolicy:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStructuredRhoSelfDominancePolicy(
            minimum_self_to_cross_ratio=require_non_negative_finite_number(
                raw_payload,
                "minimum_self_to_cross_ratio",
                f"{field_path}.minimum_self_to_cross_ratio",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationObjectiveTermInput(ContractModel):
    term_id: str
    weight: float
    observed_score: float

    def __post_init__(self) -> None:
        if not self.term_id.strip():
            raise _calibration_error("term_id", "must be a non-empty string")
        if self.weight < 0.0:
            raise _calibration_error("weight", "must be non-negative")

        require_finite_number(
            {"observed_score": self.observed_score},
            "observed_score",
            "observed_score",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any, *, field_path: str) -> CalibrationObjectiveTermInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationObjectiveTermInput(
            term_id=require_non_empty_string(
                raw_payload,
                "term_id",
                f"{field_path}.term_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            weight=require_non_negative_finite_number(
                raw_payload,
                "weight",
                f"{field_path}.weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            observed_score=require_finite_number(
                raw_payload,
                "observed_score",
                f"{field_path}.observed_score",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesStageCandidateInput(ContractModel):
    candidate_id: str
    lambda_bar: float
    beta: float
    structured_alpha: CalibrationHawkesStructuredAlphaParameterization
    structured_rho: CalibrationHawkesStructuredRhoParameterization
    source_regime: CalibrationHawkesObservedSourceRegime
    directional_label_diagnostics: CalibrationHawkesDirectionalLabelDiagnostics
    self_dominance_policy: Optional[CalibrationHawkesStructuredRhoSelfDominancePolicy] = None
    stability_numeric_tolerance: float = 0.0

    def __post_init__(self) -> None:
        if not self.candidate_id.strip():
            raise _calibration_error("candidate_id", "must be a non-empty string")

        require_finite_number({"lambda_bar": self.lambda_bar}, "lambda_bar", "lambda_bar", CALIBRATION_ERROR_FACTORY)
        require_finite_number({"beta": self.beta}, "beta", "beta", CALIBRATION_ERROR_FACTORY)
        require_non_negative_finite_number(
            {"stability_numeric_tolerance": self.stability_numeric_tolerance},
            "stability_numeric_tolerance",
            "stability_numeric_tolerance",
            CALIBRATION_ERROR_FACTORY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesStageCandidateInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesStageCandidateInput(
            candidate_id=require_non_empty_string(
                raw_payload,
                "candidate_id",
                f"{field_path}.candidate_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            lambda_bar=require_finite_number(
                raw_payload,
                "lambda_bar",
                f"{field_path}.lambda_bar",
                CALIBRATION_ERROR_FACTORY,
            ),
            beta=require_finite_number(
                raw_payload,
                "beta",
                f"{field_path}.beta",
                CALIBRATION_ERROR_FACTORY,
            ),
            structured_alpha=CalibrationHawkesStructuredAlphaParameterization.from_mapping(
                require_mapping(
                    raw_payload,
                    "structured_alpha",
                    f"{field_path}.structured_alpha",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.structured_alpha",
            ),
            structured_rho=CalibrationHawkesStructuredRhoParameterization.from_mapping(
                require_mapping(
                    raw_payload,
                    "structured_rho",
                    f"{field_path}.structured_rho",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.structured_rho",
            ),
            self_dominance_policy=(
                CalibrationHawkesStructuredRhoSelfDominancePolicy.from_mapping(
                    require_mapping(
                        raw_payload,
                        "self_dominance_policy",
                        f"{field_path}.self_dominance_policy",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path=f"{field_path}.self_dominance_policy",
                )
                if "self_dominance_policy" in raw_payload and raw_payload["self_dominance_policy"] is not None
                else None
            ),
            stability_numeric_tolerance=require_non_negative_finite_number(
                raw_payload,
                "stability_numeric_tolerance",
                f"{field_path}.stability_numeric_tolerance",
                CALIBRATION_ERROR_FACTORY,
            ),
            source_regime=CalibrationHawkesObservedSourceRegime.from_mapping(
                require_mapping(
                    raw_payload,
                    "source_regime",
                    f"{field_path}.source_regime",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.source_regime",
            ),
            directional_label_diagnostics=CalibrationHawkesDirectionalLabelDiagnostics.from_mapping(
                require_mapping(
                    raw_payload,
                    "directional_label_diagnostics",
                    f"{field_path}.directional_label_diagnostics",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.directional_label_diagnostics",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesObservedSourceRegime(ContractModel):
    observation_duration_hours: float
    open_session_duration_hours: float
    total_tick_count: int
    open_session_tick_count: int
    closed_session_tick_count: int
    open_session_buy_tick_count: int
    open_session_sell_tick_count: int
    open_session_mean_interarrival_seconds: float
    open_session_interarrival_coefficient_of_variation: float
    open_session_short_gap_threshold_seconds: float
    open_session_short_gap_ratio: float
    known_news_open_event_count: int
    open_session_known_news_activity_share: float

    def __post_init__(self) -> None:
        require_positive_finite_number(
            {"observation_duration_hours": self.observation_duration_hours},
            "observation_duration_hours",
            "observation_duration_hours",
            CALIBRATION_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"open_session_duration_hours": self.open_session_duration_hours},
            "open_session_duration_hours",
            "open_session_duration_hours",
            CALIBRATION_ERROR_FACTORY,
        )

        if self.total_tick_count <= 0:
            raise _calibration_error("total_tick_count", "must be strictly positive")
        if self.open_session_tick_count <= 0:
            raise _calibration_error("open_session_tick_count", "must be strictly positive")
        if self.closed_session_tick_count < 0:
            raise _calibration_error("closed_session_tick_count", "must be non-negative")
        if self.open_session_buy_tick_count < 0:
            raise _calibration_error("open_session_buy_tick_count", "must be non-negative")
        if self.open_session_sell_tick_count < 0:
            raise _calibration_error("open_session_sell_tick_count", "must be non-negative")
        if self.open_session_buy_tick_count + self.open_session_sell_tick_count != self.open_session_tick_count:
            raise _calibration_error(
                "open_session_sell_tick_count",
                "open-session buy and sell counts must sum to open_session_tick_count",
            )
        if self.closed_session_tick_count + self.open_session_tick_count != self.total_tick_count:
            raise _calibration_error(
                "closed_session_tick_count",
                "open and closed tick counts must sum to total_tick_count",
            )
        if self.known_news_open_event_count < 0:
            raise _calibration_error("known_news_open_event_count", "must be non-negative")

        require_positive_finite_number(
            {"open_session_mean_interarrival_seconds": self.open_session_mean_interarrival_seconds},
            "open_session_mean_interarrival_seconds",
            "open_session_mean_interarrival_seconds",
            CALIBRATION_ERROR_FACTORY,
        )
        require_non_negative_finite_number(
            {
                "open_session_interarrival_coefficient_of_variation": (
                    self.open_session_interarrival_coefficient_of_variation
                )
            },
            "open_session_interarrival_coefficient_of_variation",
            "open_session_interarrival_coefficient_of_variation",
            CALIBRATION_ERROR_FACTORY,
        )
        require_positive_finite_number(
            {"open_session_short_gap_threshold_seconds": self.open_session_short_gap_threshold_seconds},
            "open_session_short_gap_threshold_seconds",
            "open_session_short_gap_threshold_seconds",
            CALIBRATION_ERROR_FACTORY,
        )
        require_non_negative_finite_number(
            {"open_session_short_gap_ratio": self.open_session_short_gap_ratio},
            "open_session_short_gap_ratio",
            "open_session_short_gap_ratio",
            CALIBRATION_ERROR_FACTORY,
        )

        if self.open_session_short_gap_ratio > 1.0:
            raise _calibration_error("open_session_short_gap_ratio", "must be less than or equal to 1")

        require_non_negative_finite_number(
            {
                "open_session_known_news_activity_share": self.open_session_known_news_activity_share
            },
            "open_session_known_news_activity_share",
            "open_session_known_news_activity_share",
            CALIBRATION_ERROR_FACTORY,
        )

        if self.open_session_known_news_activity_share > 1.0:
            raise _calibration_error(
                "open_session_known_news_activity_share",
                "must be less than or equal to 1",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesObservedSourceRegime:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesObservedSourceRegime(
            observation_duration_hours=require_positive_finite_number(
                raw_payload,
                "observation_duration_hours",
                f"{field_path}.observation_duration_hours",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_duration_hours=require_positive_finite_number(
                raw_payload,
                "open_session_duration_hours",
                f"{field_path}.open_session_duration_hours",
                CALIBRATION_ERROR_FACTORY,
            ),
            total_tick_count=require_positive_integer(
                raw_payload,
                "total_tick_count",
                f"{field_path}.total_tick_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_tick_count=require_positive_integer(
                raw_payload,
                "open_session_tick_count",
                f"{field_path}.open_session_tick_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            closed_session_tick_count=require_non_negative_integer(
                raw_payload,
                "closed_session_tick_count",
                f"{field_path}.closed_session_tick_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_buy_tick_count=require_non_negative_integer(
                raw_payload,
                "open_session_buy_tick_count",
                f"{field_path}.open_session_buy_tick_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_sell_tick_count=require_non_negative_integer(
                raw_payload,
                "open_session_sell_tick_count",
                f"{field_path}.open_session_sell_tick_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_mean_interarrival_seconds=require_positive_finite_number(
                raw_payload,
                "open_session_mean_interarrival_seconds",
                f"{field_path}.open_session_mean_interarrival_seconds",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_interarrival_coefficient_of_variation=require_non_negative_finite_number(
                raw_payload,
                "open_session_interarrival_coefficient_of_variation",
                f"{field_path}.open_session_interarrival_coefficient_of_variation",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_short_gap_threshold_seconds=require_positive_finite_number(
                raw_payload,
                "open_session_short_gap_threshold_seconds",
                f"{field_path}.open_session_short_gap_threshold_seconds",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_short_gap_ratio=require_non_negative_finite_number(
                raw_payload,
                "open_session_short_gap_ratio",
                f"{field_path}.open_session_short_gap_ratio",
                CALIBRATION_ERROR_FACTORY,
            ),
            known_news_open_event_count=require_non_negative_integer(
                raw_payload,
                "known_news_open_event_count",
                f"{field_path}.known_news_open_event_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_session_known_news_activity_share=require_non_negative_finite_number(
                raw_payload,
                "open_session_known_news_activity_share",
                f"{field_path}.open_session_known_news_activity_share",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationHawkesDirectionalLabelDiagnostics(ContractModel):
    policy_id: str
    first_inferred_trade_sign: CalibrationTradeSign
    provided_label_count: int
    inferred_label_count: int
    flat_move_carry_count: int
    price_up_inference_count: int
    price_down_inference_count: int

    def __post_init__(self) -> None:
        if not self.policy_id.strip():
            raise _calibration_error("policy_id", "must be a non-empty string")
        if self.provided_label_count < 0:
            raise _calibration_error("provided_label_count", "must be non-negative")
        if self.inferred_label_count < 0:
            raise _calibration_error("inferred_label_count", "must be non-negative")
        if self.flat_move_carry_count < 0:
            raise _calibration_error("flat_move_carry_count", "must be non-negative")
        if self.price_up_inference_count < 0:
            raise _calibration_error("price_up_inference_count", "must be non-negative")
        if self.price_down_inference_count < 0:
            raise _calibration_error("price_down_inference_count", "must be non-negative")
        if (
            self.price_up_inference_count
            + self.price_down_inference_count
            + self.flat_move_carry_count
            > self.inferred_label_count
        ):
            raise _calibration_error(
                "inferred_label_count",
                "directional inference counters must not exceed inferred_label_count",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationHawkesDirectionalLabelDiagnostics:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationHawkesDirectionalLabelDiagnostics(
            policy_id=require_non_empty_string(
                raw_payload,
                "policy_id",
                f"{field_path}.policy_id",
                CALIBRATION_ERROR_FACTORY,
            ),
            first_inferred_trade_sign=CalibrationTradeSign(
                require_string(
                    raw_payload,
                    "first_inferred_trade_sign",
                    f"{field_path}.first_inferred_trade_sign",
                    CALIBRATION_ERROR_FACTORY,
                )
            ),
            provided_label_count=require_non_negative_integer(
                raw_payload,
                "provided_label_count",
                f"{field_path}.provided_label_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            inferred_label_count=require_non_negative_integer(
                raw_payload,
                "inferred_label_count",
                f"{field_path}.inferred_label_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            flat_move_carry_count=require_non_negative_integer(
                raw_payload,
                "flat_move_carry_count",
                f"{field_path}.flat_move_carry_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            price_up_inference_count=require_non_negative_integer(
                raw_payload,
                "price_up_inference_count",
                f"{field_path}.price_up_inference_count",
                CALIBRATION_ERROR_FACTORY,
            ),
            price_down_inference_count=require_non_negative_integer(
                raw_payload,
                "price_down_inference_count",
                f"{field_path}.price_down_inference_count",
                CALIBRATION_ERROR_FACTORY,
            ),
        )

@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsSignalSourceWindow(ContractModel):
    window_start_utc_nanos: int
    window_end_utc_nanos: int

    def __post_init__(self) -> None:
        if self.window_end_utc_nanos <= self.window_start_utc_nanos:
            raise _calibration_error("window_end_utc_nanos", "must be strictly greater than window_start_utc_nanos")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsSignalSourceWindow:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsSignalSourceWindow(
            window_start_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "window_start_utc_nanos",
                f"{field_path}.window_start_utc_nanos",
            ),
            window_end_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "window_end_utc_nanos",
                f"{field_path}.window_end_utc_nanos",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsKnownNewsCalendarInput(ContractModel):
    source_window: CalibrationReturnComponentsSignalSourceWindow
    scheduled_event_timestamps_utc_nanos: tuple[int, ...]

    def __post_init__(self) -> None:
        previous_timestamp: Optional[int] = None
        for timestamp_utc_nanos in self.scheduled_event_timestamps_utc_nanos:
            if previous_timestamp is not None and timestamp_utc_nanos <= previous_timestamp:
                raise _calibration_error(
                    "scheduled_event_timestamps_utc_nanos",
                    "must be strictly increasing",
                )
            previous_timestamp = timestamp_utc_nanos

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsKnownNewsCalendarInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        raw_timestamps = require_sequence(
            raw_payload,
            "scheduled_event_timestamps_utc_nanos",
            f"{field_path}.scheduled_event_timestamps_utc_nanos",
            CALIBRATION_ERROR_FACTORY,
        )

        return CalibrationReturnComponentsKnownNewsCalendarInput(
            source_window=CalibrationReturnComponentsSignalSourceWindow.from_mapping(
                require_mapping(
                    raw_payload,
                    "source_window",
                    f"{field_path}.source_window",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.source_window",
            ),
            scheduled_event_timestamps_utc_nanos=tuple(
                require_integer(
                    {"value": raw_timestamp},
                    "value",
                    f"{field_path}.scheduled_event_timestamps_utc_nanos[{timestamp_index}]",
                    CALIBRATION_ERROR_FACTORY,
                )
                for timestamp_index, raw_timestamp in enumerate(raw_timestamps)
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsKnownNewsReturnSample(ContractModel):
    event_timestamp_utc_nanos: int
    open_window_jump_return: float
    closed_window_gap_return: float
    post_shock_activity_lift: float

    def __post_init__(self) -> None:
        require_finite_number(
            {"open_window_jump_return": self.open_window_jump_return},
            "open_window_jump_return",
            "open_window_jump_return",
            CALIBRATION_ERROR_FACTORY,
        )
        require_finite_number(
            {"closed_window_gap_return": self.closed_window_gap_return},
            "closed_window_gap_return",
            "closed_window_gap_return",
            CALIBRATION_ERROR_FACTORY,
        )
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsKnownNewsReturnSample:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsKnownNewsReturnSample(
            event_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "event_timestamp_utc_nanos",
                f"{field_path}.event_timestamp_utc_nanos",
            ),
            open_window_jump_return=require_finite_number(
                raw_payload,
                "open_window_jump_return",
                f"{field_path}.open_window_jump_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            closed_window_gap_return=require_finite_number(
                raw_payload,
                "closed_window_gap_return",
                f"{field_path}.closed_window_gap_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsDetectedIntradayJumpCandidate(ContractModel):
    jump_timestamp_utc_nanos: int
    jump_return: float
    post_shock_activity_lift: float

    def __post_init__(self) -> None:
        require_finite_number({"jump_return": self.jump_return}, "jump_return", "jump_return", CALIBRATION_ERROR_FACTORY)
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsDetectedIntradayJumpCandidate:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsDetectedIntradayJumpCandidate(
            jump_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "jump_timestamp_utc_nanos",
                f"{field_path}.jump_timestamp_utc_nanos",
            ),
            jump_return=require_finite_number(
                raw_payload,
                "jump_return",
                f"{field_path}.jump_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsObservedOpenSessionReturn(ContractModel):
    return_timestamp_utc_nanos: int
    log_return: float
    post_shock_activity_lift: float
    sell_share_proxy: float

    def __post_init__(self) -> None:
        require_finite_number({"log_return": self.log_return}, "log_return", "log_return", CALIBRATION_ERROR_FACTORY)
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")
        if self.sell_share_proxy < 0.0 or self.sell_share_proxy > 1.0:
            raise _calibration_error("sell_share_proxy", "must be between 0 and 1")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsObservedOpenSessionReturn:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsObservedOpenSessionReturn(
            return_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "return_timestamp_utc_nanos",
                f"{field_path}.return_timestamp_utc_nanos",
            ),
            log_return=require_finite_number(
                raw_payload,
                "log_return",
                f"{field_path}.log_return",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
            sell_share_proxy=require_non_negative_finite_number(
                raw_payload,
                "sell_share_proxy",
                f"{field_path}.sell_share_proxy",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(ContractModel):
    close_timestamp_utc_nanos: int
    close_price: float
    open_timestamp_utc_nanos: int
    open_price: float
    post_shock_activity_lift: float

    def __post_init__(self) -> None:
        if self.open_timestamp_utc_nanos <= self.close_timestamp_utc_nanos:
            raise _calibration_error("open_timestamp_utc_nanos", "must be strictly greater than close_timestamp_utc_nanos")

        require_positive_finite_number({"close_price": self.close_price}, "close_price", "close_price", CALIBRATION_ERROR_FACTORY)
        require_positive_finite_number({"open_price": self.open_price}, "open_price", "open_price", CALIBRATION_ERROR_FACTORY)
        if self.post_shock_activity_lift < 0.0:
            raise _calibration_error("post_shock_activity_lift", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsObservedCloseOpenBoundaryObservation:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsObservedCloseOpenBoundaryObservation(
            close_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "close_timestamp_utc_nanos",
                f"{field_path}.close_timestamp_utc_nanos",
            ),
            close_price=require_positive_finite_number(
                raw_payload,
                "close_price",
                f"{field_path}.close_price",
                CALIBRATION_ERROR_FACTORY,
            ),
            open_timestamp_utc_nanos=_require_timestamp_utc_nanos(
                raw_payload,
                "open_timestamp_utc_nanos",
                f"{field_path}.open_timestamp_utc_nanos",
            ),
            open_price=require_positive_finite_number(
                raw_payload,
                "open_price",
                f"{field_path}.open_price",
                CALIBRATION_ERROR_FACTORY,
            ),
            post_shock_activity_lift=require_non_negative_finite_number(
                raw_payload,
                "post_shock_activity_lift",
                f"{field_path}.post_shock_activity_lift",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsCloseOpenGapDetectionPolicy(ContractModel):
    material_gap_absolute_return_tolerance: float

    def __post_init__(self) -> None:
        if self.material_gap_absolute_return_tolerance < 0.0:
            raise _calibration_error("material_gap_absolute_return_tolerance", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsCloseOpenGapDetectionPolicy:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsCloseOpenGapDetectionPolicy(
            material_gap_absolute_return_tolerance=require_non_negative_finite_number(
                raw_payload,
                "material_gap_absolute_return_tolerance",
                f"{field_path}.material_gap_absolute_return_tolerance",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsObjectiveMetricWeights(ContractModel):
    continuous_distribution_fit_weight: float
    surprise_arrival_fit_weight: float
    jump_fit_weight: float
    gap_fit_weight: float
    activity_coupling_fit_weight: float
    price_range_sanity_fit_weight: float

    def __post_init__(self) -> None:
        if self.continuous_distribution_fit_weight < 0.0:
            raise _calibration_error("continuous_distribution_fit_weight", "must be non-negative")
        if self.surprise_arrival_fit_weight < 0.0:
            raise _calibration_error("surprise_arrival_fit_weight", "must be non-negative")
        if self.jump_fit_weight < 0.0:
            raise _calibration_error("jump_fit_weight", "must be non-negative")
        if self.gap_fit_weight < 0.0:
            raise _calibration_error("gap_fit_weight", "must be non-negative")
        if self.activity_coupling_fit_weight < 0.0:
            raise _calibration_error("activity_coupling_fit_weight", "must be non-negative")
        if self.price_range_sanity_fit_weight < 0.0:
            raise _calibration_error("price_range_sanity_fit_weight", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsObjectiveMetricWeights:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationReturnComponentsObjectiveMetricWeights(
            continuous_distribution_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "continuous_distribution_fit_weight",
                f"{field_path}.continuous_distribution_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            surprise_arrival_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "surprise_arrival_fit_weight",
                f"{field_path}.surprise_arrival_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            jump_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "jump_fit_weight",
                f"{field_path}.jump_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            gap_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "gap_fit_weight",
                f"{field_path}.gap_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            activity_coupling_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "activity_coupling_fit_weight",
                f"{field_path}.activity_coupling_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
            price_range_sanity_fit_weight=require_non_negative_finite_number(
                raw_payload,
                "price_range_sanity_fit_weight",
                f"{field_path}.price_range_sanity_fit_weight",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationDoubleExponentialBaseShockConfiguration(ContractModel):
    probability_positive: float
    eta_plus: float
    eta_minus: float

    def __post_init__(self) -> None:
        if self.probability_positive <= 0.0 or self.probability_positive >= 1.0:
            raise _calibration_error("probability_positive", "must be strictly between 0 and 1")
        if self.eta_plus <= 0.0:
            raise _calibration_error("eta_plus", "must be strictly positive")
        if self.eta_minus <= 0.0:
            raise _calibration_error("eta_minus", "must be strictly positive")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationDoubleExponentialBaseShockConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationDoubleExponentialBaseShockConfiguration(
            probability_positive=require_finite_number(
                raw_payload,
                "probability_positive",
                f"{field_path}.probability_positive",
                CALIBRATION_ERROR_FACTORY,
            ),
            eta_plus=require_positive_finite_number(
                raw_payload,
                "eta_plus",
                f"{field_path}.eta_plus",
                CALIBRATION_ERROR_FACTORY,
            ),
            eta_minus=require_positive_finite_number(
                raw_payload,
                "eta_minus",
                f"{field_path}.eta_minus",
                CALIBRATION_ERROR_FACTORY,
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationCoupledGapMarkConfiguration(ContractModel):
    sigma_g: float

    def __post_init__(self) -> None:
        if self.sigma_g < 0.0:
            raise _calibration_error("sigma_g", "must be non-negative")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationCoupledGapMarkConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationCoupledGapMarkConfiguration(
            sigma_g=require_non_negative_finite_number(
                raw_payload,
                "sigma_g",
                f"{field_path}.sigma_g",
                CALIBRATION_ERROR_FACTORY,
            )
        )


@dataclass(frozen=True, slots=True)
class CalibrationSharedNewsMarkLawConfiguration(ContractModel):
    gamma_severity_marks: GammaSeverityMarkConfiguration
    double_exponential_base_shock: CalibrationDoubleExponentialBaseShockConfiguration
    coupled_gap_marks: CalibrationCoupledGapMarkConfiguration

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationSharedNewsMarkLawConfiguration:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        return CalibrationSharedNewsMarkLawConfiguration(
            gamma_severity_marks=_parse_gamma_severity_mark_configuration(
                require_mapping(
                    raw_payload,
                    "gamma_severity_marks",
                    f"{field_path}.gamma_severity_marks",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.gamma_severity_marks",
            ),
            double_exponential_base_shock=CalibrationDoubleExponentialBaseShockConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "double_exponential_base_shock",
                    f"{field_path}.double_exponential_base_shock",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.double_exponential_base_shock",
            ),
            coupled_gap_marks=CalibrationCoupledGapMarkConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "coupled_gap_marks",
                    f"{field_path}.coupled_gap_marks",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.coupled_gap_marks",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationReturnComponentsStageCandidateGenerationInput(ContractModel):
    known_news_calendar: CalibrationReturnComponentsKnownNewsCalendarInput
    realized_known_news_return_samples: tuple[CalibrationReturnComponentsKnownNewsReturnSample, ...]
    observed_open_session_returns: tuple[CalibrationReturnComponentsObservedOpenSessionReturn, ...]
    detected_intraday_jump_candidates: tuple[CalibrationReturnComponentsDetectedIntradayJumpCandidate, ...]
    observed_close_open_boundary_observations: tuple[CalibrationReturnComponentsObservedCloseOpenBoundaryObservation, ...]
    close_open_gap_detection_policy: CalibrationReturnComponentsCloseOpenGapDetectionPolicy
    open_session_sell_share_proxy: float
    shared_mark_law_template: CalibrationSharedNewsMarkLawConfiguration
    objective_metric_weights: CalibrationReturnComponentsObjectiveMetricWeights

    def __post_init__(self) -> None:
        if len(self.observed_open_session_returns) == 0:
            raise _calibration_error(
                "observed_open_session_returns",
                "must include at least one observed open-session return",
            )
        if self.open_session_sell_share_proxy < 0.0 or self.open_session_sell_share_proxy > 1.0:
            raise _calibration_error("open_session_sell_share_proxy", "must be between 0 and 1")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationReturnComponentsStageCandidateGenerationInput:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        known_news_return_samples = require_sequence(
            raw_payload,
            "realized_known_news_return_samples",
            f"{field_path}.realized_known_news_return_samples",
            CALIBRATION_ERROR_FACTORY,
        )
        observed_open_session_returns = require_sequence(
            raw_payload,
            "observed_open_session_returns",
            f"{field_path}.observed_open_session_returns",
            CALIBRATION_ERROR_FACTORY,
        )
        intraday_jump_candidates = require_sequence(
            raw_payload,
            "detected_intraday_jump_candidates",
            f"{field_path}.detected_intraday_jump_candidates",
            CALIBRATION_ERROR_FACTORY,
        )
        boundary_observations = require_sequence(
            raw_payload,
            "observed_close_open_boundary_observations",
            f"{field_path}.observed_close_open_boundary_observations",
            CALIBRATION_ERROR_FACTORY,
        )

        return CalibrationReturnComponentsStageCandidateGenerationInput(
            known_news_calendar=CalibrationReturnComponentsKnownNewsCalendarInput.from_mapping(
                require_mapping(
                    raw_payload,
                    "known_news_calendar",
                    f"{field_path}.known_news_calendar",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.known_news_calendar",
            ),
            realized_known_news_return_samples=tuple(
                CalibrationReturnComponentsKnownNewsReturnSample.from_mapping(
                    raw_sample,
                    field_path=f"{field_path}.realized_known_news_return_samples[{sample_index}]",
                )
                for sample_index, raw_sample in enumerate(known_news_return_samples)
            ),
            observed_open_session_returns=tuple(
                CalibrationReturnComponentsObservedOpenSessionReturn.from_mapping(
                    raw_return,
                    field_path=f"{field_path}.observed_open_session_returns[{return_index}]",
                )
                for return_index, raw_return in enumerate(observed_open_session_returns)
            ),
            detected_intraday_jump_candidates=tuple(
                CalibrationReturnComponentsDetectedIntradayJumpCandidate.from_mapping(
                    raw_candidate,
                    field_path=f"{field_path}.detected_intraday_jump_candidates[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(intraday_jump_candidates)
            ),
            observed_close_open_boundary_observations=tuple(
                CalibrationReturnComponentsObservedCloseOpenBoundaryObservation.from_mapping(
                    raw_candidate,
                    field_path=f"{field_path}.observed_close_open_boundary_observations[{candidate_index}]",
                )
                for candidate_index, raw_candidate in enumerate(boundary_observations)
            ),
            close_open_gap_detection_policy=CalibrationReturnComponentsCloseOpenGapDetectionPolicy.from_mapping(
                require_mapping(
                    raw_payload,
                    "close_open_gap_detection_policy",
                    f"{field_path}.close_open_gap_detection_policy",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.close_open_gap_detection_policy",
            ),
            open_session_sell_share_proxy=require_non_negative_finite_number(
                raw_payload,
                "open_session_sell_share_proxy",
                f"{field_path}.open_session_sell_share_proxy",
                CALIBRATION_ERROR_FACTORY,
            ),
            shared_mark_law_template=CalibrationSharedNewsMarkLawConfiguration.from_mapping(
                require_mapping(
                    raw_payload,
                    "shared_mark_law_template",
                    f"{field_path}.shared_mark_law_template",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.shared_mark_law_template",
            ),
            objective_metric_weights=CalibrationReturnComponentsObjectiveMetricWeights.from_mapping(
                require_mapping(
                    raw_payload,
                    "objective_metric_weights",
                    f"{field_path}.objective_metric_weights",
                    CALIBRATION_ERROR_FACTORY,
                ),
                field_path=f"{field_path}.objective_metric_weights",
            ),
        )


@dataclass(frozen=True, slots=True)
class CalibrationStagedExecutionRuntimeResources(ContractModel):
    disk_checkpoint_directory: Optional[str] = None

    def __post_init__(self) -> None:
        if self.disk_checkpoint_directory is not None and not self.disk_checkpoint_directory.strip():
            raise _calibration_error("disk_checkpoint_directory", "must be non-empty when provided")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> CalibrationStagedExecutionRuntimeResources:
        if not isinstance(raw_payload, dict):
            raise _calibration_error(field_path, "must be an object")

        disk_checkpoint_directory = raw_payload.get("disk_checkpoint_directory")
        if disk_checkpoint_directory is not None and not isinstance(disk_checkpoint_directory, str):
            raise _calibration_error(
                f"{field_path}.disk_checkpoint_directory",
                "must be a string when provided",
            )

        return CalibrationStagedExecutionRuntimeResources(
            disk_checkpoint_directory=disk_checkpoint_directory,
        )


@dataclass(frozen=True, slots=True)
class StagedCalibrationExecutionInput(ContractModel):
    schema_version: str
    request: StagedCalibrationRequest
    seasonality_stage_input: Optional[CalibrationSeasonalityStageExecutionInput] = None
    hawkes_candidate_inputs: Optional[tuple[CalibrationHawkesStageCandidateInput, ...]] = None
    return_components_candidate_generation_input: Optional[
        CalibrationReturnComponentsStageCandidateGenerationInput
    ] = None
    runtime_resources: Optional[CalibrationStagedExecutionRuntimeResources] = None

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_STAGED_EXECUTION_INPUT_SCHEMA_VERSION}`",
            )

        selected_stage_set = set(self.request.selected_stages)

        if CalibrationStage.SEASONALITY in selected_stage_set and self.seasonality_stage_input is None:
            raise _calibration_error(
                "seasonality_stage_input",
                "is required when selected_stages contains seasonality",
            )
        if CalibrationStage.SEASONALITY not in selected_stage_set and self.seasonality_stage_input is not None:
            raise _calibration_error(
                "seasonality_stage_input",
                "must not be provided when seasonality is not selected",
            )

        if CalibrationStage.HAWKES in selected_stage_set and self.hawkes_candidate_inputs is None:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "is required when selected_stages contains hawkes",
            )
        if CalibrationStage.HAWKES not in selected_stage_set and self.hawkes_candidate_inputs is not None:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "must not be provided when hawkes is not selected",
            )
        if self.hawkes_candidate_inputs is not None and len(self.hawkes_candidate_inputs) == 0:
            raise _calibration_error(
                "hawkes_candidate_inputs",
                "must include at least one Hawkes candidate when provided",
            )

        if (
            CalibrationStage.RETURN_COMPONENTS in selected_stage_set
            and self.return_components_candidate_generation_input is None
        ):
            raise _calibration_error(
                "return_components_candidate_generation_input",
                "is required when selected_stages contains return_components",
            )
        if (
            CalibrationStage.RETURN_COMPONENTS not in selected_stage_set
            and self.return_components_candidate_generation_input is not None
        ):
            raise _calibration_error(
                "return_components_candidate_generation_input",
                "must not be provided when return_components is not selected",
            )

        match self.request.execution_mode:
            case CalibrationExecutionMode.MEMORY:
                if self.runtime_resources is not None and self.runtime_resources.disk_checkpoint_directory is not None:
                    raise _calibration_error(
                        "runtime_resources.disk_checkpoint_directory",
                        "is only supported when execution_mode=disk",
                    )
            case CalibrationExecutionMode.DISK:
                if (
                    self.runtime_resources is None
                    or self.runtime_resources.disk_checkpoint_directory is None
                ):
                    raise _calibration_error(
                        "runtime_resources.disk_checkpoint_directory",
                        "is required when execution_mode=disk",
                    )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> StagedCalibrationExecutionInput:
        from .request import StagedCalibrationRequest

        if not isinstance(raw_payload, dict):
            raise _calibration_error("staged_calibration_execution_input", "must be an object")

        raw_hawkes_candidate_inputs = raw_payload.get("hawkes_candidate_inputs")
        if raw_hawkes_candidate_inputs is not None and (
            isinstance(raw_hawkes_candidate_inputs, str) or not isinstance(raw_hawkes_candidate_inputs, list)
        ):
            raise _calibration_error("hawkes_candidate_inputs", "must be a sequence when provided")

        return StagedCalibrationExecutionInput(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            request=StagedCalibrationRequest.from_mapping(
                require_mapping(raw_payload, "request", "request", CALIBRATION_ERROR_FACTORY),
                field_path_prefix="request",
            ),
            seasonality_stage_input=(
                CalibrationSeasonalityStageExecutionInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "seasonality_stage_input",
                        "seasonality_stage_input",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="seasonality_stage_input",
                )
                if "seasonality_stage_input" in raw_payload and raw_payload["seasonality_stage_input"] is not None
                else None
            ),
            hawkes_candidate_inputs=(
                tuple(
                    CalibrationHawkesStageCandidateInput.from_mapping(
                        raw_candidate,
                        field_path=f"hawkes_candidate_inputs[{candidate_index}]",
                    )
                    for candidate_index, raw_candidate in enumerate(raw_hawkes_candidate_inputs)
                )
                if raw_hawkes_candidate_inputs is not None
                else None
            ),
            return_components_candidate_generation_input=(
                CalibrationReturnComponentsStageCandidateGenerationInput.from_mapping(
                    require_mapping(
                        raw_payload,
                        "return_components_candidate_generation_input",
                        "return_components_candidate_generation_input",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="return_components_candidate_generation_input",
                )
                if (
                    "return_components_candidate_generation_input" in raw_payload
                    and raw_payload["return_components_candidate_generation_input"] is not None
                )
                else None
            ),
            runtime_resources=(
                CalibrationStagedExecutionRuntimeResources.from_mapping(
                    require_mapping(
                        raw_payload,
                        "runtime_resources",
                        "runtime_resources",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="runtime_resources",
                )
                if "runtime_resources" in raw_payload and raw_payload["runtime_resources"] is not None
                else None
            ),
        )

def validate_staged_calibration_execution_input(
    raw_execution_input: dict[str, Any] | Any,
) -> StagedCalibrationExecutionInput:
    return StagedCalibrationExecutionInput.from_mapping(raw_execution_input)


def serialize_staged_calibration_execution_input(execution_input: StagedCalibrationExecutionInput) -> str:
    return serialize_deterministic_json(execution_input)


def deserialize_staged_calibration_execution_input(payload_json: str) -> StagedCalibrationExecutionInput:
    raw_payload = parse_json_object(
        payload_json,
        'staged calibration execution input',
        CALIBRATION_ERROR_FACTORY,
    )
    return validate_staged_calibration_execution_input(dict(raw_payload))


__all__ = [
    'CalibrationCoupledGapMarkConfiguration',
    'CalibrationDayOfWeekSeasonalityEstimatorSettings',
    'CalibrationDayOfYearSeasonalityEstimatorSettings',
    'CalibrationDoubleExponentialBaseShockConfiguration',
    'CalibrationHawkesDirectionalLabelDiagnostics',
    'CalibrationHawkesObservedSourceRegime',
    'CalibrationHawkesStageCandidateInput',
    'CalibrationHawkesStructuredAlphaParameterization',
    'CalibrationHawkesStructuredRhoParameterization',
    'CalibrationHawkesStructuredRhoSelfDominancePolicy',
    'CalibrationIntradaySeasonalityEstimatorSettings',
    'CalibrationIntradaySmoothingKernel',
    'CalibrationObjectiveTermInput',
    'CalibrationReturnComponentsCloseOpenGapDetectionPolicy',
    'CalibrationReturnComponentsDetectedIntradayJumpCandidate',
    'CalibrationReturnComponentsKnownNewsCalendarInput',
    'CalibrationReturnComponentsKnownNewsReturnSample',
    'CalibrationReturnComponentsObjectiveMetricWeights',
    'CalibrationReturnComponentsObservedCloseOpenBoundaryObservation',
    'CalibrationReturnComponentsObservedOpenSessionReturn',
    'CalibrationReturnComponentsSignalSourceWindow',
    'CalibrationReturnComponentsStageCandidateGenerationInput',
    'CalibrationScheduledEventOverlay',
    'CalibrationSeasonalityStageExecutionInput',
    'CalibrationSeasonalityStageExecutionSettings',
    'CalibrationSharedNewsMarkLawConfiguration',
    'CalibrationStagedExecutionRuntimeResources',
    'CalibrationTimestampEventRecord',
    'CalibrationTimestampEventSessionFilter',
    'StagedCalibrationExecutionInput',
    'deserialize_staged_calibration_execution_input',
    'serialize_staged_calibration_execution_input',
    'validate_staged_calibration_execution_input',
]
