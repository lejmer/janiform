# Calibration diagnostics

Calibration diagnostics help you understand a fitted model after calibration finishes.

Use this page when you have a `JaniformFitReport`, a `JaniformFitResult`, or a saved fit report file and want to answer practical questions:

- Did Janiform reproduce the main behavior in the training data?
- Which parts of the fit look weak?
- Does a high score point to activity, return size, return shape, or something else?
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

| Report section        | What it answers                                               |
|-----------------------|---------------------------------------------------------------|
| `objective_breakdown` | Which broad parts of the fit were strong or weak?             |
| `objective_terms`     | What observed and simulated measurements caused those scores? |

Start with `objective_breakdown`. If one area looks bad, use `objective_terms` to see the underlying observed and simulated numbers.

## Terms used on this page

Janiform reports some terms that are useful but easy to misread.

| Term             | Meaning                                                                                                      |
|------------------|--------------------------------------------------------------------------------------------------------------|
| Candidate        | One complete fitted model that Janiform considered during calibration.                                       |
| Baseline         | The starting fit used as a comparison point before final improvement attempts.                               |
| Objective        | A numeric score used during fitting. Lower is better. It helps Janiform choose between candidates.           |
| Diagnostic       | A measurement that explains what happened. It may or may not be used directly for final model selection.     |
| Bucket           | A named group of related fit checks, such as activity level or return scale.                                 |
| Activity         | How often market events arrive. In tick data, high activity means ticks arrive frequently.                   |
| Return scale     | The typical size of price changes. It is related to volatility, but reported here as fitted return behavior. |
| Return shape     | The distributional shape of returns, including tails and how returns are spread out.                         |
| Asymmetry        | Whether positive and negative returns behave differently.                                                    |
| Normalized score | A score divided by a comparison scale so that different measurements can be compared.                        |

The objective is not a probability, percentile, p-value, or percentage score. It is a calibration score.

## How to read scores

For most objective fields, lower is better.

A score near `0.0` means the simulated behavior is close to the training data for that part of the fit. A score near `1.0` means the mismatch is about one normal comparison scale for that diagnostic. Scores above `1.0` are allowed. They mean the mismatch is larger than that scale.

Do not read `2.0` as "200 percent wrong" or "two standard deviations." It means that Janiform's normalized distance for that diagnostic is twice the comparison scale used for the objective.

Also avoid reading one number by itself. A useful fit is usually balanced: activity, return size, return shape, and price-path sanity should all be plausible for the intended workflow.

## Broad fit areas

`objective_breakdown` groups the fit into broad areas. The field names are technical because they are saved in the report, but each one maps to a plain question.

| Bucket                           | Plain meaning                                                                          |
|----------------------------------|----------------------------------------------------------------------------------------|
| `activity_level_fit`             | Does the simulated market produce about the right amount of activity?                  |
| `activity_shape_fit`             | Does activity rise and fall over time in a similar pattern to the training data?       |
| `aggregated_return_scale_fit`    | Are interval-level price changes about the right size?                                 |
| `tick_return_scale_fit`          | Are tick-level price changes about the right size?                                     |
| `business_time_return_scale_fit` | Does return size change reasonably between quiet and active periods?                   |
| `aggregated_return_shape_fit`    | Do simulated interval returns have a similar distributional shape?                     |
| `return_asymmetry_fit`           | Do positive and negative interval returns have similar imbalance to the training data? |
| `tick_return_asymmetry_fit`      | Do positive and negative tick returns have similar imbalance to the training data?     |
| `intraday_return_level_fit`      | Does return size vary across the day in a similar way?                                 |
| `rv_activity_coupling_fit`       | Does higher activity tend to come with higher price variation in the same way?         |
| `aggregated_likelihood_fit`      | Does the fitted return distribution explain the observed returns reasonably well?      |
| `price_span_sanity_fit`          | Do simulated paths cover a plausible total price range?                                |

`weighted_objective` is the single summary score Janiform can use to compare candidates. It is useful for ranking candidate fits, but it is not a standalone quality grade. A model with a slightly worse summary score may still be easier to trust if its weak areas are less important for your use case.

