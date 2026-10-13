# Calibration diagnostics

Calibration diagnostics help you understand a fitted model after calibration finishes.

Use this page when you have a `JaniformFitReport`, a `JaniformFitResult`, or a saved fit report file and want to answer practical questions:

- Did Janiform reproduce the main behavior in the training data?
- Which parts of the fit look weak?
- Does a high loss point to activity, return size, return shape, or something else?
- Which diagnostics are about model fit, and which ones are saved settings used later by simulation or pricing?

This page explains how to read the diagnostics. It does not decide whether the model is acceptable for production use. That decision belongs to [model validation](model-validation.md), ideally on data that was not used for fitting.

## Where diagnostics appear

In Python, use `fit_with_report(...)` when you want the fitted model and diagnostics together:

```python
from janiform.calibration import JaniformEstimator, JaniformFitResult

estimator = JaniformEstimator(market_profile="eurusd_spot")
result: JaniformFitResult = estimator.fit_with_report(
    tick_data_path="data/Synthetic_EURUSD_Data_1M_ticks.parquet",
)

model = result.model
report = result.report
```

Python fits do not automatically persist the report unless the fit is configured to write artifacts. If you need a durable report file, configure output persistence for the fit workflow or save the report through the surrounding application code.

In the CLI, a normal persisted fit writes a report at:

```text
.janiform/runs/<run-id>/artifacts/report.json
```

You can render a CLI report with:

```bash
janiform report fit .janiform/runs/<run-id>
```

Extended diagnostics that add report size or finalization work are opt-in. In the CLI, request them with repeated `--diagnostic` options, for example:

```bash
janiform fit --diagnostic option-surface-shape ...
```

`option-surface-shape` includes option quote universe, tier construction, selected node residuals, maturity scores, and option search/selection agreement diagnostics when an option surface is part of the fit.

The same ideas apply whether the report is a Python object or saved as JSON. The diagnostics are mostly about the final calibration stage. By that point Janiform has already learned separate pieces of the model, such as activity behavior and return behavior. The final stage puts those pieces together, tries complete fitted models, simulates from them, and compares the simulated behavior with the training data.

In the report, two sections matter most:

| Report section                         | What it answers                                                     |
|----------------------------------------|---------------------------------------------------------------------|
| `objective_breakdown.global_loss`      | What single loss did Janiform use to rank complete candidates?      |
| `objective_breakdown.family_losses`    | Which broad parts of the fit were strong or weak?                   |
| `objective_breakdown.term_diagnostics` | What observed and modeled measurements caused those losses?         |
| `objective_breakdown.admissibility`    | Is the objective finite, and which advisory warnings were recorded? |

Start with `objective_breakdown.family_losses`. If one area looks bad, use `objective_breakdown.term_diagnostics` to see the underlying observed and modeled numbers.

## Full calibration stages

Full calibration reports separate the final pipeline into the stages you see in the terminal:

| Stage                         | When it appears        | What it does                                                                                                                             |
|-------------------------------|------------------------|------------------------------------------------------------------------------------------------------------------------------------------|
| `Fit seasonality`             | Full runs              | Fits deterministic baseline activity seasonality.                                                                                        |
| `Fit event activity`          | Full runs              | Fits Hawkes activity dynamics.                                                                                                           |
| `Fit return distribution`     | Full runs              | Fits baseline return-shape and return-clock contracts.                                                                                   |
| `Fit long variance`           | Full runs              | Initializes persistent variance parameters and reports horizon diagnostics.                                                              |
| `Fit news shocks`             | Full runs              | Fits news and gap shock parameters used as fixed inputs to the return-distribution backfit.                                              |
| `Backfit return distribution` | Full runs              | Re-estimates the physical return layer against the fixed long-variance and news/gap fit so scale is not counted twice.                   |
| `Fit mean reversion`          | Full runs              | Fits temporary-deviation mean-reversion behavior.                                                                                        |
| `Refine dynamics`             | Full runs              | Runs population refinement over complete physical-dynamics candidates fitted to underlying data.                                         |
| `Fit option surface`          | Full runs with options | Fits the option pricing block from the selected dynamics frontier.                                                                       |
| `Finalize model`              | Full runs              | Resolves upstream fit artifacts, prepares runtime inputs, builds final diagnostics, and writes the canonical final fitted model payload. |

