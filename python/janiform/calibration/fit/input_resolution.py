from dataclasses import dataclass
from datetime import UTC, datetime
from pathlib import Path
from typing import Optional, Any

from .._errors import _calibration_error
from ..market_data.contracts import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationInputFileReference,
    CalibrationMarketDataNormalizationPolicy,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from ..market_data.ingestion import _load_source_rows
from ..presets.references import MarketProfileColumnAliasHint, MarketProfileExplicitConfiguration


_GENERIC_COLUMN_ALIASES: dict[str, tuple[str, ...]] = {
    "timestamp": (
        "timestamp",
        "timestamp_utc",
        "event_timestamp_utc",
        "event_timestamp",
        "ts_utc",
        "ts",
        "trade_timestamp",
        "trade_time",
        "exchange_timestamp",
        "datetime",
        "time",
    ),
    "price": (
        "price",
        "midprice",
        "mid_price",
        "midpoint",
        "mid_point",
        "mark_price",
        "mark",
    ),
    "trade_price": (
        "trade_price",
        "tradeprice",
        "last_price",
        "last_trade_price",
        "traded_price",
        "price",
    ),
    "trade_sign": (
        "trade_sign",
        "tradesign",
        "trade_direction",
        "tradedirection",
        "trade_dir",
        "tradedir",
    ),
    "bid": (
        "bid",
        "bid_price",
        "bidprice",
        "best_bid",
    ),
    "ask": (
        "ask",
        "ask_price",
        "askprice",
        "best_ask",
    ),
    "close_price": (
        "close",
        "close_price",
        "closeprice",
        "last_close",
    ),
}
_SUPPORTED_TICK_DATA_EXTENSIONS: dict[str, CalibrationInputFileFormat] = {
    ".csv": CalibrationInputFileFormat.CSV,
    ".parquet": CalibrationInputFileFormat.PARQUET,
    ".pq": CalibrationInputFileFormat.PARQUET,
}


@dataclass(frozen=True, slots=True)
class HighLevelTickDataColumnOverrides:
    timestamp_column: Optional[str] = None
    price_column: Optional[str] = None
    trade_price_column: Optional[str] = None
    trade_sign_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None
    close_price_column: Optional[str] = None

    def __post_init__(self) -> None:
        for field_name, field_value in (
            ("timestamp_column", self.timestamp_column),
            ("price_column", self.price_column),
            ("trade_price_column", self.trade_price_column),
            ("trade_sign_column", self.trade_sign_column),
            ("bid_column", self.bid_column),
            ("ask_column", self.ask_column),
            ("close_price_column", self.close_price_column),
        ):
            if field_value is not None and not field_value.strip():
                raise _calibration_error(field_name, "must be non-empty when provided")


@dataclass(frozen=True, slots=True)
class HighLevelNormalizationPolicyOverrides:
    timestamp_timezone: Optional[str] = None
    sort_records_by_timestamp: Optional[bool] = None
    duplicate_record_policy: Optional[CalibrationDuplicateRecordPolicy] = None
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy] = None
    option_quote_input_policy: Optional[CalibrationOptionQuoteInputPolicy] = None

    def __post_init__(self) -> None:
        if self.timestamp_timezone is not None and not self.timestamp_timezone.strip():
            raise _calibration_error("timestamp_timezone", "must be non-empty when provided")


@dataclass(frozen=True, slots=True)
class HighLevelFitInference:
    run_id: str
    tick_data_source: CalibrationInputFileReference
    tick_data_column_mapping: CalibrationTickDataColumnMapping
    normalization_policy: CalibrationMarketDataNormalizationPolicy
    available_columns: tuple[str, ...]
    timestamp_candidates: tuple[str, ...]
    explicit_price_candidates: tuple[str, ...]
    trade_price_candidates: tuple[str, ...]
    trade_sign_candidates: tuple[str, ...]
    bid_candidates: tuple[str, ...]
    ask_candidates: tuple[str, ...]
    close_price_candidates: tuple[str, ...]
    timestamp_timezone_source: str
    default_calibration_hint_ids: tuple[str, ...]


