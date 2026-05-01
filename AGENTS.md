# AGENTS Instructions

## High-level Goals

- First and foremost: Do not introduce undocumented numerical heuristics, magic constants, or solver policies. Every numeric decision must be mathematically derived, data-derived, or explicitly specified and documented in a named contract or policy. This applies to epsilons (floating-point thresholds) as well. Sometimes, the data is itself very small, so usually there is no good "one size fits all"-epsilon value. Avoid them entirely when possible.
- Anything that is core library behavior must be implemented in Rust, not Python.
- If logic affects model behavior, numerical results, state evolution, calibration outcomes, pricing outputs, simulation paths, or validation decisions, it belongs in Rust unless the task explicitly says otherwise.
- Python may define user-facing typed contracts, CLI wiring, serialization helpers, and thin boundary adapters, but it must not become a second implementation of model logic.
- Ensure safe, high‑quality changes that keep tests green, without cheating.
- Follow the style and architectural conventions of this repo, including where to place blank lines.
- Prefer clarity and maintainability over cleverness.
- Do not introduce global state.
- Must work on both Windows and Linux (never hardcode Linux paths)

## Important Regression Test Policy

- Do not use regression tests as a substitute for stronger API or contract design. When a bug exposes an invalid state, first make that state hard or impossible to express through typed contracts, shared constructors, validators, or narrower public methods. Add or keep a regression test only when the invalid behavior remains genuinely easy to reintroduce after the API shape has been fixed, and document why that test is still necessary.
- Do not write tests or assertions whose only purpose is to prove that old code, old text, old fields, or removed behavior are absent. Prefer exact positive contract assertions or stronger API shapes that make the unwanted state impossible to express.

### Before finishing a task

- You avoid changing public APIs unintentionally.
- You update docs/inline comments where behavior changed.
- You verify that the code is DRY (remember to look for other functions and classes that do the same thing).
- All code compiles.
- All tests succeed.

## Contract Evolution Policy

This codebase is pre-v0.1 and has no external users.

- Backward compatibility is **forbidden** unless the task explicitly asks for it.
- Do **not** add legacy/fallback parsing, compatibility shims, deprecated aliases, dual payload support, or wrapper constructors for old APIs.
- Do **not** preserve old fields "just in case".
- Do **not** bump schema versions during refactors or when introducing breaking changes. Keep schema identifiers at `v1`.
- Replace old contracts directly, then migrate all in-repo call sites/tests/docs to the new contract in the same PR. Remove old deprecated contracts.
- If you believe compatibility is required, **stop and ask** before implementing anything.

## General Coding Instructions

- When writing a helper function, if it is a general enough helper, look for it elsewhere because it likely already exists.
  - If it does exist, that means it should be moved to a shared location. Move it! Then import! **DO NOT** write a local copy.
  - If it does not exist, write it in a shared location anyway, so that if we ever need it somewhere else in the future, we can easily find it.
- Before adding new functions or classes, look through the repo to see if something similar already exists (except for enums and dataclasses that have context-dependent names):
  - If it does exist, use that instead of writing something new.
  - If it does exist, but slightly different, if it makes sense, extend it to do what you require it to do.
    - If you extend internal or private code paths, prefer one canonical implementation and migrate in-repo callers instead of keeping duplicate legacy paths.
    - If you extend a public API, preserve compatibility unless a planned breaking change is explicitly requested.
    - If a breaking public API change is requested, update in-repo call sites and documentation in the same change.
  - If it does not exist, then you may write a new class or function.
  - If it exists in a different part of the project.
- You may ALWAYS assume that users have **every dependency available**. Do not spend effort on optional dependency fallbacks or missing-package compatibility shims.
- Naming: clear, descriptive. Avoid cryptic abbreviations and shorthands outside mathy contexts (we hate `idx` and `col`, never use that ever).
- Change management
  - NEVER repeat chunks of code for no reason.
  - Do not add consumer-specific passthrough wrappers (for example `*_for_pricing`) unless the adapter adds boundary value in the same change.
    - Boundary value means at least one of: validation, policy/default injection, type conversion, error mapping, telemetry, or compatibility constraints that are explicitly documented.
    - If no boundary value is needed, call the shared function directly or use a non-duplicative re-export.
    - If a passthrough wrapper is already public and removal would be breaking, do not remove it silently. Create a planned cleanup issue or deprecation path first.
  - Do not introduce `new_with_*` or `try_new_with_*` constructor families for general optional arguments.
    - Only use a `*_with_*` constructor when `new` or `try_new` represents one canonical default behavior and the alternate constructor is a single explicit override of that default.
    - If more than one optional override is needed, prefer a typed configuration or contract object instead of proliferating constructor combinations.
  - Maintain public APIs unless explicitly required to change.
  - Use project logging (no prints).
  - Update any relevant docs (including README.md) when changes are made, and you deem it necessary for any reason. Never word it as if something has changed, because the repo is in pre-v0.1 and we haven't reached the first version for any of the docs yet, so it cannot sound like something has changed.