Runs without an option snapshot skip `Fit option surface`. The final fitted model parameters are saved under `parameters.finalization.fitted_parameters` in `model.jf`. When an option snapshot is present, that finalization payload contains the option-calibrated pricing block while the physical Hawkes and return components remain the dynamics fit used by simulation.

## Terms used on this page

Janiform reports some terms that are useful but easy to misread.

| Term                  | Meaning                                                                                                           |
|-----------------------|-------------------------------------------------------------------------------------------------------------------|
| Candidate             | One complete fitted model that Janiform considered during calibration.                                            |
| Baseline              | The starting fit used as a comparison point before final improvement attempts.                                    |
| Global loss           | The single objective loss used to rank complete joint-refinement candidates. Lower is better.                     |
| Option surface loss   | The root mean square option quote residual after each quote is scaled by its quote spread floor. Lower is better. |
| Family loss           | The rank-normalized whitened loss for one named group of related checks, such as activity shape or return scale.  |
| Term diagnostic       | One observed-versus-modeled comparison inside a family.                                                           |
| Standardized residual | A marginal observed-versus-modeled difference divided by its coordinate uncertainty scale.                        |
| Covariance diagonal   | The coordinate variance used for marginal residual reporting inside a family covariance matrix.                   |
| Admissibility warning | A domain-margin warning recorded alongside the objective loss.                                                    |
| Activity              | How often market events arrive. In tick data, high activity means ticks arrive frequently.                        |
| Return scale          | The typical size of price changes. It is related to volatility, but reported here as fitted return behavior.      |
| Return shape          | The distributional shape of returns, including tails and how returns are spread out.                              |

The global loss is not a probability, percentile, p-value, or percentage score. It is a calibration loss.

The option search and selection tables use a different loss from population refinement. During option refinement, `Cur. loss` is the best currently observed option surface loss for the active work item, and `Final loss` is the selected option surface loss after that work item completes. It is not the underlying-data pseudo-likelihood plus an option term. A value around `1.0` means the model is around one quote scale away on average across the option tier being priced. Search-tier losses are proposal-screening losses, while selection and audit tiers use the exact pricing path as the guardrail.

## How to read losses

Lower loss is better.

Exact likelihood evidence is reported as calibrated likelihood-family diagnostics, so only relative comparisons inside the same calibration context are meaningful. Diagnostic families use residual vectors and covariance matrices. A marginal standardized residual near `0.0` means the model is close to the training data for that coordinate. A residual near `1.0` means the mismatch is about one coordinate uncertainty scale. Residuals above `1.0` are allowed.

Family loss is the squared norm of the family residual vector after covariance whitening, divided by the identified covariance rank. Correlated coordinates share evidence through the covariance matrix, and singular directions are omitted from the family loss.

Do not read `2.0` as "200 percent wrong." For a marginal standardized residual, it means the modeled coordinate is about two uncertainty scales away from the observed coordinate. For a family loss, it means the covariance-whitened residual vector has average squared distance `2.0` across identified directions.

Also avoid reading one number by itself. A useful fit is usually balanced: activity, return scale, return shape, and activity-volatility coupling should all be plausible for the intended workflow.

## Broad fit areas

`objective_breakdown.family_losses` groups the fit into broad areas. The field names are technical because they are saved in the report, but each one maps to a plain question.

| Family                       | Plain meaning                                                                   |
|------------------------------|---------------------------------------------------------------------------------|
| `returns`                    | Does the fitted return distribution explain observed tick, 5m, and 15m returns? |
| `return_shape`               | Do returns have similar center, spread, skew, and tail shape?                   |
| `return_scale`               | Are tick-level and interval-level price changes about the right size?           |
| `business_time_return_scale` | Does return size change reasonably between quiet and active periods?            |
| `activity_level_timing`      | Does the model produce the right amount and coarse timing of activity?          |
| `activity_shape`             | Does the intraday activity profile have similar shape to the training data?     |
| `rv_activity_coupling`       | Does higher activity tend to come with higher price variation in the same way?  |
| `options`                    | When options are present, do model prices match market quotes on quote scale?   |

