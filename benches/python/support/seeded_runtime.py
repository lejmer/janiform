import json
from dataclasses import dataclass
from datetime import datetime
from enum import StrEnum
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Any, Optional, cast

from pyarrow import parquet

from janiform._contracts import (
    ContractModel,
    parse_json_object,
    require_datetime_utc,
    require_non_empty_string,
    require_non_negative_finite_number,
    require_non_negative_integer,
    require_positive_finite_number,
    require_positive_integer,
    require_sequence,
    serialize_pretty_json,
)
from janiform.calibration import JaniformEstimator
from janiform.calibration.fit import JaniformFitReport, JaniformSimulationResult
from janiform.simulation.seeded import SeededTickSimulationResponse


REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION = (
    "benchmark.real_data_seeded_runtime.fixture.v1"
)
REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SET_ID = (
    "real_data_seeded_runtime_fixture_set_v1"
)
REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION = (
    "benchmark.real_data_seeded_runtime.report.v1"
)
REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION = (
    "benchmark.real_data_seeded_runtime.threshold_policy.v1"
)
REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION = (
    "benchmark.real_data_seeded_runtime.baseline.v1"
)
REAL_DATA_SEEDED_RUNTIME_COMPARISON_REPORT_SCHEMA_VERSION = (
    "benchmark.real_data_seeded_runtime.baseline_comparison.v1"
)
REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS = (
    "estimated_native_retained_bytes_proxy_not_peak_rss.v1"
)
REAL_DATA_SEEDED_RUNTIME_DATASET_SOURCE_POLICY = (
    "checked_in_synthetic_eurusd_parquet_row_slice.v1"
)
REAL_DATA_SEEDED_RUNTIME_UNSIGNED_MARKET_DATA_POLICY = (
    "checked_in_fixture_omits_trade_sign_column_and_relies_on_unsigned_fit_path.v1"
)
REAL_DATA_SEEDED_RUNTIME_FIXTURE_LOADING_ORDER_POLICY = "fixture_id_ascending.v1"
REAL_DATA_SEEDED_RUNTIME_CAPTURE_POLICY = (
    "capture_runtime_transport_materialization_and_event_regime_metrics_from_high_level_fit_and_seeded_run.v1"
)
REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY = (
    "candidate_vs_accepted_baseline_with_fixture_threshold_classification.v1"
)
REAL_DATA_SEEDED_RUNTIME_REQUIRED_REPORT_FIELDS = (
    "schema_version",
    "fixture_id",
    "fixture_version",
    "output_schema_metadata",
    "provenance",
    "observed_source_tick_count",
    "calibrated_expected_event_count_over_observation_window",
    "coarse_generated_event_count",
    "seeded_transition_count",
    "seeded_event_count",
    "native_runtime_seconds",
    "python_response_construction_seconds",
    "python_materialization_seconds",
    "boundary_transport_bytes",
    "native_estimated_retained_bytes",
    "materialized_transition_count",
    "materialized_event_count",
    "access_pattern_reports",
)
REAL_DATA_SEEDED_RUNTIME_REQUIRED_ACCESS_PATTERN_FIELDS = (
    "pattern_id",
    "access_kind",
    "boundary_transport_bytes",
    "python_materialization_seconds",
    "materialized_transition_count",
    "materialized_event_count",
)
REAL_DATA_SEEDED_RUNTIME_REQUIRED_PROVENANCE_FIELDS = (
    "dataset_source_policy",
    "unsigned_market_data_policy",
    "source_tick_data_path",
    "source_tick_data_format",
    "source_tick_data_row_offset",
    "source_tick_data_row_count",
    "source_timestamp_column",
    "source_price_column",
    "source_trade_sign_column",
    "market_profile",
    "execution_mode",
    "simulation_seed",
    "simulation_start_time_utc",
    "coarse_interval",
    "coarse_horizon_interval_count",
    "seeded_horizon_interval_count",
    "materialization_access_pattern_ids",
    "memory_metric_semantics",
)
REAL_DATA_SEEDED_RUNTIME_REQUIRED_BASELINE_FIELDS = (
    "schema_version",
    "baseline_id",
    "baseline_version",
    "capture_policy",
    "memory_metric_semantics",
    "source_fixture_set_id",
    "records",
)
REAL_DATA_SEEDED_RUNTIME_REQUIRED_COMPARISON_FIELDS = (
    "schema_version",
    "comparison_policy",
    "baseline_id",
    "baseline_version",
    "policy_id",
    "policy_version",
    "fixture_comparisons",
)
_FLOAT_ZERO_EPSILON = 1.0e-12


def _benchmark_error(field_path: str, message: str) -> ValueError:
    return ValueError(f"{field_path}: {message}")


class SeededRuntimeMaterializationAccessKind(StrEnum):
    EVENT_TAPE_HEAD = "event_tape_head"
    FULL_SEEDED_EXPORT_TO_PANDAS = "full_seeded_export_to_pandas"


class SeededRuntimeFailureClassification(StrEnum):
    MODEL_REGIME_DRIFT = "model_regime_drift"
    RUNTIME_DRIFT = "runtime_drift"
    BOUNDARY_TRANSPORT_DRIFT = "boundary_transport_drift"
    RETAINED_MEMORY_DRIFT = "retained_memory_drift"


class BenchmarkThresholdStatus(StrEnum):
    PASS = "pass"
    FAIL = "fail"


