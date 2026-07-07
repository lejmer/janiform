from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Optional

from ....contracts.base import (
    require_mapping,
    require_non_empty_string,
    require_non_empty_string_value,
    require_optional_string_value,
)
from ...errors import calibration_error


@dataclass(frozen=True, slots=True)
class ModelTickDataInputInference:
    file_format: str
    timestamp_column: str
    timestamp_timezone: str
    timestamp_timezone_source: str
    tick_price_input_policy: Optional[str]
    price_column: Optional[str]
    trade_sign_column: Optional[str]
    bid_column: Optional[str]
    ask_column: Optional[str]


def tick_data_input_inference_from_model_metadata(
    metadata: Mapping[str, Any],
) -> ModelTickDataInputInference:
    input_inference = require_mapping(
        metadata,
        "input_inference",
        "metadata.input_inference",
        calibration_error,
    )

    tick_data_source = require_mapping(
        input_inference,
        "tick_data_source",
        "metadata.input_inference.tick_data_source",
        calibration_error,
    )

    column_mapping = require_mapping(
        input_inference,
        "tick_data_column_mapping",
        "metadata.input_inference.tick_data_column_mapping",
        calibration_error,
    )

    normalization_policy = require_mapping(
        input_inference,
        "normalization_policy",
        "metadata.input_inference.normalization_policy",
        calibration_error,
    )

    return ModelTickDataInputInference(
        file_format=require_non_empty_string(
            tick_data_source,
            "file_format",
            "metadata.input_inference.tick_data_source.file_format",
            calibration_error,
        ),
        timestamp_column=require_non_empty_string(
            column_mapping,
            "timestamp_column",
            "metadata.input_inference.tick_data_column_mapping.timestamp_column",
            calibration_error,
        ),
        timestamp_timezone=require_non_empty_string(
            normalization_policy,
            "timestamp_timezone",
            "metadata.input_inference.normalization_policy.timestamp_timezone",
            calibration_error,
        ),
        timestamp_timezone_source=require_non_empty_string(
            input_inference,
            "timestamp_timezone_source",
            "metadata.input_inference.timestamp_timezone_source",
            calibration_error,
        ),
        tick_price_input_policy=require_optional_string_value(
            normalization_policy.get("tick_price_input_policy"),
            "metadata.input_inference.normalization_policy.tick_price_input_policy",
            calibration_error,
        ),
        price_column=optional_non_empty_string_field(
            column_mapping,
            "price_column",
            field_path_prefix="metadata.input_inference.tick_data_column_mapping",
        ),
        trade_sign_column=optional_non_empty_string_field(
            column_mapping,
            "trade_sign_column",
            field_path_prefix="metadata.input_inference.tick_data_column_mapping",
        ),
        bid_column=optional_non_empty_string_field(
            column_mapping,
            "bid_column",
            field_path_prefix="metadata.input_inference.tick_data_column_mapping",
        ),
        ask_column=optional_non_empty_string_field(
            column_mapping,
            "ask_column",
            field_path_prefix="metadata.input_inference.tick_data_column_mapping",
        ),
    )


def optional_non_empty_string_field(
    mapping: Mapping[str, Any],
    field_name: str,
    *,
    field_path_prefix: str,
) -> Optional[str]:
    field_path = f"{field_path_prefix}.{field_name}"
    value = require_optional_string_value(
        mapping.get(field_name),
        field_path,
        calibration_error,
    )
    if value is None:
        return None

    return require_non_empty_string_value(
        value,
        field_path,
        calibration_error,
    )
