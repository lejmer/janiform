## Split Simulation Module Into Submodules

### Description
Refactor `crates/janiform/src/simulation.rs` into `crates/janiform/src/simulation/mod.rs` with smaller domain-specific files and stable public paths.

### Acceptance Criteria
- Replace single-file `simulation.rs` with `crates/janiform/src/simulation/` and keep `janiform::simulation` API stable.
- Preserve all existing public methods, structs, and type aliases currently exposed from `simulation`.
- Keep `mod.rs` as just the module entrypoint.
- Keep `pub mod simulation;` in `crates/janiform/src/lib.rs` unchanged.
- Ensure dependent modules and tests compile without import-path edits.

### Unit tests
- Re-run simulation-focused unit and integration tests.

### Notes/Constraints
- Keep simulation-specific initialization and state transition logic behavior identical.