class BenchmarkFixtureComparisonStatus(StrEnum):
    PASS = "pass"
    FAIL = "fail"


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeMaterializationPattern(ContractModel):
    pattern_id: str
    access_kind: SeededRuntimeMaterializationAccessKind
    record_count: Optional[int] = None

    def __post_init__(self) -> None:
        if not self.pattern_id.strip():
            raise _benchmark_error("pattern_id", "must be a non-empty string")
        match self.access_kind:
            case SeededRuntimeMaterializationAccessKind.EVENT_TAPE_HEAD:
                if self.record_count is None or self.record_count <= 0:
                    raise _benchmark_error(
                        "record_count",
                        "must be strictly positive for event_tape_head access",
                    )
            case SeededRuntimeMaterializationAccessKind.FULL_SEEDED_EXPORT_TO_PANDAS:
                if self.record_count is not None:
                    raise _benchmark_error(
                        "record_count",
                        "must be omitted for full_seeded_export_to_pandas access",
                    )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> RealDataSeededRuntimeMaterializationPattern:
        if not isinstance(raw_payload, dict):
            raise _benchmark_error(field_path, "must be an object")

        raw_access_kind = require_non_empty_string(
            raw_payload,
            "access_kind",
            f"{field_path}.access_kind",
            _benchmark_error,
        )
        try:
            access_kind = SeededRuntimeMaterializationAccessKind(raw_access_kind)
        except ValueError as error:
            raise _benchmark_error(
                f"{field_path}.access_kind",
                (
                    "must be one of: "
                    "event_tape_head, full_seeded_export_to_pandas"
                ),
            ) from error

        raw_record_count = raw_payload.get("record_count")
        if raw_record_count is not None and (
            not isinstance(raw_record_count, int) or isinstance(raw_record_count, bool)
        ):
            raise _benchmark_error(
                f"{field_path}.record_count",
                "must be an integer when provided",
            )

        return RealDataSeededRuntimeMaterializationPattern(
            pattern_id=require_non_empty_string(
                raw_payload,
                "pattern_id",
                f"{field_path}.pattern_id",
                _benchmark_error,
            ),
            access_kind=access_kind,
            record_count=cast(Optional[int], raw_record_count),
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkFixture(ContractModel):
    fixture_id: str
    fixture_version: str
    description: str
    tags: tuple[str, ...]
    source_tick_data_path: str
    source_tick_data_format: str
    source_tick_data_row_offset: int
    source_tick_data_row_count: int
    source_timestamp_column: str
    source_price_column: str
    source_trade_sign_column: Optional[str]
    market_profile: str
    execution_mode: str
    simulation_seed: int
    simulation_start_time_utc: datetime
    coarse_interval: str
    coarse_horizon_interval_count: int
    seeded_horizon_interval_count: int
    materialization_access_patterns: tuple[RealDataSeededRuntimeMaterializationPattern, ...]

    def __post_init__(self) -> None:
        if not self.fixture_id.strip():
            raise _benchmark_error("fixture_id", "must be a non-empty string")
        if not self.fixture_version.strip():
            raise _benchmark_error("fixture_version", "must be a non-empty string")
        if not self.description.strip():
            raise _benchmark_error("description", "must be a non-empty string")
        if len(self.tags) == 0:
            raise _benchmark_error("tags", "must contain at least one tag")
        if any(not tag.strip() for tag in self.tags):
            raise _benchmark_error("tags", "must not contain empty tag values")
        if not self.source_tick_data_path.strip():
            raise _benchmark_error("source_tick_data_path", "must be a non-empty string")
        if self.source_tick_data_format != "parquet":
            raise _benchmark_error("source_tick_data_format", "must be `parquet`")
        if self.source_tick_data_row_offset < 0:
            raise _benchmark_error(
                "source_tick_data_row_offset",
                "must be non-negative",
            )
        if self.source_tick_data_row_count <= 1:
            raise _benchmark_error(
                "source_tick_data_row_count",
                "must be greater than 1",
            )
        if not self.source_timestamp_column.strip():
            raise _benchmark_error("source_timestamp_column", "must be a non-empty string")
        if not self.source_price_column.strip():
            raise _benchmark_error("source_price_column", "must be a non-empty string")
        if (
            self.source_trade_sign_column is not None
            and not self.source_trade_sign_column.strip()
        ):
            raise _benchmark_error(
                "source_trade_sign_column",
                "must be a non-empty string when provided",
            )
        if not self.market_profile.strip():
            raise _benchmark_error("market_profile", "must be a non-empty string")
        if self.execution_mode != "memory":
            raise _benchmark_error("execution_mode", "must be `memory`")
        if self.simulation_seed < 0:
            raise _benchmark_error("simulation_seed", "must be non-negative")
        if not self.coarse_interval.strip():
            raise _benchmark_error("coarse_interval", "must be a non-empty string")
        if self.coarse_horizon_interval_count <= 0:
            raise _benchmark_error(
                "coarse_horizon_interval_count",
                "must be strictly positive",
            )
        if self.seeded_horizon_interval_count <= 0:
            raise _benchmark_error(
                "seeded_horizon_interval_count",
                "must be strictly positive",
            )
        if len(self.materialization_access_patterns) == 0:
            raise _benchmark_error(
                "materialization_access_patterns",
                "must contain at least one access pattern",
            )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> RealDataSeededRuntimeBenchmarkFixture:
        if not isinstance(raw_payload, dict):
            raise _benchmark_error(field_path, "must be an object")

        raw_tags = require_sequence(
            raw_payload,
            "tags",
            f"{field_path}.tags",
            _benchmark_error,
        )
        raw_access_patterns = require_sequence(
            raw_payload,
            "materialization_access_patterns",
            f"{field_path}.materialization_access_patterns",
            _benchmark_error,
        )
        source_trade_sign_column = _optional_non_empty_string(
            raw_payload,
            "source_trade_sign_column",
            f"{field_path}.source_trade_sign_column",
        )

        return RealDataSeededRuntimeBenchmarkFixture(
            fixture_id=require_non_empty_string(
                raw_payload,
                "fixture_id",
                f"{field_path}.fixture_id",
                _benchmark_error,
            ),
            fixture_version=require_non_empty_string(
                raw_payload,
                "fixture_version",
                f"{field_path}.fixture_version",
                _benchmark_error,
            ),
            description=require_non_empty_string(
                raw_payload,
                "description",
                f"{field_path}.description",
                _benchmark_error,
            ),
            tags=tuple(
                require_non_empty_string(
                    {"value": raw_value},
                    "value",
                    f"{field_path}.tags[{tag_index}]",
                    _benchmark_error,
                )
                for tag_index, raw_value in enumerate(raw_tags)
            ),
            source_tick_data_path=require_non_empty_string(
                raw_payload,
                "source_tick_data_path",
                f"{field_path}.source_tick_data_path",
                _benchmark_error,
            ),
            source_tick_data_format=require_non_empty_string(
                raw_payload,
                "source_tick_data_format",
                f"{field_path}.source_tick_data_format",
                _benchmark_error,
            ),
            source_tick_data_row_offset=require_non_negative_integer(
                raw_payload,
                "source_tick_data_row_offset",
                f"{field_path}.source_tick_data_row_offset",
                _benchmark_error,
            ),
            source_tick_data_row_count=require_positive_integer(
                raw_payload,
                "source_tick_data_row_count",
                f"{field_path}.source_tick_data_row_count",
                _benchmark_error,
            ),
            source_timestamp_column=require_non_empty_string(
                raw_payload,
                "source_timestamp_column",
                f"{field_path}.source_timestamp_column",
                _benchmark_error,
            ),
            source_price_column=require_non_empty_string(
                raw_payload,
                "source_price_column",
                f"{field_path}.source_price_column",
                _benchmark_error,
            ),
            source_trade_sign_column=source_trade_sign_column,
            market_profile=require_non_empty_string(
                raw_payload,
                "market_profile",
                f"{field_path}.market_profile",
                _benchmark_error,
            ),
            execution_mode=require_non_empty_string(
                raw_payload,
                "execution_mode",
                f"{field_path}.execution_mode",
                _benchmark_error,
            ),
            simulation_seed=require_non_negative_integer(
                raw_payload,
                "simulation_seed",
                f"{field_path}.simulation_seed",
                _benchmark_error,
            ),
            simulation_start_time_utc=require_datetime_utc(
                require_non_empty_string(
                    raw_payload,
                    "simulation_start_time_utc",
                    f"{field_path}.simulation_start_time_utc",
                    _benchmark_error,
                ),
                f"{field_path}.simulation_start_time_utc",
                _benchmark_error,
            ),
            coarse_interval=require_non_empty_string(
                raw_payload,
                "coarse_interval",
                f"{field_path}.coarse_interval",
                _benchmark_error,
            ),
            coarse_horizon_interval_count=require_positive_integer(
                raw_payload,
                "coarse_horizon_interval_count",
                f"{field_path}.coarse_horizon_interval_count",
                _benchmark_error,
            ),
            seeded_horizon_interval_count=require_positive_integer(
                raw_payload,
                "seeded_horizon_interval_count",
                f"{field_path}.seeded_horizon_interval_count",
                _benchmark_error,
            ),
            materialization_access_patterns=tuple(
                RealDataSeededRuntimeMaterializationPattern.from_mapping(
                    raw_pattern,
                    field_path=(
                        f"{field_path}.materialization_access_patterns[{pattern_index}]"
                    ),
                )
                for pattern_index, raw_pattern in enumerate(raw_access_patterns)
            ),
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkFixtureDocument(ContractModel):
    schema_version: str
    fixture_set_id: str
    dataset_source_policy: str
    unsigned_market_data_policy: str
    fixture_loading_order_policy: str
    fixtures: tuple[RealDataSeededRuntimeBenchmarkFixture, ...]

    def __post_init__(self) -> None:
        if self.schema_version != REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION:
            raise _benchmark_error(
                "schema_version",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION}`",
            )
        if self.fixture_set_id != REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SET_ID:
            raise _benchmark_error(
                "fixture_set_id",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SET_ID}`",
            )

        if (
            self.dataset_source_policy
            != REAL_DATA_SEEDED_RUNTIME_DATASET_SOURCE_POLICY
        ):
            raise _benchmark_error(
                "dataset_source_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_DATASET_SOURCE_POLICY}`",
            )

        if (
            self.unsigned_market_data_policy
            != REAL_DATA_SEEDED_RUNTIME_UNSIGNED_MARKET_DATA_POLICY
        ):
            raise _benchmark_error(
                "unsigned_market_data_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_UNSIGNED_MARKET_DATA_POLICY}`",
            )

        if (
            self.fixture_loading_order_policy
            != REAL_DATA_SEEDED_RUNTIME_FIXTURE_LOADING_ORDER_POLICY
        ):
            raise _benchmark_error(
                "fixture_loading_order_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_FIXTURE_LOADING_ORDER_POLICY}`",
            )

        if len(self.fixtures) == 0:
            raise _benchmark_error("fixtures", "must contain at least one fixture")

        fixture_ids = [fixture.fixture_id for fixture in self.fixtures]
        if fixture_ids != sorted(fixture_ids):
            raise _benchmark_error(
                "fixtures",
                "must be ordered by fixture_id ascending",
            )
        if len(set(fixture_ids)) != len(fixture_ids):
            raise _benchmark_error("fixtures", "must not contain duplicate fixture_id values")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
    ) -> RealDataSeededRuntimeBenchmarkFixtureDocument:
        if not isinstance(raw_payload, dict):
            raise _benchmark_error("real_data_seeded_runtime_benchmark_fixture", "must be an object")

        raw_fixtures = require_sequence(
            raw_payload,
            "fixtures",
            "fixtures",
            _benchmark_error,
        )

        return RealDataSeededRuntimeBenchmarkFixtureDocument(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                _benchmark_error,
            ),
            fixture_set_id=require_non_empty_string(
                raw_payload,
                "fixture_set_id",
                "fixture_set_id",
                _benchmark_error,
            ),
            dataset_source_policy=require_non_empty_string(
                raw_payload,
                "dataset_source_policy",
                "dataset_source_policy",
                _benchmark_error,
            ),
            unsigned_market_data_policy=require_non_empty_string(
                raw_payload,
                "unsigned_market_data_policy",
                "unsigned_market_data_policy",
                _benchmark_error,
            ),
            fixture_loading_order_policy=require_non_empty_string(
                raw_payload,
                "fixture_loading_order_policy",
                "fixture_loading_order_policy",
                _benchmark_error,
            ),
            fixtures=tuple(
                RealDataSeededRuntimeBenchmarkFixture.from_mapping(
                    raw_fixture,
                    field_path=f"fixtures[{fixture_index}]",
                )
                for fixture_index, raw_fixture in enumerate(raw_fixtures)
            ),
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeFixtureThresholds(ContractModel):
    fixture_id: str
    maximum_seeded_event_rate_ratio_vs_observed_open: float
    maximum_seeded_event_rate_ratio_vs_calibrated_open: float
    maximum_coarse_event_rate_ratio_vs_observed_open: float
    maximum_native_runtime_absolute_regression_seconds: float
    maximum_native_runtime_relative_regression: float
    maximum_python_materialization_absolute_regression_seconds: float
    maximum_python_materialization_relative_regression: float
    maximum_boundary_transport_absolute_growth_bytes: int
    maximum_boundary_transport_relative_growth: float
    maximum_retained_memory_absolute_growth_bytes: int
    maximum_retained_memory_relative_growth: float

    def __post_init__(self) -> None:
        if not self.fixture_id.strip():
            raise _benchmark_error("fixture_id", "must be a non-empty string")

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> RealDataSeededRuntimeFixtureThresholds:
        if not isinstance(raw_payload, dict):
            raise _benchmark_error(field_path, "must be an object")

        return RealDataSeededRuntimeFixtureThresholds(
            fixture_id=require_non_empty_string(
                raw_payload,
                "fixture_id",
                f"{field_path}.fixture_id",
                _benchmark_error,
            ),
            maximum_seeded_event_rate_ratio_vs_observed_open=require_positive_finite_number(
                raw_payload,
                "maximum_seeded_event_rate_ratio_vs_observed_open",
                f"{field_path}.maximum_seeded_event_rate_ratio_vs_observed_open",
                _benchmark_error,
            ),
            maximum_seeded_event_rate_ratio_vs_calibrated_open=require_positive_finite_number(
                raw_payload,
                "maximum_seeded_event_rate_ratio_vs_calibrated_open",
                f"{field_path}.maximum_seeded_event_rate_ratio_vs_calibrated_open",
                _benchmark_error,
            ),
            maximum_coarse_event_rate_ratio_vs_observed_open=require_positive_finite_number(
                raw_payload,
                "maximum_coarse_event_rate_ratio_vs_observed_open",
                f"{field_path}.maximum_coarse_event_rate_ratio_vs_observed_open",
                _benchmark_error,
            ),
            maximum_native_runtime_absolute_regression_seconds=require_non_negative_finite_number(
                raw_payload,
                "maximum_native_runtime_absolute_regression_seconds",
                f"{field_path}.maximum_native_runtime_absolute_regression_seconds",
                _benchmark_error,
            ),
            maximum_native_runtime_relative_regression=require_non_negative_finite_number(
                raw_payload,
                "maximum_native_runtime_relative_regression",
                f"{field_path}.maximum_native_runtime_relative_regression",
                _benchmark_error,
            ),
            maximum_python_materialization_absolute_regression_seconds=require_non_negative_finite_number(
                raw_payload,
                "maximum_python_materialization_absolute_regression_seconds",
                f"{field_path}.maximum_python_materialization_absolute_regression_seconds",
                _benchmark_error,
            ),
            maximum_python_materialization_relative_regression=require_non_negative_finite_number(
                raw_payload,
                "maximum_python_materialization_relative_regression",
                f"{field_path}.maximum_python_materialization_relative_regression",
                _benchmark_error,
            ),
            maximum_boundary_transport_absolute_growth_bytes=require_non_negative_integer(
                raw_payload,
                "maximum_boundary_transport_absolute_growth_bytes",
                f"{field_path}.maximum_boundary_transport_absolute_growth_bytes",
                _benchmark_error,
            ),
            maximum_boundary_transport_relative_growth=require_non_negative_finite_number(
                raw_payload,
                "maximum_boundary_transport_relative_growth",
                f"{field_path}.maximum_boundary_transport_relative_growth",
                _benchmark_error,
            ),
            maximum_retained_memory_absolute_growth_bytes=require_non_negative_integer(
                raw_payload,
                "maximum_retained_memory_absolute_growth_bytes",
                f"{field_path}.maximum_retained_memory_absolute_growth_bytes",
                _benchmark_error,
            ),
            maximum_retained_memory_relative_growth=require_non_negative_finite_number(
                raw_payload,
                "maximum_retained_memory_relative_growth",
                f"{field_path}.maximum_retained_memory_relative_growth",
                _benchmark_error,
            ),
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkPolicy(ContractModel):
    schema_version: str
    policy_id: str
    policy_version: str
    comparison_policy: str
    memory_metric_semantics: str
    fixture_thresholds: tuple[RealDataSeededRuntimeFixtureThresholds, ...]

    def __post_init__(self) -> None:
        if self.schema_version != REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION:
            raise _benchmark_error(
                "schema_version",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION}`",
            )
        if not self.policy_id.strip():
            raise _benchmark_error("policy_id", "must be a non-empty string")
        if not self.policy_version.strip():
            raise _benchmark_error("policy_version", "must be a non-empty string")
        if self.comparison_policy != REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY:
            raise _benchmark_error(
                "comparison_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY}`",
            )
        if self.memory_metric_semantics != REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS:
            raise _benchmark_error(
                "memory_metric_semantics",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS}`",
            )
        if len(self.fixture_thresholds) == 0:
            raise _benchmark_error(
                "fixture_thresholds",
                "must contain at least one threshold band",
            )

        fixture_ids = [threshold.fixture_id for threshold in self.fixture_thresholds]
        if len(set(fixture_ids)) != len(fixture_ids):
            raise _benchmark_error(
                "fixture_thresholds",
                "must not contain duplicate fixture_id values",
            )

    def threshold_for_fixture(
        self,
        fixture_id: str,
    ) -> RealDataSeededRuntimeFixtureThresholds:
        for threshold in self.fixture_thresholds:
            if threshold.fixture_id == fixture_id:
                return threshold

        raise _benchmark_error(
            "fixture_id",
            f"missing threshold policy for fixture `{fixture_id}`",
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
    ) -> RealDataSeededRuntimeBenchmarkPolicy:
        if not isinstance(raw_payload, dict):
            raise _benchmark_error("real_data_seeded_runtime_benchmark_policy", "must be an object")

        raw_thresholds = require_sequence(
            raw_payload,
            "fixture_thresholds",
            "fixture_thresholds",
            _benchmark_error,
        )

        return RealDataSeededRuntimeBenchmarkPolicy(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                _benchmark_error,
            ),
            policy_id=require_non_empty_string(
                raw_payload,
                "policy_id",
                "policy_id",
                _benchmark_error,
            ),
            policy_version=require_non_empty_string(
                raw_payload,
                "policy_version",
                "policy_version",
                _benchmark_error,
            ),
            comparison_policy=require_non_empty_string(
                raw_payload,
                "comparison_policy",
                "comparison_policy",
                _benchmark_error,
            ),
            memory_metric_semantics=require_non_empty_string(
                raw_payload,
                "memory_metric_semantics",
                "memory_metric_semantics",
                _benchmark_error,
            ),
            fixture_thresholds=tuple(
                RealDataSeededRuntimeFixtureThresholds.from_mapping(
                    raw_threshold,
                    field_path=f"fixture_thresholds[{threshold_index}]",
                )
                for threshold_index, raw_threshold in enumerate(raw_thresholds)
            ),
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkReportOutputSchemaMetadata(ContractModel):
    required_report_fields: tuple[str, ...]
    required_access_pattern_fields: tuple[str, ...]
    required_provenance_fields: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkProvenance(ContractModel):
    dataset_source_policy: str
    unsigned_market_data_policy: str
    source_tick_data_path: str
    source_tick_data_format: str
    source_tick_data_row_offset: int
    source_tick_data_row_count: int
    source_timestamp_column: str
    source_price_column: str
    source_trade_sign_column: Optional[str]
    market_profile: str
    execution_mode: str
    simulation_seed: int
    simulation_start_time_utc: str
    coarse_interval: str
    coarse_horizon_interval_count: int
    seeded_horizon_interval_count: int
    materialization_access_pattern_ids: tuple[str, ...]
    memory_metric_semantics: str


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeAccessPatternReport(ContractModel):
    pattern_id: str
    access_kind: SeededRuntimeMaterializationAccessKind
    boundary_transport_bytes: int
    python_materialization_seconds: float
    materialized_transition_count: int
    materialized_event_count: int


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkReport(ContractModel):
    schema_version: str
    fixture_id: str
    fixture_version: str
    output_schema_metadata: RealDataSeededRuntimeBenchmarkReportOutputSchemaMetadata
    provenance: RealDataSeededRuntimeBenchmarkProvenance
    observed_source_tick_count: int
    source_open_session_duration_hours: float
    observed_open_event_rate_per_hour: float
    calibrated_expected_event_count_over_observation_window: Optional[float]
    calibrated_open_event_rate_per_hour: Optional[float]
    coarse_generated_event_count: int
    seeded_transition_count: int
    seeded_event_count: int
    native_runtime_seconds: float
    python_response_construction_seconds: float
    python_materialization_seconds: float
    boundary_transport_bytes: int
    native_estimated_retained_bytes: int
    materialized_transition_count: int
    materialized_event_count: int
    access_pattern_reports: tuple[RealDataSeededRuntimeAccessPatternReport, ...]

    def validate(self) -> None:
        if self.schema_version != REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION:
            raise _benchmark_error(
                "schema_version",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION}`",
            )
        if self.output_schema_metadata.required_report_fields != REAL_DATA_SEEDED_RUNTIME_REQUIRED_REPORT_FIELDS:
            raise _benchmark_error(
                "output_schema_metadata.required_report_fields",
                "must match the canonical required report field list",
            )
        if (
            self.output_schema_metadata.required_access_pattern_fields
            != REAL_DATA_SEEDED_RUNTIME_REQUIRED_ACCESS_PATTERN_FIELDS
        ):
            raise _benchmark_error(
                "output_schema_metadata.required_access_pattern_fields",
                "must match the canonical required access-pattern field list",
            )
        if (
            self.output_schema_metadata.required_provenance_fields
            != REAL_DATA_SEEDED_RUNTIME_REQUIRED_PROVENANCE_FIELDS
        ):
            raise _benchmark_error(
                "output_schema_metadata.required_provenance_fields",
                "must match the canonical required provenance field list",
            )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkBaselineRecord(ContractModel):
    fixture_id: str
    fixture_version: str
    observed_source_tick_count: int
    source_open_session_duration_hours: float
    observed_open_event_rate_per_hour: float
    calibrated_expected_event_count_over_observation_window: Optional[float]
    calibrated_open_event_rate_per_hour: Optional[float]
    coarse_generated_event_count: int
    seeded_transition_count: int
    seeded_event_count: int
    native_runtime_seconds: float
    python_response_construction_seconds: float
    python_materialization_seconds: float
    boundary_transport_bytes: int
    native_estimated_retained_bytes: int
    materialized_transition_count: int
    materialized_event_count: int
    access_pattern_reports: tuple[RealDataSeededRuntimeAccessPatternReport, ...]


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkBaselineArtifact(ContractModel):
    schema_version: str
    baseline_id: str
    baseline_version: str
    capture_policy: str
    memory_metric_semantics: str
    source_fixture_set_id: str
    records: tuple[RealDataSeededRuntimeBenchmarkBaselineRecord, ...]

    def validate(self) -> None:
        if self.schema_version != REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION:
            raise _benchmark_error(
                "schema_version",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION}`",
            )
        if self.capture_policy != REAL_DATA_SEEDED_RUNTIME_CAPTURE_POLICY:
            raise _benchmark_error(
                "capture_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_CAPTURE_POLICY}`",
            )
        if self.memory_metric_semantics != REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS:
            raise _benchmark_error(
                "memory_metric_semantics",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS}`",
            )

        if len(self.records) == 0:
            raise _benchmark_error("records", "must contain at least one baseline record")

        fixture_ids = [record.fixture_id for record in self.records]
        if len(set(fixture_ids)) != len(fixture_ids):
            raise _benchmark_error("records", "must not contain duplicate fixture_id values")

    def record_for_fixture(
        self,
        fixture_id: str,
    ) -> RealDataSeededRuntimeBenchmarkBaselineRecord:
        for record in self.records:
            if record.fixture_id == fixture_id:
                return record

        raise _benchmark_error(
            "fixture_id",
            f"missing baseline record for fixture `{fixture_id}`",
        )


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeThresholdEvaluation(ContractModel):
    metric_id: str
    status: BenchmarkThresholdStatus
    failure_classification: Optional[SeededRuntimeFailureClassification]
    candidate_value: float
    reference_value: Optional[float]
    absolute_delta: Optional[float]
    relative_delta: Optional[float]
    maximum_allowed_value: Optional[float]
    maximum_allowed_absolute_delta: Optional[float]
    maximum_allowed_relative_delta: Optional[float]
    summary: str


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeFixtureComparison(ContractModel):
    fixture_id: str
    fixture_version: str
    baseline_version: str
    status: BenchmarkFixtureComparisonStatus
    primary_failure_classification: Optional[SeededRuntimeFailureClassification]
    failure_classifications: tuple[SeededRuntimeFailureClassification, ...]
    threshold_evaluations: tuple[RealDataSeededRuntimeThresholdEvaluation, ...]


@dataclass(frozen=True, slots=True)
class RealDataSeededRuntimeBenchmarkComparisonReport(ContractModel):
    schema_version: str
    comparison_policy: str
    baseline_id: str
    baseline_version: str
    policy_id: str
    policy_version: str
    fixture_comparisons: tuple[RealDataSeededRuntimeFixtureComparison, ...]

    def validate(self) -> None:
        if self.schema_version != REAL_DATA_SEEDED_RUNTIME_COMPARISON_REPORT_SCHEMA_VERSION:
            raise _benchmark_error(
                "schema_version",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_COMPARISON_REPORT_SCHEMA_VERSION}`",
            )
        if self.comparison_policy != REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY:
            raise _benchmark_error(
                "comparison_policy",
                f"must be `{REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY}`",
            )


