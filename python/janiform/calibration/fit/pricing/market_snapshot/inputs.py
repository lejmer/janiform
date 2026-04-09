"""Public market-snapshot pricing input aliases."""

from __future__ import annotations

from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any

import pandas


type TabularFrameInput = str | Path | pandas.DataFrame | Mapping[str, Sequence[Any]]


__all__ = ["TabularFrameInput"]