`global_loss` is the mean of available identified family losses plus domain-margin loss plus the invalid-candidate sentinel. Diagnostic families guide ranking through their family loss. Invalid-candidate sentinels make the objective non-finite.

The global loss is useful for ranking candidate fits, but it is not a standalone quality grade. A model with a slightly worse global loss may still be easier to trust if its weak areas are less important for your use case.

`domain_margin_diagnostics` reports domain margins that Janiform treats as important for a usable model. Examples include Hawkes stationarity margin, NIG alpha-beta domain margin, and option/risk-neutral domain margin when options are present. A domain-margin violation contributes loss and records an admissibility warning.

`objective_breakdown.admissibility.excess_loss` is the feasibility penalty reported by the objective contract. For diagnostic-family scoring and domain-margin warnings it is `0.0`; invalid-candidate sentinels set it to a non-finite value.

## Detailed measurements

`objective_breakdown.term_diagnostics` shows the observed-versus-modeled measurements behind the family losses.

Each term has:

| Field                     | Meaning                                                                                                       |
|---------------------------|---------------------------------------------------------------------------------------------------------------|
| `term_id`                 | The report name for this measurement.                                                                         |
| `family_id`               | The broader behavior it belongs to, such as activity or return scale.                                         |
| `term_kind`               | The kind of term: exact likelihood, diagnostic coordinate, domain margin, option quote, or invalid candidate. |
| `observed_value`          | The value measured in the training data when applicable.                                                      |
| `model_value`             | The value from the fitted model or simulation when applicable.                                                |
| `observed_standard_error` | The estimated uncertainty of the observed value when applicable.                                              |
| `model_standard_error`    | The estimated uncertainty of the modeled value when applicable.                                               |
| `uncertainty_scale`       | The marginal covariance-diagonal scale used to standardize observed-versus-modeled mismatch.                  |
| `standardized_residual`   | The signed mismatch after scaling.                                                                            |
| `covariance_diagonal`     | The coordinate variance used by the family covariance geometry.                                               |
| `loss`                    | The marginal squared standardized residual divided by the identified family rank.                             |

Use these terms when a broad area looks weak.

For example:

- If `activity_level_timing` is high, compare observed and modeled activity count terms.
- If `return_scale` is high, check whether modeled tick or interval returns are too large or too small.
- If `return_shape` is high, inspect center, winsorized dispersion, quantile, tail, skew, and L-moment terms. The dispersion term is reported as `winsorized_volatility_q05_q95`, which means standard deviation after clamping the normalized return sample to its 5th and 95th percentiles.
- If `rv_activity_coupling` is high, check whether activity and price variation move together differently in simulation than in the training data.

Term diagnostics explain the loss, but they are not the whole model-selection rule. Use them for interpretation, not for manually recomputing the final objective.

## What a high loss usually means

A high family loss means the fitted model is not reproducing that behavior well enough under the calibration comparison.

Common patterns:

| High area                    | What to inspect                                                                                            |
|------------------------------|------------------------------------------------------------------------------------------------------------|
| Activity level and timing    | The market may be simulated too frequently, too rarely, or with the wrong coarse time profile.             |
| Activity shape               | The total activity may be acceptable, but the hourly/session profile may be wrong.                         |
| Return scale                 | Price changes may be too large or too small.                                                               |
| Return shape                 | The fitted model may miss tails, concentration near zero, skew, or other distribution features.            |
| Activity-volatility coupling | Active periods may not produce the right amount of price variation. Volatility means how much prices vary. |
| Options                      | Model prices may be outside market quote scale or imply an incompatible risk-neutral tilt.                 |

A high loss is not automatically a bug. It is a pointer. The right response is to inspect the related terms, render the fit report, simulate paths, and run validation for the intended use case.

## Why Janiform tries multiple candidates

