import argparse
from datetime import UTC, datetime
from decimal import Decimal
from typing import Final

import pandas as pd

from examples._shared import (
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    load_or_fit_persisted_example_model,
)
from janiform.calibration import (
    CurrentStateEstimationRequest,
    JaniformModel,
    JaniformVanillaOptionSurface,
)
from janiform.calibration.market_data import CalibrationTickDataColumnMapping
from janiform.errors import JaniformExecutionError
from janiform.pricing import VanillaOptionPayoffType


PRICE_COLUMN: Final[str] = "close"
INCREMENT_COLUMN: Final[str] = "increments"
OBSERVED_STATE_ROW_COUNT: Final[int] = 256
PIP_SIZE: Final[float] = 0.0001
STRIKE_OFFSETS_IN_PIPS: Final[tuple[int, ...]] = (-50, -25, -10, 0, 10, 25, 50)
MATURITY_INTERVALS: Final[tuple[float, ...]] = (1.0, 2.0, 4.0, 8.0)
RISK_FREE_RATE: Final[float] = 0.02
DIVIDEND_YIELD: Final[float] = 0.01
CALIBRATION_ROW_COUNT: Final[int] = 5_000


def _latest_timestamp_as_utc(underlying_frame: pd.DataFrame) -> datetime:
    latest_timestamp = underlying_frame.index[-1]
    if not isinstance(latest_timestamp, pd.Timestamp):
        raise TypeError("expected the underlying parquet index to contain pandas Timestamp values")

    if latest_timestamp.tzinfo is None:
        return latest_timestamp.tz_localize(UTC).to_pydatetime()

    return latest_timestamp.tz_convert(UTC).to_pydatetime()


def _latest_spot(underlying_frame: pd.DataFrame) -> float:
    latest_price = underlying_frame[PRICE_COLUMN].iloc[-1]
    if isinstance(latest_price, Decimal):
        return float(latest_price)

    return float(latest_price)


def _build_observed_state_request(
    underlying_frame: pd.DataFrame,
    *,
    observed_price: float,
) -> CurrentStateEstimationRequest:
    observed_frame = underlying_frame.tail(OBSERVED_STATE_ROW_COUNT).reset_index(names="timestamp_utc").copy()
    observed_frame["timestamp_utc"] = pd.to_datetime(observed_frame["timestamp_utc"], utc=True)
    observed_frame["trade_price"] = observed_frame[PRICE_COLUMN].astype(float)
    observed_frame = observed_frame[observed_frame[INCREMENT_COLUMN].ne(0.0)].copy()
    observed_frame["trade_sign"] = observed_frame[INCREMENT_COLUMN].map(
        lambda increment: "buy" if increment > 0.0 else "sell"
    )

    return CurrentStateEstimationRequest(
        observed_tick_data=observed_frame[["timestamp_utc", "trade_price", "trade_sign"]],
        tick_data_column_mapping=CalibrationTickDataColumnMapping(
            timestamp_column="timestamp_utc",
            price_column="trade_price",
            trade_sign_column="trade_sign",
        ),
        observed_price=observed_price,
    )


def _format_float_axis(values: tuple[float, ...]) -> list[str]:
    return [f"{value:.5f}" if value < 10.0 else f"{value:.2f}" for value in values]


def _fx_strikes_from_spot(spot: float) -> tuple[float, ...]:
    return tuple(spot + PIP_SIZE * pip_offset for pip_offset in STRIKE_OFFSETS_IN_PIPS)


def _build_surface(
    *,
    spot: float,
    strikes: tuple[float, ...],
) -> JaniformVanillaOptionSurface:
    return JaniformVanillaOptionSurface(
        spot=spot,
        strikes=strikes,
        maturity_intervals=MATURITY_INTERVALS,
        payoff_type=VanillaOptionPayoffType.CALL,
        risk_free_rate=RISK_FREE_RATE,
        dividend_yield=DIVIDEND_YIELD,
    )


