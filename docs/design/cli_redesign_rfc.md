# CLI Redesign RFC

Document status: draft

Last updated: March 13, 2026

## Summary

This RFC proposes a breaking redesign of the Janiform CLI. This is acceptable as the CLI is not released and no users are affected.

The current CLI exposes two competing mental models at the top level:

- high-level outcome verbs such as `fit` and `simulate`
- lower-level workflow namespaces such as `calibration` and `simulation`

This creates duplicate concepts, makes `--help` noisy, and forces users to learn internal workflow boundaries too early.

The proposed redesign keeps `fit` and `simulate` as the main entry points, moves their lower-level expert workflows underneath those same verbs, and keeps standalone domain tools and developer utilities separate.

## Goals

- Keep `fit` and `simulate` as the obvious first commands new users see.
- Make the primary path discoverable from `janiform --help`.
- Remove top-level duplication such as `fit` versus `calibration` and `simulate` versus `simulation`.
- Preserve access to low-level explicit-contract workflows for expert users.
- Use names and structure that match common CLI conventions.
- Keep developer-only tools clearly separated from user-facing workflows.

## Non-Goals

- Preserve compatibility with the current CLI layout.
- Add aliases for old command names.
- Redesign workflow payload schemas in this RFC.
- Redesign standalone domain tools such as `pricing` or `stylized-facts` beyond their placement in the command tree.

## Problem Statement

Today the CLI is split between:

- top-level high-level commands
- top-level low-level workflow groups

That leads to three problems.

First, users have to choose between two names for nearly the same task:

- `fit` versus `calibration`
- `simulate` versus `simulation`

Second, top-level help teaches internal architecture instead of user goals.

Third, the command tree does not reflect a normal CLI progression where a simple command has more explicit expert subcommands beneath it.

## Design Principles

- Top-level commands should express user goals or stable standalone domains.
- High-level and low-level versions of the same capability should share one root command.
- Command names should be concrete and conventional rather than meta labels such as `advanced` or `raw`.
- First-time help should point users to the default path before exposing detailed workflow boundaries.
- Expert workflows should remain explicit and scriptable.
- Developer tooling should stay outside the main user workflow surface.

## Proposed Command Tree

```text
janiform
├── fit
│   ├── [default high-level fit workflow]
│   ├── plan
│   └── staged
├── simulate
│   ├── [default high-level simulation workflow]
│   ├── coarse-interval
│   └── seeded-ticks
├── presets
│   ├── list
│   ├── show
│   └── export
├── seasonality
│   ├── diagnostics
│   └── preprocess-timestamps
├── hourly-factors
│   ├── materialize-seasonality
│   ├── materialize-news-intensity
│   └── build-contract
├── pricing
│   └── vanilla-option
├── stylized-facts
│   └── horizon-comparison
└── dev
    └── regression
        ├── deterministic
        └── open-closed-gap-leakage
```

## Command Semantics

### `janiform fit`

The default high-level model fitting workflow.

This command is the main user-facing path for fitting a model from market data, presets, and light explicit overrides. It should remain the first recommendation in docs and top-level help.

### `janiform fit plan`

The preparatory fit workflow.

This command validates the input set, resolves presets and provenance, and produces the derived plan or execution input that would be used for fitting. It is intended for users who want to inspect or materialize the planned workflow boundary before running the full fit.

This replaces the current `calibration market-data` command name. The current name overemphasizes internal implementation details. The proposed name reflects what the command actually does for a user.

### `janiform fit staged`

The expert staged fit workflow.

This command drives the explicit staged fitting or calibration contract directly. It is intended for users who already know they need control at the staged execution boundary.

This replaces the current `calibration staged` command path.

### `janiform simulate`

The default high-level simulation workflow.

This command is the main user-facing path for simulating from a persisted fitted model without manually reassembling lower-level runtime contracts.

### `janiform simulate coarse-interval`

The expert direct coarse simulation workflow.

This command drives the coarse interval simulation contract directly. It is intended for users who need the explicit coarse path artifact and already have the canonical request payload.

This replaces the current `simulation coarse-interval` command path.

### `janiform simulate seeded-ticks`

The expert seeded fine-path simulation workflow.

This command drives the seeded tick simulation contract directly. It is intended for users who need fine-path replay, direct event tape generation, or other seeded simulation internals.

This replaces the current `simulation seeded-ticks` command path.

### Standalone Domain Tools

The following remain top-level because they are real standalone domains rather than alternate forms of `fit` or `simulate`:

- `presets`
- `seasonality`
- `hourly-factors`
- `pricing`
- `stylized-facts`

### Developer Tools

`dev` remains the only developer-only top-level namespace.

## Naming Rules

- Top-level commands must represent either:
  - a primary user goal
  - a stable standalone domain
  - a developer-only utility namespace