def infer_high_level_fit_inputs(
    *,
    tick_data_path: Path,
    tick_data_format: Optional[CalibrationInputFileFormat],
    tick_data_provenance_label: str,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    run_id: Optional[str],
    column_overrides: Optional[HighLevelTickDataColumnOverrides],
    normalization_overrides: Optional[HighLevelNormalizationPolicyOverrides],
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> HighLevelFitInference:
    if not tick_data_provenance_label.strip():
        raise _calibration_error(
            "tick_data_provenance_label",
            "must be a non-empty string",
        )

    effective_column_overrides = (
        HighLevelTickDataColumnOverrides()
        if column_overrides is None
        else column_overrides
    )
    effective_normalization_overrides = (
        HighLevelNormalizationPolicyOverrides()
        if normalization_overrides is None
        else normalization_overrides
    )

    resolved_tick_data_format = (
        tick_data_format
        if tick_data_format is not None
        else _infer_tick_data_format_from_path(tick_data_path)
    )

    tick_data_source = CalibrationInputFileReference(
        path=str(tick_data_path),
        file_format=resolved_tick_data_format,
        provenance_label=tick_data_provenance_label,
    )

    available_columns = _load_source_rows(
        tick_data_source,
        field_path="tick_data_source",
    ).available_columns

    alias_map = _alias_map_for_market_profile(market_profile_configuration)

    timestamp_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["timestamp"],
    )
    explicit_price_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["price"],
    )
    trade_price_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["trade_price"],
    )
    trade_sign_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["trade_sign"],
    )
    bid_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["bid"],
    )
    ask_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["ask"],
    )
    close_price_candidates = _ordered_candidates(
        available_columns=available_columns,
        aliases=alias_map["close_price"],
    )

    timestamp_column = (
        effective_column_overrides.timestamp_column
        if effective_column_overrides.timestamp_column is not None
        else _select_required_candidate(
            candidates=timestamp_candidates,
            field_path="column_mapping.tick_data.timestamp_column",
            failure_message=(
                "could not infer a timestamp column. "
                f"Found timestamp candidates: {_format_candidate_list(timestamp_candidates)}. "
                "Please set a timestamp-column override explicitly."
            ),
        )
    )
    trade_sign_column = (
        effective_column_overrides.trade_sign_column
        if effective_column_overrides.trade_sign_column is not None
        else _select_optional_candidate(trade_sign_candidates)
    )

    tick_price_input_policy = _resolve_tick_price_input_policy(
        workflow_scope=workflow_scope,
        explicit_policy=effective_normalization_overrides.tick_price_input_policy,
        explicit_price_column=effective_column_overrides.price_column,
        explicit_trade_price_column=effective_column_overrides.trade_price_column,
        explicit_bid_column=effective_column_overrides.bid_column,
        explicit_ask_column=effective_column_overrides.ask_column,
        explicit_close_price_column=effective_column_overrides.close_price_column,
        trade_sign_column=trade_sign_column,
        explicit_price_candidates=explicit_price_candidates,
        trade_price_candidates=trade_price_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
        close_price_candidates=close_price_candidates,
    )

    tick_data_column_mapping = _build_tick_data_column_mapping(
        timestamp_column=timestamp_column,
        tick_price_input_policy=tick_price_input_policy,
        column_overrides=effective_column_overrides,
        trade_sign_column=trade_sign_column,
        explicit_price_candidates=explicit_price_candidates,
        trade_price_candidates=trade_price_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
        close_price_candidates=close_price_candidates,
    )

    timestamp_timezone, timestamp_timezone_source = _resolve_timestamp_timezone(
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
        duplicate_record_policy=(
            CalibrationDuplicateRecordPolicy.ERROR
            if effective_normalization_overrides.duplicate_record_policy is None
            else effective_normalization_overrides.duplicate_record_policy
        ),
        tick_price_input_policy=tick_price_input_policy,
        option_quote_input_policy=effective_normalization_overrides.option_quote_input_policy,
    )

    return HighLevelFitInference(
        run_id=_resolve_run_id(run_id=run_id, tick_data_path=tick_data_path),
        tick_data_source=tick_data_source,
        tick_data_column_mapping=tick_data_column_mapping,
        normalization_policy=normalization_policy,
        available_columns=available_columns,
        timestamp_candidates=timestamp_candidates,
        explicit_price_candidates=explicit_price_candidates,
        trade_price_candidates=trade_price_candidates,
        trade_sign_candidates=trade_sign_candidates,
        bid_candidates=bid_candidates,
        ask_candidates=ask_candidates,
        close_price_candidates=close_price_candidates,
        timestamp_timezone_source=timestamp_timezone_source,
        default_calibration_hint_ids=(
            ()
            if market_profile_configuration is None
            else market_profile_configuration.default_calibration_hint_ids
        ),
    )


