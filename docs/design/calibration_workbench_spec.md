# Calibration Workbench Spec

## Status

This document defines the target product for the Janiform calibration workbench.

It replaces the weaker interpretation of the workbench as a passive diagnostics viewer.
The workbench is the GUI replacement for the calibration CLI and the primary tool for
understanding whether the calibrator is behaving correctly.

It is intentionally ambitious. If a feature is required here, the expectation is that the
implementation should eventually support it natively rather than through developer-only
backdoors or offline JSON inspection.

## Product Definition

The calibration workbench is an interactive calibrator studio.

It must let a user:

- load a dataset
- review or override inferred inputs
- start calibration with one primary action
- watch calibration progress live
- inspect every explored candidate and its parameter values
- inspect why candidates were accepted, rejected, or selected
- compare training data and simulated data from many angles
- manually tweak parameters and reevaluate
- inspect whether the search ever explored the correct parameter region
- export machine-readable diagnostics for downstream tools and LLMs

The workbench has no value if it cannot help improve the calibrator itself.

## Purpose

The workbench exists to answer two classes of questions.

### 1. Fit-quality questions

- Is the selected model fit good enough?
- In what ways does simulated data fail to match training data?
- Are the failures localized to activity, returns, tails, coupling, event conditioning, or cross-tier consistency?
- Is the selected candidate genuinely good or merely the least bad one under the current objective?

### 2. Calibrator-quality questions

- Did the search explore the right parameter regions?
- Did the calibrator ever evaluate a near-correct candidate?
- If not, did it fail because of geometry, admissibility, objective design, simulator noise, or a model limitation?
- Did the search stagnate, collapse too early, or ignore important dimensions?
- Are coarse and tick evaluation tiers aligned enough to trust the search strategy?

If the workbench cannot answer both classes of questions, it is not complete.

## Non-Negotiable Design Rules

1. The workbench is a GUI replacement for the CLI, not merely a post-run report viewer.

2. Starting calibration must be easy.
   A standard workflow should be as simple as selecting a dataset and pressing a primary
   `Calibrate` button after reviewing inferred inputs.

3. The user must never be left guessing what the system is doing.
   Calibration, reevaluation, and plot generation must expose structured live status.

4. The workbench must expose the internals of the calibrator, not only the final answer.
   Every explored candidate, important search surface, rejection reason, and selected
   diagnostic must be inspectable.

5. Training data versus simulated data analysis is a first-class requirement.
   The workbench must make it easy to compare TD and SD from many angles and across
   fidelity tiers.

6. Parameter values must be visible.
   The selected candidate and every explored candidate must expose their parameter vectors
   in an inspectable form.

7. The workbench must support manual parameter editing and reevaluation.
   This is required for diagnosis and not an optional future convenience.

8. The GUI layer must remain a presentation and orchestration boundary.
   Core calibration logic, evaluation logic, target estimation, and simulation stay in the
   shared calibrator core and service layer.

9. The workbench must live under `tools/workbench/`.

10. The implementation must be modular.
    A monolithic GUI file is not acceptable.

## Primary User Modes

The application should expose five primary modes.

### 1. Setup

Used to configure and launch calibration.

### 2. Live Run

Used to monitor calibration while it is running.

### 3. Inspector

Used to inspect candidates, diagnostics, parameters, search behavior, and artifact outputs.

### 4. Compare

Used to compare TD versus SD, candidate versus candidate, and fitted versus edited candidates.

### 5. Lab

Used to edit parameters, rerun evaluation tiers, and test counterfactual parameter sets.

## Core Workflows

### 1. Fast start calibration

Required workflow:

1. select dataset
2. preview inferred timestamp, price, session, and normalization assumptions
3. optionally override any inference
4. press `Calibrate`
5. watch calibration live
6. inspect the result without leaving the workbench

This should be the default experience.

### 2. Live calibration inspection

The user must be able to inspect the current run while it is still executing, including:

- current stage
- current subtask
- current evaluation tier
- current candidate family
- current batch or neighborhood
- frontier updates
- candidate counts
- best-so-far candidate
- live plots and evolving diagnostics

### 3. Candidate inspection

The user must be able to:

- inspect the full parameter vector for any candidate
- inspect canonical coordinates when available
- inspect objective decomposition by target family
- inspect admissibility and rejection reasons
- inspect simulation summaries and cross-tier metrics

### 4. Search diagnosis

The user must be able to determine:

- whether the search explored the correct region
- whether the search collapsed too early
- whether important dimensions were ignored
- whether the selected candidate won for sensible reasons
- whether the objective or admissibility logic is pushing the search in the wrong direction

### 5. Manual parameter lab

The user must be able to:

1. clone a selected or historical candidate
2. edit parameter blocks or individual coordinates
3. rerun exact rescoring
4. rerun coarse diagnostics
5. optionally rerun seeded-tick diagnostics
6. compare the edited candidate to the fitted candidate and to TD

### 6. Synthetic truth diagnosis

When the dataset comes from a synthetic scenario with known parameters, the user must be able to:

- mark or load the ground-truth parameter set
- compare truth versus explored candidates
- see whether the search ever entered the truth neighborhood
- inspect whether the search rejected truth-like candidates and why

## Main Application Areas

The workbench should expose at least:

- run browser and history
- setup panel
- live progress and activity panel
- summary dashboard
- candidate browser
- parameter inspector
- parameter editor
- diagnostics inspector
- plot workspace
- compare workspace
- export actions

## Setup UX

### Minimum setup inputs

The setup panel must support:

- dataset path selection
- file format selection or inference
- timestamp column inference and override
- price-input-policy inference and override
- session and exchange selection or inference
- workflow scope
- normalization policy
- preset selection
- runtime budget and execution mode
- output directory selection

### Quick-start behavior

For standard use, the setup panel should default to a fast path:

- infer everything possible
- show the inferred values clearly
- highlight any low-confidence inference
- expose one primary `Calibrate` button

### Preflight validation

Before calibration begins, the setup view must show:

- dataset row count when cheaply available
- inferred columns
- inferred session assumptions
- inferred price policy
- warnings for suspicious assumptions
- expected output locations

## Live Calibration UX

## Principle

The user must continuously see meaningful evidence that calibration is active and what it is doing.

### Required live status fields

The GUI must expose:

- current calibration stage
- current stage subtask
- current evaluation tier
- current batch, neighborhood, or frontier operation
- current candidate family when meaningful
- current plot or diagnostic artifact being updated
- elapsed time
- estimated remaining time when estimable
- resource usage summary where available
- recent alerts and warnings

### Required progress models

Progress must be represented with structured progress objects, not only log text.

The GUI must include:

- overall run progress indicator
- stage-level progress indicator
- subprogress indicators when a stage has meaningful internal work units
- indeterminate progress mode when completion fraction is unknown

When exact completion is unknowable, the GUI must still show useful movement such as:

- completed batches out of scheduled batches
- completed candidate families out of total candidate families
- completed seed evaluations out of planned seed evaluations
- completed frontier reevaluations out of current frontier tasks
- current reconciliation round

### Run controls

The live run view should support:

- pause if supported by backend
- stop
- cancel
- checkpoint save
- open latest best candidate in inspector
- pin current best candidate

## Candidate Browser

The candidate browser must support:

- live population during calibration
- filtering by stage, admissibility, validity, evaluation tier, and rejection reason
- sorting by objective, lexicographic rank, family-level miss, or runtime
- selecting one candidate for inspection
- selecting multiple candidates for comparison

### Candidate row minimum fields

Each candidate row should expose at least:

- candidate id
- stage
- candidate family or source batch
- evaluation tier
- validity state
- admissibility state
- rejection reason if rejected
- scalar objective or tie-break values
- family-level diagnostics summary
- parameter snapshot reference
- evaluation timestamp

## Parameter Inspection and Editing

### Inspection requirements

For every selected candidate, the GUI must show:

