## Slim The Fat

### Description
Janiform is too large, too ambiguous at the Python boundary, and too willing to accumulate convenience layers instead of deleting weak ones. That is a production-readiness problem, not a style preference.

The intended product scope is still narrow:
- one Janiform model with calibration, pricing, simulation (seeded ticks and coarse interval), etc
- one validation harness
- one CLI
- one library surface
- Rust core with a thin Python wrapper

The current repository shape does not reflect that scope well enough. The Python wrapper is much larger than a thin control-plane layer should be, several namespaces mix public-looking layout with internal-only contents, and duplicate helpers and adapter layers continue to accumulate faster than they are removed.

This issue is a deliberate reduction pass. The goal is not to add another abstraction layer over the current shape. The goal is to delete unnecessary code, collapse duplicate logic, clarify module intent, and tighten the Rust-core / Python-wrapper split so the codebase is materially easier to maintain before first release, without removing any of the functionality itself. The user should be able to do the same things before and after the cleanup.

This is a refactor and architecture cleanup issue. It is not a feature. Do not update the initiative or project descriptions as part of this issue.

### Problem statement
- The Python wrapper is too large for a thin public control-plane layer.
- Public-looking namespaces often contain only private `_` symbols, making module intent unclear.
- Duplicate or near-duplicate helper functions exist across Python modules, tests, and Rust fixtures.
- Some Python wrappers still provide little or no boundary value beyond payload forwarding.
- Validation, serialization, and normalization logic are sometimes restated in multiple places instead of using one canonical implementation.
- The codebase often grows by local convenience decisions instead of shrinking by deliberate consolidation.

### Required design principles
- Prefer deletion over addition.
- Prefer one canonical implementation over multiple thin wrappers.
- Prefer moving shared logic to one reusable location over copying it locally.
- Prefer a small curated public API over a broad namespace with implicit or accidental public exposure.
- Prefer explicit module-boundary intent over relying on `_` prefixes alone.
- Prefer Rust for core data-plane logic and Python for control-plane orchestration only.
- Do not preserve weak abstractions for compatibility. This repository is still pre-v0.1. We must delete the words "backwards compatibility" from our minds completely, during this cleanup. The same functionality should remain, though. The same data should lead to the same calibration still, the same model should lead to the same simulation (given the same seed) etc. Nothing should functionally change. But we don't care if previous runs don't work anymore. We have no users.

### Explicit reduction policy
- No new wrapper, helper, contract, or module should be introduced unless it deletes or materially collapses an existing one in the same change.
- If a Python function only forwards to another layer without adding validation, policy injection, error mapping, telemetry, or an intentionally curated public boundary, it should be removed.
- If two functions perform the same validation, conversion, or serialization shape, they should be unified behind one canonical implementation.
- If a module sits in a public namespace but has no supported public role, it should either be moved, hidden, or documented as internal at the module boundary.
- If a test fixture manually reconstructs a large shared contract that already has a helper or builder, the fixture should use the shared helper instead.

### Required cleanup areas

### 1. Clarify public vs internal Python module intent
- Audit every module under `python/janiform/` and classify it as:
  - public surface
  - internal implementation shard
  - ambiguous and needs relocation or explicit documentation
- Public-looking modules that are implementation-only must stop being ambiguous.
- Add short module docstrings where needed so a reader can tell whether the module is supported public surface or internal implementation. Don't mention "public surface" or anything if it's a public module. But it is fine to mention that it is a private module of it is a private module.
- Do not rely on leading `_` function names alone to communicate module intent.

### 2. Collapse duplicate Python wrapper logic
- Audit Python-side helpers for:
  - payload serialization
  - payload deserialization
  - input validation
  - path normalization
  - runtime preparation
  - artifact-loading wrappers
- Remove or merge wrappers that do not add meaningful boundary value.
- Consolidate repeated helper logic into one canonical location when it is truly shared.
- Reduce repeated `from_mapping(...)`, `to_payload()`, and ad hoc loader/helper duplication where the same contract shape is already owned elsewhere.

### 3. Tighten the thin-wrapper rule
- Re-audit Python surfaces that should only be control-plane adapters:
  - `janiform.calibration.fit`
  - `janiform.calibration.market_data`
  - `janiform.calibration.staged`
  - `janiform.pricing`
  - `janiform.risk`
  - `janiform.simulation`
  - `janiform.cli`
- Remove any remaining Python-owned logic that is better treated as:
  - a native operation
  - a shared canonical contract
  - a runtime artifact handle
  - a single internal helper rather than a public-facing wrapper

