# Production Hardening

You are working in the `janiform` repository. Your job is to move the codebase closer to production quality by finding and implementing high-value fixes across the Rust core, the PyO3 binding layer, the Python package, and the CLI. This is an implementation task, not a review-only task.

Read `./AGENTS.md` first and follow it exactly. In particular:
- This repo is pre-v0.1. Backward compatibility is forbidden unless the task explicitly asks for it.
- Do not add compatibility shims, legacy parsing, fallback aliases, deprecated wrappers, or duplicate contract paths.
- Keep the Rust core Python-agnostic. PyO3 code must stay in `crates/janiform-pybind/`.
- Keep diffs focused and DRY.
- Update tests when behavior changes.
- Update docs when behavior or contracts change.

Also read the minimum relevant context before editing:
- `./README.md`
- `./docs/design/janiform_design_brainstorm_notes.md`
- `./docs/design/janiform_compact_model_spec.tex`
- `./docs/development/guides/python_style_guide.md`
- Any contract or guide documents directly related to the code you touch

## Mission

Audit the codebase for production-readiness smells and implement the fixes. Do not stop after producing findings. Continue through code changes, tests, and documentation updates unless you hit a real blocker.

Your goal is to reduce correctness risk, contract drift, operational fragility, and maintainability debt without doing unrelated refactors.

## How To Work

1. Establish a baseline.
   - Inspect the relevant project structure and identify the highest-risk entrypoints in Rust, PyO3, Python, and the CLI.
   - Run targeted searches for known production smells before making assumptions.

2. Prioritize issues that are both high-risk and fixable in-repo.
   - Prefer correctness, safety, and contract clarity over stylistic cleanup.
   - Prefer one canonical implementation over duplicated logic.
   - Prefer fixing root causes over layering validation in multiple places.

3. Implement fixes directly.
   - Do not just list problems.
   - If you find multiple small instances of the same smell in one area, clean them up together when it keeps the diff coherent.
   - If a smell reveals a contract problem, update all in-repo call sites, tests, and docs in the same change.

4. Verify thoroughly.
   - Run the narrowest useful tests while iterating.
   - Before finishing, run the relevant Rust and Python validation for every changed area.

5. Report what changed, what you verified, and what remains only if genuinely blocked.

## Primary Smells To Search For And Fix

### 1. Rust core correctness and safety

Search for and fix:
- `unwrap()`, `expect()`, `panic!`, `todo!`, `unimplemented!`, `dbg!` outside tests or clearly justified invariants
- `unwrap_or()` and `unwrap_or_else()` where they may silently hide invalid state, bad input, missing configuration, or error conditions that should instead be surfaced or validated explicitly
- User-input or data-validation failures modeled as panics instead of typed errors
- Ad hoc string errors where a crate-local error type should exist
- Unchecked indexing into slices or vectors where iterator-based logic would avoid panic risk
- Lossy or unchecked numeric casts and conversions
- Excessive or suspicious `.clone()` usage added only to satisfy ownership issues
- Large functions with deeply nested control flow that obscure validation or error handling
- Duplicate parsing, validation, or transformation logic that should be unified
- `pub` visibility wider than necessary
- `println!` or other ad hoc debug output in production paths
- `unsafe` blocks without a tight `// SAFETY:` explanation and a strong reason

Expected fixes:
- Convert panicking paths into explicit `Result`-based flows
- Centralize shared logic in `core` when multiple callers need the same validated behavior
- Narrow visibility where practical
- Add or tighten unit tests around the corrected invariants and error cases

### 2. PyO3 boundary hygiene

Search for and fix:
- Business logic living in `crates/janiform-pybind/` that belongs in `core`
- Inconsistent Rust-to-Python error mapping
- Boundary code that duplicates validation differently from `core`
- Rust panics that could cross the FFI boundary
- Python-facing types that expose internal structures too directly
- Heavy work performed while unnecessarily holding the GIL
- Thin wrapper functions that add no validation, conversion, policy, or error-mapping value

Expected fixes:
- Keep PyO3 code thin and boundary-focused
- Move reusable logic into `core`
- Normalize Python exception behavior for equivalent failure modes
- Add boundary tests for success cases and representative failures

### 3. Python package and public API quality

Search for and fix:
- Public APIs that do the same thing in slightly different ways
- Stringly-typed contracts where explicit enums or typed structures already exist or should exist
- Missing or weak type hints
- Poor import hygiene or inconsistent module boundaries
- Broad `except Exception` usage or swallowed exceptions
- `print()` in production code
- Mutable module-level state or implicit registries
- Helper wrappers that merely forward arguments without adding boundary value
- Python API defaults that diverge from Rust or CLI behavior