def column_overrides_from_mapping(
    tick_data_column_mapping: Optional[CalibrationTickDataColumnMapping],
) -> HighLevelTickDataColumnOverrides:
    if tick_data_column_mapping is None:
        return HighLevelTickDataColumnOverrides()

    return HighLevelTickDataColumnOverrides(
        timestamp_column=tick_data_column_mapping.timestamp_column,
        price_column=tick_data_column_mapping.price_column,
        trade_price_column=tick_data_column_mapping.trade_price_column,
        trade_sign_column=tick_data_column_mapping.trade_sign_column,
        bid_column=tick_data_column_mapping.bid_column,
        ask_column=tick_data_column_mapping.ask_column,
        close_price_column=tick_data_column_mapping.close_price_column,
    )


def normalization_overrides_from_policy(
    normalization_policy: Optional[CalibrationMarketDataNormalizationPolicy],
) -> HighLevelNormalizationPolicyOverrides:
    if normalization_policy is None:
        return HighLevelNormalizationPolicyOverrides()

    return HighLevelNormalizationPolicyOverrides(
        timestamp_timezone=normalization_policy.timestamp_timezone,
        sort_records_by_timestamp=normalization_policy.sort_records_by_timestamp,
        duplicate_record_policy=normalization_policy.duplicate_record_policy,
        tick_price_input_policy=normalization_policy.tick_price_input_policy,
        option_quote_input_policy=normalization_policy.option_quote_input_policy,
    )


def build_fit_inference_payload(
    *,
    inference: HighLevelFitInference,
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
            "trade_price_column": inference.tick_data_column_mapping.trade_price_column,
            "trade_sign_column": inference.tick_data_column_mapping.trade_sign_column,
            "bid_column": inference.tick_data_column_mapping.bid_column,
            "ask_column": inference.tick_data_column_mapping.ask_column,
            "close_price_column": inference.tick_data_column_mapping.close_price_column,
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
            "explicit_price": list(inference.explicit_price_candidates),
            "trade_price": list(inference.trade_price_candidates),
            "trade_sign": list(inference.trade_sign_candidates),
            "bid": list(inference.bid_candidates),
            "ask": list(inference.ask_candidates),
            "close_price": list(inference.close_price_candidates),
        },
    }


def _infer_tick_data_format_from_path(tick_data_path: Path) -> CalibrationInputFileFormat:
    suffix = tick_data_path.suffix.lower()
    if suffix in _SUPPORTED_TICK_DATA_EXTENSIONS:
        return _SUPPORTED_TICK_DATA_EXTENSIONS[suffix]

    raise _calibration_error(
        "tick_data_format",
        (
            f"could not infer tick-data format from `{tick_data_path.name}`. "
            "Supported extensions are .csv, .parquet, and .pq."
        ),
    )


