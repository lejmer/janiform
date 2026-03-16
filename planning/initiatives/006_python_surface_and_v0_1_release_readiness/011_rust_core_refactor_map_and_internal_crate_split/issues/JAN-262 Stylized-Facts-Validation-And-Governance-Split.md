## Stylized Facts Validation And Governance Split

### Description
Separate stylized-fact analytics, engine-facing validation domains, and benchmark or regression governance into distinct internal crates so repository verification logic stops accumulating inside one catch-all verification surface.

### Implementation details
- Extract stylized-fact analysis into a dedicated internal analytics crate.
- Extract engine-facing validation domains into a validation crate, including:
  - scale consistency
  - coarse-fine consistency
  - coarse-seeded consistency
  - open-closed gap leakage
  - sufficient-statistics validation
  - validation contracts and method comparison helpers
- Extract benchmark and regression governance into a governance crate, including:
  - benchmark fixture support
  - performance baselines
  - deterministic regression orchestration
  - regression drift attribution
- Physically move the extracted source trees, crate-local helpers, and unit tests into the target crates. Do not count `#[path = "..."]` indirection from `crates/core` or Cargo-only dependency changes as completing this split.
- Ensure validation crates can depend on engines, but engines do not depend back on validation.
- Ensure governance can consume validation and engine outputs without becoming a dependency of production runtime crates.

### Acceptance Criteria
- Stylized-fact analytics, validation domains, and governance tooling are no longer co-owned inside one verification module tree.
- The extracted analytics, validation, and governance crates become the physical implementation owners for the moved code and their unit tests.
- Governance code has a dedicated internal home outside production engine crates.
- Benchmark and regression helpers no longer grow foundational or engine crate surfaces unnecessarily.
- Existing regression and governance behavior remains intact through the split.

### Unit tests
- Stylized-fact regression and reporting tests continue in the extracted analytics crate.
- Validation regression suites continue in the extracted validation crate.
- Governance smoke and baseline tests continue in the extracted governance crate.
- Dependency checks assert engines do not depend on validation or governance crates.

### Notes/Constraints
- Treat governance as a repository and release-quality concern, not as a runtime dependency.
- Keep terminology precise so validation domains and governance policies do not blur together again.
- For context, see the full plan: docs/development/guides/`rust_internal_crate_graph_and_umbrella_facade_policy.md`.
