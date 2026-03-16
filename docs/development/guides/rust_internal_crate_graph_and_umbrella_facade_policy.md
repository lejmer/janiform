# Rust Internal Crate Graph And Umbrella Facade Policy

Document version: `rust.internal_crate_graph_and_umbrella_facade_policy.pre_v0_1.milestone_1`

Status: active for the current pre-extraction migration on March 16, 2026.

## Purpose

This document defines the target internal workspace crate graph for the Rust implementation and
the umbrella-facade policy for `janiform-core`.

The point of the split is ownership clarity, dependency hygiene, and compile-boundary isolation.
The point is not to maximize crate count or to expose every internal crate publicly.

Detailed staged extraction work remains tracked by:

- `JAN-258` for foundations, model kernels, and affine oracle
- `JAN-259` for market-time infrastructure
- `JAN-260` for pricing, risk, and simulation engines
- `JAN-261` for calibration artifacts and runtime
- `JAN-262` for stylized facts, validation, and governance
- `JAN-263` for `pybind` and facade realignment
- `JAN-264` for benchmark support and boundary enforcement

For every extraction issue above, "extract" means the target crate becomes the physical source
owner for the moved modules, their crate-local helpers, and their unit tests.

The following do not count as a completed extraction:

- compiling implementation files from `crates/core` through `#[path = "..."]`
- leaving unit tests and crate-local support files physically owned by `crates/core`
- treating `janiform-core` as the long-term implementation home while only changing Cargo edges

`janiform-core` may remain the umbrella facade and may re-export extracted modules during
migration, but it must not remain the physical implementation crate for code that is claimed to be
extracted.

## Problem Statement

`janiform-core` currently mixes several distinct ownership domains inside one crate:

- low-level numerical and model-kernel code
- market-time infrastructure and hourly-factor materialization
- pricing, risk, and simulation runtime engines
- calibration artifacts, adapters, objectives, and staged orchestration
- stylized-fact analytics and validation harnesses
- governance, performance baseline, and benchmark support

That concentration makes several things unclear:

- which modules are foundational and should stay dependency-light
- which modules are engine runtime concerns versus governance concerns
- which APIs should stay stable through `janiform-core` during migration
- which dependency directions are forbidden even before every extraction lands

Without an explicit target graph, extraction work would be driven by local file moves and would
likely recreate the same coupling under different crate names.

## Current Module Map To Target Ownership

The split is intended to follow current dominant ownership, not to preserve the existing module
layout indefinitely.

- `complex_math` and `randomness` move toward `janiform-foundations`
- `hawkes`, `news`, `nig`, `return_decomposition`, and shared `risk_neutral` helpers move toward
  `janiform-model-kernels`
- `affine_oracle` moves toward `janiform-affine-oracle`
- `calendar`, `seasonality`, and `hourly_factors` move toward `janiform-market-time`
- `pricing` moves toward `janiform-engine-pricing`
- `risk` moves toward `janiform-engine-risk`
- `simulation` moves toward `janiform-engine-simulation`
- calibration contracts, manifests, reload/index artifacts, and checkpoint-facing schema ownership
  move toward `janiform-calibration-artifacts`
- calibration execution bridges, objectives, stage runners, optimizers, and workflow orchestration
  move toward `janiform-calibration-runtime`
- `verification/stylized_facts` moves toward `janiform-analytics-stylized-facts`
- remaining verification domains move toward `janiform-validation`
- `benchmarking` moves toward `janiform-governance-bench`
- deterministic regression orchestration, performance baselines, and regression-governance helpers
  move toward `janiform-governance`

Behavior-preserving extraction comes first.
Cleanup, renaming, and smaller internal API reshaping come after the ownership boundary exists.

That ownership boundary is not only a dependency-graph concern.
For `JAN-259` through `JAN-264`, the destination crate should own the moved source files and unit
tests directly instead of compiling them from `crates/core` through path indirection.

## Target Workspace Crate Graph

The initial target internal workspace graph is the following:

