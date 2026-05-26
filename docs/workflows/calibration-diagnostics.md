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

The same ideas apply whether the report is a Python object or saved as JSON. The diagnostics are mostly about the final calibration stage. By that point Janiform has already learned separate pieces of the model, such as activity behavior and return behavior. The final stage puts those pieces together, tries complete fitted models, simulates from them, and compares the simulated behavior with the training data.

In the report, two sections matter most:

| Report section                         | What it answers                                                |
|----------------------------------------|----------------------------------------------------------------|
| `objective_breakdown.global_loss`      | What single loss did Janiform use to rank complete candidates? |
| `objective_breakdown.family_losses`    | Which broad parts of the fit were strong or weak?              |
| `objective_breakdown.term_diagnostics` | What observed and modeled measurements caused those losses?    |
| `objective_breakdown.admissibility`    | Which terms, if any, blocked final acceptance?                 |

Start with `objective_breakdown.family_losses`. If one area looks bad, use `objective_breakdown.term_diagnostics` to see the underlying observed and modeled numbers.

## Full calibration stages

Full calibration reports separate the final pipeline into the stages users see in the terminal:

| Stage                | When it appears       | What it does                                                                 |
|----------------------|-----------------------|-------------------------------------------------------------------------------|
| `Refine dynamics`    | Full runs             | Searches restart candidates for the physical dynamics fitted to underlying data. |
| `Fit option surface` | Full runs with options | Fits the option pricing block from the selected dynamics frontier.             |
| `Finalize model`     | Full runs             | Builds final diagnostics and writes the canonical final fitted model payload.   |

Runs without an option snapshot skip `Fit option surface`. The final fitted model parameters are saved under `parameters.finalization.fitted_parameters` in `model.jf`. When an option snapshot is present, that finalization payload contains the option-calibrated pricing block while the physical Hawkes and return components remain the dynamics fit used by simulation.

## Terms used on this page

Janiform reports some terms that are useful but easy to misread.

| Term                  | Meaning                                                                                                      |
|-----------------------|--------------------------------------------------------------------------------------------------------------|
| Candidate             | One complete fitted model that Janiform considered during calibration.                                       |
| Baseline              | The starting fit used as a comparison point before final improvement attempts.                               |
| Global loss           | The single pseudo-likelihood loss used to rank complete joint-refinement candidates. Lower is better.        |
| Option surface loss   | The root mean square option quote residual after each quote is scaled by its quote spread floor. Lower is better. |
| Family loss           | The mean loss for one named group of related checks, such as activity shape or return scale.                 |
| Term diagnostic       | One observed-versus-modeled comparison inside a family.                                                      |
| Standardized residual | The observed-versus-modeled difference divided by its uncertainty scale.                                     |
| Huber loss            | The per-term loss used for stylized facts. It is quadratic near zero and linear for large residuals.         |
| Excess loss           | The extra loss for standardized residuals or constraints beyond the admissibility threshold.                 |
| Admissibility blocker | A term or hard constraint that crossed the final acceptance threshold.                                       |
| Activity              | How often market events arrive. In tick data, high activity means ticks arrive frequently.                   |
| Return scale          | The typical size of price changes. It is related to volatility, but reported here as fitted return behavior. |
| Return shape          | The distributional shape of returns, including tails and how returns are spread out.                         |

The global loss is not a probability, percentile, p-value, or percentage score. It is a calibration loss.

The option search and selection tables use a different loss from restart optimization. During option refinement, `Cur. loss` is the best currently observed option surface loss for the active work item, and `Final loss` is the selected option surface loss after that work item completes. It is not the restart pseudo-likelihood plus an option term. A value around `1.0` means the model is around one quote scale away on average across the option tier being priced. Search-tier losses are proposal-screening losses, while selection and audit tiers use the exact pricing path as the guardrail.

## How to read losses

Lower loss is better.