def load_real_data_seeded_runtime_benchmark_fixture_document(
    payload_bytes: bytes,
) -> RealDataSeededRuntimeBenchmarkFixtureDocument:
    payload_value = json.loads(payload_bytes)
    return RealDataSeededRuntimeBenchmarkFixtureDocument.from_mapping(payload_value)


def load_real_data_seeded_runtime_benchmark_policy_from_json(
    payload_json: str,
) -> RealDataSeededRuntimeBenchmarkPolicy:
    raw_payload = parse_json_object(
        payload_json,
        "real-data seeded runtime benchmark policy",
        _benchmark_error,
    )

    return RealDataSeededRuntimeBenchmarkPolicy.from_mapping(dict(raw_payload))


def load_real_data_seeded_runtime_baseline_from_json(
    payload_json: str,
) -> RealDataSeededRuntimeBenchmarkBaselineArtifact:
    raw_payload = parse_json_object(
        payload_json,
        "real-data seeded runtime baseline artifact",
        _benchmark_error,
    )

    return _baseline_from_mapping(dict(raw_payload))


def serialize_real_data_seeded_runtime_benchmark_report_to_json(
    report: RealDataSeededRuntimeBenchmarkReport,
) -> str:
    report.validate()
    return serialize_pretty_json(report)


def serialize_real_data_seeded_runtime_baseline_to_json(
    baseline: RealDataSeededRuntimeBenchmarkBaselineArtifact,
) -> str:
    baseline.validate()
    return serialize_pretty_json(baseline)


