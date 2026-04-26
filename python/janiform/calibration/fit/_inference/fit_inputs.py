from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from ...errors import calibration_error
from ...market_data.contracts import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from ...market_data.translation import (
    infer_market_data_fit_inputs as infer_market_data_fit_inputs_native,
)
from ...presets.references import MarketProfileExplicitConfiguration


@dataclass(frozen=True, slots=True)
class TickDataColumnOverrides:
    timestamp_column: Optional[str] = None
    price_column: Optional[str] = None
    trade_sign_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("timestamp_column", self.timestamp_column),
            ("price_column", self.price_column),
            ("trade_sign_column", self.trade_sign_column),
            ("bid_column", self.bid_column),
            ("ask_column", self.ask_column),
        ):
            if field_value is not None and not field_value.strip():
                raise calibration_error(field_name, "must be non-empty when provided")


@dataclass(frozen=True, slots=True)
class NormalizationPolicyOverrides:
    timestamp_timezone: Optional[str] = None
    sort_records_by_timestamp: Optional[bool] = None
    duplicate_record_policy: Optional[CalibrationDuplicateRecordPolicy] = None
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy] = None
    option_quote_input_policy: Optional[CalibrationOptionQuoteInputPolicy] = None

    def __post_init__(self) -> None:
        if self.timestamp_timezone is not None and not self.timestamp_timezone.strip():
            raise calibration_error("timestamp_timezone", "must be non-empty when provided")


@dataclass(frozen=True, slots=True)
class FitInference:
    run_id: str
    tick_data_source: CalibrationInputFileReference
    tick_data_column_mapping: CalibrationTickDataColumnMapping
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    available_columns: tuple[str, ...]
    timestamp_candidates: tuple[str, ...]
    price_candidates: tuple[str, ...]
    trade_sign_candidates: tuple[str, ...]
    bid_candidates: tuple[str, ...]
    ask_candidates: tuple[str, ...]
    timestamp_timezone_source: str
    default_calibration_hint_ids: tuple[str, ...]


def infer_fit_inputs(
    *,
    tick_data_path: Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    run_id: Optional[str],
    column_overrides: Optional[TickDataColumnOverrides],
    normalization_overrides: Optional[NormalizationPolicyOverrides],
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> FitInference:
    if not tick_data_provenance_label.strip():
        raise calibration_error(
            "tick_data_provenance_label",
            "must be a non-empty string",
        )

    raw_inference = infer_market_data_fit_inputs_native(
        {
            "tick_data_path": str(tick_data_path),
            "tick_data_format": (
                None
                if tick_data_format is None
                else tick_data_format.value
            ),
            "tick_data_provenance_label": tick_data_provenance_label,
            "workflow_scope": workflow_scope.value,
            "run_id": run_id,
            "column_overrides": _column_overrides_payload(column_overrides),
            "normalization_overrides": _normalization_overrides_payload(
                normalization_overrides
            ),
            "market_profile_defaults": _market_profile_defaults_payload(
                market_profile_configuration
            ),
        }
    )

    tick_data_source = CalibrationInputFileReference.from_mapping(
        _require_mapping(raw_inference, "tick_data_source"),
        field_path="tick_data_source",
    )
    tick_data_column_mapping = CalibrationTickDataColumnMapping.from_mapping(
        _require_mapping(raw_inference, "tick_data_column_mapping"),
        field_path="tick_data_column_mapping",
    )

    normalization_policy = CalibrationMarketDataNormalizationPolicy.from_mapping(
        _require_mapping(raw_inference, "normalization_policy"),
        field_path="normalization_policy",
    )

    source_probe = _require_mapping(raw_inference, "source_probe")
    candidate_columns = _require_mapping(raw_inference, "candidate_columns")

    raw_available_columns = source_probe.get("available_columns", ())
    if not isinstance(raw_available_columns, list | tuple):
        raise calibration_error(
            "source_probe.available_columns",
            "must be a sequence in the native fit-input inference payload",
        )

    raw_default_calibration_hint_ids = raw_inference.get(
        "default_calibration_hint_ids",
        (),
    )
    if not isinstance(raw_default_calibration_hint_ids, list | tuple):
        raise calibration_error(
            "default_calibration_hint_ids",
            "must be a sequence in the native fit-input inference payload",
        )

    return FitInference(
        run_id=str(raw_inference["run_id"]),
        tick_data_source=tick_data_source,
        tick_data_column_mapping=tick_data_column_mapping,
        normalization_policy=normalization_policy,
        available_columns=tuple(str(column_name) for column_name in raw_available_columns),
        timestamp_candidates=_candidate_columns(candidate_columns, "timestamp"),
        price_candidates=_candidate_columns(candidate_columns, "price"),
        trade_sign_candidates=_candidate_columns(candidate_columns, "trade_sign"),
        bid_candidates=_candidate_columns(candidate_columns, "bid"),
        ask_candidates=_candidate_columns(candidate_columns, "ask"),
        timestamp_timezone_source=str(raw_inference["timestamp_timezone_source"]),
        default_calibration_hint_ids=tuple(
            str(hint_id) for hint_id in raw_default_calibration_hint_ids
        ),
    )


def _column_overrides_payload(
    column_overrides: Optional[TickDataColumnOverrides],
) -> dict[str, Optional[str]]:
    effective_column_overrides = (
        TickDataColumnOverrides() if column_overrides is None else column_overrides
    )

    return {
        "timestamp_column": effective_column_overrides.timestamp_column,
        "price_column": effective_column_overrides.price_column,
        "trade_sign_column": effective_column_overrides.trade_sign_column,
        "bid_column": effective_column_overrides.bid_column,
        "ask_column": effective_column_overrides.ask_column,
    }


def _normalization_overrides_payload(
    normalization_overrides: Optional[NormalizationPolicyOverrides],
) -> dict[str, Any]:
    effective_normalization_overrides = (
        NormalizationPolicyOverrides()
        if normalization_overrides is None
        else normalization_overrides
    )

    return {
        "timestamp_timezone": effective_normalization_overrides.timestamp_timezone,
        "sort_records_by_timestamp": effective_normalization_overrides.sort_records_by_timestamp,
        "duplicate_record_policy": (
            None
            if effective_normalization_overrides.duplicate_record_policy is None
            else effective_normalization_overrides.duplicate_record_policy.value
        ),
        "tick_price_input_policy": (
            None
            if effective_normalization_overrides.tick_price_input_policy is None
            else effective_normalization_overrides.tick_price_input_policy.value
        ),
        "option_quote_input_policy": (
            None
            if effective_normalization_overrides.option_quote_input_policy is None
            else effective_normalization_overrides.option_quote_input_policy.value
        ),
    }


def _market_profile_defaults_payload(
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> Optional[dict[str, Any]]:
    if market_profile_configuration is None:
        return None

    return {
        "timestamp_timezone_name": market_profile_configuration.timestamp_timezone_name,
        "column_alias_hints": [
            {
                "canonical_name": alias_hint.canonical_name,
                "aliases": list(alias_hint.aliases),
            }
            for alias_hint in market_profile_configuration.column_alias_hints
        ],
        "default_calibration_hint_ids": list(
            market_profile_configuration.default_calibration_hint_ids
        ),
    }


def _require_mapping(raw_payload: dict[str, Any], field_name: str) -> dict[str, Any]:
    raw_value = raw_payload.get(field_name)
    if not isinstance(raw_value, dict):
        raise calibration_error(
            field_name,
            "must be an object in the native fit-input inference payload",
        )

    return raw_value


def _candidate_columns(
    raw_payload: dict[str, Any],
    field_name: str,
) -> tuple[str, ...]:
    raw_value = raw_payload.get(field_name, ())
    if not isinstance(raw_value, list | tuple):
        raise calibration_error(
            f"candidate_columns.{field_name}",
            "must be a sequence in the native fit-input inference payload",
        )

    return tuple(str(column_name) for column_name in raw_value)


def column_overrides_from_mapping(
    tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping],
) -> TickDataColumnOverrides:
    if tick_data_column_mapping is None:
        return TickDataColumnOverrides()

    return TickDataColumnOverrides(
        timestamp_column=tick_data_column_mapping.timestamp_column,
        price_column=tick_data_column_mapping.price_column,
        trade_sign_column=tick_data_column_mapping.trade_sign_column,
        bid_column=tick_data_column_mapping.bid_column,
        ask_column=tick_data_column_mapping.ask_column,
    )


def normalization_overrides_from_policy(
    normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy],
) -> NormalizationPolicyOverrides:
    if normalization_policy is None:
        return NormalizationPolicyOverrides()

    return NormalizationPolicyOverrides(
        timestamp_timezone=normalization_policy.timestamp_timezone,
        sort_records_by_timestamp=normalization_policy.sort_records_by_timestamp,
        duplicate_record_policy=normalization_policy.duplicate_record_policy,
        tick_price_input_policy=normalization_policy.tick_price_input_policy,
        option_quote_input_policy=normalization_policy.option_quote_input_policy,
    )


