from datetime import UTC, datetime
from pathlib import Path

import matplotlib.pyplot as plt
import pandas as pd

from janiform.calibration import JaniformEstimator


DATA_PATH = (
    Path(__file__).resolve().parents[1] / "data" / "EURUSD_2023_03_21_19_00_00_to_2023_03_28_18_59_59.csv"
)
SIMULATION_START_UTC = datetime(2023, 3, 28, 19, 0, tzinfo=UTC)
COARSE_INTERVAL = "15min"
COARSE_INTERVAL_COUNT = 96
SEEDED_TICKS_HOURS = 24
TIMESTAMP_COLUMN = "time"
PRICE_COLUMN = "close"


def main() -> None:
    estimator = JaniformEstimator(market_profile="eurusd_spot")
    model = estimator.fit(tick_data_path=DATA_PATH)

    coarse_simulation = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=COARSE_INTERVAL_COUNT,
        interval=COARSE_INTERVAL,
    )
    seeded_simulation = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SEEDED_TICKS_HOURS,
    )

    coarse_frame: pd.DataFrame = coarse_simulation.to_pandas()
    seeded_frame: pd.DataFrame = seeded_simulation.to_pandas()

    figure, axes = plt.subplots(2, 1, figsize=(12, 8), sharex=False)

    axes[0].plot(
        coarse_frame[TIMESTAMP_COLUMN],
        coarse_frame[PRICE_COLUMN],
        color="tab:blue",
        linewidth=1.5,
    )
    axes[0].set_title("eurusd coarse simulation (15 minute intervals)")
    axes[0].set_ylabel(PRICE_COLUMN)
    axes[0].grid(True, alpha=0.3)

    axes[1].plot(
        seeded_frame[TIMESTAMP_COLUMN],
        seeded_frame[PRICE_COLUMN],
        color="tab:orange",
        linewidth=1.0,
    )
    axes[1].set_title("eurusd seeded tick simulation")
    axes[1].set_xlabel("timestamp")
    axes[1].set_ylabel(PRICE_COLUMN)
    axes[1].grid(True, alpha=0.3)

    figure.tight_layout()
    plt.show()


if __name__ == "__main__":
    main()
