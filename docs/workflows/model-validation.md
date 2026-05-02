# Model validation

Model validation is the evidence-gathering process that determines whether a fitted model is acceptable for the next workflow (usually simulation or pricing).

## Stylized facts validation

A stylized fact is a simplified presentation of an empirical statistical property that aims to summarize one aspect of the behavior of the data. When validating a fitted model, comparing stylized facts between the training dataset and simulated data can help validate if the fitted model is able to reproduce important empirical properties correctly.

```bash
janiform validate stylized-facts --help
```

When executing a stylized fact validation run, Janiform does three things:

1. It turns empirical tick data into interval return observations.
2. It generates seeded simulated paths from the fitted model over the same analysis window.
3. It compares empirical and simulated metrics and write a validation report.

The output of that report is not a proof whether the model is *correct* or not. It is merely an auditable acceptance check. The report says which configured metric checks pass, fail, or do not have enough data.

You can run a stylized facts validation run either from scratch by passing it a fitted model and empirical tick data, or you can provide it with empirical and simulated metric observations as a JSON comparison request, if you already have that.

## Inputs

| Input                 | Required | Notes                                                                                                                                     |
|-----------------------|----------|-------------------------------------------------------------------------------------------------------------------------------------------|
| Fitted model          | Yes      | The usual CLI input is the `model.jf` file produced by `janiform fit`.                                                                    |
| Empirical tick data   | Usually  | Can be omitted when the saved model records a usable original tick-data source.                                                           |
| Return intervals      | Yes      | Comma-separated duration labels such as `5min,1h,1D`.                                                                                     |
| Metric catalog        | No       | Defaults to the built-in stylized fact catalog. Pass a catalog JSON file when the validation policy is owned outside the default catalog. |
| Threshold policy      | No       | Pass an explicit policy JSON file when acceptance thresholds must be controlled directly.                                                 |
| Simulation path count | No       | More paths give a larger simulated comparison set and cost more runtime.                                                                  |
| Seed                  | No       | Controls deterministic seeded simulation for repeatable validation runs.                                                                  |

Intervals use exact UTC durations. Supported units include nanoseconds, microseconds, milliseconds, seconds, minutes, hours, and days, with common pandas-like labels such as `15min`, `1h`, and `1D`.

For repeatable work, make the model path, empirical data path, intervals, seed, output directory, and decision policy explicit.

## CLI preflight checks

Before executing a validation run, use `plan`. It resolves model, empirical data, interval, simulation, output, cache, and metric settings without running simulation or metric computation.

```bash
janiform validate stylized-facts plan \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h,1D \
  --simulation-path-count 4 \
  --seed 123
```

JSON mode works well when automation needs the resolved plan:

```bash
janiform --json validate stylized-facts plan \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h \
  --simulation-path-count 4 \
  --seed 123
```

The plan includes the resolved output directory, generated per-path seeds, interval durations, empirical input columns, cache artifacts, report artifacts, execution mode, and decision mode.

## CLI

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h,1D \
  --simulation-path-count 4 \
  --seed 123
```

If `--output-dir` is omitted and the model path is:

```text
.janiform/runs/<run-id>/artifacts/model.jf
```

the validation directory is:

```text
.janiform/runs/<run-id>/artifacts/validation/stylized-facts
```

The main report artifacts are written to:

```text
<validation-dir>/stylized_facts.report_bundle.json
<validation-dir>/stylized_facts.table_summary.json
<validation-dir>/stylized_facts.plot_payload.json
<validation-dir>/lineage.json
```

`--output-dir` places validation artifacts somewhere else:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h,1D \
  --simulation-path-count 4 \
  --seed 123 \
  --output-dir .janiform/runs/eurusd-fit/validation/stylized-facts
```

`--fail-on-failed-checks` makes failed metric checks produce a nonzero process exit, which is useful in CI and scheduled validation jobs:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h,1D \
  --simulation-path-count 4 \
  --seed 123 \
  --fail-on-failed-checks
```

Without `--fail-on-failed-checks`, a validation run can complete successfully while the report contains failed checks. With the flag, failed checks return exit code `1`.

## Python

Coming soon.

## Outputs

The report bundle is the primary output. The table and plot payloads exist so downstream tools can render validation results without re-parsing the full report structure.

| Output               | Path or object                              | Notes                                                                        |
|----------------------|---------------------------------------------|------------------------------------------------------------------------------|
| Report bundle        | `stylized_facts.report_bundle.json`         | Full validation report, summary counts, metric checks, and artifact receipt. |
| Table summary        | `stylized_facts.table_summary.json`         | Flattened rows for review tables and dashboards.                             |
| Plot payload         | `stylized_facts.plot_payload.json`          | Plot-ready metric points with deltas, status, and coverage metadata.         |
| Lineage              | `lineage.json`                              | Resolved validation plan used for the run.                                   |
| Python report bundle | `StylizedFactHorizonComparisonReportBundle` | Returned by `run_stylized_fact_horizon_comparison_report_generator(...)`.    |

Janiform summarizes status, metric-check counts, failures by interval, failures by metric class, selected check rows, and artifact paths. `--show-checks failed`, `--show-checks closest`, and `--show-checks all` control which checks are printed. `--max-checks` limits the number of rows.

## Advanced configuration

### Holdout data

It is often smart to use out-of-sample validation, so we recommend you pass empirical data that was not used to fit the model:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/eurusd-holdout.parquet \
  --intervals 5min,1h,1D \
  --simulation-path-count 4 \
  --seed 123
```

