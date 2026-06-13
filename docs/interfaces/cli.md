# Command Line Interface

The CLI is how we interact with Janiform via a terminal or automation system.

The CLI is built around explicit, durable workflows that remain easy to audit after the process exits. Saved artifacts and process-friendly output modes are central to how it works.

## Command families

The top-level commands are organized by task:

| Command                 | Purpose                                                               |
|-------------------------|-----------------------------------------------------------------------|
| `fit`                   | Model fitting workflows.                                              |
| `inspect`               | Inspection workflows for saved Janiform artifacts.                    |
| `report`                | Reporting workflows for saved Janiform outputs.                       |
| `simulate`              | Simulation workflows.                                                 |
| `price`                 | Derivative pricing workflows.                                         |
| `validate`              | Model validation workflows.                                           |
| `presets`               | Preset workflows.                                                     |
| `seasonality`           | Seasonality workflows.                                                |
| `interval-factor-grids` | Workflows for preparing hourly seasonality and news-intensity inputs. |
| `dev`                   | Developer-oriented workflows.                                         |

The main workflow starts with `fit`, then moves outward:

1. `janiform fit` creates the fitted model and run directory.
2. `janiform inspect run` and `janiform report fit` explain what happened.
3. `janiform simulate`, `janiform price`, and `janiform validate` use the saved model.

## Output modes

The CLI separates final command output from streaming diagnostics.

### Final stdout mode

The global output styles are:

| Style    | How to enable it | Best for                                                                                |
|----------|------------------|-----------------------------------------------------------------------------------------|
| Terminal | default          | Default workflow: you are running interactively and want summaries plus live progress.  |
| Quiet    | `--quiet`        | You want a concise terminal record, and you only require final status and output paths. |
| Silent   | `--silent`       | Another process only cares about exit status and failures.                              |
| JSON     | `--json`         | Another process needs one final machine-readable completion object on stdout.           |

### Streaming diagnostics and events

The event stream is separate from the final stdout mode:

| Option           | Meaning                                              |
|------------------|------------------------------------------------------|
| `--events jsonl` | Write one JSON event per line to stderr.             |
| `--trace-events` | Include trace-level event records in that stream.    |
| `-v` or `-vv`    | Increase terminal diagnostic verbosity.              |

This split is important for automation. `--json` keeps stdout to one final JSON object, while `--events jsonl` still gives a live machine-readable progress stream on stderr.

## Working with saved runs

The CLI is designed around saved artifacts.

### Fit runs

A disk-backed fit writes a run directory, usually under:

```text
.janiform/runs/<run-id>/
```

The standard fit artifacts are:

```text
.janiform/runs/<run-id>/artifacts/model.jf
.janiform/runs/<run-id>/artifacts/report.json
.janiform/runs/<run-id>/events.jsonl
.janiform/runs/<run-id>/latest-state.json
.janiform/runs/<run-id>/checkpoints/
```

`model.jf` is the saved model used by later commands. `report.json` is the durable fit report. `events.jsonl` and `latest-state.json` support monitoring and run inspection while a fit is still in progress.

### Downstream commands

Most downstream workflows start from the saved model path:

```bash
janiform inspect model .janiform/runs/eurusd-fit/artifacts/model.jf
janiform simulate --model-path .janiform/runs/eurusd-fit/artifacts/model.jf --horizon-interval-count 24
janiform price --model-path .janiform/runs/eurusd-fit/artifacts/model.jf --valuation-time-utc 2026-02-02T15:00:00Z --spot 1.085 --strike 1.09 --tenor 1h
```

`inspect run` and `report fit` work from the run directory instead:

```bash
janiform inspect run .janiform/runs/eurusd-fit
janiform report fit .janiform/runs/eurusd-fit
```

## Configuration

The CLI supports one global configuration file through `--config`. That file is a TOML document for CLI defaults.

For configuration structure, supported fields, and how it relates to runtime options and paths, see [Configuration](configuration.md).

## Automation

The CLI is built for automation centered on durable run artifacts and machine-readable process output.

Typical patterns:

### One final machine-readable result

```bash
janiform --json fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit
```

### Live machine events plus a final JSON result

```bash
janiform --json --events jsonl fit \
  data/Synthetic_EURUSD_Data_1M_ticks.parquet \
  --run-id eurusd-fit
```

### Read-only monitoring of a disk run

```bash
janiform --json inspect run .janiform/runs/eurusd-fit
```

The Python library covers in-memory objects, notebook integration, application callbacks, and Python-native analysis code around the same engine.

## See also

- [Python library](python.md)
- [Configuration](configuration.md)
- [External integration](external-integration.md)
- [Fit models](../workflows/fit-models.md)
- [Inspect and report](../workflows/inspect-and-report.md)
- [CLI reference](../reference/cli.md)
