# General conventions

These rules apply generally across the Janiform codebase. Use them together with the language-specific guides:

- [Python conventions](python-conventions.md)
- [Rust conventions](rust-conventions.md)
- [Maintaining public API contracts](public-api-contracts.md)
- [Writing tests](writing-tests.md)

## Core principles

- Prefer clarity and maintainability over cleverness.
- Keep behavior easy to reason about at the contract boundary and in the owning module.
- Follow the style and architectural conventions already present near the code you are editing.

## Don't repeat yourself

- Before adding a helper, function, class, type, or module, search for equivalent or near-equivalent behavior.
- If equivalent behavior exists, reuse it or move it to one canonical implementation in the appropriate shared or domain module.
- If similar behavior exists, extend the existing implementation when that keeps the design clearer.
- If no equivalent helper exists but the behavior is reusable beyond the immediate local context, place it where future callers can find it.
- Keep behavior local only when it is tightly coupled to one function or module.
- Do not repeat chunks of code without a real reason.

## Module and function size

- Keep modules focused.
- Prefer adding a focused module over growing an already large module.
- Target production modules under 500 lines of code.
- If a file exceeds roughly 800 lines, add new functionality in a new module unless there is a strong reason not to.
- Prefer adding helpers over growing large functions.
- Target functions under 100 lines of code.
- If a function exceeds roughly 200 lines, add new behavior in a new helper or function unless there is a strong reason not to.

## Documentation

- Keep documentation phrased as the current project shape, not as a migration note.
- Link to the owning reference or convention page instead of duplicating long rules across documents.

## Public API contracts

- Treat public APIs as deliberate contracts.
- Keep public API changes aligned with tests, fixtures, generated references, examples, and docs in the same pull request.
- Use the public API guardrail workflow when the tracked Python public surface changes.

For the full policy and snapshot update process, see [Maintaining public API contracts](public-api-contracts.md).
