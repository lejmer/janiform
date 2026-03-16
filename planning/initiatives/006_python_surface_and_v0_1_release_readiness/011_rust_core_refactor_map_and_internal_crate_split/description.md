## Project 011: Rust Core Refactor Map And Internal Crate Split Plan

Define a concrete refactor map for `crates/core` and a staged internal crate decomposition plan so the Rust implementation stops concentrating nearly all domain, workflow, validation, and governance concerns inside one oversized crate.

### Context and requirements
- The current checkout contains approximately `128,900` Rust lines under `crates/core`, including approximately `98,704` lines in `crates/core/src`, approximately `30,072` lines in `crates/core/tests`, and approximately `124` lines in `crates/core/benches`.
- The largest production areas are:
  - `calibration`: approximately `28,179` lines
  - `verification`: approximately `24,548` lines
  - `simulation`: approximately `9,078` lines
  - `pricing`: approximately `5,761` lines
  - `risk`: approximately `5,544` lines
- The largest individual production files include:
  - `calibration/stages/return_components.rs`: approximately `4,334` lines
  - `simulation/coarse.rs`: approximately `2,183` lines
  - `calibration/market_data_preprocessing.rs`: approximately `1,955` lines
  - `verification/coarse_fine_consistency.rs`: approximately `1,817` lines
  - `calibration/stages/hawkes.rs`: approximately `1,798` lines
  - `simulation/runtime.rs`: approximately `1,781` lines
- The current `janiform-core` crate is simultaneously carrying:
  - mathematical kernels and stochastic model primitives
  - market-time and factor materialization logic
  - pricing, risk, and simulation engines
  - calibration execution and artifact management
  - validation, regression, and stylized-fact analysis
  - benchmark governance helpers and benchmark fixture support
- This concentration makes ownership blurry, encourages cross-layer imports, inflates compile and test scope, and makes it too easy for internal helper code to become permanent library surface by accident.
- The refactor target is internal decomposition first. The public Python and CLI story can remain anchored on the same high-level workflows while the Rust implementation is split behind that facade.
- The plan should not target an arbitrary fixed number of crates. It should split only where a separate ownership boundary, compile boundary, or dependency boundary is materially justified.

### Current-state refactor map

#### A. Model and kernel foundations
- `complex_math`
- `randomness`
- `hawkes`
- `news`
- `nig`
- `return_decomposition`
- `risk_neutral`
- `affine_oracle`

Current problem:
- These modules are conceptually foundational, but they live beside workflow orchestration code and can currently be reached directly by higher-level domains with no strong boundary enforcement.

#### B. Market-time and factor infrastructure
- `calendar`
- `seasonality`
- `hourly_factors`

Current problem:
- These modules are coherent as a market-time and factor-resolution subsystem, but they are currently embedded directly in the same crate as pricing, simulation, and calibration execution.

#### C. Engine execution layer
- `pricing`
- `risk`
- `simulation`

Current problem:
- These are real runtime engines and should be owned as engines, not as just more modules inside the same top-level crate.
- `simulation` is especially large and currently mixes contracts, runtime execution, artifact projections, and tests in one area.

#### D. Calibration workflow layer
- `calibration/contracts`
- `calibration/artifact_registry`
- `calibration/checkpointing`
- `calibration/stages`
- `calibration/staged_runner`
- `calibration/execution_bridge`
- `calibration/market_data_preprocessing`
- `calibration/optimizer_adapter`
- `calibration/objective`
- `calibration/adapters`

Current problem:
- Calibration currently mixes persistent artifact schemas, artifact lookup and reload logic, stage execution, market-data preprocessing, workflow orchestration, and optimizer integration in one ownership surface.
- The size concentration is large enough that calibration should no longer be treated as one module.

#### E. Validation and governance layer
- `verification/scale_consistency`
- `verification/stylized_facts`
- `verification/coarse_fine_consistency`
- `verification/coarse_seeded_tick_consistency`
- `verification/open_closed_gap_leakage`
- `verification/sufficient_statistics`
- `verification/deterministic_regression`
- `verification/performance_baseline`
- `verification/regression_drift_attribution`
- `verification/contracts`
- `benchmarking`

Current problem:
- Validation analytics, regression harnesses, benchmark baselines, and benchmark fixture governance are too entangled.
- The current structure makes it hard to tell what is a reusable engine-facing validation domain versus what is purely governance tooling for CI and benchmark artifact management.

### Proposed internal crate decomposition

Keep `janiform-core` as an umbrella facade crate during and after the split. Over time it should become intentionally thin and mostly re-export the internal crates that define the real ownership boundaries.

#### 1. `janiform-foundation`
Contents:
- `complex_math`
- `randomness`

Why this split makes sense:
- These utilities are fundamental building blocks with broad reuse and very low policy content.
- They should compile independently of pricing, simulation, calibration, or verification workflows.

#### 2. `janiform-model`
Contents:
- `hawkes`
- `news`
- `nig`
- `return_decomposition`
- `risk_neutral`