def serialize_real_data_seeded_runtime_comparison_report_to_json(
    comparison_report: RealDataSeededRuntimeBenchmarkComparisonReport,
) -> str:
    comparison_report.validate()
    return serialize_pretty_json(comparison_report)


def _repository_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _resolve_source_tick_data_path(source_tick_data_path: str) -> Path:
    candidate_path = Path(source_tick_data_path)
    if candidate_path.is_absolute():
        return candidate_path

    return _repository_root() / candidate_path


def _write_source_tick_data_slice(
    *,
    fixture: RealDataSeededRuntimeBenchmarkFixture,
    output_path: Path,
) -> None:
    source_path = _resolve_source_tick_data_path(fixture.source_tick_data_path)
    selected_columns = list(
        dict.fromkeys(
            [
                fixture.source_timestamp_column,
                fixture.source_price_column,
                fixture.source_trade_sign_column,
            ]
        )
    )
    selected_columns = [
        column_name
        for column_name in selected_columns
        if column_name is not None
    ]

    source_table = parquet.read_table(
        source_path,
        columns=selected_columns,
    )
    sliced_table = source_table.slice(
        fixture.source_tick_data_row_offset,
        fixture.source_tick_data_row_count,
    )
    if sliced_table.num_rows != fixture.source_tick_data_row_count:
        raise _benchmark_error(
            "source_tick_data_row_count",
            (
                "requested parquet slice extends beyond the available row count "
                f"for `{fixture.source_tick_data_path}`"
            ),
        )

    parquet.write_table(sliced_table, output_path)