The command treats the given empirical file as the validation window. Keep the fit sample, holdout sample, and decision policy visible in run metadata or surrounding pipeline records.

### Empirical columns

If Janiform cannot infer the empirical input layout, pass the column names:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/ticks.parquet \
  --empirical-timestamp-column timestamp_utc \
  --empirical-price-column price \
  --empirical-trade-sign-column trade_sign \
  --empirical-price-input-policy explicit-price-column \
  --empirical-timestamp-timezone UTC \
  --intervals 5min,1h
```

Bid/ask columns work with `--empirical-price-input-policy bid-ask-midpoint` when validation should derive the empirical price from quotes.

### Threshold and significance policy

By default, raw-data validation uses the built-in metric catalog and a generated bootstrap-based decision policy. Pass `--threshold-policy` when acceptance thresholds are governed by an explicit JSON policy:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5m,1h,1D \
  --threshold-policy config/stylized-facts-thresholds.json
```

`--significance-level` applies only when Janiform is generating the bootstrap decision policy. It conflicts with `--threshold-policy`.

> **Note:** Standardized delta is the absolute metric delta divided by the pooled confidence-interval half-width. It is a scale-normalized comparison against available confidence intervals, not a z-score or a statistical significance test.

### Caching

`--cache` keeps derived validation inputs:

```bash
janiform validate stylized-facts \
  --model-path .janiform/runs/eurusd-fit/artifacts/model.jf \
  --empirical-data-path data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --intervals 5min,1h \
  --cache
```

`--cache` and `--cache=derived` write:

```text
<validation-dir>/comparison_request.json
<validation-dir>/simulation_manifest.json
<validation-dir>/metric_observations.json
```

`--cache=full` also retains the generated seeded tick paths:

```text
<validation-dir>/simulation_paths.json
```

Full caching can produce larger artifacts because it keeps generated path details.

### Precomputed comparison mode

`--comparison-request` skips directly to comparison and report generation when empirical and simulated metrics have already been computed:

```bash
janiform validate stylized-facts \
  --comparison-request .janiform/validation/comparison_request.json \
  --metric-catalog config/stylized-fact-metric-catalog.json \
  --output-dir .janiform/validation/precomputed-stylized-facts
```

In this mode Janiform skips empirical loading, simulation, and metric computation. Do not pass raw-data inputs such as `--model-path`, `--empirical-data-path`, or `--intervals` with `--comparison-request`.

## Reading the result

Validation reports should be reviewed as evidence, not as a single pass/fail badge.

| Result          | Meaning                                                                 | Typical action                                                                                     |
|-----------------|-------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------|
| Pass            | The metric is within the configured decision policy.                    | Keep the report with the model acceptance record.                                                  |
| Fail            | The empirical and simulated metric differ beyond the configured policy. | Inspect the interval, metric class, data window, and fit diagnostics before using the model.       |
| Not enough data | The metric could not be decided under the sample-sufficiency policy.    | Extend the validation window, change the interval set, or mark the coverage limitation explicitly. |

Compare failures across intervals and metric classes. A short-horizon tail failure, a long-horizon dispersion failure, and broad insufficient-data coverage point to different model or data problems.

## Troubleshooting

**The command says `--model-path` is missing**

Raw-data validation needs a fitted model. Pass the `model.jf` path from the fit run, or use precomputed comparison mode with `--comparison-request`.

**The command says `--empirical-data-path` is missing**

The model did not contain a usable original tick-data source. Pass `--empirical-data-path` explicitly.

**Columns are not recognized**

Pass explicit empirical column mappings. Check timestamp, price, bid, ask, trade-sign, price policy, and timestamp timezone before changing validation policy.

**Interval parsing fails**

Exact duration labels such as `5min`, `15min`, `1h`, or `1D` are required. Intervals must be positive, unique, and exactly representable as UTC nanoseconds.

**A validation run takes a long time**

Raw-data validation simulates seeded paths and computes metrics. `validate stylized-facts plan` helps before a long run starts, `--simulation-path-count` should stay tied to the validation policy, and `--cache` keeps intermediate artifacts available after the run.

**The process exits successfully even though checks failed**

This is expected unless `--fail-on-failed-checks` is set. The report records failed checks either way.

## See also

- [Fit models](fit-models.md)
- [Simulate paths](simulate-paths.md)
- [Price derivatives](price-derivatives.md)
- [Inspect and report](inspect-and-report.md)
- [Concepts](../getting-started/concepts.md)
- [Command line interface](../interfaces/cli.md)
- [Python library](../interfaces/python.md)
- [CLI reference](../reference/cli.md)
- [Python API reference](../reference/python-api.md)