- all parameter blocks
- all parameter names
- canonical coordinates when available
- values in a stable, readable layout
- optional baseline or truth values when available

### Editing requirements

The parameter editor must support:

- editing by logical block
- editing by canonical coordinate
- revert one parameter
- revert one block
- revert entire candidate
- load parameters from another candidate
- import a known parameter set
- reevaluate at chosen fidelity tiers

## Diagnostics Inspector

The diagnostics inspector must expose:

- objective decomposition by target family
- metric-level miss table
- admissibility state
- validity state
- rejection reasons
- coarse diagnostics
- tick diagnostics
- cross-tier consistency diagnostics
- runtime metadata
- alert and warning history

## Plot Workspace

Plots must be organized by family, not dumped into one long undifferentiated list.

The plot workspace should include at least the following families:

- run progress and runtime
- search and frontier
- parameter search coverage
- parameter versus objective
- TD versus SD returns
- TD versus SD activity
- TD versus SD coupling
- TD versus SD event-conditioned behavior
- coarse versus tick consistency
- candidate comparison

### Plot behavior requirements

Plots must support:

- live updates during calibration
- zoom and pan
- export of image and underlying plotted data
- selection of candidate, stage, tier, seed, horizon, and time window where relevant
- adaptive defaults based on data, not arbitrary fixed defaults

## Required TD Versus SD Diagnostics

The following are required minimum diagnostics. They are not exhaustive.

### Rolling coupling diagnostics

- TD rolling realized volatility versus rolling activity
- SD rolling realized volatility versus rolling activity

### Tick return histograms

- TD tick return histogram
- SD tick return histogram

Histogram bin count must be adjustable by the user.
The default must be data-adaptive and derived from the data distribution rather than hardcoded.

### Aggregated 15-minute return histograms

- TD aggregated tick return histogram at 15-minute intervals
- SD aggregated tick return histogram at 15-minute intervals
- SD coarse 15-minute return histogram

### Hourly profiles

- TD hourly activity profile aggregated into 24 hour bins
- SD hourly activity profile aggregated into 24 hour bins
- TD hourly absolute return profile aggregated into 24 hour bins
- SD hourly absolute return profile aggregated into 24 hour bins

These are profile plots, not per-hour timeseries.

### Autocorrelograms

- autocorrelogram with 50 lags for TD squared returns aggregated to 15-minute intervals
- autocorrelogram with 50 lags for SD squared tick returns aggregated to 15-minute intervals
- autocorrelogram with 50 lags for SD coarse 15-minute squared returns

### Summary statistics tables

- TD tick summary statistics
- TD aggregated 15-minute return summary statistics
- SD tick return summary statistics
- SD aggregated 15-minute tick-return summary statistics
- SD coarse 15-minute return summary statistics

### Additional required comparison plots

The workbench should also include:

- TD versus SD rebased price path
- TD versus SD return QQ plots
- TD versus SD empirical CDF overlays
- TD versus SD tail exceedance comparison
- TD versus SD realized variance distribution by horizon
- TD versus SD event count distribution by horizon
- TD versus SD mean absolute return by horizon
- TD versus SD rolling activity regime diagnostics
- TD versus SD open-to-close and close-to-open diagnostics where applicable

## Required Calibrator-Internal Diagnostics

The workbench must expose the internal behavior of the calibrator.

### Required search diagnostics

- searched parameter values versus objective score for each scalar parameter
- searched parameter values versus admissibility state for each scalar parameter
- pairwise parameter scatter for important parameter pairs
- parameter density or coverage summaries
- search surface plots for important coordinates and coordinate pairs
- frontier evolution over time
- best objective over time
- best admissible objective over time
- candidate counts by stage and family
- rejection reason counts over time

### Required candidate-internal diagnostics

- full parameter vector for each candidate
- family-level objective contribution
- metric-level contribution or miss table
- simulated-versus-observed metric scatter
- validity and admissibility annotations
- lineage or source-batch provenance when available

### Required search quality diagnostics

