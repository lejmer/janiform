"""
Package root for Janiform.

Import workflow APIs from task-oriented modules such as `janiform.simulation`,
`janiform.pricing`, and `janiform.risk`.
"""

from importlib.metadata import PackageNotFoundError, version


try:
    __version__: str = version("janiform")
except PackageNotFoundError:
    __version__ = "0.0.0"


__all__ = ["__version__"]