def build_fit_inference_payload(
    *,
    inference: FitInference,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    market_profile_name: Optional[str],
    known_news_preset_name: Optional[str],
    known_news_recommended_name: Optional[str],
    known_news_recommendation_applied: bool,
    session_source_kind: str,
    known_news_source_kind: str,
) -> dict[str, Any]:
    return {
        "run_id": inference.run_id,
        "workflow_scope": workflow_scope.value,
        "tick_data_format": inference.tick_data_source.file_format.value,
        "available_columns": list(inference.available_columns),
        "timestamp_column": inference.tick_data_column_mapping.timestamp_column,
        "tick_price_input_policy": (
            None
            if inference.normalization_policy.tick_price_input_policy is None
            else inference.normalization_policy.tick_price_input_policy.value
        ),
        "tick_price_columns": {
            "price_column": inference.tick_data_column_mapping.price_column,
            "trade_sign_column": inference.tick_data_column_mapping.trade_sign_column,
            "bid_column": inference.tick_data_column_mapping.bid_column,
            "ask_column": inference.tick_data_column_mapping.ask_column,
        },
        "timestamp_timezone": inference.normalization_policy.timestamp_timezone,
        "timestamp_timezone_source": inference.timestamp_timezone_source,
        "market_profile": market_profile_name,
        "default_calibration_hint_ids": list(inference.default_calibration_hint_ids),
        "session_source_kind": session_source_kind,
        "known_news": {
            "selected_preset": known_news_preset_name,
            "recommended_preset": known_news_recommended_name,
            "recommendation_applied": known_news_recommendation_applied,
            "source_kind": known_news_source_kind,
        },
        "candidate_columns": {
            "timestamp": list(inference.timestamp_candidates),
            "price": list(inference.price_candidates),
            "trade_sign": list(inference.trade_sign_candidates),
            "bid": list(inference.bid_candidates),
            "ask": list(inference.ask_candidates),
        },
    }


__all__ = [
    "FitInference",
    "NormalizationPolicyOverrides",
    "TickDataColumnOverrides",
    "build_fit_inference_payload",
    "column_overrides_from_mapping",
    "infer_fit_inputs",
    "normalization_overrides_from_policy",
]
