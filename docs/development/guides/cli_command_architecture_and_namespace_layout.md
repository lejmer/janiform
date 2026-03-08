# CLI Command Architecture And Namespace Layout

Document version: `cli.command_architecture.pre_v0_1.milestone_1`

Status: active for the current pre-v0.1 milestone on March 8, 2026.

## Purpose

This guide defines the top-level Janiform CLI command tree for the current pre-v0.1
workflow surface. It keeps the primary user-facing workflows separate from internal
developer utilities and aligns command names with the public Python workflow modules.

## Command tree

```text
janiform
├── simulation
│   └── seeded-ticks
├── calibration
│   └── staged
├── pricing
│   └── vanilla-option
└── dev
    └── regression
        └── open-closed-gap-leakage
```

## Namespace policy

- `simulation`, `calibration`, and `pricing` are the primary user-facing workflow groups.
- `dev` is reserved for internal and developer-only utilities.
- Developer-only commands must not be placed beside primary workflow groups at the top level.
- Command aliases are forbidden because they add ambiguity to automation and shell scripts.

## Naming policy

- Top-level command groups use the same workflow nouns as the public Python modules:
  `janiform.simulation`, `janiform.calibration`, and `janiform.pricing`.
- Subcommand names stay short while preserving the specific workflow identity:
  - `seeded-ticks` for the seeded tick-event simulator
  - `staged` for staged calibration orchestration
  - `vanilla-option` for vanilla option pricing
- Developer-only validation harnesses remain explicit about their function:
  - `dev regression open-closed-gap-leakage`

## Help-output expectations

- `janiform --help` lists the four top-level namespaces in the order above.
- `janiform simulation --help`, `janiform calibration --help`, and `janiform pricing --help`
  each list their current workflow subcommand.
- `janiform dev --help` lists internal-only utility groups.
- Incomplete command paths print the help for the selected namespace branch.

## Current workflow command contracts

The current pre-v0.1 CLI uses explicit JSON contract files at the command boundary so the
command surface stays aligned with the typed Python workflow models.

### `janiform simulation seeded-ticks`

Required arguments:
- `--request-json`: `SeededTickSimulationRequest` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Behavior:
- The command rehydrates the runtime hourly-factor handle from the request's
  `hourly_factor_summary` plus the supplied calendar registry.
- Validation fails if the public request summary does not reproduce the same hourly-factor contract.

Output:
- Writes deterministic JSON for `simulation.seeded_tick_event.path.v1` to stdout.

### `janiform pricing vanilla-option`

Required arguments:
- `--execution-input-json`: `VanillaOptionPricingExecutionInput` JSON object

Behavior:
- The command validates the execution input directly against the Python pricing contract.
- Interval factors keep `nu_h_known` and `nu_h_surprise` separate.

Output:
- Writes deterministic JSON for `pricing.vanilla_option.run_result.v1` to stdout.

### `janiform calibration staged`

Required arguments:
- `--execution-input-json`: `StagedCalibrationExecutionInput` JSON object
- `--calendar-registry-json`: `calendar.registry.v1` JSON object or a wrapper object with `calendar_registry`

Optional arguments:
- `--execution-mode {disk,memory}`: defaults to `disk` on the CLI
- `--disk-checkpoint-directory`: required when the effective execution mode is `disk` and the input JSON does not already provide one

Behavior:
- The CLI default deliberately overrides the importable-Python default and normalizes the
  execution input to `execution_mode=disk` unless `--execution-mode memory` is supplied.
- Validation rejects `--disk-checkpoint-directory` when `--execution-mode memory` is selected.
- Return-components stage inputs keep known-news open-jump and closed-gap semantics explicit.

Output:
- Writes deterministic JSON for `calibration.orchestration.staged_run.report.v1` to stdout.
