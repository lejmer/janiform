import argparse
import json
import sys
from dataclasses import dataclass
from datetime import timedelta
from pathlib import Path
from typing import Any, Final

import pandas as pd

REPOSITORY_ROOT: Final[Path] = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from examples._shared import (
    EXAMPLE_OPTION_SURFACE_ARTIFACT_ROOT,
    repository_relative_path_string,
    write_json_document,
)
from examples._synthetic_option_surface import (
    DEFAULT_CALIBRATION_ROW_COUNT,
    DEFAULT_CARRY_RATE,
    DEFAULT_DISCOUNT_RATE,
    DEFAULT_MATURITY_INTERVALS_HOURS,
    DEFAULT_OBSERVED_STATE_ROW_COUNT,
    DEFAULT_STRIKE_OFFSETS_IN_PIPS,
    SyntheticVanillaOptionSurfaceContext,
    load_synthetic_eurusd_surface_context,
)
from janiform.calibration import JaniformVanillaOptionSurfacePricingResult
from janiform.pricing import VanillaOptionPayoffType


DEFAULT_RELATIVE_BID_ASK_WIDTH: Final[float] = 0.10
DEFAULT_OUTPUT_PATH: Final[Path] = (
    EXAMPLE_OPTION_SURFACE_ARTIFACT_ROOT / "synthetic_eurusd_option_snapshot.parquet"
)


@dataclass(frozen=True, slots=True)
class SyntheticBidAskSpreadPolicy:
    relative_bid_ask_width: float

    def __post_init__(self) -> None:
        if self.relative_bid_ask_width <= 0.0:
            raise ValueError("relative_bid_ask_width must be strictly positive")

        if self.relative_bid_ask_width >= 2.0:
            raise ValueError(
                "relative_bid_ask_width must be below 2.0 so the proportional bid remains non-negative"
            )

    def quote_from_mid(self, *, mid_price: float) -> tuple[float, float]:
        if mid_price <= 0.0:
            raise ValueError(
                "mid_price must be strictly positive to build a proportional synthetic bid/ask quote"
            )

        bid_ask_width = mid_price * self.relative_bid_ask_width
        half_width = 0.5 * bid_ask_width

        return mid_price - half_width, mid_price + half_width


def _build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Generate a synthetic listed-option snapshot from the synthetic EUR/USD tick "
            "dataset so it can be used in a joint calibration example."
        )
    )

    parser.add_argument(
        "--output-path",
        type=Path,
        default=DEFAULT_OUTPUT_PATH,
        help=(
            "Where to write the generated snapshot. Supported suffixes: .parquet and .csv. "
            f"Default: {DEFAULT_OUTPUT_PATH.relative_to(REPOSITORY_ROOT)}"
        ),
    )
    parser.add_argument(
        "--metadata-path",
        type=Path,
        default=None,
        help="Optional metadata sidecar path. Defaults to <output-path>.metadata.json.",
    )
    parser.add_argument(
        "--refit-teacher",
        action="store_true",
        help="Refit the cached teacher model instead of reusing the persisted example model.",
    )
    parser.add_argument(
        "--valuation-row-offset-from-end",
        type=int,
        default=0,
        help=(
            "How many rows back from the end of the synthetic tick file to place the option "
            "snapshot. 0 means the latest tick."
        ),
    )
    parser.add_argument(
        "--observed-state-row-count",
        type=int,
        default=DEFAULT_OBSERVED_STATE_ROW_COUNT,
        help="Number of recent underlying rows to use for current-state estimation.",
    )
    parser.add_argument(
        "--calibration-row-count",
        type=int,
        default=DEFAULT_CALIBRATION_ROW_COUNT,
        help="Number of trailing synthetic tick rows to use for the teacher-model fit.",
    )
    parser.add_argument(
        "--relative-bid-ask-width",
        type=float,
        default=DEFAULT_RELATIVE_BID_ASK_WIDTH,
        help=(
            "Quoted width as a fraction of the model mid price. "
            "For example, 0.10 means ask - bid = 10%% of mid."
        ),
    )
    parser.add_argument(
        "--payoff-type",
        choices=("call", "put", "both"),
        default="both",
        help="Which payoff types to include in the generated snapshot.",
    )

    return parser


def _payoff_types_from_argument(argument_value: str) -> tuple[VanillaOptionPayoffType, ...]:
    match argument_value:
        case "call":
            return (VanillaOptionPayoffType.CALL,)
        case "put":
            return (VanillaOptionPayoffType.PUT,)
        case "both":
            return (VanillaOptionPayoffType.CALL, VanillaOptionPayoffType.PUT)
        case _:
            raise ValueError(f"unsupported payoff type selection: {argument_value}")


def _snapshot_frame_from_context(
    *,
    context: SyntheticVanillaOptionSurfaceContext,
    spread_policy: SyntheticBidAskSpreadPolicy,
    payoff_types: tuple[VanillaOptionPayoffType, ...],
) -> pd.DataFrame:
    rows: list[dict[str, Any]] = []
    model = context.persisted_model.model

    for payoff_type in payoff_types:
        pricing_result: JaniformVanillaOptionSurfacePricingResult = model.price_option_surface(
            valuation_time_utc=context.valuation_time_utc,
            surface=context.build_surface(payoff_type=payoff_type),
            observed_data=context.observed_data,
            silent=True,
        )

        rows.extend(
            _quote_rows_from_pricing_result(
                context=context,
                pricing_result=pricing_result,
                spread_policy=spread_policy,
            )
        )

    return pd.DataFrame.from_records(rows)


