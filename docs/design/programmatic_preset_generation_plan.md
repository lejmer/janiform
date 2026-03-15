# Programmatic Preset Generation Plan

## Purpose

Define how Janiform should create and maintain high-quality bundled presets programmatically instead of hand-authoring large numbers of JSON files.

This plan covers:

- source-of-truth strategy
- generator architecture
- validation and provenance
- testing and release workflow

The goal is to make bundled presets realistic, inspectable, reproducible, and maintainable.

## Why Programmatic Generation Is Necessary

Hand-maintained JSON does not scale well for:

- exchange sessions and holiday calendars
- early closes and special sessions
- scheduled macro-news calendars across years
- multiple bundled market profiles

Manual maintenance increases the risk of:

- transcription mistakes
- inconsistent formatting
- silent drift between presets
- poor provenance

Programmatic generation should be the default way bundled presets are produced.

## Guiding Principles

### 1. Source manifests should be compact and human-reviewable

Humans should edit concise source manifests, not giant expanded output JSON.

### 2. Generated outputs should be deterministic

Given the same generator version and inputs, the emitted preset artifacts must be byte-stable.

### 3. Provenance must be embedded

Every generated preset should record:

- generator identifier
- generator version
- source manifest identifiers
- generation date
- any upstream calendar or event-source version labels

### 4. Validation should be layered

We should validate:

- source manifests
- generated preset objects
- semantic invariants
- regression snapshots for representative cases

### 5. Bundled presets should remain inspectable artifacts

Generation should not hide outputs. The repo should still contain deterministic generated artifacts that are easy to inspect and diff.

## Proposed Architecture

Use a three-layer model.

### Layer 1: Source manifests

These are concise human-maintained inputs.

Examples:

- exchange session template manifest
- holiday rule manifest
- early-close rule manifest
- scheduled macro-event template manifest
- market profile composition manifest

These should live in a dedicated source directory, not mixed with generated outputs.

### Layer 2: Generators

Generators read manifests and emit canonical preset artifacts.

Generator types:

- session preset generator
- known-news preset generator
- market profile generator

These should run from a single command family so generation is reproducible and automated.

### Layer 3: Generated artifacts

These are the checked-in outputs used by the bundled preset registry.

Examples:

- canonical session preset JSON
- canonical known-news preset JSON
- canonical market profile preset JSON
- generated index manifests

## Session Preset Generation

Session presets should be produced from rule-based manifests rather than expanded day-by-day JSON where possible.

Recommended source model:

- exchange identity
- timezone
- weekly regular sessions
- holiday calendar code
- early-close rule set code
- any explicit exceptional overrides

The generator should expand that into the canonical preset format required by the runtime registry.

For bundled v1, it is acceptable to use small curated rule datasets maintained in-repo, as long as the final outputs are generated from them and validated.

## Known-News Preset Generation

Known-news preset generation should be template-based.

Recommended source model:

- event family identifiers
- event timezone
- recurrence rules or explicit rule builders
- effective date ranges
- cancellation rules
- observed-holiday shift rules when relevant
- event tags and semantic categories

Examples of event families:

- US CPI
- US NFP
- FOMC statement
- ECB rate decision
- US retail sales

The generator should expand these into explicit UTC timestamp schedules over a defined year range.

## Time Horizon Strategy For Known-News

Known-news presets should be generated for bounded calendar ranges, not forever.

Recommended approach:

- generate explicit event timestamps for a rolling multi-year window
- keep the source template rule-based
- regenerate bundled artifacts periodically or on release

This gives:

- reliable explicit timestamps at runtime
- small enough artifacts
- maintainable regeneration

## Market Profile Generation

Market profiles should be generated compositionally from references, not duplicated.

A market profile manifest should mainly declare:

- profile id
- display name
- default session preset reference
- suggested known-news preset reference
- default timezone assumptions
- optional column alias hints
- optional default calibration hints

The generator should resolve those references into a canonical bundled profile artifact with provenance links.

## Directory Layout Recommendation

Recommended structure:

- `presets/sources/sessions/...`
- `presets/sources/news/...`
- `presets/sources/market_profiles/...`
- `presets/generated/sessions/...`
- `presets/generated/news/...`
- `presets/generated/market_profiles/...`
- `python/janiform/presets/...` or equivalent runtime package location

The exact runtime location can be decided later. The important part is separating sources from generated outputs.

## Generator Entry Points

We should expose internal generator commands that are easy to automate.

Conceptual commands:

- `janiform dev generate session-presets`
- `janiform dev generate known-news-presets`
- `janiform dev generate market-profiles`
- `janiform dev validate presets`

These do not need to be part of the normal user CLI. They are a maintenance workflow.

## Validation Requirements

### Source manifest validation

Check:

- required fields
- valid identifiers
- timezone names
- non-overlapping session windows where required
- supported event-rule shapes

### Generated session validation

Check:

- exchange code uniqueness
- valid timezone mapping
- no malformed session windows
- exceptional dates do not contradict regular rules

### Generated known-news validation

Check:

- timestamps are strictly increasing
- timestamps are timezone-normalized to UTC
- no duplicate events within one preset unless explicitly allowed
- event families expand within configured date bounds

### Generated market profile validation

Check:

- referenced presets exist
- default references are compatible
- no dangling or circular references

## Semantic Regression Tests

We should add deterministic tests over representative generated outputs.

Recommended checks:

- selected known exchange presets match approved snapshots
- selected known-news presets match approved event counts and boundary examples
- profile resolution output remains stable
- regeneration is byte-identical for unchanged inputs

## Provenance Schema

Generated artifacts should include machine-readable provenance fields.

Recommended provenance payload:

- `generator_id`
- `generator_version`
- `source_manifest_ids`
- `generated_on`
- `upstream_source_versions`
- `content_hash`

This makes review and debugging much easier.

## Recommended Implementation Phases

### Phase 1: Generator framework

- define source manifest schemas
- define generated artifact schemas
- add deterministic serialization utilities
- add one generator command path

### Phase 2: Session preset generator

- implement compact exchange/session source manifests
- generate bundled session presets
- add validation and regression tests

### Phase 3: Known-news preset generator

- implement event-family rule manifests
- generate explicit UTC schedules for bounded year ranges
- add validation and regression tests

### Phase 4: Market profile generator

- implement compositional profile manifests
- generate bundled market profiles
- add registry index generation

### Phase 5: Maintenance workflow

- document regeneration procedure
- add CI checks for deterministic generation
- add snapshot approval workflow for bundled preset updates

## Review Workflow Recommendation

Preset changes should be reviewed at the source-manifest level first, then at the generated-artifact diff level.

A typical change should include:

1. source manifest edits
2. regenerated preset outputs
3. updated regression snapshots if needed
4. rationale in changelog or preset approval manifest

## What Should Not Be Automated Yet

For the initial version, avoid overreaching into:

- scraping arbitrary exchange websites directly in the build
- live API dependencies during generation
- automatic remote refresh inside normal user workflows

Those make reproducibility worse.

If external upstream data is needed later, it should be fetched into pinned source snapshots first, then fed through deterministic generators.

## Initial Source Manifest Scope

A practical first scope is:

- a small curated exchange/session rule library
- a small curated macro-event rule library
- a small market-profile composition library

That is enough to ship useful presets without pretending to solve all markets immediately.

## Recommended Decision

Bundled presets should be produced from compact, rule-based source manifests through deterministic generators, with generated artifacts checked into the repo and guarded by validation and regression tests.

That gives us:

- realistic bundled defaults
- maintainable preset updates
- low review burden
- clear provenance
- a path to expand coverage without manual JSON sprawl
