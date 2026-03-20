# AGENTS Guidelines

## Scope and Goals
- Ensure safe, minimal, high‑quality changes that keep tests green, without cheating.
- Follow the style and architectural conventions of this repo.
- Prefer clarity and maintainability over cleverness.
- Do not introduce global state; use existing services and registries.
- Implementations are Rust first for most things (unless specified otherwise or obvious from context): Python is just a thin wrapper and the user-facing API.

## Contract Evolution Policy (Mandatory)

This codebase is pre-v0.1 and has no external users.

- Backward compatibility is **forbidden** unless the task explicitly asks for it.
- Do **not** add legacy/fallback parsing, compatibility shims, deprecated aliases, dual payload support, or wrapper constructors for old APIs.
- Do **not** preserve old fields "just in case".
- Do **not** bump schema versions during refactors or when introducing breaking changes. Keep schema identifiers at `v1`.
- Replace old contracts directly, then migrate all in-repo call sites/tests/docs to the new contract in the same PR.
- If you believe compatibility is required, **stop and ask** before implementing anything.

## Available Tooling
- `./scripts/bootstrap-linux.sh .venv-linux` for Linux/WSL-based bootstrapping (if no venv exists)
- `./scripts/bootstrap-win.ps1 .venv` for Windows-based bootstrapping (if no venv exists)
- `./scripts/build-linux.sh .venv-linux` for Linux/WSL-based building
- `./scripts/build-win.ps1 .venv` for Windows-based building
- `./scripts/test-rust-linux.sh .venv-linux -- [cmd-line arguments for nextest]` for running tests on Linux/WSL
- `./scripts/test-rust-win.ps1 .venv -- [cmd-line arguments for nextest]` for running tests on Windows
- `./scripts/test-python-linux.sh .venv-linux -- [cmd-line arguments for pytest]` for running tests on Linux/WSL
- `./scripts/test-python-win.ps1 .venv -- [cmd-line arguments for pytest]` for running tests on Windows
- `cargo` for Rust
- `python3` for Python

When running `cargo test`, do so with `TMPDIR=/tmp` so that verification stays inside the sandbox.

## Agent Heuristics and Checklist
### 1. General
- When writing a helper function, if it is a general enough helper, look for it elsewhere because it likely already exists.
  - If it does exist, that means it should be moved to a shared location. Move it! Then import! Don't write a local copy.
  - If it does not exist, write it in a shared location anyway, so that if we ever need it somewhere else in the future, we can easily find it.
- Before adding new functions or classes, look through the repo to see if something similar already exists (except for enums and dataclasses that have context-dependent names):
  - If it does exist, use that instead of writing something new.
  - If it does exist, but slightly different, if it makes sense, extend it to do what you require it to do.
    - If you extend internal or private code paths, prefer one canonical implementation and migrate in-repo callers instead of keeping duplicate legacy paths.
    - If you extend a public API, preserve compatibility unless a planned breaking change is explicitly requested.
    - If a breaking public API change is requested, update in-repo call sites and documentation in the same change.
  - If it does not exist, then you may write a new class or function.
  - If it exists in a different part of the program. For example: a class exists in the world server, but you want to use it in the master server, move it to the shared folder, do **not** import it from the world server.
- You may ALWAYS assume that users have **every dependency available**. Do not spend effort on optional dependency fallbacks or missing-package compatibility shims.
- Naming: clear, descriptive; avoid cryptic abbreviations and shorthands outside mathy contexts (we hate `idx` and `col`, never use that ever).
- Change management
  - Keep diffs minimal and focused; avoid drive‑by refactors.
    - Bigger refactors are allowed only if asked for.
    - Small local refactors for DRY and clarity are allowed when they are directly related to the requested change.
    - NEVER repeat chunks of code for no reason.
  - Do not add consumer-specific passthrough wrappers (for example `*_for_pricing`) unless the adapter adds boundary value in the same change.
    - Boundary value means at least one of: validation, policy/default injection, type conversion, error mapping, telemetry, or compatibility constraints that are explicitly documented.
    - If no boundary value is needed, call the shared function directly or use a non-duplicative re-export.
    - If a passthrough wrapper is already public and removal would be breaking, do not remove it silently. Create a planned cleanup issue or deprecation path first.
  - Update or add tests when changing behavior.
  - Maintain public APIs unless explicitly required to change.
  - Use project logging (no prints) and follow error‑handling guidance below.
  - Update any relevant docs (including README.md) when changes are made, and you deem it necessary for any reason.

### 2. Python
- Type hints everywhere (functions, attributes, locals where it matters).
- Never use quoted / stringed type hints; repo requires Python >=3.12 and PEP 649 and PEP 749 is accepted in that version.
- Imports grouped and alphabetized per guide.
- Use `Any` instead of `object` for most things where inference is hard or impossible. This includes tuples, lists and dicts that take non-builtin types that are hard to infer.
- Don't be lazy and do `list[Any]` any time we have non-builtin types. That's bad practice. Most of the time types are inferable and should be used explicitly.
- Never use `object` or `Any` as a cop-out / shortcut to get around specifying a type that is known. Even if it leads to circular imports. Deal with circular imports a different way! Good type hints are a priority! They make bugs less likely.
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
  - Add other crates if the need arises but don't do it for no good reason. When you do, update this document to list it here.
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

**Before finishing the task**

- You avoided changing public APIs unintentionally.
- You updated docs/inline comments where behavior changed.
- You verify that the code is DRY (remember to look for other functions and classes that do the same thing).
- You update README.md with an updated project structure, if there are big enough changes to it.
- All code compiles.
- All tests succeed.

## Comments
Comments should only explain what the code doesn't clearly state, or help us understand the structure when the code is complex or is made up of many steps.
NOTE: Do not use ";" in comments. It is almost never used by humans, especially coders. AI agents tend to overuse this in comments. Here we have a hard rule: NEVER USE THEM!
