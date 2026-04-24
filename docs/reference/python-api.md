# Python API Reference

Import the module that matches the workflow you want. The package root stays intentionally minimal.

## Package Root

`janiform` exports only `__version__`.

## Workflow Modules

### `janiform.calibration`

Primary high-level fitting surface. Representative public symbols include:

- `JaniformEstimator`
- `JaniformFitReport`
- `JaniformFitResult`
- `JaniformModel`
- `MarketDataCalibrationRequest`

The supported fit ownership submodules also include:

- `janiform.calibration.fit.state` for the public current-state estimation contracts
  used by `JaniformModel.estimate_current_state(...)`
- `janiform.calibration.fit.pricing` for the public fitted-model option-pricing contracts used by
  `JaniformModel.price_option_grid(...)`, `JaniformModel.price_option_grid_surface(...)`,
  `JaniformModel.price_option_snapshot(...)`, and `JaniformModel.price_option_surface(...)`,
  including Janiform-computed implied-volatility fields together with per-node pricing status and
  failure diagnostics on the returned pricing artifacts
- `janiform.calibration.fit.pricing.market_snapshot.inputs` for `TabularFrameInput`, the public
  market-data tabular input alias used by `JaniformModel.price_option_snapshot(...)` and
  `JaniformModel.price_option_surface(...)`
- `janiform.calibration.market_data` for market-data request, preparation, and workflow contracts
- `janiform.calibration.presets` for preset references, preset registries, and preset resolution
- `janiform.calibration.staged` for staged calibration request, input, runtime, and response contracts

`JaniformModel` is the canonical high-level runtime surface for fitted models. In addition to
`fit` persistence helpers, the main runtime entrypoints are:

- `JaniformModel.estimate_current_state(...)`
- `JaniformModel.price_option(...)`
- `JaniformModel.price_option_grid(...)`
- `JaniformModel.price_option_grid_surface(...)`
- `JaniformModel.price_option_snapshot(...)`
- `JaniformModel.price_option_surface(...)`
- `JaniformModel.simulate(...)`

Current-state estimation is the default live-state path when observed data is supplied:

- `estimate_current_state(...)` derives both the Hawkes excitation state and the mean-reversion
  boundary state from recent observed tick data
- `price_option(..., observed_data=...)` uses that estimator automatically before pricing
- `simulate(..., continuation_data=...)` uses that estimator automatically before continuing the
  path
- `price_option_snapshot(...)` loads observed tick data, estimates the current state, prices the
  retained observed quote snapshot in fitted-model option grids, and returns both tidy rows and a
  strike-by-expiry surface frame
- `price_option_surface(...)` extends the observed quote workflow to a surface result, either for
  the retained multi-expiry snapshot or across valuation times when the retained quotes share one
  expiry
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

`JaniformEstimator.fit(...)` is the canonical high-level fit entrypoint for both underlying-only
and option calibration. It chooses the fit from the supplied inputs:

- tick data only fits the underlying model, suitable for simulations
- tick data plus an option surface fits the model to the underlying data and the selected option snapshot,
  suitable for options pricing

The high-level joint option path also exposes:

- `option_surface_valuation_time_utc` to select the option snapshot being fit
- `carry` for optional flat discount-rate and carry-rate input

The `JaniformEstimator` constructor accepts `dynamics_search_restart_candidate_budget` to set the
joint-refinement restart candidate budget used by the calibration dynamics search.

`JaniformEstimator.fit_with_report(...)` returns `JaniformFitResult`, which carries both the fitted
model and the fit report without changing the main `fit(...)` return shape. The returned
`JaniformFitReport` also exposes staged execution summaries and the resolved stage-report payloads
through `report.stage_report_payloads`.

When fitting with options, one fit run targets one option quote timestamp, tick data remains
mandatory, and the option surface must contain bid and ask quote data together with prices.

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
- `VanillaOptionSurfacePricingRequest`
- `VanillaOptionSurfacePricingResponse`
- `VanillaOptionSurfacePricingRunResult`
- `VanillaOptionImpliedVolatilityInversionStatus`
- `AnchoredVanillaOptionPricingBoundaryInput`
- `run_vanilla_option_pricing`
- `run_vanilla_option_surface_pricing`

`VanillaOptionSurfacePricingResponse` carries both the surface grids and an aligned
`VanillaOptionSurfacePricingNodeTable`, so callers can inspect, tabulate, plot, or export the
priced surface directly without rebuilding node rows from separate axes and grids first.

The public contract submodule is:

- `janiform.pricing.contracts`

### `janiform.risk`

Sufficient-statistics risk path contracts. Representative public symbols include:

- `SufficientStatisticsRiskRunRequest`
- `SufficientStatisticsRiskPath`
- `SufficientStatisticsRiskPathDiagnostics`
- `validate_sufficient_statistics_risk_run_request`

The public contract submodule is:

- `janiform.risk.contracts`

### `janiform.hourly_factors`

Hourly factor materialization, providers, and contract helpers. Representative public symbols include:

- `HourlyFactorContract`
- `HourlySeasonalityMaterialization`
- `HourlyNewsIntensityMaterialization`
- `build_hourly_factor_contract`
- `materialize_hourly_seasonality_factors`
- `materialize_hourly_news_intensity_factors`

The public contract submodule is:

- `janiform.hourly_factors.contracts`

### `janiform.seasonality`

Timestamp preprocessing, seasonality estimation, and diagnostics workflows. Representative public symbols include:

- `SessionFilter`
- `IntradaySmoothingKernel`
- `ScheduledEventOverlayDefinition`
- `preprocess_timestamp_events`
- `estimate_intraday_seasonality`
- `generate_seasonality_diagnostics_report`

The public contract submodule is:

- `janiform.seasonality.contracts`

### `janiform.validation`

Regression and behavior-validation entrypoints. Representative public symbols include:

- `DeterministicRegressionHarnessReport`
- `OpenClosedGapBehaviorDiffReport`
- `RegressionHarnessFallbackPolicy`
- `run_deterministic_regression_harness`
- `run_open_closed_gap_leakage_regression_harness`

The public contract submodule is:

- `janiform.validation.contracts`

### `janiform.stylized_facts`

Stylized-fact reporting workflows and report contracts. Representative public symbols include:

- `StylizedFactHorizonComparisonReport`
- `StylizedFactHorizonComparisonReportBundle`
- `StylizedFactHorizonComparisonTableSummary`
- `StylizedFactHorizonComparisonPlotPayload`
- `run_stylized_fact_horizon_comparison_report_generator`

The public contract submodule is:

- `janiform.stylized_facts.contracts`

### `janiform.mean_reversion`

Public mean-reversion domain contracts shared across pricing, calibration, and simulation.
Representative public symbols include:

- `MeanReversionAnchorModel`
- `MeanReversionBoundaryState`
- `MeanReversionFittedParameters`
- `MeanReversionRuntimeIntervalAnchors`

### `janiform.contracts`

Shared contract models and serialization helpers. Representative public symbols include:

- `ContractModel`
- `require_mapping`
- `require_non_empty_string`
- `require_optional_finite_scalar`
- `serialize_deterministic_json`
- `serialize_pretty_json`
