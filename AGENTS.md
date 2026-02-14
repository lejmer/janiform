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
- `src/`: Main application and libraries
- `tests/`: Unit and integration tests
- `docs/`: Design docs, examples, and the style guide

---

## Development Workflow
- Environment
  - Python: 3.14+
  - Install dev deps via `tox` or `pip` as appropriate.
- Run the app
  - `python src/main.py`
- Configure tests
  - Configure via `pyproject.toml`
  - Do not run tests, agents don't have `pytest` available in their environment
- Writing tests
  - If you need helpers, look for fixtures in `conftest.py` before writing local module helpers as the helper you require could already exist.
  - If your fixture exists but needs to be modified, modify the fixture and make sure all other tests logically work with the updated fixture.
  - If your fixture does not exist, consider if this will be useful for other tests in the future, and if so, write it as a fixture in `conftest.py` instead of as a local helper function or class.
- Change management
  - Keep diffs minimal and focused; avoid drive‑by refactors.
  - Bigger refactors are allowed only if asked for.
  - Update or add tests when changing behavior.
  - Maintain public APIs unless explicitly required to change.
  - Use project logging (no prints) and follow error‑handling guidance below.
- Database migrations (Alembic)
  - Treat migration files as append-only history. Once a revision exists, assume it may have been applied somewhere.
  - Never edit an existing revision to "fix" it. Alembic records applied revisions in `alembic_version`, so edits will not run for databases that already applied that revision, and the history becomes untrustworthy.
  - To change schema, create a new migration revision that depends on the current head, and put the new DDL there.
  - If you accidentally edited an already-existing migration, revert it back to its original contents and add a new follow-up migration with the intended changes.
  - Only use destructive reset approaches (dropping DB, stamping, deleting version rows) when the user explicitly wants a local throwaway reset and understands the implications.

---

## Agent Heuristics and Checklist
- Before adding new functions or classes, look through the repo to see if something similar already exists (except for enums and dataclasses that have context-dependent names):
  - If it does exist, use that instead of writing something new.
  - If it does exist, but slightly different, if it makes sense, extend it to do what you require it to do.
    - If you do extend it, do not leave backwards-compatible APIs; instead, refactor code that uses it so that they adhere to the new API.
  - If it does not exist, then you may write a new class or function.
  - If it exists in a different part of the program. For example: a class exists in the world server, but you want to use it in the master server, move it to the shared folder, do **not** import it from the world server.
- You may ALWAYS assume that users have **every dependency available**. No need think about backwards-compatibility issues or missing packages.
- Type hints everywhere (functions, attributes, locals where it matters).
- Never use quoted / stringed type hints; repo requires Python 3.14 and PEP 649 and PEP 749 is accepted in that version.
- Imports grouped and alphabetized per guide.
- Strings: double quotes for paths/logs/UI; single quotes for enum‑like values and keys.
- Enums: use `StrEnum`, `IntEnum`, or `IntFlag`; UPPERCASE members.
- Control flow: prefer `match` over long if/elif chains where suitable.
- Exceptions: be specific; avoid cargo‑cult try/except; log appropriately.
- OOP: avoid setters (use immutable design patterns); getters should generally be properties; use setters methods when needed; prefer helper classes over globals; be thread-safe when it matters; immutable data is good; keeping fields private is good (expose with getter property if necessary).
- Naming: clear, descriptive; avoid cryptic abbreviations and shorthands outside mathy contexts (we hate `idx` and `col`, never use that ever).
- Comments: follow the blank‑line rules from the guide.
- Logging: never `print()` in production paths.
- Never use `;` to contain two statements on one line: always put new statements on new lines.
- No global state (no global variables used for registries, etc.).
- FastAPI + SQLAlchemy: request-scoped `AsyncSession` is shared across the dependency graph. Any dependency that touches the DB (even a `SELECT`) will trigger SQLAlchemy's autobegin behavior and start a transaction. If your endpoint depends on `require_authenticated_session` (or any other DB-using dependency), do not wrap your handler in `async with session.begin()` or you'll hit `InvalidRequestError: A transaction is already begun on this Session.`. Prefer issuing statements directly and then `await session.commit()`, or use `async with session.begin_nested()` if you truly need a context manager.

Before finishing the task:
- You verified import order, type hints, and logger usage.
- You verified no try-excepts that swallow all exceptions silently.
- You verified no `getattr` calls are used unless absolutely necessary and if used, is always commented to explain why it is needed.
- You avoided changing public APIs unintentionally.
- You updated docs/inline comments where behavior changed.
- You verify that the code is DRY (remember to look for other functions and classes that do the same thing).
- You update README.md with an updated project structure, if there are big enough changes to it.
- All code compiles (verify by running `python -m py_compile`).

---

## Comments
Comments should only explain what the code doesn't clearly state, or help us understand the structure when the code is complex or is made up of many steps.
NOTE: Do not use ";" in comments. It is almost never used by humans, especially coders. AI agents tend to overuse this in comments. Here we have a hard rule: NEVER USE THEM!

## Python Style Guide
Read and follow `docs/development/guides/style_guide.md`. Agents must adhere to it strictly.