During fitting, Janiform tries more than one candidate model. A candidate is one complete set of fitted parameters. This is necessary because high-frequency market models can fit one behavior well while harming another.

For example, one candidate might match activity very well but make returns too large. Another might match return size but lose the daily activity pattern. The selected model should be a balanced fit.

Janiform therefore considers both the global loss and the shape of the family losses. Domain margins and individual standardized residuals remain visible in diagnostics, so a model is not judged only through one aggregate number.

## Saved runtime settings

Some fields near the diagnostics are not losses. They are fitted settings that Janiform saves because simulation and pricing need them later.

### Activity seasonality

Activity seasonality means the regular time-of-day or time-of-week pattern in how often ticks arrive. For example, a market may be busier near the open than in the middle of the day.

Janiform uses this as a baseline activity pattern. It changes the regular event rate for a given hour, but it is separate from self-exciting activity bursts, where recent events make more events likely soon after.

### Long variance and return clocks

Long variance is the persistent stochastic volatility state used by return clocks. Calendar seasonality changes baseline event opportunity, while long variance and Hawkes excitation determine how much stochastic return clock accumulates along a path.

The neutral long-variance level is `1.0`. Long-variance parameters control persistence, volatility of volatility, and buy/sell event feedback into future variance. Return-clock exposure parameters control how strongly Hawkes excitation contributes to buy-side and sell-side return clocks.

The `Fit long variance` stage estimates persistence from covariances of return energy across the full supplied history. The short-horizon tick-objective window does not truncate long-variance estimation. Tick observations reuse the five-minute return aggregates to form complete hourly realized variance. Coarse observations retain their native durations and fill periods without tick coverage. Each duration and sampling-count group has its own UTC-hour normalization and observation-noise amplitude. Geometrically spaced covariance lags cover the available history, with work growing logarithmically in the number of observations per group.

The initializer fits decay and persistent dispersion using the integrated CIR covariance for each observation duration. Event and return-energy feedback share a joint nonnegative predictive regression. Normalized observations are shared between covariance and feedback estimation. The CIR covariance initializer is approximate for the full endogenous-feedback model.

The `persistence_fit` diagnostic reports identification, observation coverage, persistent dispersion, mean fitted observation noise across sampling groups, and descriptive rate support. Temporal uncertainty requires more blocks than the three parameters of a single-group persistence fit. Noise-only support, unresolved rate boundaries, insufficient observations, or invalid numerical estimates disable the long-variance factor. Its multiplier remains one, with zero diffusion and zero feedback loadings. Other fitted return components determine volatility. Calibration continues and records the limitation in `warnings` and stage progress. A supplied baseline is not required.

The `long_variance.persistence` refinement family compares cached covariance summaries over matching tick windows and observation durations. Its simulation window follows the tick-objective data-usage policy independently of the estimator's history. Unsupported covariance targets are omitted. When the tick window does not support persistence refinement, the long-variance search coordinates remain fixed at the stage estimate, including estimates supported by longer tick or coarse history. Stationary feedback normalization still follows the candidate return process. A disabled factor stays disabled throughout refinement. Identified profile support extends the local rate bounds for active refinement. The support is conditional on the estimated seasonal profile and is not a likelihood confidence interval.

Refinement reuses the stage's compact fit diagnostic, including on checkpoint resume. It does not reconstruct the coarse persistence observations or refit the decay profile. Simulated covariance summaries use existing interval aggregates and seed caches, with no additional simulations or optimizer evaluations. Disabled factors omit covariance summaries and long-variance candidate seeds.

Its report contains observed horizon diagnostics at `5min`, `1h`, `1d`, and `1w`: realized variance, variance-of-variance, return skewness, return excess kurtosis, activity/variance correlation, signed-activity relation to next-interval variance, temporal block standard errors where a coarser configured horizon is available, and delete-one jackknife standard errors for skewness and excess kurtosis. Return-only diagnostics use return intervals, while activity-aware diagnostics use tick-derived activity intervals. Joint refinement scores the horizon diagnostics with available sampling uncertainty under the `long_variance` objective family so population refinement can move the long-variance parameters when those persistent variance targets support it.

