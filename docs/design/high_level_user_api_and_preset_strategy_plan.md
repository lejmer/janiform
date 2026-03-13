# High-Level User API And Preset Strategy Plan

## Purpose

Define a new default Janiform surface for most users while preserving the current staged contracts and low-level runtime wiring for power users.

This plan focuses on:

- high-level fit and simulate workflows
- bundled session and known-news presets
- how presets compose with user overrides
- how the library and CLI should align conceptually

It does not lock in exact function signatures. That will come later.

## Product Positioning

Janiform should expose two layers:

- High-level default API for most users
- Low-level advanced API for power users and internal debugging

The current staged calibration and explicit runtime-contract APIs should remain available, but they should stop being the first thing a normal user encounters.

The new default experience should let a user:

1. point Janiform at tick data
2. choose a realistic market preset or explicit configuration
3. fit a model
4. simulate later over any supported horizon and interval

## Core Design Principles

### 1. Presets are first-class, not examples

Bundled market defaults should be exposed as maintained preset artifacts with provenance, versioning, and validation, not as informal examples.

### 2. Session inference should be the default for common market-data workflows

When the input data and workflow context provide enough evidence, Janiform should infer session behavior automatically instead of requiring the user to pick a session preset or supply calendar JSON up front.

Bundled session presets still matter, but they should be positioned as:

- curated deterministic defaults
- higher-confidence override paths
- convenient named alternatives when the user knows the intended market

### 3. Bundled defaults are good starting points, not invisible truth

Presets should be realistic and useful out of the box, but clearly identified as bundled defaults. Users must be able to inspect, override, and replace them.

### 4. Known-news defaults must be opt-in

Session defaults can be safely opinionated. Known-news defaults should not be silently applied because they materially change model behavior.

### 5. One conceptual workflow across Python and CLI

The library and CLI should both expose the same mental model:

- choose or define a market profile
- fit a model
- simulate later

### 6. Low-level contracts remain available

Advanced users should still be able to:

- supply raw session JSON
- supply explicit known-news inputs
- work directly with staged artifacts
- construct runtime contracts manually

## Preset Categories

We should define three preset categories.

### Session presets

These define trading sessions and calendar rules.

Examples:

- `XNYS`
- `XNAS`
- `LSE`
- `fx_spot_24_5`
- `crypto_24_7`

These are the safest bundled defaults and should be easy to use by name.

They should not be mandatory for the high-level fit flow when session inference can resolve the same information directly from the data with acceptable confidence.

### Known-news presets

These define opt-in scheduled-event calendars for common macro workflows.

Examples:

- `none`
- `us_macro_core`
- `eu_macro_core`
- `eu_us_macro_fx_core`

These should default to off unless explicitly selected.

### Market profile presets

These are convenience bundles that point to recommended session and news defaults plus workflow hints.

Examples:

- `eurusd_spot`
- `us_equity_cash`
- `btcusd_spot`

Each market profile should describe:

- default session preset
- suggested known-news preset
- default timestamp timezone assumptions
- default column aliases when useful
- default calibration policy hints when useful

## Configuration Composition Model

The preset system should support four usage modes.

### Mode 1: Automatic inference default usage

For common market-data workflows:

- user provides data
- user may provide market or instrument hints
- Janiform infers session behavior automatically when confidence is sufficient

### Mode 2: Name-only preset usage

For most users:

- `market="eurusd_spot"`
- `exchange="XNYS"`

### Mode 3: Name plus overrides

For users who want a bundled starting point with small changes:

- market preset plus custom timezone
- session preset plus custom holiday overrides
- known-news preset plus additional event dates

### Mode 4: Fully explicit configuration

For advanced users:

- raw dict
- raw JSON file
- custom registry object

## Override Policy

Composition should be deterministic and simple.

Recommended order:

1. inferred session behavior or bundled preset baseline
2. user override patch
3. full explicit replacement if supplied

This should apply separately to:

- session settings
- known-news settings
- market profile settings

## High-Level Object Model

The high-level API should follow the common Python pattern of:

- one object for fit-time configuration and fitting
- one object for the fitted model and downstream use

Recommended objects:

- `JaniformEstimator`
- `JaniformModel`
- `JaniformFitReport`

### `JaniformEstimator`

Responsibilities:

- hold fit-time configuration
- resolve presets and session-inference policy
- validate user-facing inputs
- fit on market data
- return a fitted model, with an optional fit report

Recommended fit behavior:

- `estimator.fit(...) -> JaniformModel`
- `estimator.fit(..., report=True) -> tuple[JaniformModel, JaniformFitReport]`

### `JaniformModel`

Responsibilities:

- encapsulate calibrated Hawkes, return, seasonality, and news components
- retain model provenance and fit metadata
- expose fitted parameters in a clean inspectable form
- materialize runtime controls internally
- expose simulation and pricing methods
- save and load cleanly

Likely responsibilities on the object:

- `simulate(...)`
- `price_option(...)`
- `summary()`
- `save(...)`
- `load(...)`
- a fitted-parameter inspection surface such as `parameters` or `get_parameters()`

### `JaniformFitReport`

Responsibilities:

- hold detailed calibration diagnostics
- expose optimizer and stage-level evidence
- provide observed-versus-fitted diagnostic views
- remain separate from the fitted model so normal model objects stay lean

This separation is important. The fitted model should expose the fitted parameters and concise
metadata, but it should not automatically carry the full calibration report payload.

### Data-retention rule

Neither `JaniformEstimator` nor `JaniformModel` should store the raw market data by default.

Raw data should be:

- an input to `fit(...)`
- optionally referenced through provenance metadata
- optionally persisted separately in artifacts when the workflow requests it
- not embedded inside the estimator or fitted model object

## High-Level Simulation API Shape

The default simulation surface should use one method rather than separate names for coarse and seeded-tick generation.

Recommended shape:

- `simulate(..., output="coarse" | "ticks" | "both")`

In Python typing terms, this should likely be represented as:

- `Literal["coarse", "ticks", "both"]`

This is preferable to separate method names because it:

- mirrors the CLI more naturally
- keeps output choice as a runtime mode, not a different conceptual workflow
- reduces surface-area sprawl
- makes combined generation an ordinary case instead of a special wrapper

The returned object can be mode-dependent or can be a unified response object with optional fields.
For the high-level Python surface, pandas data frames are the most likely default return type for
simulation and pricing outputs because that matches user expectations in the Python ecosystem.

## High-Level Fit Flow

The default library flow should look conceptually like this:

1. user supplies tick data source
2. user optionally supplies market preset name, exchange hint, or explicit config
3. Janiform resolves presets and infers session behavior when enough evidence exists
4. `JaniformEstimator` runs calibration
5. `fit(...)` returns a `JaniformModel`, and optionally a `JaniformFitReport`

The user should not need to know about:

- staged artifacts
- stage parameter payloads
- hourly factor materialization internals
- native request contracts

## Library Preset Entry Model

The library should accept a preset or explicit config through a boundary value object rather than loose ad hoc kwargs everywhere.

Conceptually:

- `MarketPresetReference`
- `SessionPresetReference`
- `KnownNewsPresetReference`
- `MarketProfileReference`

These can resolve from:

- a built-in name
- an inferred session result
- a registry lookup
- a dict
- a JSON document

For the public boundary, each category should follow one typed reference pattern instead of loose kwargs:

- `usage_mode="automatic" | "named" | "explicit"` with `automatic` allowed only for sessions
- named references require `registry_scope="built_in" | "user_registered"` plus `preset_name`
- named references may include a category-specific `override_patch`
- explicit references require a category-specific `configuration` object and must not also carry an override patch

Category-specific payloads should stay user-oriented:

- sessions: exchange code, timezone, regular windows, recurring closed windows
- known-news: event list plus optional calendar timezone metadata
- market profiles: nested session and known-news references, known-news application policy, timezone assumptions, and optional alias or calibration hints

## CLI Model

The CLI should mirror the same layering.

Conceptual commands:

- `janiform fit ...`
- `janiform fit plan ...`
- `janiform fit staged ...`
- `janiform simulate ...`
- `janiform simulate coarse-interval ...`
- `janiform simulate seeded-ticks ...`
- `janiform presets list`
- `janiform presets show ...`
- `janiform presets export ...`

The CLI should let a user do:

- omit session input entirely and rely on inference
- `--market eurusd_spot`
- `--session-preset fx_spot_24_5`
- `--known-news eu_us_macro_fx_core`
- `--known-news none`
- `--session-json ...`
- `--known-news-json ...`