def _status_grid_codes(
    status_grid: tuple[tuple[str, ...], ...],
) -> tuple[list[list[int]], list[str]]:
    status_labels = [
        "succeeded",
        "below_lower_bound",
        "at_upper_bound",
        "above_upper_bound",
        "pricing_failed",
    ]
    status_code_by_value = {
        "succeeded": 0,
        "price_below_no_arbitrage_lower_bound": 1,
        "price_equals_finite_upper_bound": 2,
        "price_above_finite_upper_bound": 3,
        "pricing_failed": 4,
    }

    return (
        [
            [status_code_by_value[status] for status in status_row]
            for status_row in status_grid
        ],
        status_labels,
    )


def _short_status_label(status: str) -> str:
    match status:
        case "succeeded":
            return "OK"
        case "price_below_no_arbitrage_lower_bound":
            return "LOW"
        case "price_equals_finite_upper_bound":
            return "CAP"
        case "price_above_finite_upper_bound":
            return "HIGH"
        case "pricing_failed":
            return "ERR"
        case _:
            return "?"


def main() -> None:
    import matplotlib.pyplot as plt
    from matplotlib.colors import ListedColormap

    arguments = _parse_args()
    underlying_frame: pd.DataFrame = pd.read_parquet(SYNTHETIC_EURUSD_TICK_DATA_PATH)
    valuation_time_utc: datetime = _latest_timestamp_as_utc(underlying_frame)
    spot: float = _latest_spot(underlying_frame)
    observed_data = _build_observed_state_request(
        underlying_frame,
        observed_price=spot,
    )

    persisted_model = load_or_fit_persisted_example_model(
        market_profile="eurusd_spot",
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
        tail_row_count=CALIBRATION_ROW_COUNT,
        force_refit=arguments.refit,
    )
    model: JaniformModel = persisted_model.model
    strikes = _fx_strikes_from_spot(spot)
    price_rows: list[list[float | None]] = []
    implied_volatility_rows: list[list[float | None]] = []
    inversion_status_rows: list[list[str]] = []
    pricing_failure_messages: list[str] = []

    surface = _build_surface(
        spot=spot,
        strikes=strikes,
    )

    try:
        pricing_result = model.price_option_surface(
            valuation_time_utc=valuation_time_utc,
            surface=surface,
            observed_data=observed_data,
        )
        price_rows = [list(price_row) for price_row in pricing_result.price_grid]
        implied_volatility_rows = [
            list(implied_volatility_row)
            for implied_volatility_row in pricing_result.implied_volatility_grid
        ]
        inversion_status_rows = [
            [status.value for status in status_row]
            for status_row in pricing_result.implied_volatility_inversion_status_grid
        ]
    except JaniformExecutionError as error:
        for maturity_interval in MATURITY_INTERVALS:
            price_rows.append([None for _strike in strikes])
            implied_volatility_rows.append([None for _strike in strikes])
            inversion_status_rows.append(["pricing_failed" for _strike in strikes])
            pricing_failure_messages.append(
                f"maturity {maturity_interval:.1f}: {error}"
            )

    has_any_implied_volatility_node = any(
        implied_volatility is not None
        for implied_volatility_row in implied_volatility_rows
        for implied_volatility in implied_volatility_row
    )
    has_any_price_node = any(
        price is not None
        for price_row in price_rows
        for price in price_row
    )
    if not has_any_price_node:
        raise RuntimeError(
            "the synthetic EUR/USD example could not produce any pricing nodes"
        )

    if not has_any_implied_volatility_node:
        print("pricing notes:")
        print("  no nodes produced a successful implied-volatility inversion on this sample")

    implied_volatility_frame = pd.DataFrame(
        implied_volatility_rows,
        index=MATURITY_INTERVALS,
        columns=strikes,
    )
    price_frame = pd.DataFrame(
        price_rows,
        index=MATURITY_INTERVALS,
        columns=strikes,
    )
    inversion_status_frame = pd.DataFrame(
        inversion_status_rows,
        index=MATURITY_INTERVALS,
        columns=strikes,
    )

    if all(
        implied_volatility is None
        for implied_volatility_row in implied_volatility_rows
        for implied_volatility in implied_volatility_row
    ):
        print("option price surface:")
        print(price_frame.to_string(float_format=lambda value: f"{value:.6f}"))

    print(
        f"model cache: {persisted_model.model_path} "
        f"({'loaded' if persisted_model.loaded_from_cache else 'fit'})"
    )
    print(f"fitted run id: {model.run_id}")
    print(f"valuation time utc: {valuation_time_utc.isoformat()}")
    print(f"spot: {spot:.5f}")
    print(f"calibration rows used: {min(len(underlying_frame), CALIBRATION_ROW_COUNT)}")
    print(f"observed state rows used: {OBSERVED_STATE_ROW_COUNT}")
    print(
        "flat assumptions:",
        {
            "risk_free_rate": RISK_FREE_RATE,
            "dividend_yield": DIVIDEND_YIELD,
            "payoff_type": VanillaOptionPayoffType.CALL.value,
        },
    )
    print("strike offsets in pips:", STRIKE_OFFSETS_IN_PIPS)

    if pricing_failure_messages:
        print("pricing notes:")
        for message in pricing_failure_messages:
            print(f"  {message}")

    if has_any_implied_volatility_node:
        print("implied volatility surface:")
        print(implied_volatility_frame.to_string(float_format=lambda value: f"{value:.6f}"))

    print("implied volatility inversion status:")
    print(inversion_status_frame.to_string())

    figure, axes = plt.subplots(1, 2, figsize=(14, 5), constrained_layout=True)

    left_panel_frame = implied_volatility_frame if has_any_implied_volatility_node else price_frame
    left_panel_image = axes[0].imshow(
        left_panel_frame.to_numpy(dtype=float),
        origin="lower",
        aspect="auto",
        cmap="viridis",
    )
    axes[0].set_title(
        "synthetic eurusd implied volatility surface"
        if has_any_implied_volatility_node
        else "synthetic eurusd option price surface"
    )
    axes[0].set_xlabel("strike")
    axes[0].set_ylabel("maturity interval")
    axes[0].set_xticks(range(len(strikes)), labels=_format_float_axis(strikes))
    axes[0].set_yticks(
        range(len(MATURITY_INTERVALS)),
        labels=_format_float_axis(MATURITY_INTERVALS),
    )
    figure.colorbar(
        left_panel_image,
        ax=axes[0],
        label=(
            "black-scholes implied volatility"
            if has_any_implied_volatility_node
            else "option price"
        ),
    )

    for maturity_index, maturity_interval in enumerate(MATURITY_INTERVALS):
        for strike_index, strike in enumerate(strikes):
            left_panel_value = left_panel_frame.iloc[maturity_index, strike_index]
            annotation = "NA" if pd.isna(left_panel_value) else f"{left_panel_value:.3f}"
            axes[0].text(
                strike_index,
                maturity_index,
                annotation,
                ha="center",
                va="center",
                color="white",
                fontsize=8,
            )

    status_values = tuple(tuple(status_row) for status_row in inversion_status_rows)
    status_codes, status_labels = _status_grid_codes(status_values)
    status_image = axes[1].imshow(
        status_codes,
        origin="lower",
        aspect="auto",
        cmap=ListedColormap(["#1b9e77", "#d95f02", "#7570b3", "#e7298a", "#666666"]),
        vmin=0,
        vmax=max(len(status_labels) - 1, 0),
    )
    axes[1].set_title("implied volatility inversion status")
    axes[1].set_xlabel("strike")
    axes[1].set_ylabel("maturity interval")
    axes[1].set_xticks(range(len(strikes)), labels=_format_float_axis(strikes))
    axes[1].set_yticks(
        range(len(MATURITY_INTERVALS)),
        labels=_format_float_axis(MATURITY_INTERVALS),
    )

    for maturity_index, status_row in enumerate(status_values):
        for strike_index, status in enumerate(status_row):
            axes[1].text(
                strike_index,
                maturity_index,
                _short_status_label(status),
                ha="center",
                va="center",
                color="white",
                fontsize=8,
            )

    colorbar = figure.colorbar(
        status_image,
        ax=axes[1],
        ticks=list(range(len(status_labels))),
    )
    colorbar.ax.set_yticklabels(status_labels)

    plt.show()


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--refit",
        action="store_true",
        help="Rebuild the persisted example model before running the example.",
    )
    return parser.parse_args()


if __name__ == "__main__":
    main()