Exact likelihood terms use shifted average negative log likelihood, so only relative comparisons inside the same calibration context are meaningful. Stylized-fact terms use standardized residuals. For a stylized fact, a standardized residual near `0.0` means the model is close to the training data for that measurement. A residual near `1.0` means the mismatch is about one combined uncertainty scale. Residuals above `1.0` are allowed.

The reported stylized-fact `loss` is Huberized from the absolute standardized residual. That means a large residual still makes the candidate worse, but one extreme term cannot dominate the entire global loss as aggressively as a purely quadratic penalty would.

Do not read `2.0` as "200 percent wrong." For standardized residuals, it means the modeled value is about two uncertainty scales away from the observed value. For Huberized loss, the number is the objective contribution after the Huber transform.

Also avoid reading one number by itself. A useful fit is usually balanced: activity, return scale, return shape, and activity-volatility coupling should all be plausible for the intended workflow.

## Broad fit areas

`objective_breakdown.family_losses` groups the fit into broad areas. The field names are technical because they are saved in the report, but each one maps to a plain question.

| Family                       | Plain meaning                                                                   |
|------------------------------|---------------------------------------------------------------------------------|
| `returns`                    | Does the fitted return distribution explain observed tick, 5m, and 15m returns? |
| `return_shape`               | Do returns have similar center, spread, skew, and tail shape?                   |
| `return_scale`               | Are tick-level and interval-level price changes about the right size?           |
| `business_time_return_scale` | Does return size change reasonably between quiet and active periods?            |
| `intraday_return_level`      | Does return size vary across the day in a similar way?                          |
| `activity_level_timing`      | Does the model produce the right amount and coarse timing of activity?          |
| `activity_shape`             | Does the intraday activity profile have similar shape to the training data?     |
| `rv_activity_coupling`       | Does higher activity tend to come with higher price variation in the same way?  |
| `options`                    | When options are present, do model prices match market quotes on quote scale?   |

`global_loss` is the mean of available family losses plus domain-margin prior loss plus admissibility excess loss. The excess loss is zero when every standardized residual and hard constraint is inside the admissibility region. Once terms cross the admissibility threshold, each excess beyond that threshold is Huberized and summed into the global loss, so optimization and final acceptance point in the same direction.

The global loss is useful for ranking candidate fits, but it is not a standalone quality grade. A model with a slightly worse global loss may still be easier to trust if its weak areas are less important for your use case.

`hard_constraint_diagnostics` reports domain-margin constraints that Janiform treats as required for a usable model. Examples include Hawkes stationarity margin, NIG alpha-beta domain margin, and option/risk-neutral domain margin when options are present. A hard constraint can block admissibility even if the global loss looks low.

`objective_breakdown.admissibility.excess_loss` reports the feasibility penalty that was added to the global loss. It is `0.0` for candidates that are inside all admissibility thresholds.

## Detailed measurements

`objective_breakdown.term_diagnostics` shows the observed-versus-modeled measurements behind the family losses.

Each term has:

| Field                     | Meaning                                                                            |
|---------------------------|------------------------------------------------------------------------------------|
| `term_id`                 | The report name for this measurement.                                              |
| `family_id`               | The broader behavior it belongs to, such as activity or return scale.              |
| `term_kind`               | The kind of term: exact likelihood, stylized fact, domain margin, or option quote. |
| `observed_value`          | The value measured in the training data when applicable.                           |
| `model_value`             | The value from the fitted model or simulation when applicable.                     |
| `observed_standard_error` | The estimated uncertainty of the observed value when applicable.                   |
| `model_standard_error`    | The estimated uncertainty of the modeled value when applicable.                    |
| `uncertainty_scale`       | The pooled scale used to standardize observed-versus-modeled mismatch.             |
| `standardized_residual`   | The signed mismatch after scaling.                                                 |
| `loss`                    | The contribution this term makes before family averaging.                          |
| `admissibility_blocker`   | Whether this term blocks final acceptance.                                         |

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

