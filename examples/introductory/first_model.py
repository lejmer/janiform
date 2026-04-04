import argparse
from datetime import UTC, datetime
from typing import Final

import pandas as pd

from janiform.calibration import (
    JaniformModel,
    JaniformSimulationResult,
)

from examples._shared import (
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    load_or_fit_persisted_example_model,
)


SIMULATION_START_UTC: Final[datetime] = datetime(2023, 3, 28, 19, 0, tzinfo=UTC)
SIMULATION_INTERVAL: Final[str] = "1h"
SIMULATION_INTERVAL_COUNT: Final[int] = 24


def main() -> None:
    arguments = _parse_args()
    persisted_model = load_or_fit_persisted_example_model(
        market_profile="eurusd_spot",
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
        force_refit=arguments.refit,
    )
    model: JaniformModel = persisted_model.model
    simulation: JaniformSimulationResult = model.simulate(
        seed=17,
        start_time_utc=SIMULATION_START_UTC,
        horizon_interval_count=SIMULATION_INTERVAL_COUNT,
        interval=SIMULATION_INTERVAL,
    )
    frame: pd.DataFrame = simulation.to_pandas()

    print(
        f"model cache: {persisted_model.model_path} "
        f"({'loaded' if persisted_model.loaded_from_cache else 'fit'})"
    )
    print(f"fitted run id: {model.run_id}")
    print(f"simulated rows: {len(frame)}")
    print(frame.head(5).to_string(index=False))


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
