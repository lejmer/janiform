from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Optional, Protocol

from ...errors import calibration_error
from ...market_data.contracts import (
    CalibrationDuplicateRecordPolicy,
    CalibrationInputFileFormat,
    CalibrationMarketDataWorkflowScope,
    CalibrationOptionQuoteInputPolicy,
    CalibrationTickDataColumnMapping,
    CalibrationTickPriceInputPolicy,
)
from ...presets.references import MarketProfileColumnAliasHint, MarketProfileExplicitConfiguration
from .aliases import GENERIC_COLUMN_ALIASES


SUPPORTED_TICK_DATA_EXTENSIONS: dict[str, CalibrationInputFileFormat] = {
    ".csv": CalibrationInputFileFormat.CSV,
    ".parquet": CalibrationInputFileFormat.PARQUET,
    ".pq": CalibrationInputFileFormat.PARQUET,
}


class TickDataColumnOverridesContract(Protocol):
    timestamp_column: Optional[str]
    price_column: Optional[str]
    trade_sign_column: Optional[str]
    bid_column: Optional[str]
    ask_column: Optional[str]


class NormalizationPolicyOverridesContract(Protocol):
    timestamp_timezone: Optional[str]
    sort_records_by_timestamp: Optional[bool]
    duplicate_record_policy: Optional[CalibrationDuplicateRecordPolicy]
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy]
    option_quote_input_policy: Optional[CalibrationOptionQuoteInputPolicy]


def infer_tick_data_format_from_path(tick_data_path: Path) -> CalibrationInputFileFormat:
    suffix = tick_data_path.suffix.lower()
    if suffix in SUPPORTED_TICK_DATA_EXTENSIONS:
        return SUPPORTED_TICK_DATA_EXTENSIONS[suffix]

    raise calibration_error(
        "tick_data_format",
        (
            f"could not infer tick-data format from `{tick_data_path.name}`. "
            "Supported extensions are .csv, .parquet, and .pq."
        ),
    )


def resolve_run_id(
    *,
    run_id: Optional[str],
    tick_data_path: Path,
) -> str:
    if run_id is not None:
        if not run_id.strip():
            raise calibration_error("run_id", "must be a non-empty string")

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


def resolve_tick_price_input_policy(
    *,
    workflow_scope: CalibrationMarketDataWorkflowScope,
    explicit_policy: Optional[CalibrationTickPriceInputPolicy],
    explicit_price_column: Optional[str],
    explicit_bid_column: Optional[str],
    explicit_ask_column: Optional[str],
    price_candidates: tuple[str, ...],
    bid_candidates: tuple[str, ...],
    ask_candidates: tuple[str, ...],
) -> Optional[CalibrationTickPriceInputPolicy]:
    if workflow_scope == CalibrationMarketDataWorkflowScope.SEASONALITY_ONLY:
        return None

    if explicit_policy is not None:
        return explicit_policy

    if explicit_price_column is not None or len(price_candidates) > 0:
        return CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN

    has_bid_source = explicit_bid_column is not None or len(bid_candidates) > 0
    has_ask_source = explicit_ask_column is not None or len(ask_candidates) > 0
    if has_bid_source and has_ask_source:
        return CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT

    found_price_candidates = tuple(
        dict.fromkeys([*price_candidates, *bid_candidates, *ask_candidates])
    )

    raise calibration_error(
        "normalization_policy.tick_price_input_policy",
        (
            "could not infer a tick-price input policy. "
            f"Found price candidates: {format_candidate_list(found_price_candidates)}. "
            "Please set a price-column override or tick-price-input-policy explicitly."
        ),
    )


def build_tick_data_column_mapping(
    *,
    timestamp_column: str,
    tick_price_input_policy: Optional[CalibrationTickPriceInputPolicy],
    column_overrides: TickDataColumnOverridesContract,
    trade_sign_column: Optional[str],
    price_candidates: tuple[str, ...],
    bid_candidates: tuple[str, ...],
    ask_candidates: tuple[str, ...],
) -> CalibrationTickDataColumnMapping:
    price_column: Optional[str] = None
    bid_column: Optional[str] = None
    ask_column: Optional[str] = None

    match tick_price_input_policy:
        case CalibrationTickPriceInputPolicy.EXPLICIT_PRICE_COLUMN:
            price_column = resolve_required_price_column(
                explicit_column=column_overrides.price_column,
                candidates=price_candidates,
                field_path="column_mapping.tick_data.price_column",
            )
        case CalibrationTickPriceInputPolicy.BID_ASK_MIDPOINT:
            bid_column = resolve_required_price_column(
                explicit_column=column_overrides.bid_column,
                candidates=bid_candidates,
                field_path="column_mapping.tick_data.bid_column",
            )
            ask_column = resolve_required_price_column(
                explicit_column=column_overrides.ask_column,
                candidates=ask_candidates,
                field_path="column_mapping.tick_data.ask_column",
            )
        case None:
            pass

    return CalibrationTickDataColumnMapping(
        timestamp_column=timestamp_column,
        price_column=price_column,
        trade_sign_column=trade_sign_column,
        bid_column=bid_column,
        ask_column=ask_column,
    )


def resolve_timestamp_timezone(
    *,
    normalization_overrides: NormalizationPolicyOverridesContract,
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


def resolve_required_price_column(
    *,
    explicit_column: Optional[str],
    candidates: tuple[str, ...],
    field_path: str,
) -> str:
    if explicit_column is not None:
        return explicit_column

    selected_column = select_optional_candidate(candidates)
    if selected_column is not None:
        return selected_column

    raise calibration_error(
        field_path,
        (
            "could not infer a required price column. "
            f"Found candidates: {format_candidate_list(candidates)}."
        ),
    )


def alias_map_for_market_profile(
    market_profile_configuration: Optional[MarketProfileExplicitConfiguration],
) -> dict[str, tuple[str, ...]]:
    alias_map: dict[str, tuple[str, ...]] = dict(GENERIC_COLUMN_ALIASES)
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


def ordered_candidates(
    *,
    available_columns: tuple[str, ...],
    aliases: tuple[str, ...],
) -> tuple[str, ...]:
    normalized_column_map: dict[str, list[str]] = {}
    for column_name in available_columns:
        normalized_name = normalize_identifier(column_name)
        normalized_column_map.setdefault(normalized_name, []).append(column_name)

    ordered_matches: list[str] = []
    for alias in aliases:
        for matched_column in normalized_column_map.get(normalize_identifier(alias), []):
            if matched_column not in ordered_matches:
                ordered_matches.append(matched_column)

    return tuple(ordered_matches)


def select_optional_candidate(candidates: tuple[str, ...]) -> Optional[str]:
    if len(candidates) == 0:
        return None

    return candidates[0]


def select_required_candidate(
    *,
    candidates: tuple[str, ...],
    field_path: str,
    failure_message: str,
) -> str:
    selected_candidate = select_optional_candidate(candidates)
    if selected_candidate is not None:
        return selected_candidate

    raise calibration_error(field_path, failure_message)


def normalize_identifier(identifier: str) -> str:
    return "".join(
        character
        for character in identifier.lower()
        if character.isalnum()
    )


def format_candidate_list(candidates: tuple[str, ...]) -> str:
    if len(candidates) == 0:
        return "none"

    return ", ".join(candidates)