Why this split makes sense:
- These modules define the core stochastic model and model-adjacent primitives.
- They form a coherent ownership area for model semantics and numerical domain rules.

#### 3. `janiform-affine-oracle`
Contents:
- `affine_oracle`

Why this split makes sense:
- The affine transform solver is a shared engine kernel consumed by multiple higher-level domains.
- It is large enough and central enough to deserve a direct compile and ownership boundary instead of living implicitly inside the general model crate.

#### 4. `janiform-market-time`
Contents:
- `calendar`
- `seasonality`
- `hourly_factors`

Why this split makes sense:
- These modules define one coherent subsystem for session handling, market-time preprocessing, and factor materialization.
- They are shared by simulation, calibration, and pricing workflows but are not themselves engine execution code.

#### 5. `janiform-pricing-engine`
Contents:
- `pricing`

Why this split makes sense:
- Pricing is a distinct engine with its own contracts, numerical settings, and execution semantics.
- It should depend on model kernels and market-time infrastructure, not on calibration or verification internals.

#### 6. `janiform-risk-engine`
Contents:
- `risk`

Why this split makes sense:
- Risk sampling is a distinct engine surface with its own contracts and transform-sampling execution logic.
- It shares kernels with pricing but should not remain bundled with unrelated workflow code.

#### 7. `janiform-simulation-engine`
Contents:
- `simulation`

Why this split makes sense:
- Simulation is one of the largest runtime subsystems and mixes runtime execution, artifacts, and public engine adapters.
- It is large enough to justify separate engine ownership.

#### 8. `janiform-calibration-artifacts`
Contents:
- `calibration/contracts`
- `calibration/artifact_registry`
- `calibration/checkpointing`

Why this split makes sense:
- Artifact schemas, artifact manifests, dataset snapshots, checkpointing, and reload behavior are persistent workflow state concerns.
- They should not be coupled directly to stage execution and optimizer orchestration.

#### 9. `janiform-calibration-runtime`
Contents:
- `calibration/stages`
- `calibration/staged_runner`
- `calibration/execution_bridge`
- `calibration/market_data_preprocessing`
- `calibration/optimizer_adapter`
- `calibration/objective`
- `calibration/adapters`

Why this split makes sense:
- This is the actual execution layer for calibration workflows.
- It should depend on calibration artifacts and engine crates, not own those artifacts directly.

#### 10. `janiform-stylized-facts`
Contents:
- `verification/stylized_facts`

Why this split makes sense:
- Stylized-fact analysis is already a sizable, coherent analytics domain with its own metric catalog, reporting, and comparisons.
- It is large enough to justify dedicated ownership instead of remaining a subfolder inside a catch-all verification crate.

#### 11. `janiform-validation`
Contents:
- `verification/scale_consistency`
- `verification/coarse_fine_consistency`
- `verification/coarse_seeded_tick_consistency`
- `verification/open_closed_gap_leakage`
- `verification/sufficient_statistics`
- `verification/contracts`
- `verification/affine_oracle_method_comparison`

Why this split makes sense:
- These modules are validation and consistency domains tied directly to model and engine behavior.
- They are conceptually separate from benchmark baselines and CI governance artifacts.

#### 12. `janiform-governance`
Contents:
- `benchmarking`
- `verification/performance_baseline`
- `verification/regression_drift_attribution`
- `verification/deterministic_regression`

Why this split makes sense:
- This is the repository’s benchmark and regression governance layer.
- It should own fixture-driven benchmark harnesses, baseline capture, drift attribution, and related CI-facing contracts.
- This split also creates a natural place for bench-only support to migrate out of the umbrella core crate over time.

### Proposed dependency direction

Preferred direction:

- `janiform-foundation` -> no internal crate dependencies
- `janiform-model` -> `janiform-foundation`
- `janiform-affine-oracle` -> `janiform-foundation`, `janiform-model`
- `janiform-market-time` -> `janiform-foundation`
- `janiform-pricing-engine` -> `janiform-foundation`, `janiform-model`, `janiform-affine-oracle`, `janiform-market-time`
- `janiform-risk-engine` -> `janiform-foundation`, `janiform-model`, `janiform-affine-oracle`
- `janiform-simulation-engine` -> `janiform-foundation`, `janiform-model`, `janiform-market-time`, `janiform-risk-engine`
- `janiform-calibration-artifacts` -> `janiform-foundation`, `janiform-model`, `janiform-market-time`
- `janiform-calibration-runtime` -> `janiform-foundation`, `janiform-model`, `janiform-market-time`, `janiform-pricing-engine`, `janiform-risk-engine`, `janiform-simulation-engine`, `janiform-calibration-artifacts`
- `janiform-stylized-facts` -> `janiform-foundation`, `janiform-market-time`, `janiform-simulation-engine`
- `janiform-validation` -> `janiform-foundation`, `janiform-model`, `janiform-affine-oracle`, `janiform-pricing-engine`, `janiform-risk-engine`, `janiform-simulation-engine`, `janiform-market-time`
- `janiform-governance` -> `janiform-foundation`, `janiform-validation`, `janiform-stylized-facts`, `janiform-pricing-engine`, `janiform-risk-engine`, `janiform-simulation-engine`
- `janiform-core` -> re-export facade over the internal crates
- `janiform-pybind` -> depend on `janiform-core` or on explicitly curated workflow crates, but not on low-level helper crates ad hoc