def _resolve_run_id(
    *,
    run_id: Optional[str],
    tick_data_path: Path,
) -> str:
    if run_id is not None:
        if not run_id.strip():
            raise _calibration_error("run_id", "must be a non-empty string")

        return run_id

    timestamp_text = datetime.now(tz=UTC).strftime("%Y%m%dT%H%M%SZ")
    file_stem = tick_data_path.stem.strip()

    sanitized_stem = "".join(
        character if character.isalnum() else "_"
        for character in file_stem
    ).strip("_")
    if not sanitized_stem:
        sanitized_stem = "janiform_fit"

    return f"{sanitized_stem}_{timestamp_text}"


def _resolve_tick_price_input_policy(
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    explicit_policy: Optional[CalibrationTickPriceInputPolicy],
    explicit_price_column: Optional[str],
    explicit_trade_price_column: Optional[str],
    explicit_bid_column: Optional[str],
    explicit_ask_column: Optional[str],
    explicit_close_price_column: Optional[str],
    trade_sign_column: Optional[str],
    explicit_price_candidates: tuple[str, ...],
    trade_price_candidates: tuple[str, ...],
    bid_candidates: tuple[str, ...],
    ask_candidates: tuple[str, ...],
    close_price_candidates: tuple[str, ...],
) -> Optional[CalibrationTickPriceInputPolicy]:
    if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        return None

    if explicit_policy is not None:
        return explicit_policy

    if explicit_trade_price_column is not None or len(trade_price_candidates) > 0:
        return CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN

    if trade_sign_column is not None and (
        explicit_price_column is not None or len(explicit_price_candidates) > 0
    ):
        return CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN

    if explicit_price_column is not None or len(explicit_price_candidates) > 0:
        return CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN

    has_bid_source = explicit_bid_column is not None or len(bid_candidates) > 0
    has_ask_source = explicit_ask_column is not None or len(ask_candidates) > 0
    if has_bid_source and has_ask_source:
        return CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT

    if explicit_close_price_column is not None or len(close_price_candidates) > 0:
        return CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN

    found_price_candidates = tuple(
        dict.fromkeys(
            [
                *trade_price_candidates,
                *explicit_price_candidates,
                *bid_candidates,
                *ask_candidates,
                *close_price_candidates,
            ]
        )
    )

    raise _calibration_error(
        "normalization_policy.tick_price_input_policy",
        (
            "could not infer a tick-price input policy. "
            f"Found price candidates: {_format_candidate_list(found_price_candidates)}. "
            "Please set a price-column override or tick-price-input-policy explicitly."
        ),
    )


def _build_tick_data_column_mapping(
    *,
    timestamp_column: str,
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy],
    column_overrides: HighLevelTickDataColumnOverrides,
    trade_sign_column: Optional[str],
    explicit_price_candidates: tuple[str, ...],
    trade_price_candidates: tuple[str, ...],
    bid_candidates: tuple[str, ...],
    ask_candidates: tuple[str, ...],
    close_price_candidates: tuple[str, ...],
) -> CalibrationTickDataColumnMapping:
    price_column: Optional[str] = None
    trade_price_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None
    close_price_column: Optional[str] = None

    match tick_price_input_policy:
        case CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN:
            price_column = _resolve_required_price_column(
                explicit_column=column_overrides.price_column,
                candidates=explicit_price_candidates,
                field_path="column_mapping.tick_data.price_column",
            )
        case CalibrationTickPriceInputPolicy.TRADE_PRICE_COLUMN:
            trade_price_column = _resolve_required_price_column(
                explicit_column=column_overrides.trade_price_column,
                candidates=trade_price_candidates
                if len(trade_price_candidates) > 0
                else explicit_price_candidates,
                field_path="column_mapping.tick_data.trade_price_column",
            )
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            bid_column = _resolve_required_price_column(
                explicit_column=column_overrides.bid_column,
                candidates=bid_candidates,
                field_path="column_mapping.tick_data.bid_column",
            )
            ask_column = _resolve_required_price_column(
                explicit_column=column_overrides.ask_column,
                candidates=ask_candidates,
                field_path="column_mapping.tick_data.ask_column",
            )
        case CalibrationTickPriceInputPolicy.CLOSE_PRICE_COLUMN:
            close_price_column = _resolve_required_price_column(
                explicit_column=column_overrides.close_price_column,
                candidates=close_price_candidates,
                field_path="column_mapping.tick_data.close_price_column",
            )
        case None:
            pass

    return CalibrationTickDataColumnMapping(
        timestamp_column=timestamp_column,
        price_column=price_column,
        trade_price_column=trade_price_column,
        trade_sign_column=trade_sign_column,
        bid_column=bid_column,
        ask_column=ask_column,
        close_price_column=close_price_column,
    )


