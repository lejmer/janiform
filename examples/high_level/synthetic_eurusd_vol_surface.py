import argparse
from pathlib import Path

import pandas as pd

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
from janiform.calibration import JaniformModel
from janiform.errors import JaniformExecutionError
from janiform.pricing import VanillaOptionPayoffType


def _format_float_axis(values: tuple[float, ...]) -> list[str]:
    return [f"{value:.5f}" if value < 10.0 else f"{value:.2f}" for value in values]


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
    surface_context: SyntheticVanillaOptionSurfaceContext = (
        load_synthetic_eurusd_surface_context(
            force_refit=arguments.refit,
            calibration_row_count=DEFAULT_CALIBRATION_ROW_COUNT,
            observed_state_row_count=DEFAULT_OBSERVED_STATE_ROW_COUNT,
            maturity_intervals_hours=DEFAULT_MATURITY_INTERVALS_HOURS,
            discount_rate=DEFAULT_DISCOUNT_RATE,
            carry_rate=DEFAULT_CARRY_RATE,
        )
    )
    valuation_time_utc = surface_context.valuation_time_utc
    spot = surface_context.spot
    observed_data = surface_context.observed_data
    persisted_model = surface_context.persisted_model
    model: JaniformModel = persisted_model.model
    strikes = surface_context.strikes
    price_rows: list[list[float | None]] = []
    implied_volatility_rows: list[list[float | None]] = []
    inversion_status_rows: list[list[str]] = []
    pricing_failure_messages: list[str] = []

    surface = surface_context.build_surface(
        payoff_type=VanillaOptionPayoffType.CALL,
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
        for maturity_interval in surface_context.maturity_intervals_hours:
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
        index=surface_context.maturity_intervals_hours,
        columns=strikes,
    )
    price_frame = pd.DataFrame(
        price_rows,
        index=surface_context.maturity_intervals_hours,
        columns=strikes,
    )
    inversion_status_frame = pd.DataFrame(
        inversion_status_rows,
        index=surface_context.maturity_intervals_hours,
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
    print(f"calibration rows used: {surface_context.calibration_row_count}")
    print(f"observed state rows used: {surface_context.observed_state_row_count}")
    print(
        "flat assumptions:",
        {
            "risk_free_rate": surface_context.discount_rate,
            "dividend_yield": surface_context.carry_rate,
            "payoff_type": VanillaOptionPayoffType.CALL.value,
        },
    )
    print("strike offsets in pips:", DEFAULT_STRIKE_OFFSETS_IN_PIPS)

    if pricing_failure_messages:
        print("pricing notes:")
        for message in pricing_failure_messages:
            print(f"  {message}")

    strike_axis = _format_float_axis(strikes)
    maturity_axis = [
        f"{maturity:.1f}" for maturity in surface_context.maturity_intervals_hours
    ]
    status_codes, status_labels = _status_grid_codes(
        tuple(tuple(status for status in status_row) for status_row in inversion_status_rows)
    )

    figure, axes = plt.subplots(nrows=1, ncols=3, figsize=(15, 4.75), constrained_layout=True)

    implied_volatility_plot = axes[0]
    if has_any_implied_volatility_node:
        implied_volatility_mesh = implied_volatility_plot.imshow(
            implied_volatility_frame.astype(float),
            aspect="auto",
            origin="lower",
            cmap="viridis",
        )
        figure.colorbar(implied_volatility_mesh, ax=implied_volatility_plot, shrink=0.9)
    else:
        implied_volatility_plot.imshow(
            [
                [0.0 for _strike in strikes]
                for _maturity in surface_context.maturity_intervals_hours
            ],
            aspect="auto",
            origin="lower",
            cmap="Greys",
            vmin=0.0,
            vmax=1.0,
        )
        implied_volatility_plot.text(
            0.5,
            0.5,
            "no successful IV inversions",
            ha="center",
            va="center",
            transform=implied_volatility_plot.transAxes,
        )

    implied_volatility_plot.set_title("Implied Volatility")
    implied_volatility_plot.set_xticks(range(len(strikes)), strike_axis, rotation=45, ha="right")
    implied_volatility_plot.set_yticks(
        range(len(surface_context.maturity_intervals_hours)),
        maturity_axis,
    )
    implied_volatility_plot.set_xlabel("Strike")
    implied_volatility_plot.set_ylabel("Maturity (hours)")

    price_plot = axes[1]
    price_mesh = price_plot.imshow(
        price_frame.astype(float),
        aspect="auto",
        origin="lower",
        cmap="magma",
    )
    figure.colorbar(price_mesh, ax=price_plot, shrink=0.9)
    price_plot.set_title("Option Price")
    price_plot.set_xticks(range(len(strikes)), strike_axis, rotation=45, ha="right")
    price_plot.set_yticks(range(len(surface_context.maturity_intervals_hours)), maturity_axis)
    price_plot.set_xlabel("Strike")
    price_plot.set_ylabel("Maturity (hours)")

    status_plot = axes[2]
    status_mesh = status_plot.imshow(
        status_codes,
        aspect="auto",
        origin="lower",
        cmap=ListedColormap(["#2e7d32", "#f9a825", "#ef6c00", "#c62828", "#6a1b9a"]),
        vmin=0,
        vmax=len(status_labels) - 1,
    )
    figure.colorbar(
        status_mesh,
        ax=status_plot,
        shrink=0.9,
        ticks=range(len(status_labels)),
    ).ax.set_yticklabels(status_labels)
    status_plot.set_title("IV Inversion Status")
    status_plot.set_xticks(range(len(strikes)), strike_axis, rotation=45, ha="right")
    status_plot.set_yticks(range(len(surface_context.maturity_intervals_hours)), maturity_axis)
    status_plot.set_xlabel("Strike")
    status_plot.set_ylabel("Maturity (hours)")

    for maturity_index, maturity_interval in enumerate(surface_context.maturity_intervals_hours):
        for strike_index, strike in enumerate(strikes):
            status_plot.text(
                strike_index,
                maturity_index,
                _short_status_label(inversion_status_frame.loc[maturity_interval, strike]),
                ha="center",
                va="center",
                color="white",
                fontsize=8,
            )

    if arguments.output_path is None:
        plt.show()
    else:
        arguments.output_path.parent.mkdir(parents=True, exist_ok=True)
        figure.savefig(arguments.output_path, dpi=180)
        print(f"saved plot to: {arguments.output_path}")
        plt.close(figure)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Price and plot a synthetic EUR/USD option surface from a fitted Janiform model.",
    )
    parser.add_argument(
        "--output-path",
        type=Path,
        default=None,
        help="Optional path to save the figure instead of displaying it interactively.",
    )
    parser.add_argument(
        "--refit",
        action="store_true",
        help="Refit the cached synthetic EUR/USD example model before pricing.",
    )
    return parser.parse_args()


if __name__ == "__main__":
    main()
