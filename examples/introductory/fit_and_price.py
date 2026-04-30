import argparse
from datetime import UTC, datetime
from typing import Final

from janiform.calibration import JaniformModel
from janiform.pricing import VanillaOptionPricingRunResult

from examples._shared import (
    SYNTHETIC_EURUSD_TICK_DATA_PATH,
    load_or_fit_persisted_example_model,
)


VALUATION_TIME_UTC: Final[datetime] = datetime(2026, 2, 2, 15, 0, tzinfo=UTC)
SPOT: Final[float] = 1.0850
STRIKE: Final[float] = 1.0900
TENOR: Final[str] = "1h"


def main() -> None:
    arguments = _parse_args()

    persisted_model = load_or_fit_persisted_example_model(
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
        force_refit=arguments.refit,
    )

    model: JaniformModel = persisted_model.model

    pricing_result: VanillaOptionPricingRunResult = model.price_option(
        valuation_time_utc=VALUATION_TIME_UTC,
        spot=SPOT,
        strike=STRIKE,
        tenor=TENOR,
    )

    print(
        f"model cache: {persisted_model.model_path} "
        f"({'loaded' if persisted_model.loaded_from_cache else 'fit'})"
    )
    print(f"fitted run id: {model.run_id}")
    print(f"option price: {pricing_result.response.price:.6f}")
    print(
        "pricing diagnostics:",
        pricing_result.response.diagnostics.to_payload(),
    )


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
