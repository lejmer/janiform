# Python Public API Surface Map And Namespace Policy

Document version: `python.public_api_surface_map.pre_v0_1.milestone_2`

Status: active for the current pre-v0.1 milestone on March 13, 2026.

## Purpose

This document defines the supported Python namespace for Janiform's current pre-v0.1 milestone.
The policy is maintainability-first and discoverability-first.
Curated package roots remain part of the design, but they are no longer treated as the only
acceptable public import location for workflow packages.

Compatibility review and snapshot-update workflow for this public surface lives in
`docs/development/guides/python_api_contract_compatibility_and_version_guardrails.md`.

Experimental namespace policy lives in
`docs/development/guides/python_experimental_api_policy.md`.

Exact exported symbol sets are tracked by the approved contract snapshot fixture. This guide defines
which package roots and submodules are intentionally public and how they should be organized.

## Superseded guidance

The earlier milestone 1 facade-first guidance is superseded wherever it implied any of the following:

- workflow packages should primarily expose one flat root module
- importable public domain submodules are a design smell by themselves
- underscore-prefixed packages are an acceptable substitute for coherent public package structure

Pre-v0.1 contract evolution rules apply to this reset. When a better package structure requires a
breaking import-path change, replace the old path directly and update in-repo callers, tests, docs,
and examples in the same change.

## Design goals

- Discoverability is a first-class public API concern.
- Package decomposition is a first-class public API concern.
- Curated convenience imports should reduce friction without hiding the natural domain structure.
- Public workflow and domain modules should read like a map of the product.
- Private-by-convention naming is not a substitute for clear package boundaries.

## Public package root

The package root `janiform` remains intentionally minimal.

- Public exports:
  - `__version__`
- Policy:
  - Do not add workflow functions, request types, response types, or helper classes to `janiform`.
  - Users should import from task-specific workflow packages or supporting public modules.

## Namespace policy

### Curated root re-export criteria

Expose a symbol from a workflow package root such as `janiform.simulation` or
`janiform.calibration` when all of the following are true:

- the symbol is part of the workflow package's main user path
- root-level availability materially improves ergonomics for common examples or scripts
- the root stays curated enough that `__all__` is still a readable overview of the package
- the re-export does not hide the symbol's natural domain module from docs or tests

Curated root exports are convenience imports. They are not a rule that every public symbol must live
only at the root.

### Public submodule criteria

Create or keep a public submodule when all of the following are true:

- it names a real workflow area or domain concept
- it gives users and maintainers a more discoverable place to look than a large root module
- the contents form a coherent group that can grow without turning into another monolith
- the submodule name is suitable for documentation, examples, and long-lived import paths

Public submodules are recommended for workflow packages whenever they improve decomposition or
clarity. Importability of a non-root public domain module is not a design problem by itself.

### Private underscore helper criteria

Reserve underscore-prefixed modules and packages for true internals only, such as:

- low-level adapters
- boundary plumbing
- serialization helpers
- internal composition helpers
- implementation shards that are not meant to be part of the supported mental model

Do not use underscore-prefixed modules to hide coherent workflow areas that should instead have
named public submodules.

## Multiple supported import paths

One symbol may intentionally be available from both:

- a curated convenience root export
- its natural public domain module

That is an allowed and recommended pattern when the root import is ergonomic and the submodule
import is clearer about ownership.

For example, both of these are supported:

```python
from janiform.simulation import (
    SeededTickSimulationRequest,
    run_seeded_tick_event_simulation,
)

from janiform.simulation.seeded import (
    SeededTickSimulationRequest,
    run_seeded_tick_event_simulation,
)
```

The root import is the convenience path. The submodule import is the natural ownership path.

## Supported public modules

### Package roots

- `janiform.simulation`
  - Ownership: curated convenience surface for seeded-tick and coarse-interval simulation workflows
