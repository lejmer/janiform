# CLI Reference

This is a curated reference for the public CLI surface. Use `janiform <command> --help` for the complete live help text.

## Top-Level Commands

| Command | Purpose |
| --- | --- |
| `janiform fit` | Fit a Janiform model from market data |
| `janiform simulate` | Simulate from a persisted model |
| `janiform price` | Price a vanilla option from a persisted model |
| `janiform presets` | Inspect bundled and user-registered presets |
| `janiform seasonality` | Seasonality preprocessing workflows |
| `janiform hourly-factors` | Hourly factor materialization and contract-building workflows |
| `janiform stylized-facts` | Stylized-fact reporting workflows |
| `janiform dev regression deterministic` | Developer deterministic regression harness |

## High-Level Workflow Selection

| Need | Command |
| --- | --- |
| Validate and derive a fit plan | `janiform fit plan` |
| Run the prepared staged calibration boundary | `janiform fit staged` |
| Run the default seeded simulation workflow | `janiform simulate` |
| Run the explicit coarse path | `janiform simulate coarse-interval` |
| Run the explicit seeded path | `janiform simulate seeded-ticks` |

## Common Flags

| Area | Representative flags |
| --- | --- |
| Output | `--json`, `--silent`, `--quiet` |
| Diagnostics | `--color {auto,always,never}`, `--verbose`, `--config` |
| Events | `--events jsonl`, `--trace-events` |
| Persistence | `--output-dir`, `--output-path`, `--disk-checkpoint-directory` |
| Data inputs | `--tick-data-path`, `--known-news-path`, `--option-surface-path`, `--calendar-registry-json` |
| Workflow shape | `--workflow-scope`, `--execution-mode` |
| Calibration search | `--dynamics-search-restart-candidate-budget` |

## Shared CLI Boundary

Root-level options are available to every command.

| Option | Contract |
| --- | --- |
| `--json` | Writes one final JSON object on stdout and keeps human progress off stdout. |
| `--silent` | Suppresses normal output while preserving warnings and errors. |
| `--quiet` | Limits human output to final status and artifact paths. |
| `--events jsonl` | Writes machine events to stderr as JSONL. |
| `--trace-events` | Enables trace-level records and requires an event stream. |
| `--color auto\|always\|never` | Controls ANSI styling for human diagnostics. Automatic color respects `NO_COLOR` and terminal capability. JSON and JSONL outputs do not contain ANSI styling. |
| `--config PATH` | Loads TOML configuration for CLI boundary values only, such as output and diagnostic defaults. Model behavior, numerical policy, and workflow-specific calibration defaults remain in typed Rust contracts. |

`--json`, `--silent`, and `--quiet` are mutually exclusive where their contracts would conflict.

## Notes

- `fit plan` defaults to an automation-friendly disk-oriented execution policy.
- `fit` chooses the fit from the supplied inputs, so adding `--option-surface-path` fits the model
  to the underlying data *and* the selected option snapshot.
- `fit staged` is the explicit orchestration boundary, not the recommended starting point.
- `simulate` defaults to the seeded-tick path unless an explicit interval workflow is chosen.
- `price` derives its pricing state from the persisted model, the valuation timestamp, and the supplied spot.