- Avoid large modules:
  - Prefer adding new modules instead of growing existing ones.
  - Target modules under 500 LoC, excluding tests.
  - If a file exceeds roughly 800 LoC, add new functionality in a new module instead of extending the existing file unless there is a strong-documented reason not to.
  - This rule applies especially to high-touch files that already attract unrelated changes.
  - When extracting code from a large module, move the related tests and module/type docs toward the new implementation so the invariants stay close to the code that owns them.
- Avoid large functions:
  - Prefer adding focused helpers over growing large functions even larger.
  - Target functions under 100 LoC (tests don't have this limit).
  - If a function exceeds roughly 200 LoC, add new functionality in a new function or helper instead of growing the existing function body.
- Use blank lines to separate logical blocks of statements so code reads in phases, especially after setup, validation, transformation, I/O, and before returning constructed values. Do not compress unrelated statements together just to minimize vertical space.
- Do not add blank lines inside argument lists, parameter lists, struct literals, enum variants, match cases, or chains of near-identical statements. Keep tightly related assignment plus immediate guard/check together when the assignment is short and the guard directly validates it.

## Rust

Implementations are Rust first for most things (unless specified otherwise or obvious from context).

In the `crates/` folder where the Rust code is located:

- `lib.rs` files must be thin and never include any declarations.
- Never use wildcard importing (`::*`). Always be explicit with everything you import.
- When introducing a new concept/feature/API, consider whether:
  - There is an existing crate  that is an appropriate place for your new code to live.
  - It is time to introduce a new crate to the Cargo workspace for your new functionality. Refactor existing code as necessary to make this happen.
- Default visibility to `pub(crate)`. Only make items `pub` when part of a deliberate public API.
- Document public APIs with `///` doc comments. Include minimal examples where helpful.
- Prefer borrowing over owning:
  - Accept `&str` instead of `String` when you don't need ownership.
  - Accept slices `&[T]` instead of `Vec<T>` when you don't need ownership.
- Avoid unnecessary clones:
  - Do not `.clone()` "to make the borrow checker happy". Restructure code first.
  - If cloning is needed, comment why (e.g., "intentional copy to decouple lifetime / avoid holding lock").
- Any `.clone()` added in core must be either (a) on a small Copy-ish type, or (b) accompanied by a comment explaining why cloning is the best tradeoff.
- Prefer iterators to indexing: avoid panics from out-of-bounds indexing.
- Use `Option<T>`/`Result<T, E>` for absence/failure, not sentinel values.
- No `unwrap()` / `expect()` in production paths.
  - Allowed only in tests, benchmarks, or when guarded by an invariant, with a comment explaining that invariant.
- Model errors explicitly:
  - Use a crate-local error enum (e.g., `thiserror`) for library layers.
  - Convert to user-facing errors at boundaries (PyO3, CLI).
- Panics must not cross FFI boundaries. Boundary code should catch/avoid panics where possible.
- Prefer `match` over deep `if/else` ladders when branching on enums/options/results.
- Use early returns (`return Err(...)`) to keep the happy path left-aligned.
- Keep functions small. Refactor when nesting exceeds ~2–3 levels.
- `unsafe` is forbidden.
- Avoid unchecked conversions/casts unless validated and justified.
- When writing concurrent code:
  - Do not introduce shared mutability.
  - Prefer message passing or ownership transfer. If sharing is required:
    - Use `Arc<T>` + `Mutex/RwLock` thoughtfully, minimize lock scope, avoid holding locks across await points.
  - No global mutable state. If you truly need a global, use `OnceLock`/`LazyLock` with immutable data and document why.
- For logging/telemetry, use `tracing` (preferred) or `log`. Do not `println!` in production paths.
- Log at boundaries and high-level operations. Avoid noisy per-element logs in hot loops unless there is a verbosity parameter that allows it that defaults to off.
- For serialization:
  - Use `serde` derives where sensible and avoid hand-rolled parsers unless truly necessary.
  - Validate external inputs at boundaries. Keep core logic operating on validated, well-typed structures.
- PyO3 is allowed only in the binding crate. Other crates must remain Python-agnostic.
  - Keep boundary code thin:
    - Convert Python inputs to Rust types ASAP.
    - Convert Rust results/errors back to Python at the boundary.
  - Never hold the GIL longer than necessary. Avoid long CPU work while holding GIL.
    - Detach from Python for heavy work with `Python::detach` when safe.
  - Ensure types exposed to Python are stable and documented. Do not leak internal Rust structs by default.
- Crate README's are never written manually. They are generated from crate metadata from the canonical script `scripts/generate_crate_readmes.py`.

### Testing

- Prefer deep equals comparisons whenever possible. Perform `assert_eq!()` on entire objects, rather than individual fields.
- Avoid mutating process environment in tests. Prefer passing environment-derived flags or dependencies from above.
- Prefer fast unit tests in Rust core. Property tests are welcome for tricky invariants.
- Write tests for:
  - error cases (including conversion/validation)
  - boundary conversions (pybind)
  - numerical stability (if relevant)
- Benchmarks (criterion) for performance-critical kernels. Optimize based on measurement.

### Expensive runs

If you run an expensive command, test or run (e.g. pricing or calibration):
  - run one command at a time
  - for tests: always use a 120 second timeout for the process
  - for other commands: always use a 360 second timeout for the process

You don't have the necessary permissions to kill processes manually, so do not rely on that.

### Dependencies

- Add dependencies deliberately. Avoid "just in case" crates.
- Prefer widely-used crates with good maintenance signals.
- Keep feature flags minimal. Document why each non-default feature is enabled.

## Python

Python is mostly supposed to be a thin wrapper and serves as the user-facing API and CLI.

In the `python/janiform/` folder where the Python code is located:

- `__init__.py` files must be thin and never include any declarations.
- Never use wildcard importing (`*`). Always be explicit with everything you import.
- Keep Python as a boundary layer. It is for typed request/response contracts, ergonomic public APIs, CLI orchestration, serialization/deserialization, and error mapping.
- Do not implement core model behavior in Python. This includes state-estimation algorithms, simulation evolution, pricing recursions, calibration math, optimization logic, statistical transforms, numerical integration, or any other computation that meaningfully determines outputs.
- If a feature needs new computation, the default is:
  - Rust owns the algorithm and validation of the computational contract.
  - PyO3 exposes the native entrypoint.
  - Python calls into that native entrypoint and converts inputs and outputs.
- If you are about to add a Python helper that loops over market observations, evolves model state, computes fitted/runtime state, applies numerical recursions, or reconstructs any model boundary condition, stop and move that logic into Rust.
- `import ...` before `from ... import ...` per group.
- Imports grouped and alphabetized per guide.
- Type hints everywhere: functions, attributes, locals (where it matters, which is most of them).
- Use `Optional` instead of PEP 604-style `None` unions.
- Use PEP 604-style unions instead of explicit `Union` type hints (except for the case with `Optional` just mentioned).
- Never use quoted / stringed type hints. Repo requires Python >=3.12 and PEP 649 and PEP 749 is accepted in that version.
- Use `Any` instead of `object` for most things where inference is hard or impossible. This includes tuples, lists and dicts that take non-builtin types that are hard to infer.
- Never use `object` or `Any` as a cop-out / shortcut to get around specifying a type that is known. Even if it leads to circular imports. Deal with circular imports a different way! Good type hints are a priority! They make bugs less likely.
- Don't be lazy and do `list[Any]` any time we have non-builtin types. That's bad practice. Most of the time types are inferable and should be used explicitly.
- Strings: double quotes for paths/logs/UI. Single quotes for enum‑like values and keys.
- Enums: use `StrEnum`, `IntEnum`, or `IntFlag`. UPPERCASE members.
- Control flow: prefer `match` over long if/elif chains where suitable.
- Exceptions: be specific. Avoid `except Exception:`. Log appropriately.
- OOP:
  - Avoid setters (use immutable design patterns).
  - Getters should generally be properties
  - Use setters methods when needed
  - Prefer helper classes over globals
  - Be thread-safe when it matters
  - Immutable data is good
  - Keeping fields private is good (expose with getter property if necessary)
- Add a blank line before a comment unless it's the first line in a block.
- When indentation level decreases, add a blank line before the new block of code.
- Logging: never `print()` in production paths.
- Never use `;` to contain two statements on one line: always put new statements on new lines.
- No global state (no global variables used for registries, etc.).
- Writing tests
  - If you need helpers, look for fixtures in `conftest.py` before writing local module helpers as the helper you require could already exist.
  - If your fixture exists but needs to be modified, modify the fixture and make sure all other tests logically work with the updated fixture.
  - If your fixture does not exist, consider if this will be useful for other tests in the future, and if so, write it as a fixture in `conftest.py` instead of as a local helper function or class.
  - If you need to add test support helpers, consider if it is better as a shared support helper.

### Before finishing the task

- You verify import order, type hints, and logger usage.
- You verify no try-excepts that swallow all exceptions silently.
- You verify no `getattr` calls are used unless absolutely necessary and if used, is always commented to explain why it is needed.
- You update docs/inline comments where behavior changed.
- You verify that the code is DRY (remember to look for other functions and classes that do the same thing).

### Python Style Guide

Read and follow `docs/contributing/python-style.md`. Agents must adhere to it strictly when writing Python code.

## Comments

- Comments should only explain what the code doesn't clearly state, or help us understand the structure when the code is complex or is made up of many steps.
- Do not use ";" in comments. It is almost never used by humans, especially coders. AI agents tend to overuse this in comments. Here we have a hard rule: NEVER USE THEM!
- Only standard keyboard symbols. Absolutely **no** emojis :)

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

If you choose to run `cargo test`, do so with `TMPDIR=/tmp` so that verification stays inside the sandbox.