### 4. Remove duplicate validation and conversion paths
- Audit validation logic duplicated across:
  - Python contracts and Python wrappers
  - multiple Python modules handling the same boundary
  - Rust fixtures and Rust test support
- Keep Python validation where it materially improves user-facing error quality.
- Remove Python-side duplication when Rust already owns the canonical validation and the Python layer is only re-stating it.
- Normalize error formatting and transport logic so platform handling is consistent without one-off hacks.

### 5. Consolidate test helpers and fixture builders
- Audit tests for repeated contract assembly and repeated fixture-shape construction.
- Move genuinely shared builders into shared test support.
- Delete local test helpers that duplicate existing ones.
- Keep fixtures explicit, but do not repeat large native/public contract payloads by hand in multiple places without need.

### 6. Reduce namespace sprawl
- Audit packages where module layout is larger than the conceptual surface actually requires.
- Collapse overly fragmented modules when the split does not provide real architectural value.
- Preserve clear domain boundaries, but stop keeping files separate only because of historical growth.
- Do not create or preserve “public-looking but implementation-only” package slices without justification.

### 7. Add contributor guardrails against future bloat
- Update contributor guidance so it explicitly rejects:
  - adding wrappers without boundary value
  - duplicating helper logic locally
  - introducing ambiguous public/internal modules
  - growing Python-owned orchestration when Rust should own the logic
- Extend review checklists and surface-map guidance to make deletion and consolidation the default expectation.

### Exact surfaces that should be audited first

### Python wrapper and namespace audit
- Audit:
  - `python/janiform/calibration/`
  - `python/janiform/cli/`
  - `python/janiform/simulation/`
  - `python/janiform/pricing.py`
  - `python/janiform/risk.py`
  - `python/janiform/hourly_factors.py`
  - `python/janiform/seasonality.py`
- For each module:
  - identify whether it is truly public
  - identify whether each wrapper adds real boundary value
  - identify repeated helpers that should be merged or deleted

### Rust fixture and support audit
- Audit:
  - `crates/janiform-validation/tests/`
  - `crates/janiform-engine-*/src/tests/`
  - shared test support crates
- Replace repeated manual contract construction with shared builders where appropriate.
- Remove duplicate helper logic that differs only by local naming.

### Documentation and governance audit
- Update:
  - `README.md`
  - `docs/development/guides/python_public_api_surface_map_and_namespace_policy.md`
  - `docs/development/guides/python_api_contract_compatibility_and_version_guardrails.md`
  - `docs/development/guides/python_control_plane_and_rust_data_plane_boundary.md`
  - any Python API or CLI guide that currently normalizes ambiguous namespace exposure
- The docs must explain why a module is public or internal, not just list names.

### Acceptance Criteria
- The Python wrapper is materially smaller after this issue. This must be achieved by deletion and consolidation, not by moving the same complexity around under new names.
- Public vs internal Python module intent is explicit enough that a reader can tell which modules are supported surfaces without guessing from `_` prefixes alone.
- Public-looking namespaces no longer contain ambiguous implementation-only modules without explanation.
- Duplicate Python wrapper logic for serialization, validation, and runtime preparation is reduced to one canonical path per boundary.
- Duplicate Rust test fixture construction is reduced through shared builders where it materially improves maintainability.
- No new compatibility shims, fallback wrappers, or dual old/new paths are introduced.
- The updated docs and contributor guidance explicitly discourage codebase growth by convenience wrappers and duplicate helpers.
- The final diff shows meaningful net code deletion or consolidation across the audited areas.
- The full Rust test suite still takes no longer than 60 seconds to run.
- The full Python test suite still takes no longer than 60 seconds to run.

### Unit tests
- Guardrail tests for public API surface classification and module-boundary expectations where appropriate.
- Contract and workflow tests updated to consume consolidated helpers rather than duplicated local payload builders.
- Regression tests proving that deleted wrapper layers did not change supported public behavior.
- Tests for any normalized cross-platform error formatting behavior affected by wrapper consolidation.

### Integration tests
- High-level CLI and library workflows still execute correctly after wrapper and module consolidation.
- Validation and artifact-backed workflows continue to operate without reintroducing Python-owned bulk data paths.
- Documentation regression tests continue to reflect the supported surface after the namespace cleanup.

### Notes/Constraints
- This issue is intentionally reduction-first. Do not add broad new abstractions to “organize” existing complexity unless that same change removes more code than it adds.
- Do not preserve weak helper layers in Python because they might become useful later, unless they really are reasonable to keep.
- Do not treat this as a comments-only cleanup. Structural consolidation and deletion are required.
- Be especially skeptical of Python helper functions that only forward into Rust or into another Python helper.
- The repository should feel professional, well-crafted and production ready.