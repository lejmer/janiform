# Python API Reference

Import the module that matches the workflow you want. The package root stays intentionally minimal.

## Package Root

`janiform` exports only `__version__`.

## Workflow Modules

### `janiform.calibration`

Primary high-level fitting surface. Representative public symbols include:

- `CurrentStateObservation`
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

The supported fit ownership submodules also include `janiform.calibration.fit.state_estimation`
for the public current-state estimation contracts used by `JaniformModel.estimate_current_state(...)`.

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
