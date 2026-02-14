# Python Style Guide

Greetings!

You have stumbled upon a sacred scroll, one that guards us from an ancient evil. Follow it faithfully, and your code will be readable, consistent, and future-proof. Follow it, and the sun will continue to rise every day.

---

## 1. General Principles

- **Python version:** Always use Python **3.14+**. No excuses, no backwards compatibility hacks. Repo REQUIRES Python 3.14. NO NEED for backwards compatibility.
- **Standards:** Follow **PEP 8** generally for style.
- **DRY (Don't Repeat Yourself):** Don't copy-paste logic. If you must repeat yourself, evaluate life choices. Then refactor.
- **Be practical:** DRY is good and all, but don't overdo it. Over-engineering is the death of productivity.
- **No global state, no rogue singletons.** Do not introduce new global variables. Only the registry pattern is allowed when exposing singletons.
- **Make it readable:** Never use `;` to contain two statements on one line, `;` is strictly forbidden. Always put new statements on new lines.

---

## 2. Imports

Imports are grouped and ordered like so:

1. **Python standard library**

   1.1 `from ... import ...`

   1.2 `import ...`

2. **Third-party modules/packages**

   2.1 `from ... import ...`
   
   2.2 `import ...`

3. **Repo-level modules/packages**

   3.1 `from ... import ...`

   3.2 `import ...`

4. **Relative imports within the repo**
   
   4.1 `from ... import ...`
   
   4.2 `import ...`

Rules of thumb:
- Always put a blank line before `import ...` when switching from `from ... import ...`.
- Always put a blank line between the major import groups above.
- Always sort each group alphabetically

### Example:

```python
from collections.abc import Callable
from typing import Optional

import math

from qtpy.QtCore import Qt
from qtpy.QtGui import QColor

import numpy as np
import pandas as pd

from repo_level_package.module import Something

from .relative_module import helper_function
```

---

## 3. Strings and Enums

- Use **double quotes (`"`)** for paths, logs, and UI text.
- Use **single quotes (`'`)** for enum-like values, keys, columns, attribute names, and the like.

### Example:

```python
log_path: str = "/var/log/app.log"
status: str = 'running'
value: Optional[int] = some_dict.get('key', None)

try:
    value = some_dict['key']
except KeyError as e:
    logger.debug(f"Lost my 'key': {traceback.format_exc()}")
    raise KeyError("Seems 'key' was missing. Bummer.") from e

df['my_column'] = ...

try:
    # Dynamic object, attribute may be missing
    getattr(some_dynamic_object, 'some_method')
except AttributeError:
    ...  # Handle in some way
```

- Actual enums must be `StrEnum`, `IntEnum`, or `IntFlag`. No legacy `Enum` unless strictly necessary.
- All enums (and all other constants) should be **UPPERCASE**.

```python
from enum import StrEnum


class Status(StrEnum):
    RUNNING = 'running'
    STOPPED = 'stopped'

```

---

## 4. Type Hints (Mandatory Everywhere)

Type hints are required for **functions, attributes, fields, parameters, generics, and local variables**. Skip them only for `*args` and `**kwargs`, `__init__` return types, or for those local variables where it really doesn't matter.

### Rules:
- Always use built-in generics (`list[int]`, `dict[str, Any]`) instead of the deprecated `List`, `Dict`, etc.
- Use `Optional[T]` instead of `T | None`.
- Use `Type1 | Type2` instead of `Union[Type1, Type2]`
- Use `Any` when you mean "literally anything," use `object` only when you truly mean *object* (eg. `Signal(object)`).
- Use **PEP 695 generics** (Python 3.12+ syntax). Don't touch the old `TypeVar`/`Generic[...]` mess.
- Never use stringed type hints.
- When simplifying complicated type hints with type aliases, use `type` statements (Python 3.12+ syntax).
- Import type classes from their non-deprecated homes: e.g. `Callable` from `collections.abc`, not `typing`.
- Always use `@override` when overriding methods (**PEP 698**).
- Use `@final` from `typing` when a method or property is law.
- Prefer `Protocols`, `TypedDict`, and dataclasses where appropriate.
- Prefer the actual type over using `Self` in return types, as forward referencing is possible in Python 3.14 without using hacks.

### Example:

```python
from collections.abc import Callable
from typing import Any, Optional, override

from .my_class import MyClass


class MyOtherClass(MyClass):
    @override
    def load_config(path: str, parser: Callable[[str], dict[str, Any]]) -> Optional[dict[str, Any]]:
        return parser(path)

    @staticmethod
    def create() -> MyOtherClass:
        return MyOtherClass(...)
```

---

## 5. Control Flow

- Use `match` statements instead of switch-like `if` chains.

```python
def server_status(status: str) -> str:
    match status:
        case 'running':
            return "Server is running"
        case 'stopped':
            return "Server has halted"
        case _:
            return "Unknown server status"
```

- Avoid unnecessary `try`/`except`. Don't catch exceptions "just to be safe". If you must catch something impossible, log it properly, including stack trace at debug-level.
- Try to be specific with exceptions.
- When catching multiple types of exceptions when not using `as`, do not use parentheses as they are now deprecated as of Python 3.14.
- In the rare cases where catching all exceptions is necessary, always write a comment as to why.
- When using `as`, always call the exception `e`, unless the try-except is nested and/or the name is taken.

```python
try:
    dangerous_stuff()
except ValueError as e:
    logger.warning(f"Unexpected ValueError: {e}")  # Notice that the warning level is for the error
    logger.debug(traceback.format_exc())  # And the debug level is for the stack trace

try:
    dangerous_stuff2()
except (KeyError, ValueError) as e:  # `as` is used, so keep parentheses here
    logger.warning(f"Unexpected error: {e}")
    logger.debug(traceback.format_exc())

try:
    dangerous_stuff3()
except KeyError, ValueError:  # No `as` is used, so no parentheses this time (valid as of Python 3.14)
    logger.debug(traceback.format_exc())
```

---

## 6. Logging

- Never `print()` (except before loggers have been created, like early in `main.py`). Printing is for script kiddies debugging incorrectly, not for production code.
- If the repository has a system for logging, always use that.

---

## 7. Object-Oriented Style

- Use `@property` only for cheap, non-mutating getters.
- Use `get_*` methods if the property does real computation (generally shouldn't mutate state either).
- Never use property setters. Always use `set_*` methods.
- Avoid setters when dealing with non-GUI stuff. Use immutable design patterns (`my_object.with_some_property(value_of_property)` <- new instance with updated state).
- Prefer helper classes with `@staticmethod` (or `@classmethod` if you need access to `cls`) over global helpers.
- No rogue singletons.
- Be thread-safe when it matters.
- Keep fields private unless it's a dataclass (expose with getter property if necessary).

---

## 8. Naming Conventions

- No cryptic abbreviations (`idx`, `col`, `ts`). You may use mathy abbreviations when appropriate, like in math formulas or a loop index like `i`, `j`.
- Use clear, descriptive names: `index`, `column`, `timestamp`.
- Don't shadow outer-scope names when writing local functions.
- Follow PEP 8. Exception: *math equations*.

---

## 9. Comments and Documentation

- Only standard keyboard symbols. Absolutely **no** emojis :)
- Add a blank line before a comment unless it's the first line in a block.
- When indentation level decreases, add a blank line before the new block of code.

### Do:

```python
if condition:
    # This comment is the first line of a block -- no blank line
    do_something()
    
    # This comment is not the first line in the block -- blank line above it!
    do_something_else()

# Blank line here too because indentation level decreased
next_block_of_code()
```

### Don't:

```python
if condition:
    # This comment is the first line of a block
    do_something()
    # This comment is not the first line in the block
    do_something_else()
next_block_of_code()
```

---

## 10. Parameters and Base Classes

### Function and method parameters

When formatting function and method parameters, if the line exceeds 120 characters, put each parameter on a new line, two indentation levels in (not one!). When putting parameters on new lines, the last parameter should have a trailing comma as well. The same goes for base class lists.

#### Do:

```python
def foo_bar(param_with_long_name: int, another_param_with_long_name: bool) -> None:
    # We had room left to fill the 120 char limit so this is fine
    do_stuff()


def foo_bar_again(
        param_with_long_name: int,
        another_param_with_long_name: bool,
        yet_another_param: str,
        and_another_one: float,  # <-- Trailing comma!
) -> None:
    # Here the line would have exceeded 120 chars so we put the parameters each on their own lines,
    # and used two indentation levels!
    do_stuff()


class MySubClass(BaseClass, SomeMixin):
    # Here we put the base classes we inherit from on the same line since we did not exceed 120 chars.
    ...


class MyOtherSubClassWithALongName[TMyTypeVar](
        ABaseClassWithALongNameThatTakesUpSpace,
        AMixinThatAlsoHasAVeryLongName,
        AndYetAnotherBaseClass,  # <-- Trailing comma!
):
    # Here the line would have exceeded 120 chars so we put the base classes on their own lines,
    # and used two indentation levels!
    ...
```

#### Don't:

```python
def foo_bar(param_with_long_name: int,
            another_param_with_long_name: bool) -> None:
    # This is wrong. Don't do this!
    do_stuff()


def foo_bar_again(param_with_long_name: int, another_param_with_long_name: bool, yet_another_param: str, and_another_one: float) -> None:
    # This is wrong. Don't do this!
    do_stuff()


class MySubClass(BaseClass,
                 SomeMixin):
    # This is wrong. Don't do this!
    ...


class MyOtherSubClassWithALongName[TMyTypeVar](ABaseClassWithALongNameThatTakesUpSpace, AMixinThatAlsoHasAVeryLongName, AndYetAnotherBaseClass):
    # This is wrong. Don't do this!
    ...
```

---

## 11. Refactoring and Maintenance

- Refactor only when it makes code cleaner, more maintainable, or more correct.
- Again though, DON'T OVERDO IT. <3
- Always follow the rules above when refactoring.

---

## 12. Wall of Shame

You'll be sorry if you use:
- `getattr`/`setattr` (unless dynamic attributes are truly unavoidable). Use `isinstance` + `cast` instead to help your IDE out.
- Deprecated imports and type hints (`Dict`, `List`, `typing.Callable`, etc.).
- Overly defensive `try`/`except` nests.
- No logging in `except Exception:` clauses.
- Global variables.
- Printing instead of logging.
- Half-baked "temporary" implementations.

---

## 12. Heed My Words

It is **imperative** that you follow this document *carefully*.

Ignore any of these words, and... you'll awaken the ancient one from its slumber and initiate the sequence of events that eventually lead to **the end of the world**. It brings with it infinite recursion, long stack traces, silent errors, and oh so subtle (yet deadly) bugs.

A slight inconvenience, one might say.

So follow the rules.

Do it.

Do.

It.

Doit!
