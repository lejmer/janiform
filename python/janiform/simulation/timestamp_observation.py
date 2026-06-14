from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from typing import Any, Final, Optional

from ..contracts import ContractModel
from ..contracts.base import (
    require_positive_integer,
    require_string,
)
from .shared.errors import SIMULATION_ERROR_FACTORY, _simulation_error


class TimestampBatchingObservationMode(StrEnum):
    DISABLED = 'disabled'
    FIXED_BUCKET_FLOOR = 'fixed_bucket_floor'


TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION: Final[str] = "model.timestamp_batching_observation.v1"
TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY: Final[str] = (
    "same_session_duplicate_timestamps_with_positive_timestamp_quantum_gcd.v1"
)
TIMESTAMP_BATCHING_DISABLED_SOURCE: Final[str] = "unavailable"
TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY: Final[str] = (
    "latent_event_time_equals_observed_timestamp.v1"
)
TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY: Final[str] = (
    "floor_latent_event_time_to_fixed_width_bucket_within_latent_interval.v1"
)
TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY: Final[str] = (
    "observed_timestamp_ownership_constrained_to_latent_interval_for_direct_coarse_consistency.v1"
)


@dataclass(frozen=True, slots=True)
class TimestampBatchingObservationModel(ContractModel):
    schema_version: str
    mode: TimestampBatchingObservationMode
    bucket_width_source: str
    inference_policy: str
    observation_policy: str
    coarse_interval_compatibility_policy: str
    bucket_width_nanos: Optional[int] = None

    def __post_init__(self) -> None:
        if self.schema_version != TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION:
            raise _simulation_error(
                "timestamp_batching_observation_model.schema_version",
                f"must be `{TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION}`",
            )

        for field_name, field_value in (
            ("bucket_width_source", self.bucket_width_source),
            ("inference_policy", self.inference_policy),
            ("observation_policy", self.observation_policy),
            (
                "coarse_interval_compatibility_policy",
                self.coarse_interval_compatibility_policy,
            ),
        ):
            if not field_value.strip():
                raise _simulation_error(
                    f"timestamp_batching_observation_model.{field_name}",
                    "must be a non-empty string",
                )

        if self.coarse_interval_compatibility_policy != TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY:
            raise _simulation_error(
                "timestamp_batching_observation_model.coarse_interval_compatibility_policy",
                f"must be `{TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY}`",
            )

        match self.mode:
            case TimestampBatchingObservationMode.DISABLED:
                if self.bucket_width_nanos is not None:
                    raise _simulation_error(
                        "timestamp_batching_observation_model.bucket_width_nanos",
                        "must be omitted when timestamp batching is disabled",
                    )

                if self.observation_policy != TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY:
                    raise _simulation_error(
                        "timestamp_batching_observation_model.observation_policy",
                        f"must be `{TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY}` when mode is disabled",
                    )
            case TimestampBatchingObservationMode.FIXED_BUCKET_FLOOR:
                if self.bucket_width_nanos is None or self.bucket_width_nanos <= 0:
                    raise _simulation_error(
                        "timestamp_batching_observation_model.bucket_width_nanos",
                        "must be strictly positive when mode is fixed_bucket_floor",
                    )

                if self.observation_policy != TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY:
                    raise _simulation_error(
                        "timestamp_batching_observation_model.observation_policy",
                        f"must be `{TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY}` when mode is fixed_bucket_floor",
                    )

    @staticmethod
    def disabled() -> TimestampBatchingObservationModel:
        return TimestampBatchingObservationModel(
            schema_version=TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION,
            mode=TimestampBatchingObservationMode.DISABLED,
            bucket_width_nanos=None,
            bucket_width_source=TIMESTAMP_BATCHING_DISABLED_SOURCE,
            inference_policy=TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY,
            observation_policy=TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY,
            coarse_interval_compatibility_policy=TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY,
        )

    @staticmethod
    def from_mapping(
        raw_payload: dict[str, Any] | Any,
        *,
        field_path: str,
    ) -> TimestampBatchingObservationModel:
        if not isinstance(raw_payload, dict):
            raise _simulation_error(field_path, "must be an object")

        raw_mode = require_string(
            raw_payload,
            "mode",
            f"{field_path}.mode",
            SIMULATION_ERROR_FACTORY,
        )

        try:
            mode = TimestampBatchingObservationMode(raw_mode)
        except ValueError as e:
            raise _simulation_error(
                f"{field_path}.mode",
                "must be either 'disabled' or 'fixed_bucket_floor'",
            ) from e

        raw_bucket_width_nanos = raw_payload.get("bucket_width_nanos")

        bucket_width_nanos = (
            require_positive_integer(
                raw_payload,
                "bucket_width_nanos",
                f"{field_path}.bucket_width_nanos",
                SIMULATION_ERROR_FACTORY,
            )
            if raw_bucket_width_nanos is not None
            else None
        )

        return TimestampBatchingObservationModel(
            schema_version=require_string(
                raw_payload,
                "schema_version",
                f"{field_path}.schema_version",
                SIMULATION_ERROR_FACTORY,
            ),
            mode=mode,
            bucket_width_nanos=bucket_width_nanos,
            bucket_width_source=require_string(
                raw_payload,
                "bucket_width_source",
                f"{field_path}.bucket_width_source",
                SIMULATION_ERROR_FACTORY,
            ),
            inference_policy=require_string(
                raw_payload,
                "inference_policy",
                f"{field_path}.inference_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            observation_policy=require_string(
                raw_payload,
                "observation_policy",
                f"{field_path}.observation_policy",
                SIMULATION_ERROR_FACTORY,
            ),
            coarse_interval_compatibility_policy=require_string(
                raw_payload,
                "coarse_interval_compatibility_policy",
                f"{field_path}.coarse_interval_compatibility_policy",
                SIMULATION_ERROR_FACTORY,
            ),
        )


__all__ = [
    "TIMESTAMP_BATCHING_DIRECT_COARSE_COMPATIBILITY_POLICY",
    "TIMESTAMP_BATCHING_DISABLED_OBSERVATION_POLICY",
    "TIMESTAMP_BATCHING_DISABLED_SOURCE",
    "TIMESTAMP_BATCHING_FIXED_BUCKET_INFERENCE_POLICY",
    "TIMESTAMP_BATCHING_FIXED_BUCKET_OBSERVATION_POLICY",
    "TIMESTAMP_BATCHING_OBSERVATION_MODEL_SCHEMA_VERSION",
    "TimestampBatchingObservationMode",
    "TimestampBatchingObservationModel",
]
