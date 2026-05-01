# Python conventions

These rules apply to Python code under `python/janiform/`, Python tests, Python scripts, and Python examples unless a narrower rule is stated.

Python is a user-facing boundary layer in Janiform. It defines typed contracts, ergonomic APIs, command-line entrypoints, serialization and deserialization, error mapping, and thin adapters around native Rust behavior. It must not become a second implementation of Janiform's model logic.

Write code that is valid on the minimum supported Python version for the project. Do not use syntax or standard-library features that require a newer interpreter unless the project minimum is raised in the same change.

## Role of Python

- Python is mostly a thin wrapper and user-facing API or CLI layer.
- Core library behavior must be implemented in Rust, not Python.
- Logic that affects model behavior, numerical results, state evolution, calibration outcomes, pricing outputs, simulation paths, or validation decisions belongs in Rust unless a task explicitly says otherwise.
- Python may define typed contracts, command-line entrypoints, serialization helpers, and boundary adapters.
- Python must not implement state-estimation algorithms, simulation evolution, pricing recursions, calibration math, optimization logic, statistical transforms, numerical integration, or any computation that meaningfully determines outputs.
- If a feature needs new computation, Rust owns the algorithm and validation of the computational contract, PyO3 exposes the native entrypoint, and Python calls the native entrypoint while converting inputs and outputs.
- If a Python helper would loop over market observations, evolve model state, compute fitted or runtime state, apply numerical recursions, or reconstruct a model boundary condition, stop and move that logic into Rust.
- Do not introduce undocumented numerical heuristics, magic constants, or solver policies. Numeric decisions must be mathematically derived, data-derived, or explicitly specified and documented in a named contract or policy.
- Python code must work on both Windows and Linux. Never hardcode Linux-only paths.

## Public contracts

- Treat public Python APIs as deliberate typed contracts.
- Avoid changing public APIs unintentionally.
- Boundary value means validation, policy or default injection, type conversion, error mapping, or telemetry.
- If no boundary value is needed, call the shared function directly or use a non-duplicative re-export.
- If more than one optional override is needed, prefer a typed configuration or contract object.

## General code quality

- Follow PEP 8 unless a repo rule is more specific.
- Never place two statements on one line with `;`.

## Modules and package files

- `__init__.py` files must be thin and never include declarations.
- Never use wildcard imports.
- Avoid global mutable state and registries.
- Keep boundary code thin. Convert Python inputs to Rust types as early as possible, and convert Rust results and errors back to Python at the boundary.
- PyO3 is allowed only in the binding crate. Python-facing code should not leak internal Rust structs by default.

## Imports

Group imports in this order:

1. Standard library imports.
2. Third-party package imports.
3. Repo-level absolute imports.
4. Relative imports.

Within each group:

- Put `import ...` lines before `from ... import ...` lines.
- Keep `import ...` lines together and sorted alphabetically.
- Keep `from ... import ...` lines together and sorted alphabetically.
- Separate major groups with one blank line.
- Keep `as` imports on their own line.
- Import type classes from their current non-deprecated locations, such as `Callable` from `collections.abc`.
- Do not use deprecated imports or deprecated typing forms.

Example:

```python
import math
from collections.abc import Callable
from typing import Optional

import numpy as np
import pandas as pd
from scipy.stats import gamma, laplace

import janiform.some_module
from janiform.some_package import Something

from .relative_module import helper_function
```

## Type hints

- Type hints are required for functions, attributes, fields, parameters, generics, and important local variables.
- Use built-in generics such as `list[int]` and `dict[str, Any]`.
- Use `Optional[T]` instead of `T | None`.
- Use PEP 604 unions such as `Type1 | Type2` instead of `Union[Type1, Type2]`, except for optional values, where `Optional[T]` is required.
- Never use quoted or stringed type hints. The repo requires Python 3.12 or newer, and PEP 649 and PEP 749 are accepted in that version.
- Use PEP 695 generic syntax instead of `TypeVar` and `Generic[...]` when generics are needed.
- Use `type` statements for type aliases when that makes complex annotations clearer.
- Use `Any` when a value can genuinely be anything.
- Use `Any` instead of `object` for values whose type is hard or impossible to infer, including tuples, lists, and dictionaries containing hard-to-infer non-builtin types.
- Use `object` only when you specifically mean `object`.
- For heterogeneous JSON-like mappings, prefer `dict[str, Any]` over `dict[str, object]`.
- Never use `Any` or `object` as a shortcut when the real type is known.
- Do not write lazy annotations such as `list[Any]` when a more precise type is inferable.
- Resolve circular imports without degrading type quality.
- Use `Final` for constants.
- Prefer `Protocol`, `TypedDict`, and dataclasses where they fit naturally.
- Use an explicit class name instead of `Self` in return types when forward references are straightforward.
- Do not annotate `*args` or `**kwargs`.

Example:

```python
from collections.abc import Callable
from typing import Any, Optional, final, override


@final
class MyOtherClass(MyClass):
    @override
    def load_config(
        self,
        path: str,
        parser: Callable[[str], dict[str, Any]],
    ) -> Optional[dict[str, Any]]:
        return parser(path)

    @staticmethod
    def create() -> MyOtherClass:
        return MyOtherClass(...)
```

## Strings and keys

- Use double quotes for all strings.
- Use standard keyboard symbols only.
- Do not use emojis.

## Enums and constants

