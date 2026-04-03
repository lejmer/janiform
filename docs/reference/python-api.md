# Python API Reference

Import the module that matches the workflow you want. The package root stays intentionally minimal.

## Package Root

`janiform` exports only `__version__`.

## Workflow Modules

### `janiform.calibration`

Primary high-level fitting surface. Representative public symbols include:

- `CurrentStateEstimationRequest`
- `CurrentStateEstimationResult`
- `JaniformEstimator`
- `JaniformModel`
- `JaniformFitReport`
- `MarketDataCalibrationRequest`
- `StagedCalibrationExecutionInput`
- `StagedCalibrationRequest`
- `StagedCalibrationResponse`
- `prepare_market_data_calibration`
- `run_market_data_calibration`
- `run_staged_calibration`

The supported fit ownership submodules also include:

- `janiform.calibration.fit.state_estimation` for the public current-state estimation contracts
  used by `JaniformModel.estimate_current_state(...)`
- `janiform.calibration.fit.pricing` for the public batch and surface pricing contracts used by
  `JaniformModel.price_options(...)` and `JaniformModel.price_option_surface(...)`

`JaniformModel` is the canonical high-level runtime surface for fitted models. In addition to
`fit` persistence helpers, the main runtime entrypoints are:

- `JaniformModel.estimate_current_state(...)`
- `JaniformModel.price_option(...)`
- `JaniformModel.price_options(...)`
- `JaniformModel.price_option_surface(...)`
- `JaniformModel.simulate(...)`

Current-state estimation is the default live-state path when observed data is supplied:

- `estimate_current_state(...)` derives both the Hawkes excitation state and the mean-reversion
  boundary state from recent observed tick data
- `price_option(..., observed_data=...)` uses that estimator automatically before pricing
- `simulate(..., continuation_data=...)` uses that estimator automatically before continuing the
  path
- when no observed data is supplied, Janiform falls back to the stationary Hawkes state
- explicit `initial_excitation_state` and `initial_mean_reversion_boundary_state` overrides remain
  available for diagnostic workflows
- `JaniformSimulationResult.to_pandas()`, `to_numpy()`, and `to_parquet(...)` include the simulated
  boundary-state fields `observed_log_price`, `anchor_value`, `pressure_state`, and
  `deviation_from_anchor`

`CurrentStateEstimationRequest` is batch-shaped. It accepts observed tick data as either:

- a pandas `DataFrame`
- a mapping of column arrays, including numpy arrays

By default, Janiform reads the timestamp, price, and trade-sign columns from the fitted model's
saved input inference, so users can pass data that looks like the data they fit on without
restating the column mapping. `tick_data_column_mapping` remains available when an override is
needed.

### `janiform.simulation`

Public simulation workflows and shared runtime contracts. Representative public symbols include:

- `CoarseSimulationRunRequest`
- `CoarseSimulationPath`
- `SeededTickSimulationRequest`
- `SeededTickSimulationResponse`
- `HawkesKernelParameters`
- `MeanReversionBoundaryState`
- `run_coarse_simulation`
- `run_seeded_tick_event_simulation`

### `janiform.pricing`

Vanilla option pricing contracts and runtime entrypoints. Representative public symbols include:

- `VanillaOptionPricingRequest`
- `VanillaOptionPricingResponse`
- `VanillaOptionPricingRunResult`
- `AnchoredVanillaOptionPricingBoundaryInput`
- `run_vanilla_option_pricing`
- `validate_vanilla_option_pricing_request`

### `janiform.risk`

Sufficient-statistics risk path contracts. Representative public symbols include:

- `SufficientStatisticsRiskRunRequest`
- `SufficientStatisticsRiskPath`
- `SufficientStatisticsRiskPathDiagnostics`
- `validate_sufficient_statistics_risk_run_request`

### `janiform.hourly_factors`

Hourly factor materialization, providers, and contract helpers. Representative public symbols include:

- `HourlyFactorContract`
- `HourlySeasonalityMaterialization`
- `HourlyNewsIntensityMaterialization`
- `build_hourly_factor_contract`
- `materialize_hourly_seasonality_factors`
- `materialize_hourly_news_intensity_factors`

### `janiform.validation`

Regression and behavior-validation entrypoints. Representative public symbols include:

- `DeterministicRegressionHarnessReport`
- `OpenClosedGapBehaviorDiffReport`
- `RegressionHarnessFallbackPolicy`
- `run_deterministic_regression_harness`
- `run_open_closed_gap_leakage_regression_harness`

### `janiform.contracts`

Shared contract models and serialization helpers. Representative public symbols include:

- `ContractModel`
- `MeanReversionAnchorModel`
- `MeanReversionBoundaryState`
- `serialize_deterministic_json`
- `serialize_pretty_json`