Janiform therefore considers both the global loss and the shape of the family losses. Admissibility also checks individual standardized residuals and hard constraints, so a model cannot pass only because one number hides an unacceptable weakness elsewhere.

## Saved runtime settings

Some fields near the diagnostics are not losses. They are fitted settings that Janiform saves because simulation and pricing need them later.

### Activity seasonality

Activity seasonality means the regular time-of-day or time-of-week pattern in how often ticks arrive. For example, a market may be busier near the open than in the middle of the day.

Janiform uses this as a baseline activity pattern. It changes the regular event rate for a given hour, but it is separate from self-exciting activity bursts, where recent events make more events likely soon after.

### Return-size seasonality

The report may call this return-mark seasonality. It means the regular time-of-week pattern in the size of tick returns.

This is separate from activity seasonality. A market can have many ticks at one time of day, but those ticks do not necessarily have larger price changes. Janiform stores 168 hourly factors, one for each local hour of the week, so simulation can reflect both patterns.

The neutral factor is `1.0`, which means "use the average return scale." Values above `1.0` mean larger-than-average tick returns for that local hour. Values below `1.0` mean smaller-than-average tick returns.

### Business time for returns

Business time is Janiform's way of measuring how much open-market opportunity has passed between returns. A return after a long quiet open-session interval is not treated the same as a return after a very short interval, because more market time has passed even if few ticks arrived.

This matters for return diagnostics. Janiform compares return size against the amount of open-session business time that produced it. Returns across a market closure are handled as gaps, not as ordinary same-session tick returns.

### Tick-grid observation

A tick grid is the set of prices the market can actually display. For example, an instrument with a tick size of `0.25` can trade at `100.00` and `100.25`, but not `100.13`.

Janiform's underlying simulated price can move by less than one displayed tick. The tick-grid setting controls how that price is converted back to a value that could appear in market data.

If you provide a tick size during fitting, Janiform uses it. Otherwise, Janiform may infer one from the smallest positive same-session price change in the training data. Small moves that do not yet reach a full tick are carried forward, so repeated small moves can eventually become a visible price change.

### Search restarts

A search restart is another starting point for calibration. Restarts are useful when the first starting point might lead to a fit that is locally reasonable but not well balanced.

Search restarts are built from the fit residuals Janiform can evaluate before the full simulation pass. Some restarts adjust return scale, some preserve the current activity model while changing return shape, and some combine return-shape changes with activity-level alignment. If the hand-built restart pool is too narrow, Janiform fills the frontier with local quasi-random points around the staged model.

The restart frontier is selected from low-cost screening losses before expensive restart optimization. The screening step keeps candidates that improve different weak areas, but admissibility is still decided from refreshed selection or audit-fidelity loss diagnostics.

Before filling the frontier with quasi-random seeds, Janiform builds sensitivity repair seeds from the best screened candidates. For each anchor candidate, it perturbs each active parameter inside the validated bounds, measures how active standardized residuals respond, solves a small damped least-squares repair step in normalized parameter space, and adds bounded trust-region versions of that step back into the same screening and diversity selection path. This is term-driven rather than asset-specific: whichever residuals are currently outside admissibility define the repair direction.

During restart search, Janiform optimizes the full active parameter vector with one bounded adaptive direct search per restart. The search state owns return-shape, activity, and coupling parameters together, so accepted updates are not forced through a fixed NIG, Hawkes, and coupling block order. Poll probes use coordinate positive and negative directions, which form a positive spanning set for direct search. Coupled repair directions are produced by restart sensitivity seeds before restart optimization. Exact tick return likelihood is evaluated on a deterministic weighted representative sample so each optimizer probe remains cheap while preserving support across return sign, return size, and business-time regimes. Coarse interval probes also include a reduced deterministic tick-shape proxy from the NIG return-composition sampler, so the optimizer still sees tick-level shape pressure before the refreshed seeded-tick evaluations. Refreshed local/final evaluations use full-fidelity tick likelihood before a candidate can be selected or accepted as admissible.