| Crate | Role | Primary ownership | Allowed internal dependencies |
| --- | --- | --- | --- |
| `janiform-foundations` | low-level shared base | math helpers, deterministic utilities, shared primitives | none |
| `janiform-model-kernels` | model-domain kernel crate | Hawkes, NIG, news-process domain, return decomposition, risk-neutral kernel helpers | `janiform-foundations` |
| `janiform-affine-oracle` | shared transform solver crate | affine transform contracts and solver internals | `janiform-foundations`, `janiform-model-kernels` |
| `janiform-market-time` | shared time/calendar infrastructure | calendar, seasonality, hourly factors, interval alignment | `janiform-foundations`, `janiform-model-kernels` |
| `janiform-engine-pricing` | pricing runtime engine | pricing contracts and execution | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time` |
| `janiform-engine-risk` | risk runtime engine | sufficient-statistics risk sampling and path outputs | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time` |
| `janiform-engine-simulation` | simulation runtime engine | coarse and seeded simulation execution | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time` |
| `janiform-calibration-artifacts` | calibration contract and artifact crate | run specs, manifests, reload/index helpers, artifact schema ownership | `janiform-foundations`, `janiform-model-kernels`, `janiform-market-time` |
| `janiform-calibration-runtime` | calibration execution crate | staged execution, objectives, optimizers, runtime orchestration | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time`, `janiform-engine-pricing`, `janiform-engine-risk`, `janiform-engine-simulation`, `janiform-calibration-artifacts` |
| `janiform-analytics-stylized-facts` | analytics crate | stylized-fact metrics, comparisons, and reporting | `janiform-foundations`, `janiform-model-kernels`, `janiform-market-time`, `janiform-engine-simulation`, `janiform-calibration-artifacts` |
| `janiform-validation` | validation crate | regression harnesses, scale consistency, deterministic checks, validation contracts | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time`, `janiform-engine-pricing`, `janiform-engine-risk`, `janiform-engine-simulation`, `janiform-calibration-artifacts`, `janiform-analytics-stylized-facts`, `janiform-governance-bench` |
| `janiform-governance` | governance crate | regression drift attribution, acceptance governance, deterministic regression orchestration, performance baseline ownership | `janiform-foundations`, `janiform-model-kernels`, `janiform-market-time`, `janiform-engine-pricing`, `janiform-engine-risk`, `janiform-engine-simulation`, `janiform-calibration-artifacts`, `janiform-analytics-stylized-facts`, `janiform-validation`, `janiform-governance-bench` |
| `janiform-governance-bench` | benchmark-only governance crate | benchmark catalogs, harnesses, policies, and microbenchmark fixture support shared by validation and governance workflows | `janiform-foundations`, `janiform-model-kernels`, `janiform-affine-oracle`, `janiform-market-time`, `janiform-engine-pricing`, `janiform-engine-risk`, `janiform-engine-simulation`, `janiform-calibration-artifacts` |
| `janiform-test-support` | test-only support crate | shared regression-fixture readers and helper contracts used only by Rust tests | `janiform-market-time`, `janiform-validation` |
| `janiform-core` | umbrella facade | curated public Rust surface during migration | all internal crates above except `janiform-pybind` |
| `janiform-pybind` | Python boundary crate | PyO3 conversion and error mapping | `janiform-core` only |

The graph is intentionally directional:

- foundations sit at the bottom
- model and time infrastructure stay below engines
- calibration runtime may call into engines
- validation may observe engines and calibration artifacts
- governance may observe engines and validation outputs
- shared Rust test-fixture readers should live in a test-only support crate instead of being copied across crate-local test trees
- `janiform-core` stays on top as the curated Rust facade
- `janiform-pybind` stays above the facade rather than reconstructing the graph directly

## Umbrella Facade Policy

`janiform-core` remains the single umbrella Rust facade during migration.
Internal crates are ownership and compile boundaries behind that facade.

### What stays re-exported from `janiform-core`

Continue re-exporting the current high-level module roots that define the curated Rust map used by
`pybind` and by in-repo integration callers:

- `affine_oracle`
- `benchmarking`
- `calendar`
- `calibration`
- `complex_math`
- `hawkes`
- `hourly_factors`
- `news`
- `nig`
- `pricing`
- `randomness`
- `return_decomposition`
- `risk`
- `risk_neutral`
- `seasonality`
- `simulation`
- `verification`

Within those module roots, re-export only the stable workflow and domain surfaces already acting as
cross-crate contracts:

- request and response types
- error enums that cross crate boundaries
- workflow entrypoints
- model-domain types already used by `pybind` or integration tests
- stable artifact and diagnostics contracts that are part of approved workflows

### What must stay crate-local inside internal crates

New internal crates should default to `pub(crate)` or private visibility for implementation
details.

The following should remain internal unless an explicit policy update says otherwise:

- numerical helper modules that do not define cross-crate contracts
- serde implementation shards and parsing helpers
- adapter glue without boundary value
- benchmark input builders and benchmark-only fixture utilities
- test support, regression-fixture readers, and helper constructors used only by tests
- stage-specific calibration internals that do not need cross-crate reuse
- governance-only orchestration internals

Internal crate names themselves are not the public product surface.
The public product-facing Rust map stays organized by `janiform-core`.

### Transitional facade rule

`benchmarking` and parts of `verification` remain transitional module roots during migration
because current in-repo callers already rely on them.

That transitional status is narrow:

- preserve existing curated callers while extraction proceeds
- do not add new benchmark-only or governance-only exports to `janiform-core` by default
- move ownership out first, then reduce facade exposure in a later explicitly approved change

## Dependency Direction Rules

The following dependency directions are forbidden unless this policy is explicitly revised.

### Engine isolation rules

- engine crates must not depend on `janiform-calibration-runtime`
- engine crates must not depend on `janiform-validation`
- engine crates must not depend on `janiform-governance`
- engine crates must not depend on `janiform-governance-bench`
- engine crates may share dependencies through foundations, model kernels, affine oracle, and
  market-time infrastructure only

### Calibration rules

- `janiform-calibration-runtime` may depend on engines and calibration artifacts
- `janiform-calibration-artifacts` must stay dependency-light and must not depend on engines,
  validation, or governance
- calibration runtime must not become the place where governance or benchmark policy leaks into
  production execution paths

### Validation and analytics rules

- `janiform-analytics-stylized-facts` may depend on engine outputs when required for analytics
- `janiform-analytics-stylized-facts` may depend on calibration-artifact contracts when
  report-generation ownership metadata needs them
- `janiform-validation` may depend on engines, analytics outputs, calibration artifacts, and
  benchmark-only governance support required to build benchmark-shaped validation inputs
- validation and analytics crates must not introduce reverse dependencies from engines back into
  validation workflows

### Governance rules

- governance may depend on engines and validation outputs, but not the reverse
- governance may depend on benchmark-only governance support for baseline capture inputs and
  benchmark report contracts
- governance may aggregate benchmark, regression, and acceptance-policy logic
- governance must not become a foundational dependency for runtime execution crates

### Benchmark-only rules

- benchmark-only support must not grow production crate surfaces
- `janiform-governance-bench` is benchmark-only and may support validation and governance
  workflows, but it must not become a dependency of foundational, model-kernel, market-time,
  engine, or calibration crates
- new benchmark helpers should land in governance-bench or test-only code unless a production
  contract need is explicitly documented

### Test-only support rules

- shared Rust regression-fixture readers should move into `janiform-test-support` instead of
  being copied into multiple crate-local `tests/support` directories
- `janiform-test-support` may depend on internal crates only when a fixture contract genuinely
  needs those types, and it must stay out of production dependency paths

### Pybind boundary rule

- `janiform-pybind` should continue depending on `janiform-core` rather than taking direct
  dependencies on each internal crate
- exceptions require an explicit policy update and should be treated as boundary regressions by
  default

## Review And CI Enforcement

The target graph must be enforceable before the migration is complete.

Review and CI should treat source-ownership indirection as a boundary regression.
When an issue says a domain moved to an internal crate, the implementation and unit tests for that
domain should live under that crate's directory unless an explicit policy update says otherwise.

### Required mechanical checks

- a checked-in internal crate graph policy fixture must define the allowed workspace crates and
  their allowed internal dependency directions
- a cargo-metadata based test must fail when an actual workspace dependency direction is not in the
  approved policy
- a facade surface test must fail when `janiform-core` drops approved module roots or the
  representative imports used by `pybind` and in-repo callers

### Required review rules

Reviewers should reject changes that do any of the following without a corresponding policy update:

- add a new internal workspace crate not listed in the approved graph
- add a new dependency direction between internal crates that is not in the approved graph
- expose a new internal crate directly to `pybind`
- add benchmark-only helpers to foundational or engine crates
- broaden `janiform-core` with new governance-only exports just because a new internal crate exists

### Migration discipline

When an extraction PR introduces a new internal crate:

- add the crate to the workspace
- add its dependency edges to the machine-readable policy
- keep `janiform-core` compiling as the umbrella facade
- move behavior-preserving ownership first
- defer secondary cleanup until after the boundary is established

This policy is the approval baseline for Project 011 issue `JAN-257`.