def run_real_data_seeded_runtime_benchmark_fixture(
    fixture: RealDataSeededRuntimeBenchmarkFixture,
) -> RealDataSeededRuntimeBenchmarkReport:
    with TemporaryDirectory(prefix="janiform-real-data-seeded-runtime-benchmark-") as temp_dir:
        temp_path = Path(temp_dir)
        tick_data_path = temp_path / "source_tick_slice.parquet"
        _write_source_tick_data_slice(
            fixture=fixture,
            output_path=tick_data_path,
        )

        model, fit_report = JaniformEstimator(
            market_profile=fixture.market_profile
        ).fit(
            tick_data_path=tick_data_path,
            report=True,
            silent=True,
        )
        if model is None:
            raise RuntimeError(f"fit failed for fixture `{fixture.fixture_id}`")

        hawkes_candidate = _selected_hawkes_candidate_diagnostics(fit_report)
        observed_source_tick_count = fit_report.preparation_diagnostics.tick_record_count
        source_regime = cast(dict[str, Any], hawkes_candidate["source_regime"])
        implied_metrics = cast(dict[str, Any], hawkes_candidate["implied_metrics"])

        seeded_simulation = model.simulate(
            seed=fixture.simulation_seed,
            start_time_utc=fixture.simulation_start_time_utc,
            horizon_interval_count=fixture.seeded_horizon_interval_count,
            silent=True,
        )
        seeded_response = cast(SeededTickSimulationResponse, seeded_simulation.raw_result)

        access_pattern_reports = _run_materialization_access_patterns(
            simulation_result=seeded_simulation,
            response=seeded_response,
            access_patterns=fixture.materialization_access_patterns,
        )
        diagnostics_payload = seeded_response.diagnostics.to_payload()

        coarse_simulation = model.simulate(
            seed=fixture.simulation_seed,
            start_time_utc=fixture.simulation_start_time_utc,
            horizon_interval_count=fixture.coarse_horizon_interval_count,
            interval=fixture.coarse_interval,
            silent=True,
        )
        coarse_diagnostics = coarse_simulation.diagnostics
        coarse_generated_event_count = int(
            round(
                cast(float, coarse_diagnostics["cumulative_n_plus"])
                + cast(float, coarse_diagnostics["cumulative_n_minus"])
            )
        )

        report = RealDataSeededRuntimeBenchmarkReport(
            schema_version=REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION,
            fixture_id=fixture.fixture_id,
            fixture_version=fixture.fixture_version,
            output_schema_metadata=RealDataSeededRuntimeBenchmarkReportOutputSchemaMetadata(
                required_report_fields=REAL_DATA_SEEDED_RUNTIME_REQUIRED_REPORT_FIELDS,
                required_access_pattern_fields=(
                    REAL_DATA_SEEDED_RUNTIME_REQUIRED_ACCESS_PATTERN_FIELDS
                ),
                required_provenance_fields=REAL_DATA_SEEDED_RUNTIME_REQUIRED_PROVENANCE_FIELDS,
            ),
            provenance=RealDataSeededRuntimeBenchmarkProvenance(
                dataset_source_policy=REAL_DATA_SEEDED_RUNTIME_DATASET_SOURCE_POLICY,
                unsigned_market_data_policy=REAL_DATA_SEEDED_RUNTIME_UNSIGNED_MARKET_DATA_POLICY,
                source_tick_data_path=fixture.source_tick_data_path,
                source_tick_data_format=fixture.source_tick_data_format,
                source_tick_data_row_offset=fixture.source_tick_data_row_offset,
                source_tick_data_row_count=fixture.source_tick_data_row_count,
                source_timestamp_column=fixture.source_timestamp_column,
                source_price_column=fixture.source_price_column,
                source_trade_sign_column=fixture.source_trade_sign_column,
                market_profile=fixture.market_profile,
                execution_mode=fixture.execution_mode,
                simulation_seed=fixture.simulation_seed,
                simulation_start_time_utc=fixture.simulation_start_time_utc.isoformat(),
                coarse_interval=fixture.coarse_interval,
                coarse_horizon_interval_count=fixture.coarse_horizon_interval_count,
                seeded_horizon_interval_count=fixture.seeded_horizon_interval_count,
                materialization_access_pattern_ids=tuple(
                    pattern.pattern_id for pattern in fixture.materialization_access_patterns
                ),
                memory_metric_semantics=REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS,
            ),
            observed_source_tick_count=observed_source_tick_count,
            source_open_session_duration_hours=cast(
                float,
                source_regime["open_session_duration_hours"],
            ),
            observed_open_event_rate_per_hour=cast(
                float,
                implied_metrics["observed_open_event_rate_per_hour"],
            ),
            calibrated_expected_event_count_over_observation_window=cast(
                float,
                implied_metrics["model_implied_expected_event_count_over_observation_window"],
            ),
            calibrated_open_event_rate_per_hour=cast(
                float,
                implied_metrics["model_implied_open_event_rate_per_hour"],
            ),
            coarse_generated_event_count=coarse_generated_event_count,
            seeded_transition_count=cast(int, diagnostics_payload["transition_count"]),
            seeded_event_count=cast(int, diagnostics_payload["event_count"]),
            native_runtime_seconds=cast(float, diagnostics_payload["native_runtime_seconds"]),
            python_response_construction_seconds=cast(
                float,
                diagnostics_payload["python_response_construction_seconds"],
            ),
            python_materialization_seconds=cast(
                float,
                diagnostics_payload["python_materialization_seconds"],
            ),
            boundary_transport_bytes=cast(
                int,
                diagnostics_payload["boundary_transport_bytes"],
            ),
            native_estimated_retained_bytes=cast(
                int,
                diagnostics_payload["native_estimated_retained_bytes"],
            ),
            materialized_transition_count=cast(
                int,
                diagnostics_payload["materialized_transition_count"],
            ),
            materialized_event_count=cast(
                int,
                diagnostics_payload["materialized_event_count"],
            ),
            access_pattern_reports=access_pattern_reports,
        )

        report.validate()
        return report


def capture_real_data_seeded_runtime_baseline_artifact(
    *,
    fixture_set_id: str,
    reports: tuple[RealDataSeededRuntimeBenchmarkReport, ...],
    baseline_id: str,
    baseline_version: str,
) -> RealDataSeededRuntimeBenchmarkBaselineArtifact:
    if len(reports) == 0:
        raise _benchmark_error("reports", "must contain at least one report")

    baseline = RealDataSeededRuntimeBenchmarkBaselineArtifact(
        schema_version=REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION,
        baseline_id=baseline_id,
        baseline_version=baseline_version,
        capture_policy=REAL_DATA_SEEDED_RUNTIME_CAPTURE_POLICY,
        memory_metric_semantics=REAL_DATA_SEEDED_RUNTIME_MEMORY_METRIC_SEMANTICS,
        source_fixture_set_id=fixture_set_id,
        records=tuple(
            RealDataSeededRuntimeBenchmarkBaselineRecord(
                fixture_id=report.fixture_id,
                fixture_version=report.fixture_version,
                observed_source_tick_count=report.observed_source_tick_count,
                source_open_session_duration_hours=report.source_open_session_duration_hours,
                observed_open_event_rate_per_hour=report.observed_open_event_rate_per_hour,
                calibrated_expected_event_count_over_observation_window=(
                    report.calibrated_expected_event_count_over_observation_window
                ),
                calibrated_open_event_rate_per_hour=report.calibrated_open_event_rate_per_hour,
                coarse_generated_event_count=report.coarse_generated_event_count,
                seeded_transition_count=report.seeded_transition_count,
                seeded_event_count=report.seeded_event_count,
                native_runtime_seconds=report.native_runtime_seconds,
                python_response_construction_seconds=(
                    report.python_response_construction_seconds
                ),
                python_materialization_seconds=report.python_materialization_seconds,
                boundary_transport_bytes=report.boundary_transport_bytes,
                native_estimated_retained_bytes=report.native_estimated_retained_bytes,
                materialized_transition_count=report.materialized_transition_count,
                materialized_event_count=report.materialized_event_count,
                access_pattern_reports=report.access_pattern_reports,
            )
            for report in reports
        ),
    )

    baseline.validate()
    return baseline