The Hawkes search coordinate is a stationary event-rate coordinate rather than a raw `lambda_bar` coordinate. Janiform solves `lambda_bar` from the current excitation matrix and the requested stationary open-event rate, which lets the search change clustering and decay without accidentally changing total activity on every single-coordinate probe.

During restart optimization, terminal progress reports the shared pseudo-likelihood loss used by the search. `Trial loss` is the most recent full-vector probe, `Accepted loss` is the best accepted loss for the active restart, and `Final loss` is the completed restart loss when the restart has reached a final evaluation. Smaller loss is better.

The restart table may also show exact return-likelihood evaluator counters. `CF-Buy` is the number of return blocks evaluated by the closed-form single-side buy density, `CF-Sell` is the number evaluated by the closed-form single-side sell density, and `Mix` is the number evaluated by the tilted mixed buy/sell inversion path. High `CF-Buy` or `CF-Sell` means the return likelihood mostly used the cheaper closed-form kernels. High `Mix` means more blocks required the more expensive mixed-side tilted inversion path.

The report may mention these restarts because they explain how Janiform searched. You usually do not need to tune them when reading a normal fit report.

### Option refinement progress

Option refinement uses option quote residual scores, not the restart optimization pseudo-likelihood loss. The option score is the maturity-weighted root-mean-square of option quote residuals after each residual has been scaled by its quote spread. A score near `1.0` means the model is typically about one quoted spread away from the observed mid-price on the tier being evaluated. Smaller is better.

The option search tier first prices an exact sensitivity baseline, then validates pricing-block proposals with search-tier operator approximation. The default search budget is deterministic: one exact baseline evaluation plus one proposal evaluation per active pricing-block search coordinate. If `joint_refinement_option_refinement_wall_clock_budget_millis` is provided, it acts as an explicit wall-clock guard around that deterministic search work. Selection and audit tier pricing remain exact.

In option progress tables, `Cur. loss` is the latest option residual score known for the running candidate. During a pricing call this can be the score from the previous completed evaluation for that candidate, because the current pricing call has not produced a score yet. `Best loss` is the best option score seen inside the active option phase. `Final loss` is the option score recorded when that candidate finishes the phase. These columns are not numerically comparable with restart `Trial loss`, `Accepted loss`, or `Final loss`, because those restart columns come from the joint underlying-data objective.

The `Interval` column reports option-pricing interval progress. When row-level pricing detail is available, `Interval` shows the current interval position and `Rows` shows row progress inside that interval. When row detail is not available, `Interval` shows the completed interval count.

### Runtime profile

Joint refinement reports `runtime_profile` in successful and failed stage reports. The wall-clock fields separate restart optimization, coarse consistency checks, final seed evaluation, and final assembly.

The cumulative fields split objective evaluation cost into deterministic work, likelihood-context construction, tick/5m/15m likelihood evaluation, expected activity counts, deterministic activity shape/level checks, seed-summary simulation, raw seed metric extraction, loss assembly, and option pricing. The cache counters show whether repeated evaluations reused objective, deterministic, seed-summary, aggregated-likelihood-context, or option-pricing artifacts.

Use `cumulative_deterministic_evaluation_millis` as the broad total for deterministic objective work, then use the narrower cumulative fields to identify which deterministic substep dominates. Millisecond counters can remain zero for very small samples or very cheap substeps.

Restart optimization also records `restart_evaluation_timing_diagnostics` for completed probes. These diagnostics include restart id, scheduling category, poll direction, elapsed milliseconds, objective value, admissibility blocker count, standardized residual summary, and exact likelihood evaluator counters. The same completed-probe diagnostics are streamed into `events.jsonl` as `metric_update` events during the run, so long restart searches leave a durable timing trail before the final report is assembled.

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
