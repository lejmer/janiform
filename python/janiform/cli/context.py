from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True, slots=True)
class CliCommandContext:
    command_path: tuple[str, ...]
    command_identifier: str | None
    default_map: dict[str, Any]
    environment: Mapping[str, str]
    repository_root: Path


__all__ = ["CliCommandContext"]
