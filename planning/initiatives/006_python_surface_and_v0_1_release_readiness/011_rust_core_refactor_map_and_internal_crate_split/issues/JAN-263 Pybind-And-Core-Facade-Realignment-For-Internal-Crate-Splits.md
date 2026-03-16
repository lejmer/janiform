## Pybind And Core Facade Realignment For Internal Crate Splits

### Description
Realign `pybind` and the umbrella `janiform-core` facade after the internal crate splits so Python bindings stay thin and curated while internal Rust ownership boundaries become more explicit.

### Implementation details
- Define which internal crates `pybind` should reach through `janiform-core` and which, if any, it may depend on directly.
- Keep the public Python and CLI-facing behavior stable while internal Rust crates move.
- Update the `janiform-core` facade to re-export only the intended stable Rust workflow and domain surfaces.
- Prevent `pybind` from taking opportunistic direct dependencies on every new internal crate just because they exist.
- Ensure boundary conversion and error-mapping code remains confined to `pybind`.

### Acceptance Criteria
- `pybind` remains thin and does not become the place where the new internal crate graph is reassembled ad hoc.
- `janiform-core` provides a deliberate umbrella facade over the extracted internal crates.
- Public Python behavior remains stable while Rust internals are decomposed.
- Internal crate extraction does not leak Python-specific concerns into pure Rust crates.

### Unit tests
- Pybind integration tests confirming bindings still compile and call through the intended facade.
- Rust facade tests confirming re-export stability for the curated `janiform-core` surface.
- Negative dependency checks preventing Python-specific code from entering internal Rust crates.

### Notes/Constraints
- The point of the split is not to make pybind smarter about internal structure. The point is to make Rust ownership cleaner while keeping the boundary thin.
- Preserve the existing rule that Python-specific code belongs only in `pybind`.