def _quote_rows_from_pricing_result(
    *,
    context: SyntheticVanillaOptionSurfaceContext,
    pricing_result: JaniformVanillaOptionSurfacePricingResult,
    spread_policy: SyntheticBidAskSpreadPolicy,
) -> list[dict[str, object]]:
    rows: list[dict[str, Any]] = []

    for maturity_index, maturity_interval_hours in enumerate(pricing_result.maturity_intervals):
        expiry_time_utc = context.valuation_time_utc + timedelta(hours=maturity_interval_hours)
        for strike_index, strike in enumerate(pricing_result.strikes):
            mid_price = pricing_result.price_grid[maturity_index][strike_index]
            bid_price, ask_price = spread_policy.quote_from_mid(mid_price=mid_price)

            rows.append(
                {
                    "quote_timestamp_utc": context.valuation_time_utc.isoformat(),
                    "expiry_utc": expiry_time_utc.isoformat(),
                    "strike": strike,
                    "option_type": pricing_result.payoff_type.value,
                    "mid_price": mid_price,
                    "bid": bid_price,
                    "ask": ask_price,
                }
            )

    return rows


def _write_snapshot_frame(
    snapshot_frame: pd.DataFrame,
    *,
    output_path: Path,
) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)

    match output_path.suffix.lower():
        case ".parquet":
            snapshot_frame.to_parquet(output_path, index=False)
        case ".csv":
            snapshot_frame.to_csv(output_path, index=False)
        case _:
            raise ValueError(
                f"unsupported output path suffix for synthetic option snapshot: {output_path.suffix}"
            )


def _metadata_document(
    *,
    context: SyntheticVanillaOptionSurfaceContext,
    output_path: Path,
    spread_policy: SyntheticBidAskSpreadPolicy,
    payoff_types: tuple[VanillaOptionPayoffType, ...],
) -> dict[str, Any]:
    return {
        "generator": "scripts/generate_synthetic_option_snapshot.py",
        "tick_data_path": "data/Synthetic_EURUSD_Data_1M_ticks.parquet",
        "output_path": repository_relative_path_string(output_path),
        "market_profile": "eurusd_spot",
        "teacher_model_run_id": context.persisted_model.model.run_id,
        "teacher_model_path": repository_relative_path_string(
            context.persisted_model.model_path
        ),
        "teacher_model_loaded_from_cache": context.persisted_model.loaded_from_cache,
        "valuation_time_utc": context.valuation_time_utc.isoformat(),
        "spot": context.spot,
        "calibration_row_count": context.calibration_row_count,
        "observed_state_row_count": context.observed_state_row_count,
        "valuation_row_offset_from_end": context.valuation_row_offset_from_end,
        "payoff_types": [payoff_type.value for payoff_type in payoff_types],
        "strike_offsets_in_pips": list(DEFAULT_STRIKE_OFFSETS_IN_PIPS),
        "maturity_intervals_hours": list(context.maturity_intervals_hours),
        "discount_rate": context.discount_rate,
        "carry_rate": context.carry_rate,
        "quoted_spread_policy": {
            "type": "proportional_mid_width",
            "relative_bid_ask_width": spread_policy.relative_bid_ask_width,
        },
        "column_mapping": {
            "quote_timestamp_column": "quote_timestamp_utc",
            "expiry_timestamp_column": "expiry_utc",
            "strike_column": "strike",
            "option_type_column": "option_type",
            "mid_price_column": "mid_price",
            "bid_column": "bid",
            "ask_column": "ask",
        },
    }


def main() -> int:
    parser = _build_argument_parser()
    arguments = parser.parse_args()

    spread_policy = SyntheticBidAskSpreadPolicy(
        relative_bid_ask_width=arguments.relative_bid_ask_width
    )

    payoff_types = _payoff_types_from_argument(arguments.payoff_type)

    output_path = arguments.output_path
    metadata_path = (
        output_path.with_suffix(output_path.suffix + ".metadata.json")
        if arguments.metadata_path is None
        else arguments.metadata_path
    )

    print("Loading surface context...")

    context = load_synthetic_eurusd_surface_context(
        force_refit=arguments.refit_teacher,
        valuation_row_offset_from_end=arguments.valuation_row_offset_from_end,
        calibration_row_count=arguments.calibration_row_count,
        observed_state_row_count=arguments.observed_state_row_count,
        maturity_intervals_hours=DEFAULT_MATURITY_INTERVALS_HOURS,
        discount_rate=DEFAULT_DISCOUNT_RATE,
        carry_rate=DEFAULT_CARRY_RATE,
    )

    print("Fetching snapshot frame from context...")

    snapshot_frame = _snapshot_frame_from_context(
        context=context,
        spread_policy=spread_policy,
        payoff_types=payoff_types,
    )

    print(f"Writing snapshot frame to disk: {output_path}...")

    _write_snapshot_frame(snapshot_frame, output_path=output_path)

    print(f"Writing metadata to disk: {metadata_path}...")

    write_json_document(
        metadata_path,
        _metadata_document(
            context=context,
            output_path=output_path,
            spread_policy=spread_policy,
            payoff_types=payoff_types,
        ),
    )

    print(json.dumps({"snapshot_path": str(output_path.resolve()), "rows": len(snapshot_frame)}))
    print(
        json.dumps(
            {
                "metadata_path": str(metadata_path.resolve()),
                "valuation_time_utc": context.valuation_time_utc.isoformat(),
                "teacher_model_run_id": context.persisted_model.model.run_id,
            }
        )
    )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
