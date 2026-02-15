# AGENTS Guidelines

This document provides instructions and guardrails for AI coding agents (and humans) working in this repository. It orients you to the codebase, defines the development workflow, and embeds the complete Python style guide you must follow.

---

## Scope and Goals
- Ensure safe, minimal, high‑quality changes that keep tests green, without cheating.
- Follow the style and architectural conventions of this repo.
- Prefer clarity and maintainability over cleverness.
- Do not introduce global state; use existing services and registries.

---

## Repo Orientation
- `crates/core/`: The Rust core of the library
- `crates/pybind/`: Depends on `core` and translates
- `python/janiform/`: Python package and CLI
- `tests/python/`: Unit and integration tests for the Python li
- `tests/rust/`: Unit and integration tests for Rust crates
- `docs/`: Design docs, development guides, planning

---

## Available Tooling
- `cargo` for Rust
- `python3` for Python
- `pytest` for Python tests

---

## Agent Heuristics and Checklist
### 1. General
- Before adding new functions or classes, look through the repo to see if something similar already exists (except for enums and dataclasses that have context-dependent names):
  - If it does exist, use that instead of writing something new.
  - If it does exist, but slightly different, if it makes sense, extend it to do what you require it to do.
    - If you do extend it, do not leave backwards-compatible APIs; instead, refactor code that uses it so that they adhere to the new API.
  - If it does not exist, then you may write a new class or function.
  - If it exists in a different part of the program. For example: a class exists in the world server, but you want to use it in the master server, move it to the shared folder, do **not** import it from the world server.
- You may ALWAYS assume that users have **every dependency available**. No need think about backwards-compatibility issues or missing packages.
- Naming: clear, descriptive; avoid cryptic abbreviations and shorthands outside mathy contexts (we hate `idx` and `col`, never use that ever).
- Change management
  - Keep diffs minimal and focused; avoid drive‑by refactors.
    - Bigger refactors are allowed only if asked for.
    - Exception: Prefer refactoring when it makes things DRY.
    - NEVER repeat chunks of code for no reason.
  - Update or add tests when changing behavior.
  - Maintain public APIs unless explicitly required to change.
  - Use project logging (no prints) and follow error‑handling guidance below.
  - Update any relevant docs (including README.md) when changes are made, and you deem it necessary for any reason.

### 2. Python
- Type hints everywhere (functions, attributes, locals where it matters).
- Never use quoted / stringed type hints; repo requires Python 3.14 and PEP 649 and PEP 749 is accepted in that version.
- Imports grouped and alphabetized per guide.
- Strings: double quotes for paths/logs/UI; single quotes for enum‑like values and keys.
- Enums: use `StrEnum`, `IntEnum`, or `IntFlag`; UPPERCASE members.
- Control flow: prefer `match` over long if/elif chains where suitable.
- Exceptions: be specific; avoid cargo‑cult try/except; log appropriately.
- OOP: avoid setters (use immutable design patterns); getters should generally be properties; use setters methods when needed; prefer helper classes over globals; be thread-safe when it matters; immutable data is good; keeping fields private is good (expose with getter property if necessary).
- Comments: follow the blank‑line rules from the guide.
- Logging: never `print()` in production paths.
- Never use `;` to contain two statements on one line: always put new statements on new lines.
- No global state (no global variables used for registries, etc.).
- Writing tests
  - If you need helpers, look for fixtures in `conftest.py` before writing local module helpers as the helper you require could already exist.
  - If your fixture exists but needs to be modified, modify the fixture and make sure all other tests logically work with the updated fixture.
  - If your fixture does not exist, consider if this will be useful for other tests in the future, and if so, write it as a fixture in `conftest.py` instead of as a local helper function or class.
  - Do not run tests, agents don't have `pytest` available in their environment

**Before finishing the task**

- You verified import order, type hints, and logger usage.
- You verified no try-excepts that swallow all exceptions silently.
- You verified no `getattr` calls are used unless absolutely necessary and if used, is always commented to explain why it is needed.
- You avoided changing public APIs unintentionally.
- You updated docs/inline comments where behavior changed.
- You verify that the code is DRY (remember to look for other functions and classes that do the same thing).
- You update README.md with an updated project structure, if there are big enough changes to it.
- All code compiles (verify by running `python3 -m py_compile`).
- All tests succeed (verify using `pytest`).

**Python Style Guide**

Read and follow `docs/development/guides/python_style_guide.md`. Agents must adhere to it strictly when writing Python code.