def compare_real_data_seeded_runtime_benchmark_report_to_baseline(
    *,
    candidate_reports: tuple[RealDataSeededRuntimeBenchmarkReport, ...],
    baseline: RealDataSeededRuntimeBenchmarkBaselineArtifact,
    policy: RealDataSeededRuntimeBenchmarkPolicy,
) -> RealDataSeededRuntimeBenchmarkComparisonReport:
    if len(candidate_reports) == 0:
        raise _benchmark_error(
            "candidate_reports",
            "must contain at least one benchmark report",
        )

    fixture_comparisons = tuple(
        _compare_fixture_report_to_baseline(
            candidate_report=report,
            baseline_record=baseline.record_for_fixture(report.fixture_id),
            thresholds=policy.threshold_for_fixture(report.fixture_id),
            baseline_version=baseline.baseline_version,
        )
        for report in candidate_reports
    )

    comparison_report = RealDataSeededRuntimeBenchmarkComparisonReport(
        schema_version=REAL_DATA_SEEDED_RUNTIME_COMPARISON_REPORT_SCHEMA_VERSION,
        comparison_policy=REAL_DATA_SEEDED_RUNTIME_COMPARISON_POLICY,
        baseline_id=baseline.baseline_id,
        baseline_version=baseline.baseline_version,
        policy_id=policy.policy_id,
        policy_version=policy.policy_version,
        fixture_comparisons=fixture_comparisons,
    )

    comparison_report.validate()
    return comparison_report


def render_real_data_seeded_runtime_comparison_summary_markdown(
    comparison_report: RealDataSeededRuntimeBenchmarkComparisonReport,
) -> str:
    lines = [
        "## Real-Data Seeded Runtime Benchmark",
        "",
        f"- Baseline id: `{comparison_report.baseline_id}`",
        f"- Baseline version: `{comparison_report.baseline_version}`",
        f"- Policy: `{comparison_report.policy_id}` `{comparison_report.policy_version}`",
        (
            "- Memory metric: `native_estimated_retained_bytes` is an estimated retained-memory "
            "proxy and not true peak RSS."
        ),
    ]

    for fixture_comparison in comparison_report.fixture_comparisons:
        lines.append("")
        lines.append(f"### {fixture_comparison.fixture_id}")
        lines.append(f"- Status: `{fixture_comparison.status.value}`")

        if fixture_comparison.primary_failure_classification is not None:
            lines.append(
                "- Primary failure classification: "
                f"`{fixture_comparison.primary_failure_classification.value}`"
            )
        else:
            lines.append("- Primary failure classification: `none`")

        failing_evaluations = [
            evaluation
            for evaluation in fixture_comparison.threshold_evaluations
            if evaluation.status is BenchmarkThresholdStatus.FAIL
        ]
        if len(failing_evaluations) == 0:
            lines.append("- Threshold failures: none")
            continue

        for evaluation in failing_evaluations:
            delta_suffix = ""
            if evaluation.absolute_delta is not None:
                delta_suffix = f", delta={evaluation.absolute_delta:.6f}"
                if evaluation.relative_delta is not None and evaluation.relative_delta != float("inf"):
                    delta_suffix += f", relative_delta={evaluation.relative_delta:.6f}"

            lines.append(
                f"- {evaluation.metric_id}: {evaluation.summary}{delta_suffix}"
            )

    return "\n".join(lines) + "\n"


def _run_materialization_access_patterns(
    *,
    simulation_result: JaniformSimulationResult,
    response: SeededTickSimulationResponse,
    access_patterns: tuple[RealDataSeededRuntimeMaterializationPattern, ...],
) -> tuple[RealDataSeededRuntimeAccessPatternReport, ...]:
    reports: list[RealDataSeededRuntimeAccessPatternReport] = []
    for access_pattern in access_patterns:
        before = response.diagnostics.to_payload()

        match access_pattern.access_kind:
            case SeededRuntimeMaterializationAccessKind.EVENT_TAPE_HEAD:
                _ = response.event_tape[: cast(int, access_pattern.record_count)]
            case SeededRuntimeMaterializationAccessKind.FULL_SEEDED_EXPORT_TO_PANDAS:
                _ = simulation_result.to_pandas()

        after = response.diagnostics.to_payload()
        boundary_transport_bytes = cast(
            int,
            after["boundary_transport_bytes"],
        ) - cast(int, before["boundary_transport_bytes"])
        python_materialization_seconds = cast(
            float,
            after["python_materialization_seconds"],
        ) - cast(float, before["python_materialization_seconds"])
        materialized_transition_count = cast(
            int,
            after["materialized_transition_count"],
        ) - cast(int, before["materialized_transition_count"])
        materialized_event_count = cast(
            int,
            after["materialized_event_count"],
        ) - cast(int, before["materialized_event_count"])

        if (
            access_pattern.access_kind
            == SeededRuntimeMaterializationAccessKind.FULL_SEEDED_EXPORT_TO_PANDAS
            and materialized_transition_count == 0
        ):
            materialized_transition_count = len(response.fine_path)
            boundary_transport_bytes = max(
                0,
                cast(int, response.diagnostics.boundary_transport_bytes)
                - cast(int, before["boundary_transport_bytes"]),
            )
            python_materialization_seconds = max(
                0.0,
                cast(float, response.diagnostics.python_materialization_seconds)
                - cast(float, before["python_materialization_seconds"]),
            )

        reports.append(
            RealDataSeededRuntimeAccessPatternReport(
                pattern_id=access_pattern.pattern_id,
                access_kind=access_pattern.access_kind,
                boundary_transport_bytes=boundary_transport_bytes,
                python_materialization_seconds=python_materialization_seconds,
                materialized_transition_count=materialized_transition_count,
                materialized_event_count=materialized_event_count,
            )
        )

    return tuple(reports)


def _selected_hawkes_candidate_diagnostics(
    fit_report: JaniformFitReport,
) -> dict[str, Any]:
    hawkes_stage_payload = next(
        payload
        for payload in fit_report.stage_report_payloads
        if payload.get("stage") == "hawkes"
    )
    fitted_stage_payload = cast(
        dict[str, Any],
        hawkes_stage_payload["fitted_stage_payload"],
    )
    selected_candidate_id = cast(str, fitted_stage_payload["selected_candidate_id"])
    candidate_diagnostics = cast(
        list[dict[str, Any]],
        fitted_stage_payload["candidate_diagnostics"],
    )

    return next(
        candidate
        for candidate in candidate_diagnostics
        if candidate["candidate_id"] == selected_candidate_id
    )


