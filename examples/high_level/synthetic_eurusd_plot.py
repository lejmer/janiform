import argparse
from datetime import UTC, datetime
from typing import Final

import pandas as pd

from janiform.model import JaniformModel
from janiform.simulation import JaniformSimulationResult

from examples._shared import (
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    load_or_fit_persisted_example_model,
)


SIMULATION_START_UTC: Final[datetime] = datetime(2023, 3, 28, 19, 0, tzinfo=UTC)
COARSE_INTERVAL: Final[str] = "15min"
COARSE_INTERVAL_COUNT: Final[int] = 96 * 7
SEEDED_TICKS_HOURS: Final[int] = 24 * 7
TIMESTAMP_COLUMN: Final[str] = "time"
PRICE_COLUMN: Final[str] = "close"


def main() -> None:
    import matplotlib.pyplot as plt

    arguments = _parse_args()
    persisted_model = load_or_fit_persisted_example_model(
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
        force_refit=arguments.refit,
    )
    model: JaniformModel = persisted_model.model
    coarse_simulation: JaniformSimulationResult = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=COARSE_INTERVAL_COUNT,
        interval=COARSE_INTERVAL,
    )
    seeded_simulation: JaniformSimulationResult = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SEEDED_TICKS_HOURS,
    )

    coarse_df: pd.DataFrame = coarse_simulation.to_pandas()
    seeded_df: pd.DataFrame = seeded_simulation.to_pandas()

    print(
        f"model cache: {persisted_model.model_path} "
        f"({'loaded' if persisted_model.loaded_from_cache else 'fit'})"
    )

    figure, axes = plt.subplots(2, 1, figsize=(12, 8), sharex=False)

    axes[0].plot(
        coarse_df[TIMESTAMP_COLUMN],
        coarse_df[PRICE_COLUMN],
        color="tab:blue",
        linewidth=1.5,
    )
    axes[0].set_title("synthetic eurusd coarse simulation (15 minute intervals)")
    axes[0].set_ylabel(PRICE_COLUMN)
    axes[0].grid(True, alpha=0.3)

    axes[1].plot(
        seeded_df[TIMESTAMP_COLUMN],
        seeded_df[PRICE_COLUMN],
        color="tab:orange",
        linewidth=1.0,
    )
    axes[1].set_title("synthetic eurusd seeded tick simulation")
    axes[1].set_xlabel("timestamp")
    axes[1].set_ylabel(PRICE_COLUMN)
    axes[1].grid(True, alpha=0.3)

    figure.tight_layout()
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