### 3. Rust
#### Tooling & style
- Edition/MSRV: Rust edition 2024; do not use features requiring newer Rust than the repo MSRV.
- No "style wars": follow standard Rust conventions (snake_case for fn/vars/modules, CamelCase for types/traits, SCREAMING_SNAKE_CASE for consts/statics).

#### API & module hygiene
- Keep the crate graph clean:
  - `core` crate must be pure Rust (no PyO3, no Python-specific types).
  - `pybind` crate owns all PyO3 boundary code and conversions.
- Visibility: default to `pub(crate)`; only make items `pub` when part of a deliberate public API.
- Prefer small, focused modules; avoid "god modules" and long files.
- Document public APIs with `///` doc comments; include minimal examples where helpful.

#### Types, ownership, and allocations
- Prefer borrowing over owning:
  - Accept `&str` instead of `String` when you don't need ownership.
  - Accept slices `&[T]` instead of `Vec<T>` when you don't need ownership.
- Avoid unnecessary clones:
  - Do not `.clone()` "to make the borrow checker happy"; restructure code first.
  - If cloning is needed, comment why (e.g., "intentional copy to decouple lifetime / avoid holding lock").
- Any `.clone()` added in core must be either (a) on a small Copy-ish type, or (b) accompanied by a comment explaining why cloning is the best tradeoff.
- Prefer iterators over indexing; avoid panics from out-of-bounds indexing.
- Use `Option<T>`/`Result<T, E>` for absence/failure, not sentinel values.

#### Error handling & panics
- No `unwrap()` / `expect()` in production paths.
  - Allowed only in tests, benchmarks, or when guarded by an invariant, with a comment explaining that invariant.
- Model errors explicitly:
  - Use a crate-local error enum (e.g., `thiserror`) for library layers.
  - Convert to user-facing errors at boundaries (PyO3, CLI).
- Panics must not cross FFI boundaries; boundary code should catch/avoid panics where possible.

#### Pattern matching & control flow
- Prefer `match` over deep `if/else` ladders when branching on enums/options/results.
- Use early returns (`return Err(...)`) to keep the happy path left-aligned.
- Keep functions small; refactor when nesting exceeds ~2–3 levels.

#### Safety & `unsafe`
- `unsafe` is forbidden unless:
  - There is a measured performance/FFI need, and
  - The code includes a `// SAFETY:` comment stating the invariants and why the block is sound, and
  - A safe wrapper exists and is tested.
- Avoid unchecked conversions/casts unless validated and justified.

#### Concurrency
- Do not introduce shared mutability casually.
- Prefer message passing or ownership transfer; if sharing is required:
  - Use `Arc<T>` + `Mutex/RwLock` thoughtfully, minimize lock scope, avoid holding locks across await points.
- No global mutable state. If you truly need a global, use `OnceLock`/`LazyLock` with immutable data and document why.

#### Logging/telemetry
- Use `tracing` (preferred) or `log`; do not `println!` in production paths.
- Log at boundaries and high-level operations; avoid noisy per-element logs in hot loops.

#### Serialization & config (if applicable)
- Use `serde` derives where sensible; avoid hand-rolled parsers unless necessary.
- Validate external inputs at boundaries; keep core logic operating on validated, well-typed structures.

#### PyO3 / Python boundary rules (if using PyO3)
- PyO3 is allowed only in the binding crate; core must remain Python-agnostic.
- Keep boundary code thin:
  - Convert Python inputs to Rust types ASAP.
  - Convert Rust results/errors back to Python at the boundary.
- Never hold the GIL longer than necessary; avoid long CPU work while holding GIL.
  - Release GIL for heavy work (e.g., `Python::allow_threads`) when safe.
- Ensure types exposed to Python are stable and documented; do not leak internal Rust structs by default.

#### Testing
- Prefer fast unit tests in Rust core; property tests are welcome for tricky invariants.
- Write tests for:
  - error cases (including conversion/validation),
  - boundary conversions (pybind),
  - numerical stability (if relevant).
- Benchmarks (criterion) for performance-critical kernels; optimize based on measurement, not vibes.

#### Dependencies
- Add dependencies deliberately; avoid "just in case" crates.
- Prefer widely-used crates with good maintenance signals.
- Keep feature flags minimal; document why each non-default feature is enabled.

---

## Comments
Comments should only explain what the code doesn't clearly state, or help us understand the structure when the code is complex or is made up of many steps.
NOTE: Do not use ";" in comments. It is almost never used by humans, especially coders. AI agents tend to overuse this in comments. Here we have a hard rule: NEVER USE THEM!
