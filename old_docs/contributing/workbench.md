# Calibration Workbench

The calibration workbench is an internal contributor tool for inspecting and iterating on Janiform calibration runs. It is not part of the public user-facing API.

## What It Is For

Use the workbench when you want a visual way to:

- launch a high-level calibration run from a dataset
- open an existing run directory or checked-in fit artifact
- inspect candidate-frontier and selection-rationale diagnostics
- compare observed data against simulated output
- review runtime and coarse-versus-tick consistency diagnostics
- reevaluate edited parameter sets in the lab tab without manually wiring every artifact

## Launch

With the repo environment active, start it from the repository root:

```bash
python -m tools.workbench
```

You can also open an existing artifact directly:

```bash
python -m tools.workbench path/to/run-directory
python -m tools.workbench path/to/fit_report.json
```

The launcher accepts a run directory, `fit_report.json`, staged-response JSON, or a diagnostics JSON file.

## Main Workflows

### Start a new run

Use the setup pane to choose a dataset, infer the input columns, choose the workflow scope, and select the session source before pressing `Calibrate`.

The workbench uses the same high-level fit path as the rest of the repo. For normal calibration scopes it defaults to endogenous mean reversion, and for `Seasonality Only` it disables anchor-family selection.

### Open an existing run

Use `Open Existing Run` when you already have a `.janiform` run directory or a persisted fit artifact. The workbench loads the checked-in diagnostic artifacts and summary payloads from that run so you can inspect them without rerunning calibration.

### Inspect diagnostics

The workbench surfaces the diagnostic artifacts that calibration already emits, including:

- observed targets
- candidate frontier
- selection rationale
- coarse-tick consistency
- simulation summary
- search-surface manifest
- runtime profile

Use it when you need to understand why a candidate won, where the search spent time, or how coarse and fine diagnostics compare.

### Reevaluate edited parameters

The lab tab lets you edit fitted parameters, rerun selected diagnostics, and compare the result against the currently loaded model. This is useful when you are investigating calibration behavior or checking how sensitive a result is to a parameter change before touching the main runtime code.

## Inputs And Outputs

The default output directory for new runs is `.janiform/` under the repository root. The workbench reads the same fit artifacts and diagnostics that the normal high-level workflow writes, so it fits best into contributor investigation rather than as a separate product surface.

## Scope

Treat the workbench as internal tooling. It exists to help contributors understand calibration behavior and diagnostics more quickly, not as a stable end-user interface.