def _resolve_timestamp_timezone(
    *,
    normalization_overrides: HighLevelNormalizationPolicyOverrides,
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> tuple[str, str]:
    if normalization_overrides.timestamp_timezone is not None:
        return normalization_overrides.timestamp_timezone, "explicit_override"

    if (
        market_profile_configuration is not None
        and market_profile_configuration.timestamp_timezone_name is not None
    ):
        return market_profile_configuration.timestamp_timezone_name, "market_profile"

    return "UTC", "default_utc"


def _resolve_required_price_column(
    *,
    explicit_column: Optional[str],
    candidates: tuple[str, ...],
    field_path: str,
) -> str:
    if explicit_column is not None:
        return explicit_column

    selected_column = _select_optional_candidate(candidates)
    if selected_column is not None:
        return selected_column

    raise _calibration_error(
        field_path,
        (
            "could not infer a required price column. "
            f"Found candidates: {_format_candidate_list(candidates)}."
        ),
    )


def _alias_map_for_market_profile(
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> dict[str, tuple[str, ...]]:
    alias_map: dict[str, tuple[str, ...]] = dict(_GENERIC_COLUMN_ALIASES)
    if market_profile_configuration is None:
        return alias_map

    hints_by_name: dict[str, MarketProfileColumnAliasHint] = {
        hint.canonical_name: hint
        for hint in market_profile_configuration.column_alias_hints
    }

    for canonical_name, default_aliases in tuple(alias_map.items()):
        market_hint = hints_by_name.get(canonical_name)
        if market_hint is None:
            continue

        alias_map[canonical_name] = tuple(
            dict.fromkeys([*market_hint.aliases, *default_aliases])
        )

    return alias_map


def _ordered_candidates(
    *,
    available_columns: tuple[str, ...],
    aliases: tuple[str, ...],
) -> tuple[str, ...]:
    normalized_column_map: dict[str, list[str]] = {}
    for column_name in available_columns:
        normalized_name = _normalize_identifier(column_name)
        normalized_column_map.setdefault(normalized_name, []).append(column_name)

    ordered_matches: list[str] = []
    for alias in aliases:
        for matched_column in normalized_column_map.get(_normalize_identifier(alias), []):
            if matched_column not in ordered_matches:
                ordered_matches.append(matched_column)

    return tuple(ordered_matches)


def _select_optional_candidate(candidates: tuple[str, ...]) -> Optional[str]:
    if len(candidates) == 0:
        return None

    return candidates[0]


def _select_required_candidate(
    *,
    candidates: tuple[str, ...],
    field_path: str,
    failure_message: str,
) -> str:
    selected_candidate = _select_optional_candidate(candidates)
    if selected_candidate is not None:
        return selected_candidate

    raise _calibration_error(field_path, failure_message)


def _normalize_identifier(identifier: str) -> str:
    return "".join(
        character
        for character in identifier.lower()
        if character.isalnum()
    )


def _format_candidate_list(candidates: tuple[str, ...]) -> str:
    if len(candidates) == 0:
        return "none"

    return ", ".join(candidates)


__all__ = [
    "HighLevelFitInference",
    "HighLevelNormalizationPolicyOverrides",
    "HighLevelTickDataColumnOverrides",
    "build_fit_inference_payload",
    "column_overrides_from_mapping",
    "infer_high_level_fit_inputs",
    "normalization_overrides_from_policy",
]