`hard_constraint_penalty` means a candidate crossed a rule that Janiform treats as a serious fit problem. A constraint is a required condition for a usable fit, not just another soft preference. If this value is nonzero, inspect the weak buckets before trusting the model.

## Detailed measurements

`objective_terms` shows the observed-versus-simulated measurements behind the broad scorecard.

Each term has:

| Field               | Meaning                                                               |
|---------------------|-----------------------------------------------------------------------|
| `metric_id`         | The report name for this measurement.                                 |
| `target_family`     | The broader behavior it belongs to, such as activity or return scale. |
| `observed_value`    | The value measured in the training data.                              |
| `simulated_value`   | The value measured in simulations from the fitted model.              |
| `uncertainty_scale` | The scale Janiform used to compare observed and simulated values.     |
| `normalized_error`  | The reported mismatch after scaling. Lower is better.                 |

Use these terms when a broad area looks weak.

For example:

- If `activity_level_fit` is high, compare observed and simulated activity terms.
- If `tick_return_scale_fit` is high, check whether simulated tick returns are too large or too small.
- If `rv_activity_coupling_fit` is high, check whether activity and price variation move together differently in simulation than in the training data.

`objective_terms` explain the score, but they are not the whole model-selection rule. Use them for interpretation, not for manually recomputing the final objective.

## What a high score usually means

A high score means the fitted model is not reproducing that behavior well enough under the calibration comparison.

Common patterns:

| High area                    | What to inspect                                                                                            |
|------------------------------|------------------------------------------------------------------------------------------------------------|
| Activity level               | The market may be simulated too frequently or too rarely.                                                  |
| Activity shape               | The total activity may be acceptable, but the timing pattern may be wrong.                                 |
| Return scale                 | Price changes may be too large or too small.                                                               |
| Return shape                 | The fitted model may miss tails, concentration near zero, or other distribution features.                  |
| Return asymmetry             | The model may miss directional imbalance between positive and negative moves.                              |
| Activity-volatility coupling | Active periods may not produce the right amount of price variation. Volatility means how much prices vary. |
| Price span sanity            | Simulated paths may travel too far or not far enough over the training-window horizon.                     |

A high score is not automatically a bug. It is a pointer. The right response is to inspect the related terms, render the fit report, simulate paths, and run validation for the intended use case.

## Why Janiform tries multiple candidates

During fitting, Janiform tries more than one candidate model. A candidate is one complete set of fitted parameters. This is necessary because high-frequency market models can fit one behavior well while harming another.

For example, one candidate might match activity very well but make returns too large. Another might match return size but lose the daily activity pattern. The selected model should be a balanced fit, not just the candidate with one excellent diagnostic.

Janiform therefore considers both the summary score and the shape of the scorecard. This helps avoid selecting a model that looks good only because one number hides an unacceptable weakness elsewhere.

## Saved runtime settings

Some fields near the diagnostics are not scores. They are fitted settings that Janiform saves because simulation and pricing need them later.

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

### Return-scale search restarts

A search restart is another starting point for calibration. Restarts are useful when the first starting point might lead to a fit that is locally reasonable but not well balanced.

Return-scale search restarts are starting points that adjust the typical size of returns before trying the final fit again. They are not diagnostic scores. They only help Janiform search for a better balanced model.

The report may mention these restarts because they explain how Janiform searched. You usually do not need to tune them when reading a normal fit report.

## A practical reading order

When reading a fit report, read it in this order:

1. Check whether the fit completed successfully.
2. Look at `objective_breakdown` for the largest weak areas.
3. Use `objective_terms` to compare observed and simulated values for those areas.
4. Inspect saved runtime settings only if simulation or pricing behavior looks surprising.
5. Run model validation before relying on the model for a downstream workflow.

The most important habit is to connect diagnostics to your use case. A simulation workflow may care most about activity, return scale, and path realism. An option-pricing workflow may also care deeply about tails, asymmetry, and activity-volatility coupling.

## See also

- [Fit models](fit-models.md)
- [Inspect and report](inspect-and-report.md)
- [Model validation](model-validation.md)
- [Simulate paths](simulate-paths.md)
