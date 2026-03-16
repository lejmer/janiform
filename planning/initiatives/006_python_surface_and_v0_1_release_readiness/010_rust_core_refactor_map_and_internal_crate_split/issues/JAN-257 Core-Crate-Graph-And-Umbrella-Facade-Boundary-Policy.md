## Core Crate Graph And Umbrella Facade Boundary Policy

### Description
Establish the target internal crate graph and the umbrella-facade policy for `janiform` before code extraction begins, so the refactor is driven by explicit dependency rules rather than by a sequence of local moves.

### Implementation details
- Define the initial internal workspace crate graph for:
  - foundations
  - model kernels
  - affine oracle
  - market-time infrastructure
  - engine crates
  - calibration crates
  - validation and analytics crates
  - governance crates
- Keep `janiform` as the umbrella facade crate during the migration.
- Define which items should continue to be re-exported from `janiform` and which items should remain crate-local to the new internal crates.
- Define forbidden dependency directions, especially:
  - engines must not depend on calibration runtime
  - engines must not depend on validation or governance
  - governance may depend on engines and validation outputs, but not the reverse
  - benchmark-only support must not grow production crate surfaces
- Define the review and CI checks needed to keep the target crate graph from regressing while the migration is incomplete.

### Acceptance Criteria
- Planning or implementation artifacts define one explicit internal crate graph and one explicit umbrella-facade policy for `janiform`.
- The facade policy keeps public churn bounded while internal crates are introduced.
- Dependency direction rules are explicit enough that future extraction work can be evaluated mechanically.
- Benchmark, regression, and validation helpers are explicitly prevented from leaking into foundational or engine crates unless justified.

### Unit tests
- Cargo metadata or crate-graph checks asserting the allowed internal dependency directions.
- Re-export surface tests asserting `janiform` continues to provide the curated facade expected by pybind and in-repo callers during migration.

### Notes/Constraints
- Do not start extracting crates before the graph and facade policy are written down.
- The goal is not to expose every internal crate publicly. The goal is to define real ownership and compile boundaries behind the facade.
