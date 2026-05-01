# Python Style

Follow these rules for code under `python/janiform/` and for Python tests.

## 1. General Principles

- Use Python 3.12 or newer and write code against the modern language features available in that version.
- Follow PEP 8 unless a rule below is more specific.
- Keep code DRY, but do not over-engineer abstractions.
- Do not introduce new global state or ad hoc singletons.
- Never place two statements on one line with `;`.
- Prefer readable, maintainable code over clever code.

## 2. Imports

Group imports in this order:

1. Standard library
2. Third-party packages
3. Repo-level absolute imports
4. Relative imports

Within each group:

- keep `import ...` lines together and sorted alphabetically
- keep `from ... import ...` lines together and sorted alphabetically
- separate major groups with a blank line
- keep `as` imports on their own line

Example:

```python
import math
from collections.abc import Callable
from typing import Optional

import numpy as np
import pandas as pd
from scipy.stats import gamma, laplace

import some_module
from repo_level_package.module import Something

from .relative_module import helper_function
```

## 3. Enums

- Use `StrEnum`, `IntEnum`, or `IntFlag` for enums unless a different base is strictly necessary.
- Keep enum members and other constants uppercase.

Example:

```python
from enum import StrEnum


class Status(StrEnum):
    RUNNING = "running"
    STOPPED = "stopped"
```

## 4. Type Hints

Type hints are required for functions, attributes, fields, parameters, generics, and important local variables.

Rules:

- Use built-in generics such as `list[int]` and `dict[str, Any]`.
- Use `Optional[T]` instead of `T | None`.
- Use `Type1 | Type2` instead of `Union[Type1, Type2]`.
- Use `Any` when the value can genuinely be anything. Use `object` only when you specifically mean `object`.
- For heterogeneous JSON-like mappings, prefer `dict[str, Any]` over `dict[str, object]`.
- Use PEP 695 generic syntax instead of `TypeVar` and `Generic[...]` when generics are needed.
- Do not use quoted type hints.
- Use `type` statements for type aliases when that makes complex annotations clearer.
- Import type classes from their current non-deprecated locations, such as `Callable` from `collections.abc`.
- Use `@override` when overriding methods.
- Use `@final` when a method or property is intended to remain closed to overriding.
- Prefer `Protocol`, `TypedDict`, and dataclasses where they fit naturally.
- Use explicit class name instead of `Self` in return types when forward references are straightforward.

Example:

```python
from collections.abc import Callable
from typing import Any, Optional, final, override


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

## 5. Control Flow

- Prefer `match` for switch-like branching. Avoid using a bunch of if-elif statements.
- Avoid defensive `try`/`except` blocks that add no value.
- Catch specific exceptions whenever possible.
- If you must catch an unexpected broad exception, document why, and never do so without preserving traces for logs.
- Use `e` as the exception variable name in `except` blocks unless there is a concrete reason not to.
- When logging an exception, log the message at the appropriate level and the stack trace at debug level if the code path expects that pattern.

Example:

```python
try:
    dangerous_stuff()
except ValueError as e:
    logger.warning(f"Unexpected ValueError: {e}")
    logger.debug(traceback.format_exc())
```

## 6. Logging

- Do not use `print()` in normal application code.
- Use the project logging system when one is available.
- Printing is only acceptable before logging is initialized, such as very early in a bootstrap entrypoint.

## 7. Object-Oriented Style

- Use `@property` only for cheap, non-mutating getters.
- Use `get_*` methods for non-trivial computed accessors.
- Do not use property setters.
- Prefer explicit `set_*` methods if mutation is truly necessary.
- Prefer immutable update patterns for non-GUI domain objects.
- Prefer helper classes with `@staticmethod` or `@classmethod` over global helper state when those helpers are only used in a single class.
- Avoid singleton patterns unless the boundary explicitly requires one.
- Keep fields private unless a dataclass or similar data carrier makes public fields intentional.
- Be thread-safe where shared access matters.
- Do not annotate `*args` or `**kwargs`.

## 8. Naming

- Use clear names such as `index`, `column`, and `timestamp`.
- Avoid abbreviations like `idx`, `col`, and `ts`.
- Short math-oriented names such as `i` and `j` are acceptable in genuinely mathy contexts, and in for-loops.
- Do not shadow outer-scope names when defining nested functions or local helpers.
- Follow PEP 8 naming unless the code is directly expressing a mathematical formula.

## 9. Comments And Structure

- Use standard keyboard symbols only.
- Add a blank line before a comment unless it is the first line in a block.
- When indentation decreases, add a blank line before the next block or statement.
- Comments should explain intent, structure, or non-obvious constraints, not restate code mechanically.

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

## 10. Parameters And Base Classes

When a function signature or base-class list exceeds 120 characters:

- put each parameter or base class on its own line
- indent by one level
- keep a trailing comma on the last item

Examples:

```python
def foo_bar(
    param_with_long_name: int,
    another_param_with_long_name: bool,
    yet_another_param: str,
    and_another_one: float,
) -> None:
    do_stuff()


class MyOtherSubClassWithALongName[TMyTypeVar](
    ABaseClassWithALongNameThatTakesUpSpace,
    AMixinThatAlsoHasAVeryLongName,
    AndYetAnotherBaseClass,
):
    ...
```

Do not use hanging signatures like this:

```python
def foo_bar(param_with_long_name: int,
            another_param_with_long_name: bool) -> None:
    do_stuff()
```

## 11 Avoid

- `getattr` and `setattr` unless dynamic attributes are truly unavoidable
- deprecated imports and deprecated typing forms
- broad defensive exception nests
- broad exception handlers with no logging or rationale
- global mutable state
- `print()` in production paths
- temporary implementations left behind as if they were final

When dynamic attributes are unavoidable, add a comment explaining why a normal typed access path is not possible.
