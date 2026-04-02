# Examples

Run the examples as Python modules from the repository root.

## Introductory

- `python -m examples.introductory.first_model`
- `python -m examples.introductory.fit_and_price`

These are the shortest workflows for fitting the checked-in synthetic EUR/USD data and turning the fitted model into a simulation or an option price.

## High-Level

- `python -m examples.high_level.synthetic_eurusd_plot`
- `python -m examples.high_level.market_data_calibration`
- `python -m examples.high_level.staged_calibration`
- `python -m examples.high_level.preset_resolution`

These examples stay on the curated workflow surface and show preset resolution, market-data preparation, staged execution, and the plotting walkthrough.

## Low-Level

- `python -m examples.low_level.simulation_requests`
- `python -m examples.low_level.hourly_factor_contract`
- `python -m examples.low_level.vanilla_option_pricing_request`
- `python -m examples.low_level.risk_request_contract`
- `python -m examples.low_level.validation_harness`
- `python -m examples.low_level.stylized_fact_report`

These examples construct the public request and contract models directly when you need explicit execution inputs instead of the higher-level estimator flow.