- Use `StrEnum`, `IntEnum`, or `IntFlag` for enums unless a different base is strictly necessary.
- Keep enum members uppercase.
- Keep other constants uppercase.

Example:

```python
from enum import StrEnum


class Status(StrEnum):
    RUNNING = "running"
    STOPPED = 'stopped'
```

## Control flow

- Prefer `match` for switch-like branching.
- Avoid long `if` or `elif` chains when `match` fits.
- Avoid defensive `try` or `except` blocks that add no value.
- Catch specific exceptions whenever possible.
- Avoid `except Exception:`.
- If a broad exception handler is truly unavoidable, document why and preserve traces.
- Never swallow all exceptions silently.
- Use `e` as the exception variable name in `except` blocks unless there is a concrete reason not to.
- Use early validation and clear error paths instead of deeply nested code.

## Dependencies

- Add dependencies deliberately.
- Avoid just-in-case packages.
- Prefer widely used packages with good maintenance signals.
- Keep feature flags minimal.
- Document why each non-default feature is enabled.
- Assume required dependencies are available instead of adding fallback code for missing packages.

## Object-oriented style

- Prefer immutable design patterns.
- Use immutable update patterns for non-GUI domain objects.
- Use `@property` only for cheap, non-mutating getters.
- Use `@final` when a method or property is intended to remain closed to overriding.
- Use `get_*` methods for non-trivial computed accessors.
- Avoid property setters.
- Prefer explicit `set_*` methods if mutation is truly necessary.
- Keep fields private unless a dataclass or similar data carrier makes public fields intentional.
- Expose private fields with getter properties when needed.
- Use `@override` when overriding methods.
- Prefer helper classes with `@staticmethod` or `@classmethod` over global helper state when those helpers are only used in a single class.
- Prefer helper classes over globals.
- Avoid singleton patterns unless the boundary explicitly requires one.

## Naming

- Use clear, descriptive names.
- Use names such as `index`, `column`, and `timestamp`.
- Avoid abbreviations such as `idx`, `col`, and `ts`.
- Avoid cryptic abbreviations and shorthands outside mathy contexts.
- Short math-oriented names such as `i` and `j` are acceptable in genuinely mathy contexts and in for-loops.
- Do not shadow outer-scope names when defining nested functions or local helpers.
- Follow PEP 8 naming unless the code is directly expressing a mathematical formula.

## Comments and structure

- Comments should explain intent, structure, non-obvious constraints, or what the code does not clearly state.
- Comments must not restate code mechanically.
- Use comments sparingly.
- Use standard keyboard symbols only, except where mathematical notation is genuinely needed.
- Do not use emojis.
- Add a blank line before a comment unless the comment is the first line in a block.
- When indentation decreases, add a blank line before the next block or statement.
- Use blank lines to separate logical phases after setup, validation, transformation, I/O, and before returning constructed values.
- Do not compress unrelated statements together just to minimize vertical space.
- Do not add blank lines inside argument lists, parameter lists, struct literals, enum variants, match cases, or chains of near-identical statements.
- Keep tightly related assignment plus immediate guard or check together when the assignment is short and the guard directly validates it.

Example:

```python
if condition:
    # First line in the block, so no blank line above.
    do_something()

    # Not the first line in the block, so keep a blank line above.
    do_something_else()

# Indentation decreased, so keep a blank line before the next block.
next_block_of_code()
```

## Parameters and base classes

When a function signature or base-class list exceeds 120 characters:

- Put each parameter or base class on its own line.
- Indent by one level.
- Keep a trailing comma on the last item.

Example:

```python
def foo_bar(
    param_with_long_name: int,
    another_param_with_long_name: bool,
    yet_another_param: str,
    and_another_one: float,
) -> None:
    do_stuff()


class MyOtherSubClassWithALongName[TMyType](
    ABaseClassWithALongNameThatTakesUpSpace,
    AMixinThatAlsoHasAVeryLongName,
    AndYetAnotherBaseClass,
):
    ...
```

Do not use hanging signatures:

```python
def foo_bar(param_with_long_name: int,
            another_param_with_long_name: bool) -> None:
    do_stuff()
```

## Dynamic attributes

- Avoid `getattr` and `setattr` unless dynamic attributes are truly unavoidable.
- When dynamic attributes are unavoidable, add a comment explaining why a normal typed access path is not possible.

## Tests

- Prefer fast unit tests for Python boundary behavior.
- If you need helpers, look for fixtures in `conftest.py` before writing local helpers.
- If an existing fixture can be adapted, update it and make sure all other tests logically work with the updated fixture.
- If a fixture does not exist, consider whether it will be useful for other tests in the future.
- If a helper is likely to be reused, write it as a fixture in `conftest.py` or as shared test support instead of a local helper.

## Review checklist

Before finishing a Python change:

- Verify import order.
- Verify type hints, including important local variables.
- Verify no quoted type hints were added.
- Verify `Optional[T]` is used instead of `T | None`.
- Verify exceptions are specific and no broad handler silently swallows errors.
- Verify no `getattr` or `setattr` calls were added unless necessary and commented.
- Verify no new global state was introduced.
- Verify the code remains DRY and equivalent behavior was not duplicated.
- Verify public APIs were not changed unintentionally.
- Update docs and inline comments where behavior changed.
- Update relevant docs, including `README.md`, when behavior changes.
- Run the formatting, compile, and test commands.
- Verify code compiles.
- Verify tests succeed.
- Do a final formatting pass to catch issues such as missing blank lines.
