# Python Public API Surface Map And Namespace Policy

Document version: `python.public_api_surface_map.pre_v0_1.control_plane_reset.v1`

Status: active for the current pre-v0.1 milestone on March 26, 2026.

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
- Curated roots should stay small enough that `__all__` reads like an overview, not an index dump.
- Public workflow and domain modules should read like a map of the product.
- Private-by-convention naming is not a substitute for clear package boundaries.

## Boundary policy

Janiform now treats Python as the control plane and Rust as the data plane.

- `config`
  - Small immutable request, settings, policy, preset, or parameter objects.
  - Public Python dataclasses and enums are allowed here.
- `bulk data`
  - Empirical observations, event tables, candidate sets, lower-frequency grids, interval vectors,
    and other dataset-sized payloads.
  - Python tuple-of-record ownership is forbidden on the supported production path.
  - Public boundaries must use native-owned handles, file references, dataset references, or
    columnar buffers.
- `runtime artifact`
  - Prepared calibration plans, fitted models, lazy simulation paths, priced path views, reusable
    runtime handles, and artifact-backed execution inputs.
  - Runtime artifacts may expose lazy accessors and persistence helpers, but they must not eagerly
    duplicate bulk datasets into Python-owned record tuples.
  - Public and public-facing signatures must not name concrete `janiform._native` classes.
    If Python typing needs a native-backed handle or result shape, use a Janiform-owned protocol
    boundary type instead.
- `report`
  - Diagnostics, provenance, validation summaries, and fit reports.
  - Reports may contain small tuples and nested summaries, but they must not embed bulk empirical
    payloads.

## Surface classification map

The following public surface families are intentionally supported and explicitly classified:

| Surface family | Classification |
| --- | --- |
| `janiform` | `config` |
| `janiform.errors` | `report` |
| `janiform.calendar` | `config`, `runtime artifact` |
| `janiform.contracts` | `config` |
| `janiform.hourly_factors` | `config`, `runtime artifact`, `bulk data` |
| `janiform.seasonality` | `config`, `report`, `bulk data` |
| `janiform.simulation` | `config`, `runtime artifact` |
| `janiform.simulation.coarse` | `config`, `runtime artifact` |
| `janiform.simulation.seeded` | `config`, `runtime artifact` |
| `janiform.simulation.shared` | `config` |
| `janiform.pricing` | `config`, `runtime artifact`, `report`, `bulk data` |
| `janiform.risk` | `config`, `runtime artifact`, `report`, `bulk data` |
| `janiform.calibration` | `config`, `runtime artifact`, `report` |
| `janiform.calibration.fit` | `config`, `runtime artifact`, `report` |
| `janiform.calibration.fit.estimator` | `config`, `runtime artifact` |
| `janiform.calibration.fit.model` | `runtime artifact` |
| `janiform.calibration.fit.report` | `report` |
| `janiform.calibration.market_data` | `config`, `runtime artifact`, `report` |
| `janiform.calibration.market_data.contracts` | `config` |
| `janiform.calibration.market_data.request` | `config` |
| `janiform.calibration.market_data.ingestion` | `report` |
| `janiform.calibration.market_data.translation` | `runtime artifact`, `report` |
| `janiform.calibration.market_data.workflow` | `runtime artifact`, `report` |
| `janiform.calibration.staged` | `config`, `runtime artifact`, `report` |
| `janiform.calibration.staged.contracts` | `config` |
| `janiform.calibration.staged.inputs` | `config`, `runtime artifact` |
| `janiform.calibration.staged.request` | `config` |
| `janiform.calibration.staged.results` | `runtime artifact`, `report` |
| `janiform.calibration.staged.runtime` | `runtime artifact` |
| `janiform.calibration.presets` | `config` |
| `janiform.calibration.presets.references` | `config` |
| `janiform.calibration.presets.registry` | `config`, `runtime artifact` |
| `janiform.calibration.presets.resolution` | `config`, `report` |
| `janiform.calibration.sessions` | `config`, `report` |
| `janiform.calibration.sessions.contracts` | `config`, `report` |
| `janiform.validation` | `runtime artifact`, `report` |
| `janiform.stylized_facts` | `runtime artifact`, `report` |
| `janiform.cli` | `config`, `report` |
| `janiform.cli.commands.calibration` | `config`, `report` |
| `janiform.cli.commands.simulation` | `config`, `report` |

## Review checklist

Reject a Python public-surface change when any of the following is true:

- it introduces a new tuple-of-record or `Sequence[Mapping[...]]` bulk-evidence contract on the
  default path
- it makes a report or runtime artifact eagerly duplicate a native-owned dataset
- it expands `janiform.contracts` or market-data request modules with data-bearing ownership
- it makes a CLI default require users to assemble large staged evidence payloads by hand
- it adds a package-root re-export that only forwards to an ownership module without making the
  root materially clearer for normal users
- it keeps a public-looking module importable without making it obvious, at the module boundary,
  whether that module is public ownership surface or internal plumbing

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

Curated roots should prefer workflow entrypoints, the most common request or result contracts, and a
small number of cross-cutting shared types. Serialization helpers, schema constants, and advanced
plumbing contracts should usually stay in ownership modules unless the root is still clearly
curated after adding them.

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

For coarse workflows and shared simulation contracts, the same pattern applies:

```python
from janiform.simulation.coarse import CoarseSimulationRunRequest, run_coarse_simulation

from janiform.simulation.shared import HawkesKernelParameters

from janiform.contracts import MeanReversionFittedParameters
```

## Supported public modules

### Package roots

- `janiform.simulation`
  - Ownership: curated convenience surface for seeded-tick and coarse-interval simulation workflows
  - Root-size policy: keep only the common workflow entrypoints, request and response contracts, and the shared inputs needed to construct those workflows from examples or notebooks
  - Progress policy: long-running public simulation entrypoints may expose typed progress callbacks when the progress contract is shared across seeded and coarse workflows
- `janiform.pricing`
  - Ownership: vanilla option pricing contracts, anchored boundary-state pricing diagnostics, and runtime factor loading
  - Anchored pricing policy: the module re-exports the canonical pressure-aware pricing boundary input and local state-dependent tilt diagnostics alongside the shared mean-reversion contracts required to build that request surface
  - Execution contract policy: `VanillaOptionPricingExecutionInput` carries an interval-aligned tuple of pricing factors for the requested horizon, and optional run diagnostics may include anchored recursion state-propagation output
- `janiform.risk`
  - Ownership: sufficient-statistics risk workflow contracts
  - Observed-state policy: public risk path records and diagnostics distinguish core Janiform interval returns from observed log-price state, anchor state, pressure state, and deviation-from-anchor summaries
- `janiform.calibration`
  - Ownership: curated convenience surface for high-level fitting, market-data calibration, staged execution, and preset-facing calibration objects
  - Root-size policy: prefer the estimator, main workflow requests and responses, and the small set
    of preset-facing types used by the high-level path. Leave schema constants, low-level session
    contracts, and advanced validation helpers in ownership modules
- `janiform.calendar`
  - Ownership: exchange calendar queries shared across workflows
- `janiform.hourly_factors`
  - Ownership: interval-aligned `s_h` and `nu_h` contract materialization shared by simulation and pricing
- `janiform.contracts`
  - Ownership: shared contract primitives and cross-cutting domain contracts, including pressure-to-level mean reversion
- `janiform.seasonality`
  - Ownership: deterministic seasonality estimation and composition
- `janiform.validation`
  - Ownership: regression harnesses and validation artifacts
- `janiform.errors`
  - Ownership: public error taxonomy and formatting helpers
- `janiform.stylized_facts`
  - Ownership: stylized-fact comparison reporting
- `janiform.cli`
  - Ownership: curated CLI contract constants and supported command-group organization
  - Root-size policy: the package root should export the supported exit-code enum and
    the small set of schema, entrypoint, path-policy, and environment-variable constants that are
    referenced by CLI-facing docs and workflow tests. Command-tree assembly, rendering,
    configuration loading, path resolution, and document helpers stay in dedicated helper modules
  - Execution-module policy: the executable process boundary lives in `janiform.__main__`, not in
    `janiform.cli.__init__`
  - Beginner-path policy: `janiform fit PATH --market MARKET` is the preferred first-time path, with `fit inspect` and `--explain` exposing inferred defaults

### Public domain submodules

- `janiform.simulation.coarse`
  - Ownership: coarse-interval simulation request and path contracts, news-process configuration, and coarse execution entrypoints
  - Observed-state policy: coarse path records and diagnostics expose both core interval return totals and observed-state end-of-interval fields, including pressure state, so downstream exports and analytics do not reconstruct prices from core returns alone
  - Root coexistence policy: common coarse workflow symbols are also re-exported from `janiform.simulation` as curated convenience imports
