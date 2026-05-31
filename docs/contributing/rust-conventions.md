# Rust conventions

These rules apply to Rust code under `crates/` unless a narrower rule is stated.

Rust owns Janiform's core behavior. Model logic, numerical behavior, state evolution, calibration, pricing, simulation, validation decisions, and workflow execution belong in Rust unless a task explicitly says otherwise.

For crate layout, crate ownership, and workspace structure, see [Crates](crates.md).

## Role of Rust

- Rust is the default implementation language for core Janiform behavior.
- Anything that affects model behavior, numerical results, state evolution, calibration outcomes, pricing outputs, simulation paths, or validation decisions belongs in Rust.
- Python-facing code may call Rust through PyO3, but Rust owns the algorithm and validation of computational contracts.
- Keep Rust crates Python-agnostic except for the PyO3 binding crate.
- Do not introduce undocumented numerical heuristics, magic constants, or solver policies.
- Numeric decisions must be mathematically derived, data-derived, or explicitly specified and documented in a named contract or policy.
- Avoid one-size-fits-all epsilons. Data may be very small, so floating-point thresholds need explicit justification.
- Rust code must work on both Windows and Linux. Never hardcode Linux-only paths.

## Public contracts

- Treat public Rust APIs as deliberate contracts.
- Default visibility to `pub(crate)`.
- Only make items `pub` when they are part of a deliberate public API.
- Avoid changing public APIs unintentionally.
- Document public APIs with `///` doc comments.
- Include minimal examples in public docs when they help users understand the contract.
- If a breaking public API change is requested, update in-repo call sites and documentation in the same change.
- Keep Python-facing public contract changes aligned with the public API guardrail workflow.
- Boundary value means validation, policy or default injection, type conversion, error mapping, or telemetry.
- If no boundary value is needed, call the shared function directly or use a non-duplicative re-export.
- If a passthrough wrapper is already public and removing it would be breaking, do not remove it silently.
- If more than one optional override is needed, prefer a typed configuration or contract object instead of constructor proliferation.

For the full public API policy and Python snapshot guardrail workflow, see [Maintaining public API contracts](public-api-contracts.md).

## General code quality

- Never use `unsafe`.

## Crates and modules

- `lib.rs` files must be thin and never include declarations.
- Never use wildcard imports such as `::*`.
- Always import items explicitly.
- When introducing a new concept, feature, or API, decide whether an existing crate is the right owner.
- Consider whether a new crate is warranted for a new substantial concept.
- Refactor existing code as needed so ownership remains clear.
- Crate README files are generated from crate metadata by `scripts/generate_crate_readmes.py`.
- Do not write crate README files manually.

For workspace-level crate guidance, see [Crates](crates.md).

## Ownership and borrowing

- Prefer borrowing over owning.
- Accept `&str` instead of `String` when ownership is not needed.
- Accept slices such as `&[T]` instead of `Vec<T>` when ownership is not needed.
- Avoid unnecessary clones.
- Do not use `.clone()` just to make the borrow checker happy. Restructure the code first.
- Any `.clone()` added in core code must be on a small Copy-like type or have a comment explaining why cloning is the best tradeoff.
- If cloning is needed to decouple a lifetime or avoid holding a lock, say so in a short comment.
- Prefer iterators over indexing.
- Avoid indexing patterns that can panic from out-of-bounds access.

## Types and values

- Use `Option<T>` for absence.
- Use `Result<T, E>` for fallible behavior.
- Do not use sentinel values for absence or failure.
- Avoid unchecked conversions and casts unless they are validated and justified.
- Keep core logic operating on validated, well-typed structures.
- Validate external inputs at boundaries.
- Use typed configuration or contract objects when behavior has multiple options.

## Errors and panics

- Model errors explicitly.
- Use crate-local error enums for library layers, commonly with `thiserror`.
- Convert internal errors to user-facing errors at boundaries such as PyO3 and command-line interfaces.
- Do not use `unwrap()` or `expect()` in production paths.
- `unwrap()` and `expect()` are allowed only in tests, benchmarks, or when guarded by an invariant and accompanied by a comment explaining that invariant.
- Panics must not cross FFI boundaries.
- Boundary code should catch or avoid panics where possible.
- Prefer clear validation errors over panics.

## Control flow

- Prefer `match` over deep `if` or `else` ladders when branching on enums, options, or results.
- Use early returns such as `return Err(...)` to keep the happy path left-aligned.
- Keep functions small enough that control flow remains obvious.
- Prefer explicit error handling over implicit fallthrough.

