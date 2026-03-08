## Refactor Calibration Module

### Description
`crates/core/src/calibration.rs` is currently too large and mixes unrelated concerns (contracts, optimizer composition, stage execution, checkpointing, orchestration, and tests) in one file.

Refactor the calibration module into a multi-file layout with clear boundaries while preserving the existing public API surface at `janiform_core::calibration::*` via re-exports from a module facade.

This refactor is structural only. It must not change runtime behavior, schema constants, serialized payload shapes, or mode-aware orchestration semantics.

### Acceptance Criteria
- `crates/core/src/calibration.rs` is replaced by a directory-based module with focused files for:
  - adapters
  - contracts/run-spec and stage graph
  - objective composition
  - constrained optimizer adapter
  - seasonality stage
  - Hawkes stage
  - return-components stage
  - checkpointing
  - staged runner orchestration
- `crates/core/src/calibration/mod.rs` re-exports all currently public calibration items so existing import paths continue to work.
- No public API breakage for existing in-repo consumers, including:
  - `crates/core/src/hourly_factor_provider.rs` test imports from `crate::calibration`
  - `crates/core/tests/pricing_calibration_hook_consumer_integration.rs` imports from `janiform_core::calibration`
- Existing schema/version constants and serialized structures remain unchanged.
- All calibration tests pass after relocation, and no calibration behavior regressions are introduced.
- Documentation is updated for module path references:
  - `docs/development/guides/calibration_optimizer_adapter_and_objective_composition_schema.md`
  - verify references in `docs/development/guides/pricing_diagnostics_and_calibration_hooks_schema.md` remain accurate

### Unit tests
- Relocate existing calibration unit tests to module-local test blocks and shared `#[cfg(test)]` support where needed.
- Keep coverage for:
  - objective composition correctness and invalid input handling
  - optimizer adapter determinism and validation paths
  - run spec and stage graph validation, including mode-incompatible classifications
  - stage execution success and failure in `memory` and `disk` modes
  - staged orchestration checkpoint emission, resume, compatibility checks, and failure modes

### Notes/Constraints
- Keep this refactor structural and DRY. Do not duplicate logic while splitting files.
- Preserve deterministic behavior and existing error messages unless a move requires a mechanical adjustment.
- Avoid unintended visibility changes. Default to `pub(crate)` for new internal helpers.
- Do not introduce global mutable state.
- Keep stage/orchestration contracts aligned with JAN-122 and JAN-130.
