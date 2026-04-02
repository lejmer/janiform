from datetime import UTC, datetime
from typing import Final

from janiform.calibration import JaniformEstimator, JaniformModel
from janiform.pricing import VanillaOptionPricingRunResult

from examples._shared import SYNTHETIC_EURUSD_TICK_DATA_PATH


VALUATION_TIME_UTC: Final[datetime] = datetime(2026, 2, 2, 15, 0, tzinfo=UTC)
SPOT: Final[float] = 1.0850
STRIKE: Final[float] = 1.0900
MATURITY_INTERVAL: Final[float] = 1.0


def main() -> None:
    estimator: JaniformEstimator = JaniformEstimator(market_profile="eurusd_spot")
    model: JaniformModel = estimator.fit(
        tick_data_path=SYNTHETIC_EURUSD_TICK_DATA_PATH,
    )
    pricing_result: VanillaOptionPricingRunResult = model.price_option(
        valuation_time_utc=VALUATION_TIME_UTC,
        spot=SPOT,
        strike=STRIKE,
        maturity_interval=MATURITY_INTERVAL,
    )

    print(f"fitted run id: {model.run_id}")
    print(f"option price: {pricing_result.response.price:.6f}")
    print(
        "pricing diagnostics:",
        pricing_result.response.diagnostics.to_payload(),
    )


if __name__ == "__main__":
    main()
