## Refactor Verification Domain And Harness Modules

### Description
`crates/janiform/src/validation.rs` currently acts as a mixed regression and verification harness module, while other verification-heavy workflows live in `crates/janiform/src/benchmarking.rs` and `crates/janiform/src/affine_oracle.rs`. This spreads verification orchestration across multiple large files and makes maintenance harder.

Refactor verification and regression harness code into a dedicated `crates/janiform/src/verification/` domain with focused modules, and keep domain input validation logic in its domain modules (`pricing`, `risk`, `hawkes`, `seasonality`, `calendar`, `news`, etc).

This refactor is structural. Do not preserve legacy module paths. We are in initial development, so direct path updates are allowed.

### Acceptance Criteria
- `crates/janiform/src/validation.rs` is removed and replaced by `crates/janiform/src/verification/`.
- `crates/janiform/src/verification/mod.rs` provides the public verification surface.
- Verification harness code is split into focused modules, including at minimum:
  - deterministic regression harness
  - open/closed gap leakage regression harness
  - sufficient statistics multi-interval validation suite
  - shared verification contracts and errors
- Verification-oriented functionality currently in other large modules is moved into `verification` submodules, including:
  - affine oracle method-comparison harness and tolerance-policy/report types
  - coarse/fine consistency regression harness and tolerance-policy loading
  - performance baseline comparison and regression drift attribution report building
- `crates/janiform/src/lib.rs` exports `pub mod verification;` and no longer exports `pub mod validation;`.
- All in-repo imports are updated to the new module paths (core, pybind, tests, and docs where relevant).
- No behavior or schema payload changes are introduced by relocation.
- No compatibility re-exports are added.

### Unit tests
- Existing harness and regression tests continue to pass after relocation.
- Update tests to ensure moved verification modules still produce deterministic outputs under fixed seeds where previously guaranteed.

### Notes/Constraints
- Keep this refactor DRY and structural. Avoid rewriting logic unless needed for module boundary cleanup.
- Domain-level `validate()` methods remain in their domain modules and should not be moved into `verification`.
- Prefer small focused files over new large aggregators.
- Keep serialization/report schema constants unchanged.