- `janiform.simulation.seeded`
  - Ownership: seeded-tick simulation request and response contracts, seeded-tick execution entrypoints, and seeded-tick workflow-specific record types
  - Observed-state policy: the public seeded fine-path and event-tape records expose observed log-price, anchor, pressure-state, deviation, and core-increment fields explicitly, and the package re-exports the endogenous mean-reversion runtime-input helper used by examples and high-level workflow wiring
  - Root coexistence policy: key seeded workflow symbols are also re-exported from `janiform.simulation` as curated convenience imports
- `janiform.simulation.shared`
  - Ownership: shared Hawkes-state and simulation error contracts used across coarse and seeded workflows
  - Root coexistence policy: core shared contracts remain re-exported from `janiform.simulation` for convenience, while `janiform.simulation.shared` is the ownership path
- `janiform.calibration.fit`
  - Ownership: default estimator-model-report fit workflow
  - Root coexistence policy: the fit classes remain re-exported from `janiform.calibration` for convenience, while `janiform.calibration.fit` is the ownership path
  - Beginner-path policy: `JaniformEstimator()` should remain usable for the common tick-data path without requiring explicit workflow scope, normalization-policy construction, or file-format selection
  - Simulation policy: `JaniformModel.simulate(...)` returns `JaniformSimulationResult`, which keeps workflow metadata and diagnostics attached while exposing `to_pandas()`, `to_numpy()`, and `to_parquet(path)`
- `janiform.calibration.fit.estimator`
  - Ownership: estimator construction and high-level fit orchestration
  - Public tuning policy: workflow-level execution controls such as `cpu_count` belong here when
    they map directly onto the canonical staged optimizer settings
- `janiform.calibration.fit.model`
  - Ownership: fitted model runtime, persistence, pricing, and simulation helpers
- `janiform.calibration.fit.report`
  - Ownership: high-level fit report contract
- `janiform.calibration.market_data`
  - Ownership: market-data request contracts, source probing, normalization handles, session planning, preparation diagnostics, and the market-data calibration entrypoints
  - Root coexistence policy: the most common market-data calibration entrypoints remain re-exported from `janiform.calibration` for convenience, while `janiform.calibration.market_data` is the ownership path
- `janiform.calibration.market_data.contracts`
  - Ownership: market-data request and normalization contracts
- `janiform.calibration.market_data.request`
  - Ownership: typed market-data request validation and serialization
- `janiform.calibration.market_data.ingestion`
  - Ownership: source-schema inspection helpers and market-data source summary contracts
- `janiform.calibration.market_data.translation`
  - Ownership: phased native market-data preparation including source probing, single-pass normalization, session planning, and translation into prepared calibration handles plus diagnostics and provenance
  - Boundary rule: translation results may expose a staged request, a native-backed normalized market-data handle, a native-backed prepared handle, artifact references, diagnostics, provenance, and fit-level summaries, but must not expose a Python-owned full-dataset IR of loaded market-data rows
- `janiform.calibration.market_data.workflow`
  - Ownership: prepared market-data calibration artifacts and workflow entrypoints
- `janiform.calibration.staged`
  - Ownership: staged execution contracts, staged execution inputs and responses, and staged runner entrypoints
  - Root coexistence policy: the root keeps the main staged runner and request boundary as convenience imports, while `janiform.calibration.staged` is the ownership path for advanced orchestration contracts
- `janiform.calibration.staged.contracts`
  - Ownership: shared staged execution contracts and stage-selection settings
  - Optimizer-setting policy: stage solver settings are the explicit low-level boundary for shared
    execution controls such as candidate-search CPU count
- `janiform.calibration.staged.inputs`
  - Ownership: advanced staged execution wrapper contracts that reference prepared payload artifacts and runtime resources
- `janiform.calibration.staged.request`
  - Ownership: staged calibration request contract
- `janiform.calibration.staged.results`
  - Ownership: staged execution response, stage report, and checkpoint report contracts
- `janiform.calibration.staged.runtime`
  - Ownership: staged runner entrypoints and native execution bridge
- `janiform.calibration.presets`
  - Ownership: preset references, registry artifacts, resolved preset workflow configuration, and preset-resolution entrypoints
  - Root coexistence policy: the root keeps the common preset-facing workflow references as convenience imports, while `janiform.calibration.presets` is the ownership path
- `janiform.calibration.presets.references`
  - Ownership: preset reference contracts and explicit override configuration models
- `janiform.calibration.presets.registry`
  - Ownership: preset artifact registries and serialization helpers