Expected fixes:
- Make contracts explicit and typed
- Remove redundant wrappers when they add no value
- Tighten exception handling and logging
- Add or update Python tests, including public API and failure-path coverage

### 4. CLI production behavior

Search for and fix:
- CLI defaults or semantics that diverge from the importable Python API for the same operation
- Weak, ambiguous, or developer-centric error messages
- Inconsistent exit codes
- Output that is hard to script against or is unstable without justification
- Argument parsing logic mixed with core business logic
- Missing validation or contradictory flags that should be rejected early

Expected fixes:
- Make CLI behavior explicit, predictable, and aligned with the underlying library contract
- Keep command parsing and presentation logic at the boundary
- Add or update CLI tests for errors, flags, and output contracts

### 5. Tests and regression coverage

Search for and fix:
- Happy-path-only coverage
- Missing tests for malformed input, edge conditions, empty data, large input, and failure propagation
- Rust core behavior validated only indirectly through Python
- Python or CLI contracts missing direct tests
- Snapshot tests that assert formatting but not semantics
- Missing regression tests for bugs you fix

Expected fixes:
- Add narrow tests that lock in the intended behavior
- Prefer meaningful assertions over brittle formatting checks
- Cover boundary conversions and error mapping explicitly

### 6. Operational readiness and diagnostics

Search for and fix:
- Missing structured logging at important boundaries
- Errors that drop crucial context
- Silent fallback behavior that hides bad inputs or invalid states
- Unbounded buffering, eager loading, or retention of large intermediates where a safer pattern is obvious
- Configuration precedence or environment-driven behavior that is unclear or inconsistent

Expected fixes:
- Improve context in errors and logs
- Remove silent fallback behavior unless the contract explicitly requires it
- Make high-impact operational behavior easier to reason about and test

## Concrete Search Heuristics

Use targeted code search to build your worklist. At minimum, search for:

```text
unwrap\(
expect\(
panic!
todo!
unimplemented!
dbg!
println!
unsafe
clone\(
unwrap_or\(
unwrap_or_else\(
getattr\(
except Exception
except:
print\(
TODO
FIXME
```

Also search for:
- Very large files and very large functions
- Duplicate logic across `crates/janiform/`, `crates/janiform-pybind/`, and `python/janiform/`
- CLI command implementations that appear to re-validate or transform data differently from the library path
- Tests that mention old contract names or behavior after you refactor an API

## Implementation Rules

- Do not do a giant cleanup PR. Focus on coherent, production-relevant fixes.
- Do not preserve old APIs unless explicitly asked. This repo forbids compatibility shims by default.
- Do not introduce parallel contract paths.
- Do not move business logic into the Python layer if it belongs in Rust core.
- Do not add wrappers that only rename arguments and forward calls.
- Do not use broad exception handling as a substitute for design.
- Do not leave panic-prone behavior in user-facing or FFI-facing code paths.
- Do not silently ignore invalid input.

## Validation Requirements

You are expected to validate your changes, not just reason about them.

At minimum:
- Run relevant Rust tests for touched areas
- Run relevant Python tests for touched areas
- Run `python3 -m py_compile` on changed Python modules or an equivalent repo-appropriate check

Use repo scripts where appropriate:
- `./scripts/test-rust-linux.sh .venv-linux -- [args]`
- `./scripts/test-python-linux.sh .venv-linux -- [args]`

If you use `cargo test`, run it with `TMPDIR=/tmp`.

## Deliverable Standard

Your final output should reflect completed work, not just observations.

Include:
- What production-readiness issues you fixed
- Which contracts, tests, and docs you updated
- What validation you ran
- Any remaining blockers, only if they prevented safe completion

If you discover a broad class of repeated smells, fix the class coherently where feasible. Do not stop after the first example unless the remaining work becomes clearly separate in scope.

## Suggested Execution Order

1. Scan for panic and error-handling problems in Rust core
2. Audit the PyO3 boundary and Python exception mapping
3. Check CLI and Python API contract alignment
4. Tighten tests around the changed behavior
5. Update relevant documentation
6. Run validation and summarize the completed hardening work

## Definition Of Done

You are done when:
- The highest-value production smells you found in the scoped areas are fixed
- Changed behavior is covered by tests
- Relevant docs are updated
- Validation passes for the changed areas
- No compatibility shim or fallback path was introduced unless explicitly required
