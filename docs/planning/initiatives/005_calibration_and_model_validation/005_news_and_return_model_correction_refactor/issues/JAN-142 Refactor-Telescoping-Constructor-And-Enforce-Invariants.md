## Refactor Telescoping Constructor And Enforce Invariants Via Types Or Builders

### Description
We have a recurring anti-pattern across the Rust codebase: structs expose many "telescoping" constructors like `try_new_*` / `new_*` that mostly permute argument combinations, while invariants are enforced via ad-hoc `validate()` calls. This inflates API surface, increases maintenance cost, and allows invalid intermediate states.

Refactor the codebase to use more idiomatic Rust construction patterns and encode invariants in the type system where practical.

Goals:
1. Reduce constructor/API surface area:
    - Prefer a single canonical constructor per type (`new` if infallible, otherwise `try_new`).
    - Replace `try_new_*` and `new_*` convenience permutations with:
        - semantic constructors (`from_*`, `with_*`) only when they represent genuinely different meaning, OR
        - a builder pattern when there are many optional params.

2. Make invalid states unrepresentable when possible:
    - Replace "struct with `Option` fields + `validate()`" patterns with:
        - enums representing mutually exclusive construction modes, OR
        - newtypes that enforce constraints (non-negative, bounded, etc.), OR
        - builder that checks invariants at `build()`.

3. Standardize naming and error handling:
    - `try_new` returns `Result<Self, <Type>Error>`.
    - `TryFrom` / `From` used when converting from another type is the semantic intent.
    - Avoid `const fn new` unless compile-time construction is a real requirement.
    - Avoid public constructors that require calling `validate()` externally.

### Implementation approach (repo-wide)
- Scan for patterns:
    - multiple `try_new_*`/`new_*` methods on the same type
    - `Option` fields that must satisfy relationships (sum equality, mutual presence, etc.)
    - `validate()` called immediately after construction
- For each type:
    - identify invariants
    - choose one of:
        1) encode invariants via enum (mutually exclusive modes)
        2) encode constraints via newtypes (e.g., NonNegativeF64, PositiveF64)
        3) builder pattern for many optional knobs
        4) keep `try_new` + a small number of semantic `from_*` constructors (only if meaningfully distinct)
- Update call sites to use the new API.
- Ensure public API remains coherent and minimal. If breaking changes are unavoidable, update internal call sites and any Python bindings accordingly.

### Mechanical refactoring playbook
- For any type with `Option` fields that must appear together or be mutually exclusive: replace with an `enum`.
- For numeric constraints (non-negative, bounded, strictly positive, etc.): introduce newtypes with `TryFrom<f64>` enforcing invariants at construction.
- For types with more than ~4 optional configuration parameters: introduce `TypeBuilder` with chained setters and a single `build()` that validates invariants.
- Remove `validate()` methods that are only used immediately after construction; move validation logic into constructors or builders.
- Collapse redundant `try_new_*` variants into one canonical constructor unless they represent genuinely different semantic construction modes.

### Acceptance criteria
- For refactored types, there is at most one primary constructor (`new` or `try_new`).
- Additional constructors (if any) are semantic (`from_*`, `with_*`) and justified by meaning, not parameter permutations.
- Illegal states are prevented by construction for refactored types whenever feasible.
- No external "construct then validate" pattern remains.
- No net increase in public API surface (prefer reduction).
- Zero compatibility/fallback code added.
- Old contract paths removed, not retained.
- Unit tests cover invariants and representative construction paths.
- `cargo test` passes and formatting/lints are clean.

### Unit tests
- Ensure unit tests cover invariants and representative construction paths.

### Out of scope
- Algorithmic or performance changes not required for enforcing invariants.
- Architectural rewrites unrelated to constructor patterns.