When coarse data is present, hourly long-variance return intervals keep tick-derived hourly intervals first and use directly accumulated coarse hourly return intervals only for gaps. Coarse intervals never contribute activity counts. The fields `interval_source_selection_policy`, `return_interval_tick_count`, `return_interval_coarse_candidate_count`, `return_interval_coarse_gap_fill_count`, `return_interval_tick_coarse_overlap_count`, `return_interval_count`, and `activity_interval_count` summarize that source-selection step. The fields `persistence_lag_diagnostics`, `persistence_fit`, `selected_mean_reversion_rate`, `event_loading_fit_candidate_pair_count`, `event_loading_fit_pair_count`, and `event_loading_fit_coverage_ratio` summarize the parameter-estimation evidence. Tick persistence observations require complete hours. Coarse persistence observations retain each measured return interval, including intervals longer than an hour. `single_return_observation_count` reports observations represented by one return square. Each persistence lag diagnostic records the observation duration and sampling count.

The `Backfit return distribution` stage is a variance-budget backfit. It keeps the fitted activity, long-variance, seasonality, and news/gap components fixed, then makes the return-distribution handoff represent the residual physical return layer that downstream mean-reversion, market-pressure, joint-refinement, simulation, and pricing workflows should consume. The report includes the selected backfit candidate, whether a no-op candidate was selected, and the variance-budget diagnostic fields used to compare the initial NIG contribution, fixed slow/news contribution, backfitted NIG contribution, modeled scale, and observed target scale when those measurements are available.

### Mean reversion

The `Fit mean reversion` stage decides whether the retained observations support temporary-deviation relaxation. In this context, mean reversion means that the fitted log state is represented as `X = A + D`, with `kappa` controlling how quickly the temporary deviation `D` relaxes.

The fitted mean-reversion speed is called `kappa`:

| Value       | Practical meaning                                                                                                                                    |
|-------------|------------------------------------------------------------------------------------------------------------------------------------------------------|
| `kappa > 0` | Janiform selected a model with nonzero temporary-deviation relaxation.                                                                               |
| `kappa = 0` | Janiform selected the random-walk interpretation for the temporary-deviation relaxation factor.                                                      |

The stage is asset-agnostic. It does not assume that FX, equities, futures, or crypto should behave differently. It compares each fitted `kappa > 0` candidate with `random_walk_null`, which predicts the next retained log price as the current retained log price. Market pressure is fitted separately by the return-components stage and enters forecasts, pricing, and simulation as its own model factor.

The diagnostics use the word "grid" for the time spacing used by mean-reversion calibration. This is not the tick grid. The tick grid is the set of prices the instrument can display, such as increments of `0.25` or `0.00001`. The mean-reversion calibration grid is a time spacing, such as one hour, one session, or one day, used to line up retained price observations for this stage.

You normally do not set a mean-reversion source. Janiform chooses from the retained data:

| Data available in the fit  | Mean-reversion evidence source                                                                                       |
|----------------------------|----------------------------------------------------------------------------------------------------------------------|
| Tick data only             | Retained open-session tick prices.                                                                                   |
| Tick data plus coarse data | Retained open-session tick prices and retained coarse price observations, evaluated as separate source-backed grids. |

There is no separate primary or secondary mean-reversion source setting. If slower price history is available, provide it as coarse data. Coarse history can start before the tick data, overlap it, or extend after it. Janiform evaluates whatever retained source-backed grids are viable under the same model-selection rule.

Janiform evaluates all viable automatic time grids unless the request provides an explicit `mean_reversion.calibration_grid_interval_hours`. Automatic grids are derived from the retained data. They can include the typical time between retained boundaries, the typical observed session length, a spacing derived from the full retained history length, and the coarse-bar cadence when coarse data is available. The selected grid summary is in `diagnostics.calibration_grid`, and all evaluated grids are listed in `diagnostics.evaluated_grids`.

