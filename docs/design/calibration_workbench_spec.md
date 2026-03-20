# Calibration Workbench Spec

## Status

This document defines the interactive calibration workbench for Janiform.

It is derived from
[calibrator_v2_spec.md](/mnt/c/Users/anton/source/repos/janiform/docs/design/calibrator_v2_spec.md)
and assumes that the calibrator core emits the structured artifacts described there.

The workbench is a first-class part of the system. It is not a thin viewer with a few convenience
plots. Its purpose is to make calibration runs inspectable, diagnosable, and editable.

## Purpose

The workbench must support all of the following:

- passive inspection of completed calibration runs
- active diagnosis of solver behavior and objective behavior
- manual parameter tweaking followed by reevaluation
- comparison of candidate parameter sets and evaluation tiers
- visualization of search geometry and candidate frontier evolution
- structured export suitable for machine inspection and LLM-assisted debugging

## Non-Negotiable Design Rules

1. The GUI must be built with `PyQt6`.

2. The GUI layer must remain a thin orchestration and presentation boundary.
   Core calibration logic belongs in Rust-backed services and shared contracts, not duplicated
   in Python.

3. The user must never be left wondering what the system is doing.
   Calibration and plot generation must expose explicit progress and activity state.

4. The workbench must support manual parameter editing and reevaluation.
   This is a core diagnostic capability, not an optional extra.

5. The workbench must not be a monolithic file.
   It must be split into focused modules with clear responsibilities.

6. General-purpose helpers must be reused from shared locations.
   Any helper that is general enough to reuse must not be reimplemented locally if it already
   belongs in shared modules or crates such as `janiform-foundations`.
   If an appropriate helper already exists, it must be reused or moved to a proper shared location.
   It must never be copied into a second implementation. Duplicate helpers are forbidden.

7. The workbench code must live under `tools/workbench/`.
   Its implementation location should be fixed from the start rather than chosen ad hoc later.

## Scope

The workbench covers:

- calibration run loading
- artifact inspection
- parameter editing
- evaluation reruns
- progress reporting
- diagnostics display
- plot management
- structured export

It does not own:

- the mathematical calibration logic itself
- core simulator implementation
- low-level target estimation logic

## Workbench Inputs

The workbench consumes the structured artifacts emitted by the calibrator core, including at least:

- observed targets
- candidate frontier snapshots
- selection rationale
- simulation summaries
- search surface manifests
- runtime profiles
- coarse-versus-tick consistency diagnostics

The workbench may also invoke exact, coarse, and tick reevaluation services for edited parameter
sets using the same underlying evaluation contracts as the calibrator core.

## Core User Workflows

### 1. Run inspection

The user must be able to:

- open a completed run
- inspect observed targets
- inspect stage outputs
- inspect the selected candidate
- inspect the admissible and rejected frontier
- inspect runtime and budget breakdown

### 2. Candidate comparison

The user must be able to:

- compare multiple candidates side by side
- compare staged candidates versus final candidates
- compare exact, coarse, and tick diagnostics for the same candidate
- compare edited candidates against the original fitted candidate

### 3. Manual parameter tweaking

The workbench must support the following required diagnosis loop:

1. load a fitted or candidate parameter set
2. clone it into an editable working copy
3. edit parameters by block and by canonical coordinate
4. rerun exact rescoring
5. rerun coarse simulation diagnostics
6. optionally rerun seeded-tick diagnostics
7. refresh plots and structured diagnostics
8. export the edited parameter set and its evaluation output

This workflow must feel native to the tool, not like an internal developer backdoor.

### 4. Search diagnosis

The user must be able to inspect:

- search surfaces
- parameter pair slices
- proposal clouds
- frontier evolution
- rejection reasons
- progress of reconciliation rounds

This is necessary for understanding whether failures are due to:

- bad search geometry
- bad solver choice
- bad admissibility policy
- bad objective design
- or genuine model-data mismatch

## GUI and Interaction Design

### Main application areas

The workbench should expose at least:

- run browser
- summary dashboard
- parameter editor
- diagnostics inspector
- plot workspace
- progress and activity panel
- export actions

### Parameter editor requirements

The parameter editor must:

- group parameters by logical block
- show canonical coordinates clearly
- show current values, edited values, and optionally baseline values
- support reverting individual parameters or whole blocks
- support loading values from another candidate
- allow reevaluation at chosen fidelity tiers

### Diagnostics inspector requirements