def _compare_fixture_report_to_baseline(
    *,
    candidate_report: RealDataSeededRuntimeBenchmarkReport,
    baseline_record: RealDataSeededRuntimeBenchmarkBaselineRecord,
    thresholds: RealDataSeededRuntimeFixtureThresholds,
    baseline_version: str,
) -> RealDataSeededRuntimeFixtureComparison:
    evaluations: list[RealDataSeededRuntimeThresholdEvaluation] = []

    seeded_event_rate_ratio_vs_observed = (
        candidate_report.seeded_event_count
        / candidate_report.provenance.seeded_horizon_interval_count
    ) / max(candidate_report.observed_open_event_rate_per_hour, _FLOAT_ZERO_EPSILON)
    evaluations.append(
        _value_threshold_evaluation(
            metric_id="seeded_event_rate_ratio_vs_observed_open",
            candidate_value=seeded_event_rate_ratio_vs_observed,
            maximum_allowed_value=thresholds.maximum_seeded_event_rate_ratio_vs_observed_open,
            failure_classification=SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT,
        )
    )

    coarse_event_rate_ratio_vs_observed = (
        candidate_report.coarse_generated_event_count
        / candidate_report.provenance.coarse_horizon_interval_count
    ) / max(candidate_report.observed_open_event_rate_per_hour, _FLOAT_ZERO_EPSILON)
    evaluations.append(
        _value_threshold_evaluation(
            metric_id="coarse_event_rate_ratio_vs_observed_open",
            candidate_value=coarse_event_rate_ratio_vs_observed,
            maximum_allowed_value=thresholds.maximum_coarse_event_rate_ratio_vs_observed_open,
            failure_classification=SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT,
        )
    )

    if candidate_report.calibrated_open_event_rate_per_hour is not None:
        seeded_event_rate_ratio_vs_calibrated = (
            candidate_report.seeded_event_count
            / candidate_report.provenance.seeded_horizon_interval_count
        ) / max(candidate_report.calibrated_open_event_rate_per_hour, _FLOAT_ZERO_EPSILON)
        evaluations.append(
            _value_threshold_evaluation(
                metric_id="seeded_event_rate_ratio_vs_calibrated_open",
                candidate_value=seeded_event_rate_ratio_vs_calibrated,
                maximum_allowed_value=(
                    thresholds.maximum_seeded_event_rate_ratio_vs_calibrated_open
                ),
                failure_classification=(
                    SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT
                ),
            )
        )

    evaluations.append(
        _regression_threshold_evaluation(
            metric_id="native_runtime_seconds",
            candidate_value=candidate_report.native_runtime_seconds,
            baseline_value=baseline_record.native_runtime_seconds,
            maximum_allowed_absolute_delta=(
                thresholds.maximum_native_runtime_absolute_regression_seconds
            ),
            maximum_allowed_relative_delta=(
                thresholds.maximum_native_runtime_relative_regression
            ),
            failure_classification=SeededRuntimeFailureClassification.RUNTIME_DRIFT,
        )
    )
    evaluations.append(
        _regression_threshold_evaluation(
            metric_id="python_materialization_seconds",
            candidate_value=candidate_report.python_materialization_seconds,
            baseline_value=baseline_record.python_materialization_seconds,
            maximum_allowed_absolute_delta=(
                thresholds.maximum_python_materialization_absolute_regression_seconds
            ),
            maximum_allowed_relative_delta=(
                thresholds.maximum_python_materialization_relative_regression
            ),
            failure_classification=SeededRuntimeFailureClassification.RUNTIME_DRIFT,
        )
    )
    evaluations.append(
        _regression_threshold_evaluation(
            metric_id="boundary_transport_bytes",
            candidate_value=float(candidate_report.boundary_transport_bytes),
            baseline_value=float(baseline_record.boundary_transport_bytes),
            maximum_allowed_absolute_delta=float(
                thresholds.maximum_boundary_transport_absolute_growth_bytes
            ),
            maximum_allowed_relative_delta=(
                thresholds.maximum_boundary_transport_relative_growth
            ),
            failure_classification=(
                SeededRuntimeFailureClassification.BOUNDARY_TRANSPORT_DRIFT
            ),
        )
    )
    evaluations.append(
        _regression_threshold_evaluation(
            metric_id="native_estimated_retained_bytes",
            candidate_value=float(candidate_report.native_estimated_retained_bytes),
            baseline_value=float(baseline_record.native_estimated_retained_bytes),
            maximum_allowed_absolute_delta=float(
                thresholds.maximum_retained_memory_absolute_growth_bytes
            ),
            maximum_allowed_relative_delta=(
                thresholds.maximum_retained_memory_relative_growth
            ),
            failure_classification=(
                SeededRuntimeFailureClassification.RETAINED_MEMORY_DRIFT
            ),
        )
    )

    failing_classifications = tuple(
        dict.fromkeys(
            evaluation.failure_classification
            for evaluation in evaluations
            if evaluation.status is BenchmarkThresholdStatus.FAIL
            and evaluation.failure_classification is not None
        )
    )
    status = (
        BenchmarkFixtureComparisonStatus.FAIL
        if len(failing_classifications) > 0
        else BenchmarkFixtureComparisonStatus.PASS
    )
    primary_failure_classification = _primary_failure_classification(
        failing_classifications
    )

    return RealDataSeededRuntimeFixtureComparison(
        fixture_id=candidate_report.fixture_id,
        fixture_version=candidate_report.fixture_version,
        baseline_version=baseline_version,
        status=status,
        primary_failure_classification=primary_failure_classification,
        failure_classifications=cast(
            tuple[SeededRuntimeFailureClassification, ...],
            failing_classifications,
        ),
        threshold_evaluations=tuple(evaluations),
    )


def _value_threshold_evaluation(
    *,
    metric_id: str,
    candidate_value: float,
    maximum_allowed_value: float,
    failure_classification: SeededRuntimeFailureClassification,
) -> RealDataSeededRuntimeThresholdEvaluation:
    status = (
        BenchmarkThresholdStatus.FAIL
        if candidate_value > maximum_allowed_value
        else BenchmarkThresholdStatus.PASS
    )

    return RealDataSeededRuntimeThresholdEvaluation(
        metric_id=metric_id,
        status=status,
        failure_classification=(
            failure_classification if status is BenchmarkThresholdStatus.FAIL else None
        ),
        candidate_value=candidate_value,
        reference_value=None,
        absolute_delta=None,
        relative_delta=None,
        maximum_allowed_value=maximum_allowed_value,
        maximum_allowed_absolute_delta=None,
        maximum_allowed_relative_delta=None,
        summary=(
            f"candidate value {candidate_value:.6f} must be <= {maximum_allowed_value:.6f}"
        ),
    )


def _regression_threshold_evaluation(
    *,
    metric_id: str,
    candidate_value: float,
    baseline_value: float,
    maximum_allowed_absolute_delta: float,
    maximum_allowed_relative_delta: float,
    failure_classification: SeededRuntimeFailureClassification,
) -> RealDataSeededRuntimeThresholdEvaluation:
    absolute_delta = candidate_value - baseline_value
    relative_delta = _relative_delta(candidate_value, baseline_value)
    exceeds_absolute_threshold = absolute_delta > maximum_allowed_absolute_delta
    exceeds_relative_threshold = relative_delta > maximum_allowed_relative_delta
    status = (
        BenchmarkThresholdStatus.FAIL
        if exceeds_absolute_threshold and exceeds_relative_threshold
        else BenchmarkThresholdStatus.PASS
    )

    return RealDataSeededRuntimeThresholdEvaluation(
        metric_id=metric_id,
        status=status,
        failure_classification=(
            failure_classification if status is BenchmarkThresholdStatus.FAIL else None
        ),
        candidate_value=candidate_value,
        reference_value=baseline_value,
        absolute_delta=absolute_delta,
        relative_delta=relative_delta,
        maximum_allowed_value=None,
        maximum_allowed_absolute_delta=maximum_allowed_absolute_delta,
        maximum_allowed_relative_delta=maximum_allowed_relative_delta,
        summary=(
            "candidate regression must stay within both absolute and relative growth limits "
            f"(baseline={baseline_value:.6f}, candidate={candidate_value:.6f})"
        ),
    )


def _relative_delta(candidate_value: float, baseline_value: float) -> float:
    if abs(baseline_value) <= _FLOAT_ZERO_EPSILON:
        return 0.0 if abs(candidate_value) <= _FLOAT_ZERO_EPSILON else float("inf")

    return (candidate_value - baseline_value) / baseline_value


def _primary_failure_classification(
    classifications: tuple[Optional[SeededRuntimeFailureClassification], ...],
) -> Optional[SeededRuntimeFailureClassification]:
    ordering = (
        SeededRuntimeFailureClassification.MODEL_REGIME_DRIFT,
        SeededRuntimeFailureClassification.RUNTIME_DRIFT,
        SeededRuntimeFailureClassification.BOUNDARY_TRANSPORT_DRIFT,
        SeededRuntimeFailureClassification.RETAINED_MEMORY_DRIFT,
    )
    present = {
        classification
        for classification in classifications
        if classification is not None
    }

    for classification in ordering:
        if classification in present:
            return classification

    return None


def _baseline_from_mapping(
    raw_payload: dict[str, Any],
) -> RealDataSeededRuntimeBenchmarkBaselineArtifact:
    raw_records = require_sequence(
        raw_payload,
        "records",
        "records",
        _benchmark_error,
    )
    baseline = RealDataSeededRuntimeBenchmarkBaselineArtifact(
        schema_version=require_non_empty_string(
            raw_payload,
            "schema_version",
            "schema_version",
            _benchmark_error,
        ),
        baseline_id=require_non_empty_string(
            raw_payload,
            "baseline_id",
            "baseline_id",
            _benchmark_error,
        ),
        baseline_version=require_non_empty_string(
            raw_payload,
            "baseline_version",
            "baseline_version",
            _benchmark_error,
        ),
        capture_policy=require_non_empty_string(
            raw_payload,
            "capture_policy",
            "capture_policy",
            _benchmark_error,
        ),
        memory_metric_semantics=require_non_empty_string(
            raw_payload,
            "memory_metric_semantics",
            "memory_metric_semantics",
            _benchmark_error,
        ),
        source_fixture_set_id=require_non_empty_string(
            raw_payload,
            "source_fixture_set_id",
            "source_fixture_set_id",
            _benchmark_error,
        ),
        records=tuple(
            _baseline_record_from_mapping(
                cast(dict[str, Any], raw_record),
                field_path=f"records[{record_index}]",
            )
            for record_index, raw_record in enumerate(raw_records)
        ),
    )

    baseline.validate()
    return baseline


