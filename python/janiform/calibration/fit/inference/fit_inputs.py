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
from ...market_data.ingestion import _load_source_columns
from ...presets.references import MarketProfileExplicitConfiguration
from .tick_data import (
    alias_map_for_market_profile,
    build_tick_data_column_mapping,
    format_candidate_list,
    infer_tick_data_format_from_path,
    ordered_candidates,
    resolve_run_id,
    resolve_tick_price_input_policy,
    resolve_timestamp_timezone,
    select_optional_candidate,
    select_required_candidate,
)


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

    effective_column_overrides = (
        TickDataColumnOverrides() if column_overrides is None else column_overrides
    )
    effective_normalization_overrides = (
        NormalizationPolicyOverrides()
        if normalization_overrides is None
        else normalization_overrides
    )

    resolved_tick_data_format = (
        tick_data_format
        if tick_data_format is not None
        else infer_tick_data_format_from_path(tick_data_path)
    )

    tick_data_source = CalibrationInputFileReference(
        path=str(tick_data_path),
        file_format=resolved_tick_data_format,
        provenance_label=tick_data_provenance_label,
    )

    available_columns = _load_source_columns(
        tick_data_source,
        field_path="tick_data_source",
    )

    alias_map = alias_map_for_market_profile(market_profile_configuration)

    timestamp_candidates = ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["timestamp"],
    )
    price_candidates = ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["price"],
    )
    trade_sign_candidates = ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["trade_sign"],
    )
    bid_candidates = ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["bid"],
    )
    ask_candidates = ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["ask"],
    )

    timestamp_column = (
        effective_column_overrides.timestamp_column
        if effective_column_overrides.timestamp_column is not None
        else select_required_candidate(
            candidates=timestamp_candidates,
            field_path="column_mapping.tick_data.timestamp_column",
            failure_message=(
                "could not infer a timestamp column. "
                f"Found timestamp candidates: {format_candidate_list(timestamp_candidates)}. "
                "Please set a timestamp-column override explicitly."
            ),
        )
    )
    trade_sign_column = (
        effective_column_overrides.trade_sign_column
        if effective_column_overrides.trade_sign_column is not None
        else select_optional_candidate(trade_sign_candidates)
    )

    tick_price_input_policy = resolve_tick_price_input_policy(
        workflow_scope=workflow_scope,
        explicit_policy=effective_normalization_overrides.tick_price_input_policy,
        explicit_price_column=effective_column_overrides.price_column,
        explicit_bid_column=effective_column_overrides.bid_column,
        explicit_ask_column=effective_column_overrides.ask_column,
        price_candidates=price_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
    )

    tick_data_column_mapping = build_tick_data_column_mapping(
        timestamp_column=timestamp_column,
        tick_price_input_policy=tick_price_input_policy,
        column_overrides=effective_column_overrides,
        trade_sign_column=trade_sign_column,
        price_candidates=price_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
    )

    timestamp_timezone, timestamp_timezone_source = resolve_timestamp_timezone(
        normalization_overrides=effective_normalization_overrides,
        market_profile_configuration=market_profile_configuration,
    )

    normalization_policy = CalibrationMarketDataNormalizationPolicy(
        timestamp_timezone=timestamp_timezone,
        sort_records_by_timestamp=(
            True
            if effective_normalization_overrides.sort_records_by_timestamp is None
            else effective_normalization_overrides.sort_records_by_timestamp
        ),
        duplicate_record_policy=effective_normalization_overrides.duplicate_record_policy,
        tick_price_input_policy=tick_price_input_policy,
        option_quote_input_policy=effective_normalization_overrides.option_quote_input_policy,
    )

    return FitInference(
        run_id=resolve_run_id(run_id=run_id, tick_data_path=tick_data_path),
        tick_data_source=tick_data_source,
        tick_data_column_mapping=tick_data_column_mapping,
        normalization_policy=normalization_policy,
        available_columns=available_columns,
        timestamp_candidates=timestamp_candidates,
        price_candidates=price_candidates,
        trade_sign_candidates=trade_sign_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
        timestamp_timezone_source=timestamp_timezone_source,
        default_calibration_hint_ids=(
            ()
            if market_profile_configuration is None
            else market_profile_configuration.default_calibration_hint_ids
        ),
    )


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