## Registry Design

We should expose a public registry abstraction for preset resolution.

Capabilities:

- resolve built-in presets
- register user-defined presets in memory
- load user-defined presets from JSON
- export resolved presets for inspection
- resolve one canonical workflow configuration that records component provenance, override fields, session-inference diagnostics, and known-news recommendation outcomes

We should keep built-in and user registries separate conceptually, even if the runtime resolver composes them.

Session inference should feed into this system as a resolved session configuration with provenance, not as a parallel hidden calendar stack.

## Resolution Precedence

High-level workflows should resolve one canonical configuration object after preset lookup and session inference.

For each component category, precedence should be explicit and deterministic:

1. inferred session behavior or bundled preset baseline
2. user override patch
3. full explicit replacement when supplied

Apply that stack separately for:

- session settings
- known-news settings
- market-profile settings

Resolution diagnostics should explain when:

- session inference was low confidence or ambiguous
- an explicit user selection replaced an inferred or profile-derived baseline
- a market profile recommended a component but the resolved configuration kept a different explicit choice

## Provenance Requirements

Every fitted model should record:

- which market preset was used, if any
- which session preset was used, if any
- whether the session was inferred from data
- which known-news preset was used, if any
- whether each component was built-in, overridden, inferred, or fully custom
- when profile recommendations were inspected but not applied, such as the default `known_news="none"` path for suggest-only market profiles

This is critical for reproducibility and debugging.

## Known-News Policy

Known-news presets should be explicit and inspectable.

Recommendations:

- default high-level policy should be `known_news="none"`
- market profile presets may recommend a known-news preset without auto-applying it unless the market profile explicitly says so
- the fitted model summary should always show whether known-news was active

## Recommended Phase Plan

### Phase 1: Preset model and registry foundations

- define preset schema types in Python and Rust-facing contracts where needed
- add built-in registry loader
- add user registry and JSON loading support
- implement resolution and override merge rules
- define how inferred session results enter the same resolved configuration model

### Phase 2: High-level fit object

- add `JaniformEstimator`
- add `JaniformModel`
- add `JaniformFitReport`
- add fit helpers for CSV and dataframe-like inputs
- store preset provenance and runtime materialization inputs
- integrate automatic session inference as the default session-resolution path

### Phase 3: High-level simulation API

- add one `simulate(...)` object method with an output mode selector
- add `price_option(...)` on the fitted model object
- hide runtime factor wiring internally
- support arbitrary fixed interval specs
- define pandas-first high-level result objects or data-frame outputs

### Phase 4: CLI exposure

- add preset discovery commands
- add high-level fit and simulate commands
- keep low-level commands available for advanced workflows

### Phase 5: Docs and examples

- rewrite top-level examples to use the high-level API
- move staged calibration and explicit runtime wiring into advanced docs

## Non-Goals For The First Iteration

- perfect global exchange coverage
- exhaustive known-news coverage
- automatic remote syncing of exchange rules or event calendars
- hiding all advanced concepts from expert users

## Initial Bundled Preset Set

A minimal first set should be deliberately small.

Recommended v1:

- Session presets:
  - `XNYS`
  - `XNAS`
  - `fx_spot_24_5` (opens at 21:00 UTC+0 on Sundays, and closes week at 22:00 UTC+3 on Fridays)
  - `crypto_24_7`
- Known-news presets:
  - `none` (default)
  - `us_macro_core`
  - `eu_us_macro_fx_core`
- Market profiles:
  - `eurusd_spot`
  - `us_equity_cash`
  - `btcusd_spot`

## Open Questions To Resolve Later

- whether market profiles should auto-enable recommended known-news presets or only suggest them
- whether built-in preset versioning should be exposed directly to users
- whether the high-level fit flow should default to disk-backed artifacts or be memory-first with optional export
- whether preset registry export should emit canonical JSON or richer model metadata bundles

## Recommended Decision

Adopt a preset architecture built around:

- default automatic session inference when confidence is sufficient
- built-in session presets
- opt-in built-in known-news presets
- bundled market profile presets
- user-extensible registries
- explicit provenance in fitted models

That gives most users a short, reliable path while keeping curated deterministic presets and the current low-level contracts available for advanced workflows.
