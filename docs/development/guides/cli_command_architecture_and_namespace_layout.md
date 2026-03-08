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
