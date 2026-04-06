"""Internal repository-root and path-resolution helpers for the CLI."""

from pathlib import Path
from typing import Optional


def resolve_repository_root() -> Path:
    return Path(__file__).resolve().parents[3]


def resolve_path_from_repository_root(repository_root: Path, path: Path) -> Path:
    if path.is_absolute():
        return path

    return repository_root / path


def require_path_argument(
    repository_root: Path,
    path: Optional[Path],
    *,
    argument_name: str,
) -> Path:
    if path is None:
        raise ValueError(f"--{argument_name.replace('_', '-')} is required")

    return resolve_path_from_repository_root(repository_root, path)