- stagnation indicators
- boundary-hitting indicators
- noisy-surface indicators
- flat-surface indicators
- truth-neighborhood coverage when truth is known
- whether the selected candidate is in a sparse or well-explored region

## Live Plot Updating

Live updating is required.

As calibration proceeds, the workbench must update:

- progress plots
- frontier plots
- search surfaces
- parameter-versus-objective plots
- candidate tables
- best-so-far diagnostics
- selected-candidate diagnostics

Updates should be incremental where possible rather than rebuilding the entire UI from scratch.

## Structured Event Model

The workbench requires a structured event stream from the calibrator backend.

### Required event categories

- run started
- run completed
- run failed
- stage started
- stage completed
- subtask started
- subtask completed
- batch started
- batch completed
- candidate proposed
- candidate evaluated
- candidate rejected
- candidate admitted to frontier
- frontier updated
- surface point recorded
- simulation summary recorded
- consistency metric recorded
- checkpoint emitted
- resource snapshot recorded
- alert emitted

### Required event fields

Each event should include, where applicable:

- timestamp
- run id
- stage
- subtask
- evaluation tier
- candidate id
- candidate family
- parameter payload or parameter reference
- objective summary
- admissibility state
- rejection reason
- progress counters
- resource usage snapshot

## Comparison Workspace

The compare workspace must support:

- candidate versus candidate
- candidate versus selected candidate
- candidate versus truth
- TD versus SD
- tick versus coarse SD
- original fitted candidate versus edited candidate

Comparison views should support side-by-side tables and overlaid plots.

## Exports

The workbench must support exporting:

- selected candidate parameters
- edited parameter sets
- reevaluation results
- selected plot data
- selected diagnostics tables
- candidate comparison bundles
- live run event traces

Exports must remain machine-readable and self-describing.

### LLM-friendly output rule

When exporting JSON, the workbench must preserve:

- metric names
- units
- target family grouping
- whether values are TD, SD, normalized, observed, simulated, or derived
- admissibility and ranking annotations
- stage and tier metadata
- parameter names and canonical coordinates

## Code Architecture

The workbench should be split into modules such as:

- application shell and app state
- setup and run-launch controllers
- calibration execution controller
- progress event adapter and models
- candidate store
- parameter inspector and editor panels
- diagnostics inspector panels
- plot manifest and plot controllers
- compare workspace
- reevaluation actions
- export actions

These names may vary, but this separation of concerns is required.

## Relationship To Calibrator Core

The calibrator core owns:

- target estimation
- parameter geometry
- solver logic
- evaluator logic
- simulation logic
- artifact generation
- event emission

The workbench owns:

- interactive setup
- interactive run orchestration
- live progress presentation
- live candidate presentation
- diagnostics presentation
- parameter editing and reevaluation orchestration
- export of user-selected views

## Definition Of Done

The workbench is not done until all of the following are true:

- a user can start calibration by loading a dataset and pressing a button
- calibration progress is inspectable while the run is executing
- every candidate's parameter set is inspectable
- searched parameter values versus objective are inspectable
- TD versus SD diagnostics are rich enough to diagnose fit quality
- the search path is visible enough to diagnose calibrator behavior
- edited candidates can be reevaluated from inside the workbench
- the user can determine whether failures come from search, objective, admissibility, simulator, or model mismatch

## Immediate Derived Specs

The following derived specs extend this document:

1. [calibration_workbench_state_model_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_state_model_spec.md)
2. [calibration_workbench_event_schema.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_event_schema.md)
3. [calibration_workbench_run_setup_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_run_setup_spec.md)
4. [calibration_workbench_candidate_contract.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_candidate_contract.md)
5. [calibration_workbench_plot_manifest_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_plot_manifest_spec.md)
6. [calibration_workbench_reevaluation_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_reevaluation_spec.md)
7. [calibration_workbench_export_schema.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_export_schema.md)
8. [calibration_workbench_implementation_plan.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibration_workbench_implementation_plan.md)