- `janiform.calibration.presets.resolution`
  - Ownership: resolved preset workflow configuration and resolution entrypoints
- `janiform.calibration.sessions`
  - Ownership: market-data session-resolution contracts and session window models
  - Root coexistence policy: session-resolution contracts stay on the ownership path instead of
    being re-exported from `janiform.calibration`
- `janiform.calibration.sessions.contracts`
  - Ownership: session-resolution contracts and weekly session window models
  - Internal-helper policy: session-registry materialization and weekly-window derivation stay in
    `janiform.calibration.sessions._resolution`, which is private implementation plumbing rather
    than part of the supported import surface
- `janiform.cli.commands.calibration`
  - Ownership: calibration CLI command handlers and calibration-specific CLI boundary normalization
- `janiform.cli.commands.simulation`
  - Ownership: explicit simulation CLI command handlers

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

The high-level runtime methods on `JaniformModel` are:

- `JaniformModel.simulate(...)`
- `JaniformModel.price_option(...)`

When the fitted model enables mean reversion, `JaniformModel.price_option(...)` requires an
explicit `observed_boundary_state: MeanReversionBoundaryState`. The high-level API keeps anchored
pricing state explicit and does not synthesize missing observed-state boundary inputs. The boundary
state now carries observed log price, anchor value, pressure state, and deviation.

The advanced staged-calibration entrypoint is:

- `run_staged_calibration`
- `StagedCalibrationResponse` carries `diagnostic_artifact_bundle` for successful in-memory runs
  and `diagnostic_artifact_references` for successful disk-mode runs

The default full staged calibration path now executes:

- `seasonality`
- `hawkes`
- `return_components`
- `mean_reversion`
- `joint_refinement`

Advanced staged execution inputs are now file-backed wrappers. `StagedCalibrationExecutionInput`
owns a small declarative staged request plus a `prepared_payload_file` reference and optional
runtime resources. The bulk evidence and mean-reversion observation grid remain in the prepared
artifact, not in Python-owned tuples.

The supported calibration ownership modules are:

- `janiform.calibration.fit`
- `janiform.calibration.fit.estimator`
- `janiform.calibration.fit.model`
- `janiform.calibration.fit.report`
- `janiform.calibration.market_data`
- `janiform.calibration.market_data.contracts`
- `janiform.calibration.market_data.request`
- `janiform.calibration.market_data.ingestion`
- `janiform.calibration.market_data.translation`
- `janiform.calibration.market_data.workflow`
- `janiform.calibration.staged`
- `janiform.calibration.staged.contracts`
- `janiform.calibration.staged.inputs`
- `janiform.calibration.staged.request`
- `janiform.calibration.staged.results`
- `janiform.calibration.staged.runtime`
- `janiform.calibration.presets`
- `janiform.calibration.presets.references`
- `janiform.calibration.presets.registry`
- `janiform.calibration.presets.resolution`
- `janiform.calibration.sessions`
- `janiform.calibration.sessions.contracts`

The calibration package root is curated convenience surface. It is not the only discoverable map of
the package anymore.

For example, both of these import styles are supported:

```python
from janiform.calibration import JaniformEstimator, MarketDataCalibrationRequest

from janiform.calibration.fit import JaniformEstimator
from janiform.calibration.fit.model import JaniformModel
from janiform.calibration.market_data import MarketDataCalibrationRequest
from janiform.calibration.market_data.translation import (
    normalize_market_data_source,
    probe_market_data_source,
    resolve_market_data_session,
    translate_market_data_request_to_prepared_calibration,
)
from janiform.calibration.staged import StagedCalibrationExecutionInput
from janiform.calibration.staged.request import StagedCalibrationRequest
from janiform.calibration.staged.results import StagedCalibrationResponse
from janiform.calibration.presets import PresetRegistry, SessionPresetReference
from janiform.calibration.presets.references import MarketProfileReference
from janiform.calibration.sessions import MarketDataSessionResolution
```

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
- non-underscored helper modules that declare themselves internal at the module docstring boundary,
  such as the CLI command-tree, rendering, configuration, and path helpers

Private modules must not be re-exported from `janiform` or from public module `__all__` lists.
Underscore-prefixed symbols must not appear in any module `__all__` list unless they are
double-underscore protocol names such as `__version__`.

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
- Does each non-underscored helper module say at the module boundary that it is internal?
- Did this change delete at least as much wrapper or duplicate surface area as it added?
- Are README, docs, examples, and export guardrails updated in the same change?
