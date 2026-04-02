from datetime import UTC, datetime
from typing import Final

import pandas as pd

from janiform.calibration import (
    JaniformEstimator,
    JaniformModel,
    JaniformSimulationResult,
)

from examples._shared import SYNTHETIC_EURUSD_TICK_DATA_PATH


SIMULATION_START_UTC: Final[datetime] = datetime(2023, 3, 28, 19, 0, tzinfo=UTC)
COARSE_INTERVAL: Final[str] = "15min"
COARSE_INTERVAL_COUNT: Final[int] = 96 * 7
SEEDED_TICKS_HOURS: Final[int] = 24 * 7
TIMESTAMP_COLUMN: Final[str] = "time"
PRICE_COLUMN: Final[str] = "close"


def main() -> None:
    import matplotlib.pyplot as plt

    estimator: JaniformEstimator = JaniformEstimator(market_profile="eurusd_spot")
    model: JaniformModel = estimator.fit(tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH)
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

    coarse_frame: pd.DataFrame = coarse_simulation.to_pandas()
    seeded_frame: pd.DataFrame = seeded_simulation.to_pandas()

    figure, axes = plt.subplots(2, 1, figsize=(12, 8), sharex=False)

    axes[0].plot(
        coarse_frame[TIMESTAMP_COLUMN],
        coarse_frame[PRICE_COLUMN],
        color="tab:blue",
        linewidth=1.5,
    )
    axes[0].set_title("synthetic eurusd coarse simulation (15 minute intervals)")
    axes[0].set_ylabel(PRICE_COLUMN)
    axes[0].grid(True, alpha=0.3)

    axes[1].plot(
        seeded_frame[TIMESTAMP_COLUMN],
        seeded_frame[PRICE_COLUMN],
        color="tab:orange",
        linewidth=1.0,
    )
    axes[1].set_title("synthetic eurusd seeded tick simulation")
    axes[1].set_xlabel("timestamp")
    axes[1].set_ylabel(PRICE_COLUMN)
    axes[1].grid(True, alpha=0.3)

    figure.tight_layout()
    plt.show()


if __name__ == "__main__":
    main()