The diagnostics inspector must expose:

- validity and admissibility state
- exact diagnostics
- coarse diagnostics
- tick diagnostics
- cross-tier consistency diagnostics
- rejection reasons
- ranking-relevant target families

### Plot workspace requirements

The plot workspace must support:

- return distribution overlays by horizon
- center-mass and tail diagnostics
- activity level and activity shape comparisons
- rolling coupling diagnostics
- event-conditioned diagnostics
- coarse-versus-tick agreement plots
- candidate frontier evolution plots
- search surfaces for important coordinates and coordinate pairs
- runtime and budget plots

Plots must be organized by family rather than dumped into one long scroll with no structure.

## Progress and Status UX

### Principle

The workbench must continuously show what is happening during calibration and during any expensive
reevaluation or plot-generation operation.

### Required calibration status information

The GUI must continuously expose:

- current calibration stage
- current stage subtask
- current evaluation tier
- current batch or frontier operation
- current candidate family or neighborhood batch when meaningful
- current plot or diagnostic artifact being computed
- elapsed time
- estimated remaining time when estimable
- resource usage summary where available

### Required progress models

Progress must be represented through explicit progress objects, not only log text.

The GUI must include:

- an overall calibration progress indicator
- a stage-level progress indicator
- subprogress indicators when a stage has meaningful internal work units
- indeterminate progress mode when exact completion fraction is unknowable

When exact completion is unknowable, the GUI must still show useful evidence of movement, such as:

- completed batches out of scheduled batches
- completed candidate families out of total candidate families
- completed seed evaluations out of planned seed evaluations
- completed frontier reevaluations out of current frontier tasks
- current reconciliation round

### Plot generation visibility

If the workbench is generating or refreshing plots, it must show:

- which plot family is currently being computed
- which plots are queued
- which plots are completed

Long-running plot work must not appear as a frozen application.

## Artifact and Export Requirements

### Structured artifact viewing

The workbench must make it easy to inspect:

- observed targets
- candidate frontier evolution
- exact rescoring diagnostics
- coarse simulation diagnostics
- seeded-tick diagnostics
- coarse-versus-tick consistency diagnostics
- runtime breakdown
- rejection reasons and admissibility failures
- search surfaces and neighborhood clouds

### Required structured exports

The workbench must support exporting:

- edited parameter sets
- reevaluation results
- selected plot data
- selected diagnostics tables
- selected candidate comparisons

These exports must remain machine-readable and self-describing.

### LLM-friendly output rule

When exporting JSON, the workbench must preserve:

- metric names
- units
- target family grouping
- whether fields are observed, simulated, normalized, or derived
- admissibility and ranking annotations

The output should be directly usable by automated analysis tools and LLMs without needing hidden
code knowledge.

## Code Architecture

### Workspace location

The workbench implementation must live in:

- `tools/workbench/`

The directory should contain the `PyQt6` application code, workbench-specific adapters, and
presentation-layer modules. It must not be spread arbitrarily across unrelated folders.

### Required module separation

The workbench must not be implemented as a monolithic Python file.

It should be split into focused modules such as:

- application shell and shared app state
- run loading and artifact adapters
- parameter editor panels
- calibration progress models
- plot controllers
- plot widgets
- diagnostics tables and inspectors
- reevaluation actions
- export actions

The exact module names may differ, but the separation of concerns is required.

### Shared-helper policy

General-purpose helpers must not be duplicated locally.

Examples include:

- z-score and standardization utilities
- mean absolute deviation
- standard deviation
- rolling summary helpers
- vector arithmetic helpers
- common normalization helpers

These are only examples, not an exhaustive list.

If a helper is general enough to reuse, it must live in a shared location. Existing shared
implementations in `janiform-foundations` or other appropriate shared modules should be reused or
extended rather than copied. If a helper already exists, duplicating it is forbidden.

## Relationship To Calibrator Core

The calibrator core owns:

- target estimation
- parameter geometry
- solver logic
- evaluator logic
- artifact generation

The workbench owns:

- interactive inspection
- interactive reevaluation orchestration
- visual presentation
- progress presentation
- structured export from user-selected views

This separation is intentional and should remain explicit in the codebase.

## Immediate Next Derived Specs

The following detailed specs should be derived from this document:

1. workbench application state model
2. progress event and progress model schema
3. parameter editor contract
4. plot manifest and plot-controller contract
5. reevaluation action contract
6. workbench export schema
