## Project 010: Rust Core Refactor Map And Internal Crate Split Plan

Define the refactor map and staged internal crate split for `crates/janiform` so the Rust implementation stops concentrating most domain, workflow, validation, and governance code inside one oversized crate.

### Context and requirements
- `crates/janiform` is now large enough that ownership, dependency direction, and compile boundaries are no longer clear enough.
- The main hotspots are calibration, verification, and simulation, with foundational model code, engine code, validation logic, and governance tooling all living in the same crate.
- The split should be internal-first. `janiform` should remain the umbrella facade while the implementation is decomposed behind it.
- The goal is not to hit a fixed number of crates. The goal is to split where separate ownership and dependency boundaries materially improve maintainability.
- Benchmark and regression support should not keep growing the umbrella runtime crate unnecessarily.

### Planned decomposition
- Extract foundational and model-kernel code into low-level internal crates.
- Extract market-time infrastructure into its own shared internal crate.
- Extract pricing, risk, and simulation into explicit engine crates.
- Split calibration into separate artifact and runtime ownership surfaces.
- Split stylized-fact analytics, validation domains, and governance tooling into separate internal crates.
- Keep `janiform` as a curated umbrella facade over the extracted crates.

### Definition of Done
- Planning documents the current `crates/janiform` problem clearly enough to justify the split.
- Planning defines the intended internal crate decomposition and keeps `janiform` as the umbrella facade.
- Planning defines the migration at the level of ownership boundaries and staged extraction, without requiring all splits to happen at once.
- The detailed extraction order, dependency rules, and boundary enforcement work are tracked in the project issues rather than expanded inline here.

### Agent implementation notes
- Do not treat crate count as the primary goal. Ownership clarity and dependency hygiene are the goals.
- Prefer behavior-preserving extraction first and cleanup second.
- Keep Python-specific concerns in `pybind`, not in the new internal Rust crates.
- Keep benchmark and governance support from becoming accidental production surface area.

### Initiative alignment
- Extends Initiative 006 from Python and release-surface planning into the Rust architecture that underpins those surfaces.
- Reduces the risk that v0.1 ships with a better external interface while the Rust implementation remains too monolithic to sustain iteration and ownership.

### Planning artifacts
- Policy guide: `docs/development/guides/rust_internal_crate_graph_and_umbrella_facade_policy.md`
- Machine-readable graph fixture: `tests/fixtures/rust_architecture/internal_crate_graph_policy.json`
- Temporary cargo metadata guard test: `crates/janiform/tests/internal_crate_graph_policy.rs`
- Temporary facade surface guard test: `crates/janiform/tests/umbrella_facade_surface.rs`
- Removal of the temporary guard tests is part of `JAN-264` once permanent extracted-crate enforcement replaces the planning-phase scaffolding.