Each candidate also includes `series_summaries` for the calibration series used internally by the stage: core expected increments, market pressure state path when present, boundary anchor values, deviations from anchor, and residuals. The report keeps counts, endpoints, range, mean, and constant-value detection so you can audit scale and shape.

A model with `kappa > 0` is selected when it is the best admissible candidate under the Gaussian quasi-likelihood BIC policy. The policy ranks viable retained-grid candidates by their profiled one-step residual likelihood with the candidate parameter count included in the BIC penalty.

To diagnose a zero or positive `kappa`, read the mean-reversion diagnostics in this order:

1. Start with `diagnostics.stage_decision_reason`. It distinguishes BIC selection from missing-grid fallback and runtime-projection fallback.
2. Check `diagnostics.calibration_grid` to see the selected grid, then inspect `diagnostics.evaluated_grids` to see whether another automatic grid was considered.
3. On each evaluated time grid, compare `random_walk_null` with the `kappa > 0` candidates.
4. Compare `information_criterion_bic`, `residual_sum_of_squares`, and `regression_pair_count` before interpreting the selected candidate.

When `kappa = 0`, the practical reading is: Janiform selected no temporary-deviation relaxation after comparing viable retained-grid candidates.

### Business time for returns

Business time is Janiform's way of measuring how much open-market opportunity has passed between returns. A return after a long quiet open-session interval is not treated the same as a return after a very short interval, because more market time has passed even if few ticks arrived.

This matters for return diagnostics. Janiform compares return size against the amount of open-session business time that produced it. Returns across a market closure are handled as gaps, not as ordinary same-session tick returns.

### Tick-grid observation

A tick grid is the set of prices the market can actually display. For example, an instrument with a tick size of `0.25` can trade at `100.00` and `100.25`, but not `100.13`.

Janiform's underlying simulated price can move by less than one displayed tick. The tick-grid setting controls how that price is converted back to a value that could appear in market data.

If you provide a tick size during fitting, Janiform uses it. Otherwise, a selected market profile can provide a fallback tick size. When neither is available, Janiform may infer one from the smallest positive same-session price change in the training data. Small moves that do not yet reach a full tick are carried forward, so repeated small moves can eventually become a visible price change.

### Candidate seed frontier

Joint refinement starts from a frontier of candidate seeds. A seed is a complete active parameter vector built from residual evidence Janiform can evaluate before the full simulation pass.

Some seeds adjust return scale, some preserve the current activity model while changing return shape, and some combine return-shape changes with activity-level alignment. If the structured seed pool is too narrow, Janiform fills the frontier with local quasi-random points around the staged model.

The candidate seed frontier is selected from low-cost screening losses before population refinement. The screening step keeps candidates that improve different weak areas, but admissibility is still decided from refreshed selection or audit-fidelity loss diagnostics.

Before filling the frontier with quasi-random seeds, Janiform builds sensitivity repair seeds from the best screened candidates. For each anchor candidate, it perturbs each active parameter inside the validated bounds, measures how active finite standardized residuals respond, solves a small damped least-squares repair step in normalized parameter space, and adds bounded trust-region versions of that step back into the same screening and diversity selection path. The active residual geometry defines the term-driven repair direction.

During population refinement, Janiform evaluates a population of full active parameter vectors across the candidate seed frontier. Seed candidates anchor the population, sampled candidates are drawn from an elite-refitted distribution inside the validated parameter bounds, and finalization validates distinct ranked candidates at local and final fidelity. The search state updates return-shape, activity, and coupling parameters as one joint candidate. Exact tick return likelihood is evaluated on a deterministic weighted representative sample so each optimizer probe remains cheap while preserving support across return sign, return size, and business-time regimes. Coarse interval probes also include a reduced deterministic tick-shape proxy from the NIG return-composition sampler, so the optimizer still sees tick-level shape pressure before the refreshed seeded-tick evaluations. Refreshed local/final evaluations use full-fidelity tick likelihood before a candidate can be selected or accepted as admissible.

The Hawkes search coordinate is a stationary event-rate coordinate. Janiform solves `lambda_bar` from the current excitation matrix and the requested stationary open-event rate, which lets the search change clustering and decay without accidentally changing total activity on every single-coordinate probe.