Dependency rules to enforce:
- Model and engine crates must never depend on calibration runtime, validation, or governance crates.
- Validation crates may depend on engines, but engines must not depend back on validation.
- Governance crates may depend on validation outputs and engine contracts, but no production engine crate should depend on governance.
- Bench-only support should not be added to production engine crates just to accommodate local benchmarking.

### Ownership model after the split

Target ownership surfaces:

- Foundation and model owners
  - `janiform-foundation`
  - `janiform-model`
  - `janiform-affine-oracle`
- Market-time owners
  - `janiform-market-time`
- Engine owners
  - `janiform-pricing-engine`
  - `janiform-risk-engine`
  - `janiform-simulation-engine`
- Calibration workflow owners
  - `janiform-calibration-artifacts`
  - `janiform-calibration-runtime`
- Validation and analytics owners
  - `janiform-validation`
  - `janiform-stylized-facts`
- Governance owners
  - `janiform-governance`

This ownership model is the main reason to split. The goal is not crate count for its own sake. The goal is to make it obvious who owns which rules and to keep policy-heavy workflow code from quietly leaking back into foundational crates.

### Migration sequence

#### Phase 1: Establish the facade and dependency rules
- Keep `janiform-core` as the public umbrella crate.
- Define the desired internal crate graph and forbid reverse dependencies before moving large amounts of code.
- Add cargo-level and review-level rules that prevent new workflow code from landing in foundational areas.

#### Phase 2: Extract the lowest-risk foundational crates
- Extract `janiform-foundation`
- Extract `janiform-model`
- Extract `janiform-affine-oracle`

Rationale:
- These splits create the strongest compile-boundary wins with the lowest workflow-level churn.

#### Phase 3: Extract market-time and engine crates
- Extract `janiform-market-time`
- Extract `janiform-pricing-engine`
- Extract `janiform-risk-engine`
- Extract `janiform-simulation-engine`

Rationale:
- This phase turns the runtime code into explicit engine surfaces and removes the largest non-calibration workflow domains from the umbrella core implementation.

#### Phase 4: Split calibration by responsibility
- Extract `janiform-calibration-artifacts`
- Extract `janiform-calibration-runtime`

Rationale:
- This is the largest ownership hotspot in the current crate and should not be moved as one unit.
- Artifacts and orchestration need separate boundaries.

#### Phase 5: Split validation, analytics, and governance
- Extract `janiform-stylized-facts`
- Extract `janiform-validation`
- Extract `janiform-governance`

Rationale:
- This phase removes the other major hotspot after calibration.
- It also gives benchmark and regression governance a home outside the umbrella runtime crate.

#### Phase 6: Thin the umbrella crate
- Convert `janiform-core` into a deliberately thin facade with curated re-exports.
- Remove implementation-heavy helpers from the umbrella crate whenever they belong to a more specific internal crate.
- Audit bench and test support so benchmark-only helpers do not live in production crates unnecessarily.

### Definition of Done
- Planning documents a current-state refactor map for `crates/core`, including the major size hotspots and subsystem boundaries.
- Planning defines an internal crate decomposition that separates foundations, model kernels, market-time infrastructure, engines, calibration workflows, validation analytics, and governance tooling.
- Planning keeps `janiform-core` as an umbrella facade rather than forcing immediate public crate churn.
- Planning defines dependency-direction rules that prevent higher-level workflow crates from leaking back into foundational crates.
- Planning defines a staged migration order that starts with low-risk foundations and only then moves calibration and governance hotspots.
- Planning explicitly identifies benchmark governance and bench-only support as concerns that should not grow the umbrella runtime crate unnecessarily.

### Agent implementation notes
- Do not split crates mechanically by file count. Split only where the boundary reflects real ownership or dependency rules.
- Prefer one umbrella facade crate over forcing pybind and in-repo callers to absorb every internal crate move immediately.
- Treat calibration artifacts and calibration runtime as different ownership surfaces.
- Treat validation domains and governance tooling as different ownership surfaces.
- Use the split to reduce accidental library-surface growth, especially for benchmark and regression helpers.
- When implementation begins, each extraction should preserve behavior first and clean up API shape second.

### Non-goals
- This project does not change the Python API by itself.
- This project does not redesign the model or numerical methods.
- This project does not commit to stabilizing every internal crate as a public external dependency.
- This project does not require every proposed crate to be extracted in one PR.

### Initiative alignment
- Extends Initiative 006 beyond Python surface cleanup into the Rust architecture that underpins the Python and CLI release surface.
- Reduces the risk that v0.1 ships with improved user-facing workflows while the Rust implementation remains too monolithic to sustain ownership and iteration.