- `janiform.pricing`
  - Ownership: vanilla option pricing contracts, execution input, and runtime factor loading
- `janiform.risk`
  - Ownership: sufficient-statistics risk workflow contracts
- `janiform.calibration`
  - Ownership: curated convenience surface for high-level fitting, market-data calibration, staged execution, and preset-facing calibration objects
- `janiform.calendar`
  - Ownership: exchange calendar queries shared across workflows
- `janiform.hourly_factors`
  - Ownership: interval-aligned `s_h` and `nu_h` contract materialization shared by simulation and pricing
- `janiform.seasonality`
  - Ownership: deterministic seasonality estimation and composition
- `janiform.validation`
  - Ownership: regression harnesses and validation artifacts
- `janiform.errors`
  - Ownership: public error taxonomy and formatting helpers
- `janiform.stylized_facts`
  - Ownership: stylized-fact comparison reporting

### Public domain submodules

- `janiform.simulation.seeded`
  - Ownership: seeded-tick simulation request and response contracts, seeded-tick execution entrypoints, and seeded-tick workflow-specific record types
  - Root coexistence policy: key seeded workflow symbols are also re-exported from `janiform.simulation` as curated convenience imports

Additional importable submodules are not public by default. A submodule becomes supported only when
this document lists it explicitly and the public contract snapshot tracks it.

## Calibration namespace status

`janiform.calibration` is public for the recommended market-data request contract, the default
high-level fit surface, advanced staged execution contracts, and preset-backed calibration
configuration objects.

The recommended public request boundary is:

- `MarketDataCalibrationRequest`
- `validate_market_data_calibration_request`
- `prepare_market_data_calibration`
- `run_market_data_calibration`

The default high-level path is:

- `JaniformEstimator.fit(...) -> JaniformModel`
- optional `JaniformFitReport`

The advanced staged-calibration entrypoint is:

- `run_staged_calibration`

The calibration package root is curated convenience surface. Future calibration decomposition should
use named public domain submodules when that produces a clearer map than keeping everything inside
`janiform.calibration.__init__`.

## Experimental namespace policy

`janiform.experimental` is a separate preview namespace for opt-in workflow iteration.

- preview endpoint modules live under `janiform.experimental`
- the current landing zone for future preview entrypoints is `janiform.experimental.api`
- preview modules require explicit opt-in through `JANIFORM_EXPERIMENTAL`
- preview modules are not part of the primary public API contract or snapshot guardrails
- preview modules must never be re-exported from `janiform` or from primary public modules

## Private namespace policy

The following namespaces are private implementation details and may change without warning inside the
pre-v0.1 line:

- any module or package whose name starts with `_`
- any submodule not listed in this document
- implementation helpers nested under public packages, such as `janiform.validation.harness`
- low-level native adapters and boundary plumbing, such as `janiform._native` and `janiform._native_api`

Private modules must not be re-exported from `janiform` or from public module `__all__` lists.

## Contract evolution rules

- Pre-v0.1 correctness, maintainability, and clarity take priority over compatibility.
- Breaking API and import-path changes are allowed when they produce a better public package design.
- Do not add compatibility shims, legacy aliases, or dual old-and-new public layouts unless a task explicitly asks for them.
- When a symbol is public, choose both its natural domain module and any curated root re-export intentionally.
- When a public symbol moves, update in-repo call sites, tests, docs, examples, and the approved snapshot in the same change.
- If a symbol or submodule is not listed in this document, treat it as private even if Python still allows importing it.

## Review checklist for future API changes

- Is the symbol part of a real user-facing workflow or domain concept?
- Should it live in an existing curated root export set, a named public submodule, or a private underscore helper?
- Does the package layout improve discoverability for both users and maintainers?
- Are multiple supported import paths intentional and documented rather than accidental?
- Are private helpers still private instead of becoming a shadow public API through examples or tests?
- Are README, docs, examples, and export guardrails updated in the same change?