## Serialization

- Use `serde` derives where sensible.
- Avoid hand-rolled parsers unless they are truly necessary.
- Validate external input at serialization boundaries.
- Keep serialized contracts explicit and typed.
- Keep core model logic separate from loosely typed payload handling.

## PyO3 and FFI

- PyO3 is allowed only in the binding crate.
- Other crates must remain Python-agnostic.
- Keep boundary code thin.
- Convert Python inputs to Rust types as soon as possible.
- Convert Rust results and errors back to Python at the boundary.
- Never hold the GIL longer than necessary.
- Detach from Python for heavy work with `Python::detach` when safe.
- Ensure types exposed to Python are stable and documented.
- Do not leak internal Rust structs by default.
- Panics must not cross FFI boundaries.

## Concurrency

- Do not introduce shared mutability.
- Prefer message passing or ownership transfer.
- If sharing is required, use `Arc<T>` with `Mutex` or `RwLock` thoughtfully.
- Minimize lock scope.
- Do not hold locks across await points.
- Do not introduce global mutable state.
- If a global is truly needed, use `OnceLock` or `LazyLock` with immutable data and document why.

## Logging and telemetry

- Use `tracing` where possible.
- Use `log` when it is the appropriate existing project pattern.
- Do not use `println!` in production paths.
- Log at boundaries and high-level operations.
- Avoid noisy per-element logs in hot loops.
- If hot-loop detail is useful, guard it behind a verbosity setting that defaults to off.

## Dependencies

- Add dependencies deliberately.
- Avoid just-in-case crates.
- Prefer widely used crates with good maintenance signals.
- Keep feature flags minimal.
- Document why each non-default feature is enabled.

## Tests

For shared test-writing policy, see [Writing tests](writing-tests.md).

- Prefer fast unit tests in Rust core.
- Write tests for boundary conversions, including PyO3 bindings when relevant.
- Use `assert_eq!()` on entire objects instead of checking fields one by one when that is practical.
- Use Criterion benchmarks for performance-critical kernels.

## Naming

- Use clear, descriptive names.
- Avoid cryptic abbreviations and shorthands outside mathy contexts.
- Use names such as `index`, `column`, and `timestamp`.
- Avoid abbreviations such as `idx` and `col`.
- Short math-oriented names are acceptable in genuinely mathematical contexts.
- Follow local Rust naming conventions unless a mathematical formula is being expressed directly.

## Comments and structure

- Comments should explain intent, structure, non-obvious constraints, or what the code does not clearly state.
- Comments must not restate code mechanically.
- Use comments sparingly.
- Use standard keyboard symbols only, except where mathematical notation is genuinely needed.
- Do not use emojis.
- Use blank lines to separate logical phases after setup, validation, transformation, I/O, and before returning constructed values.
- Do not compress unrelated statements together just to minimize vertical space.
- Do not add blank lines inside argument lists, parameter lists, struct literals, enum variants, match cases, or chains of near-identical statements.
- Keep tightly related assignment plus immediate guard or check together when the assignment is short and the guard directly validates it.

## Review checklist

Before finishing a Rust change:

- Verify crate placement and ownership.
- Verify public APIs were not changed unintentionally.
- Verify public APIs have appropriate `///` docs.
- Verify visibility is no broader than needed.
- Verify imports are explicit and no wildcard imports were added.
- Verify `lib.rs` files remain thin.
- Verify no Python-specific behavior leaked outside the PyO3 binding crate.
- Verify no undocumented numerical heuristic, magic constant, epsilon, or solver policy was introduced.
- Verify error handling uses explicit error types and boundary error conversion.
- Verify production paths do not use `unwrap()` or `expect()`.
- Verify no `unsafe` was introduced.
- Verify clones are necessary and documented when required by the policy.
- Verify conversions and casts are validated and justified.
- Verify concurrency code does not introduce shared mutable state or hold locks too long.
- Verify logging uses `tracing` or `log`, not `println!`.
- Verify serialization uses structured contracts rather than ad hoc parsing.
- Verify equivalent behavior was not duplicated.
- Verify docs and inline comments were updated where behavior changed.
- Run formatting, compile, and test commands.
- Verify code compiles.
- Verify tests succeed.
- Do a final formatting pass to catch issues such as missing blank lines.

## See also

For language-agnostic project rules, see [General conventions](general-conventions.md).
