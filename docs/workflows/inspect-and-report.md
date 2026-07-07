# Inspect and report

Inspection and reporting commands read artifacts that Janiform has already written. Inspection answers status and artifact questions. Reporting summarizes outcomes.

## Inspect a run

`inspect run` reads a run directory and summarizes status, stages, output paths, checkpoints, warnings, and progress-log health.

```bash
janiform inspect run .janiform/runs/eurusd-fit
```

Add event metadata when you need to check progress-log and output details:

```bash
janiform inspect run .janiform/runs/eurusd-fit --include-events
```

For automation, use JSON output:

```bash
janiform --json inspect run .janiform/runs/eurusd-fit
```

The command reads the latest status snapshot first. If that snapshot is missing or stale, it falls back to replaying the run event log.

## Inspect a model

`inspect model` reads a saved model artifact and summarizes model identity, provenance, fitted components, capabilities, and compact parameter information.

```bash
janiform inspect model .janiform/runs/eurusd-fit/artifacts/model.jf
```

Include fuller fitted-parameter summaries with `--parameters`:

```bash
janiform inspect model .janiform/runs/eurusd-fit/artifacts/model.jf --parameters
```

This shows the lineage and model parameters of the saved model.

## Report a fit

`report fit` renders a fit outcome from either a run directory or a fit report file.

```bash
janiform report fit .janiform/runs/eurusd-fit
```

The command also accepts the report artifact directly:

```bash
janiform report fit .janiform/runs/eurusd-fit/artifacts/report.json
```

JSON output works well when another process should read the summary:

```bash
janiform --json report fit .janiform/runs/eurusd-fit
```

Write an HTML report with `--html`:

```bash
janiform report fit .janiform/runs/eurusd-fit --html
```

Choose an explicit HTML path with `--output-path`:

```bash
janiform report fit .janiform/runs/eurusd-fit \
  --html \
  --output-path .janiform/runs/eurusd-fit/artifacts/report.html
```

## Common paths

For a fit run named `eurusd-fit`, the usual artifacts are:

```text
.janiform/runs/eurusd-fit/
.janiform/runs/eurusd-fit/artifacts/model.jf
.janiform/runs/eurusd-fit/artifacts/report.json
```

## Troubleshooting

**The run cannot be inspected**

Check that the path points to the run directory, not directly to `model.jf` or `report.json`.

**The fit report cannot be found**

Pass either the run directory or the report file. A run directory is expected to contain `artifacts/report.json`.

**The model parameter summary is incomplete**

`inspect model --parameters` includes fuller fitted-parameter summaries.

**The HTML report path is not what you expected**

When `--html` is used without `--output-path`, Janiform writes beside the fit report with an `.html` extension.

## See also

- [Fit models](fit-models.md)
- [Simulate paths](simulate-paths.md)
- [Price derivatives](price-derivatives.md)
- [Model validation](model-validation.md)
- [Calibration diagnostics](calibration-diagnostics.md)
- [Command line interface](../interfaces/cli.md)
- [CLI reference](../reference/cli.md)