During population refinement, terminal progress reports the shared pseudo-likelihood loss used by the search. `Trial loss` is the most recent full-vector probe, `Best loss` is the best retained population candidate, and `Final loss` is the completed candidate loss when the candidate has reached a final evaluation. Smaller loss is better.

Population refinement progress may also show exact return-likelihood evaluator counters. `CF-Buy` is the number of return blocks evaluated by the closed-form single-side buy density, `CF-Sell` is the number evaluated by the closed-form single-side sell density, and `Mix` is the number evaluated by the tilted mixed buy/sell inversion path. High `CF-Buy` or `CF-Sell` means the return likelihood mostly used the cheaper closed-form kernels. High `Mix` means more blocks required the more expensive mixed-side tilted inversion path.

The report may mention seed candidates because they explain how Janiform searched. They are not user-tuned model components, but diagnostic search artifacts.

### Option refinement progress

Option refinement uses option quote residual scores, not the population refinement pseudo-likelihood loss. The option score is the maturity-weighted root-mean-square of option quote residuals after each residual has been scaled by its quote spread. A score near `1.0` means the model is typically about one quoted spread away from the observed mid-price on the tier being evaluated. Smaller is better.

The option search tier first prices an exact sensitivity baseline, then validates pricing-measure proposals with the same terminal-affine pricing semantics used by selection and audit. The default search budget is deterministic: one exact baseline evaluation plus one proposal evaluation per active pricing coordinate. If `joint_refinement_option_refinement_wall_clock_budget_millis` is provided, it acts as an explicit wall-clock guard around that deterministic search work.

In option progress tables, `Cur. loss` is the latest option residual score known for the running candidate. During a pricing call this can be the score from the previous completed evaluation for that candidate, because the current pricing call has not produced a score yet. `Best loss` is the best option score seen inside the active option phase. `Final loss` is the option score recorded when that candidate finishes the phase. These columns are not numerically comparable with population refinement `Trial loss`, `Best loss`, or `Final loss`, because those population refinement columns come from the joint underlying-data objective.

The `Interval` column reports option-pricing interval progress. When row-level pricing detail is available, `Interval` shows the current interval position and `Rows` shows row progress inside that interval. When row detail is not available, `Interval` shows the completed interval count.

### Runtime profile

Joint refinement reports `runtime_profile` in successful and failed stage reports. The wall-clock fields separate population refinement, coarse consistency checks, final seed evaluation, and final assembly.

The cumulative fields split objective evaluation cost into deterministic work, likelihood-context construction, tick/5m/15m likelihood evaluation, expected activity counts, deterministic activity shape/level checks, seed-summary simulation, raw seed metric extraction, loss assembly, and option pricing. The cache counters show whether repeated evaluations reused objective, deterministic, seed-summary, aggregated-likelihood-context, or option-pricing artifacts.

Use `cumulative_deterministic_evaluation_millis` as the broad total for deterministic objective work, then use the narrower cumulative fields to identify which deterministic substep dominates. Millisecond counters can remain zero for very small samples or very cheap substeps.

Population refinement also records completed-probe timing diagnostics. These diagnostics include candidate id, scheduling category, search direction, elapsed milliseconds, objective value, admissibility warning count, standardized residual summary, and exact likelihood evaluator counters. The same completed-probe diagnostics are streamed into `events.jsonl` as `metric_update` events during the run, so long refinement searches leave a durable timing trail before the final report is assembled.

## A practical reading order

When reading a fit report, read it in this order:

1. Check whether the fit completed successfully.
2. Look at `objective_breakdown.family_losses` for the largest weak areas.
3. Use `objective_breakdown.term_diagnostics` to compare observed and modeled values for those areas.
4. Inspect saved runtime settings only if simulation or pricing behavior looks surprising.
5. Run model validation before relying on the model for a downstream workflow.

## See also

- [Fit models](fit-models.md)
- [Inspect and report](inspect-and-report.md)
- [Model validation](model-validation.md)
- [Simulate paths](simulate-paths.md)
