from dataclasses import dataclass
from typing import Any, Optional

from .contracts import (
    CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationKnownNewsInput,
    CalibrationMarketDataColumnMapping,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataWorkflowScope,
    CalibrationSessionConfigurationInput,
    validate_known_news_inputs,
    validate_option_surface_scope,
    validate_tick_price_input_policy,
)
from .._errors import CALIBRATION_ERROR_FACTORY, _calibration_error
from ...contracts import (
    ContractModel,
    parse_json_object,
    require_mapping,
    require_non_empty_string,
    require_string,
    serialize_deterministic_json,
)


@dataclass(frozen=True, slots=True)
class MarketDataCalibrationRequest(ContractModel):
    schema_version: str
    run_id: str
    tick_data_source: CalibrationInputFileReference
    session_configuration: CalibrationSessionConfigurationInput
    column_mapping: CalibrationMarketDataColumnMapping
    workflow_scope: CalibrationMarketDataWorkflowScope
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    output_preferences: CalibrationMarketDataOutputPreferences
    option_surface_source: Optional[CalibrationInputFileReference] = None
    known_news_inputs: Optional[CalibrationKnownNewsInput] = None

    def __post_init__(self) -> None:
        if self.schema_version != CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION:
            raise _calibration_error(
                "schema_version",
                f"must be `{CALIBRATION_MARKET_DATA_REQUEST_SCHEMA_VERSION}`",
            )
        if not self.run_id.strip():
            raise _calibration_error("run_id", "must be a non-empty string")
        if self.tick_data_source.file_format not in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET):
            raise _calibration_error("tick_data_source.file_format", "must be one of: csv, parquet")
        if (
            self.option_surface_source is not None
            and self.option_surface_source.file_format not in (CalibrationInputFileFormat.CSV, CalibrationInputFileFormat.PARQUET)
        ):
            raise _calibration_error("option_surface_source.file_format", "must be one of: csv, parquet")

        validate_tick_price_input_policy(
            workflow_scope=self.workflow_scope,
            column_mapping=self.column_mapping.tick_data,
            normalization_policy=self.normalization_policy,
        )
        validate_option_surface_scope(
            workflow_scope=self.workflow_scope,
            option_surface_source=self.option_surface_source,
            option_surface_column_mapping=self.column_mapping.option_surface,
            option_quote_input_policy=self.normalization_policy.option_quote_input_policy,
        )
        validate_known_news_inputs(
            known_news_inputs=self.known_news_inputs,
            known_news_column_mapping=self.column_mapping.known_news,
        )

    @staticmethod
    def from_mapping(raw_payload: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
        if not isinstance(raw_payload, dict):
            raise _calibration_error("market_data_calibration_request", "must be an object")

        return MarketDataCalibrationRequest(
            schema_version=require_non_empty_string(
                raw_payload,
                "schema_version",
                "schema_version",
                CALIBRATION_ERROR_FACTORY,
            ),
            run_id=require_non_empty_string(raw_payload, "run_id", "run_id", CALIBRATION_ERROR_FACTORY),
            tick_data_source=CalibrationInputFileReference.from_mapping(
                require_mapping(raw_payload, "tick_data_source", "tick_data_source", CALIBRATION_ERROR_FACTORY),
                field_path="tick_data_source",
            ),
            session_configuration=CalibrationSessionConfigurationInput.from_mapping(
                require_mapping(raw_payload, "session_configuration", "session_configuration", CALIBRATION_ERROR_FACTORY),
                field_path="session_configuration",
            ),
            column_mapping=CalibrationMarketDataColumnMapping.from_mapping(
                require_mapping(raw_payload, "column_mapping", "column_mapping", CALIBRATION_ERROR_FACTORY),
                field_path="column_mapping",
            ),
            workflow_scope=CalibrationMarketDataWorkflowScope(
                require_string(raw_payload, "workflow_scope", "workflow_scope", CALIBRATION_ERROR_FACTORY)
            ),
            normalization_policy=CalibrationMarketDataNormalizationPolicy.from_mapping(
                require_mapping(raw_payload, "normalization_policy", "normalization_policy", CALIBRATION_ERROR_FACTORY),
                field_path="normalization_policy",
            ),
            output_preferences=CalibrationMarketDataOutputPreferences.from_mapping(
                require_mapping(raw_payload, "output_preferences", "output_preferences", CALIBRATION_ERROR_FACTORY),
                field_path="output_preferences",
            ),
            option_surface_source=(
                CalibrationInputFileReference.from_mapping(
                    require_mapping(
                        raw_payload,
                        "option_surface_source",
                        "option_surface_source",
                        CALIBRATION_ERROR_FACTORY,
                    ),
                    field_path="option_surface_source",
                )
                if "option_surface_source" in raw_payload and raw_payload["option_surface_source"] is not None
                else None
            ),
            known_news_inputs=(
                CalibrationKnownNewsInput.from_mapping(
                    require_mapping(raw_payload, "known_news_inputs", "known_news_inputs", CALIBRATION_ERROR_FACTORY),
                    field_path="known_news_inputs",
                )
                if "known_news_inputs" in raw_payload and raw_payload["known_news_inputs"] is not None
                else None
            ),
        )


def validate_market_data_calibration_request(raw_request: dict[str, Any] | Any) -> MarketDataCalibrationRequest:
    return MarketDataCalibrationRequest.from_mapping(raw_request)


def serialize_market_data_calibration_request(request: MarketDataCalibrationRequest) -> str:
    return serialize_deterministic_json(request)


def deserialize_market_data_calibration_request(payload_json: str) -> MarketDataCalibrationRequest:
    raw_payload = parse_json_object(payload_json, "market_data_calibration_request", CALIBRATION_ERROR_FACTORY)
    return validate_market_data_calibration_request(dict(raw_payload))


__all__ = [
    "MarketDataCalibrationRequest",
    "deserialize_market_data_calibration_request",
    "serialize_market_data_calibration_request",
    "validate_market_data_calibration_request",
]
