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
SIMULATION_INTERVAL: Final[str] = "1h"
SIMULATION_INTERVAL_COUNT: Final[int] = 24


def main() -> None:
    estimator: JaniformEstimator = JaniformEstimator(market_profile="eurusd_spot")
    model: JaniformModel = estimator.fit(
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
    )
    simulation: JaniformSimulationResult = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SIMULATION_INTERVAL_COUNT,
        interval=SIMULATION_INTERVAL,
    )
    frame: pd.DataFrame = simulation.to_pandas()

    print(f"fitted run id: {model.run_id}")
    print(f"simulated rows: {len(frame)}")
    print(frame.head(5).to_string(index=False))


if __name__ == "__main__":
    main()