- A low-level workflow must live beneath the high-level command it refines.
- Top-level duplicate concepts are forbidden.
- Verb roots are preferred when the command tells Janiform to perform a user-visible action.
- Noun roots are preferred only when the command group is a standalone domain with multiple workflows of its own.
- Internal implementation nouns should not be promoted to the top level when a clearer user-facing verb exists.
- Meta labels such as `advanced`, `raw`, `expert`, or `low-level` must not appear in command paths.
- Aliases are forbidden.
- Combined convenience commands should not become top-level peers unless they represent a stable concept users will naturally search for.

## Rename And Deletion Plan

Delete these top-level command paths:

- `janiform calibration`
- `janiform simulation`
- `janiform fit-and-simulate`

Replace them with these canonical paths:

- `janiform calibration market-data` -> `janiform fit plan`
- `janiform calibration staged` -> `janiform fit staged`
- `janiform simulation coarse-interval` -> `janiform simulate coarse-interval`
- `janiform simulation seeded-ticks` -> `janiform simulate seeded-ticks`

Keep these top-level commands:

- `janiform fit`
- `janiform simulate`
- `janiform presets`
- `janiform seasonality`
- `janiform hourly-factors`
- `janiform pricing`
- `janiform stylized-facts`
- `janiform dev`

## Position On `fit-and-simulate`

`fit-and-simulate` should be removed as a top-level command.

It is a convenience mode of fitting, not a primary top-level concept. Keeping it as a peer to `fit` and `simulate` makes the top-level surface noisier and teaches users an unnecessary third entry point.

If the combined workflow remains supported, it should be expressed as an option on `fit`, for example:

- `janiform fit --simulate`
- `janiform fit --simulation-output coarse`
- `janiform fit --simulation-seed 17 --simulation-start-time-utc ...`

This keeps the command model aligned with common CLI practice where the main verb owns its convenience variations.

## Help Design

### Top-Level Help

Top-level help should show two things immediately:

- the primary entry points
- a short path for first-time users to get started

Suggested shape:

```text
Usage: janiform <command> [options]

Primary workflows:
  fit           Fit a model from market data
  simulate      Simulate data from a fitted model

Supporting tools:
  presets        Inspect bundled presets used by fit
  seasonality    Work with seasonality diagnostics and preprocessing
  hourly-factors Build and materialize hourly factor inputs
  pricing        Run standalone pricing workflows
  stylized-facts Compare empirical and simulated stylized facts

Developer tools:
  dev            Internal regression and contributor utilities

Getting started:
  janiform fit --help
  janiform simulate --help
```

### `fit` Help

`fit --help` should make the default high-level path explicit and present the subcommands as more explicit control levels rather than separate unrelated commands.

Suggested shape:

```text
Usage: janiform fit [options]
       janiform fit <subcommand>

Fit a Janiform model from market data.

Default workflow:
  Run the recommended end-to-end fitting workflow from raw inputs,
  presets, and light overrides.

Subcommands:
  plan     Validate inputs and build the derived fit plan
  staged   Run the staged fit workflow directly

Examples:
  janiform fit ...
  janiform fit plan ...
  janiform fit staged ...
```

### `simulate` Help

`simulate --help` should follow the same pattern as `fit --help`.

Suggested shape:

```text
Usage: janiform simulate [options]
       janiform simulate <subcommand>

Simulate data from a fitted Janiform model.

Default workflow:
  Simulate from a persisted model without rebuilding internal contracts.

Subcommands:
  coarse-interval  Run the coarse simulation contract directly
  seeded-ticks     Run the seeded tick simulation contract directly

Examples:
  janiform simulate ...
  janiform simulate coarse-interval ...
  janiform simulate seeded-ticks ...
```

## User Education Rule

Documentation and help should consistently teach the same progression:

1. inspect presets when needed
2. run `fit` to create a model
3. run `simulate` to generate data
4. use `fit plan`, `fit staged`, `simulate coarse-interval`, or `simulate seeded-ticks` only when direct workflow-boundary control is required

No document should present `calibration` or `simulation` as competing top-level entry points after this redesign.

## Migration Notes

This redesign is intentionally breaking.

Because the project is pre-v0.1, the command tree should be replaced directly rather than preserving compatibility shims or deprecated aliases. All in-repo tests, docs, examples, and automation references should be migrated in the same change.

## Open Questions

- Whether `fit plan` should emit only a summary by default or also support writing the derived staged execution input artifact directly.
- Whether the combined fit-plus-simulate mode should exist at all after the redesign.
- Whether the standalone domain tools should later be grouped further for help presentation without changing their command paths.

## Implementation Impact

The redesign will require coordinated updates across:

- parser construction
- command dispatch
- CLI help and rendering
- command reference docs
- architecture docs
- smoke tests and help-output tests
- any examples or scripts that still invoke `calibration`, `simulation`, or `fit-and-simulate`