def _baseline_record_from_mapping(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> RealDataSeededRuntimeBenchmarkBaselineRecord:
    if not isinstance(raw_payload, dict):
        raise _benchmark_error(field_path, "must be an object")

    raw_access_pattern_reports = require_sequence(
        raw_payload,
        "access_pattern_reports",
        f"{field_path}.access_pattern_reports",
        _benchmark_error,
    )

    return RealDataSeededRuntimeBenchmarkBaselineRecord(
        fixture_id=require_non_empty_string(
            raw_payload,
            "fixture_id",
            f"{field_path}.fixture_id",
            _benchmark_error,
        ),
        fixture_version=require_non_empty_string(
            raw_payload,
            "fixture_version",
            f"{field_path}.fixture_version",
            _benchmark_error,
        ),
        observed_source_tick_count=require_non_negative_integer(
            raw_payload,
            "observed_source_tick_count",
            f"{field_path}.observed_source_tick_count",
            _benchmark_error,
        ),
        source_open_session_duration_hours=require_positive_finite_number(
            raw_payload,
            "source_open_session_duration_hours",
            f"{field_path}.source_open_session_duration_hours",
            _benchmark_error,
        ),
        observed_open_event_rate_per_hour=require_positive_finite_number(
            raw_payload,
            "observed_open_event_rate_per_hour",
            f"{field_path}.observed_open_event_rate_per_hour",
            _benchmark_error,
        ),
        calibrated_expected_event_count_over_observation_window=_optional_non_negative_number(
            raw_payload,
            "calibrated_expected_event_count_over_observation_window",
            f"{field_path}.calibrated_expected_event_count_over_observation_window",
        ),
        calibrated_open_event_rate_per_hour=_optional_non_negative_number(
            raw_payload,
            "calibrated_open_event_rate_per_hour",
            f"{field_path}.calibrated_open_event_rate_per_hour",
        ),
        coarse_generated_event_count=require_non_negative_integer(
            raw_payload,
            "coarse_generated_event_count",
            f"{field_path}.coarse_generated_event_count",
            _benchmark_error,
        ),
        seeded_transition_count=require_non_negative_integer(
            raw_payload,
            "seeded_transition_count",
            f"{field_path}.seeded_transition_count",
            _benchmark_error,
        ),
        seeded_event_count=require_non_negative_integer(
            raw_payload,
            "seeded_event_count",
            f"{field_path}.seeded_event_count",
            _benchmark_error,
        ),
        native_runtime_seconds=require_non_negative_finite_number(
            raw_payload,
            "native_runtime_seconds",
            f"{field_path}.native_runtime_seconds",
            _benchmark_error,
        ),
        python_response_construction_seconds=require_non_negative_finite_number(
            raw_payload,
            "python_response_construction_seconds",
            f"{field_path}.python_response_construction_seconds",
            _benchmark_error,
        ),
        python_materialization_seconds=require_non_negative_finite_number(
            raw_payload,
            "python_materialization_seconds",
            f"{field_path}.python_materialization_seconds",
            _benchmark_error,
        ),
        boundary_transport_bytes=require_non_negative_integer(
            raw_payload,
            "boundary_transport_bytes",
            f"{field_path}.boundary_transport_bytes",
            _benchmark_error,
        ),
        native_estimated_retained_bytes=require_non_negative_integer(
            raw_payload,
            "native_estimated_retained_bytes",
            f"{field_path}.native_estimated_retained_bytes",
            _benchmark_error,
        ),
        materialized_transition_count=require_non_negative_integer(
            raw_payload,
            "materialized_transition_count",
            f"{field_path}.materialized_transition_count",
            _benchmark_error,
        ),
        materialized_event_count=require_non_negative_integer(
            raw_payload,
            "materialized_event_count",
            f"{field_path}.materialized_event_count",
            _benchmark_error,
        ),
        access_pattern_reports=tuple(
            _access_pattern_report_from_mapping(
                raw_access_pattern_report,
                field_path=(
                    f"{field_path}.access_pattern_reports[{access_pattern_index}]"
                ),
            )
            for access_pattern_index, raw_access_pattern_report in enumerate(
                raw_access_pattern_reports
            )
        ),
    )


def _access_pattern_report_from_mapping(
    raw_payload: dict[str, Any] | Any,
    *,
    field_path: str,
) -> RealDataSeededRuntimeAccessPatternReport:
    if not isinstance(raw_payload, dict):
        raise _benchmark_error(field_path, "must be an object")

    raw_access_kind = require_non_empty_string(
        raw_payload,
        "access_kind",
        f"{field_path}.access_kind",
        _benchmark_error,
    )

    try:
        access_kind = SeededRuntimeMaterializationAccessKind(raw_access_kind)
    except ValueError as error:
        raise _benchmark_error(
            f"{field_path}.access_kind",
            "must be a supported materialization access kind",
        ) from error

    return RealDataSeededRuntimeAccessPatternReport(
        pattern_id=require_non_empty_string(
            raw_payload,
            "pattern_id",
            f"{field_path}.pattern_id",
            _benchmark_error,
        ),
        access_kind=access_kind,
        boundary_transport_bytes=require_non_negative_integer(
            raw_payload,
            "boundary_transport_bytes",
            f"{field_path}.boundary_transport_bytes",
            _benchmark_error,
        ),
        python_materialization_seconds=require_non_negative_finite_number(
            raw_payload,
            "python_materialization_seconds",
            f"{field_path}.python_materialization_seconds",
            _benchmark_error,
        ),
        materialized_transition_count=require_non_negative_integer(
            raw_payload,
            "materialized_transition_count",
            f"{field_path}.materialized_transition_count",
            _benchmark_error,
        ),
        materialized_event_count=require_non_negative_integer(
            raw_payload,
            "materialized_event_count",
            f"{field_path}.materialized_event_count",
            _benchmark_error,
        ),
    )


def _optional_non_empty_string(
    mapping: dict[str, Any],
    field_name: str,
    field_path: str,
) -> Optional[str]:
    if field_name not in mapping or mapping[field_name] is None:
        return None

    raw_value = mapping[field_name]
    if not isinstance(raw_value, str):
        raise _benchmark_error(field_path, "must be a string when provided")

    if not raw_value.strip():
        raise _benchmark_error(field_path, "must be a non-empty string when provided")

    return raw_value


def _optional_non_negative_number(
    mapping: dict[str, Any],
    field_name: str,
    field_path: str,
) -> Optional[float]:
    if field_name not in mapping or mapping[field_name] is None:
        return None

    return require_non_negative_finite_number(
        mapping,
        field_name,
        field_path,
        _benchmark_error,
    )


__all__ = [
    "REAL_DATA_SEEDED_RUNTIME_BASELINE_SCHEMA_VERSION",
    "REAL_DATA_SEEDED_RUNTIME_BENCHMARK_FIXTURE_SCHEMA_VERSION",
    "REAL_DATA_SEEDED_RUNTIME_BENCHMARK_POLICY_SCHEMA_VERSION",
    "REAL_DATA_SEEDED_RUNTIME_BENCHMARK_REPORT_SCHEMA_VERSION",
    "REAL_DATA_SEEDED_RUNTIME_COMPARISON_REPORT_SCHEMA_VERSION",
    "RealDataSeededRuntimeBenchmarkBaselineArtifact",
    "RealDataSeededRuntimeBenchmarkComparisonReport",
    "RealDataSeededRuntimeBenchmarkFixture",
    "RealDataSeededRuntimeBenchmarkFixtureDocument",
    "RealDataSeededRuntimeBenchmarkPolicy",
    "RealDataSeededRuntimeBenchmarkReport",
    "capture_real_data_seeded_runtime_baseline_artifact",
    "compare_real_data_seeded_runtime_benchmark_report_to_baseline",
    "load_real_data_seeded_runtime_baseline_from_json",
    "load_real_data_seeded_runtime_benchmark_fixture_document",
    "load_real_data_seeded_runtime_benchmark_policy_from_json",
    "render_real_data_seeded_runtime_comparison_summary_markdown",
    "run_real_data_seeded_runtime_benchmark_fixture",
    "serialize_real_data_seeded_runtime_baseline_to_json",
    "serialize_real_data_seeded_runtime_comparison_report_to_json",
    "serialize_real_data_seeded_runtime_benchmark_report_to_json",
]
