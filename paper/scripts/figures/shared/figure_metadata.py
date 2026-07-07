"""Shared metadata helpers for paper figure artifacts."""

import hashlib
import json
import platform
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Final, Optional


FIGURE_METADATA_SCHEMA_VERSION: Final[str] = "paper.figure_metadata.v1"


def file_artifact_metadata(
    path: Path,
    *,
    role: str,
    artifact_type: str,
    media_type: Optional[str] = None,
    include_sha256: bool = False,
    repository_root: Optional[Path] = None,
) -> dict[str, Any]:
    resolved_path = path.resolve()

    metadata: dict[str, Any] = {
        "role": role,
        "artifact_type": artifact_type,
        "path": path_for_metadata(path, repository_root=repository_root),
        "exists": resolved_path.exists(),
    }

    if media_type is not None:
        metadata["media_type"] = media_type

    if resolved_path.exists():
        stat_result = resolved_path.stat()
        metadata["size_bytes"] = stat_result.st_size

        if include_sha256:
            metadata["sha256"] = _sha256_file(resolved_path)

    return metadata


def build_figure_metadata(
    *,
    figure_id: str,
    title: str,
    script_path: Path,
    inputs: list[dict[str, Any]],
    outputs: list[dict[str, Any]],
    parameters: dict[str, Any],
    data_summary: dict[str, Any],
    plot_summary: dict[str, Any],
    command: list[str],
    notes: Optional[list[str]] = None,
    repository_root: Optional[Path] = None,
) -> dict[str, Any]:
    metadata: dict[str, Any] = {
        "schema_version": FIGURE_METADATA_SCHEMA_VERSION,
        "figure": {
            "id": figure_id,
            "title": title,
        },
        "generated_at_utc": datetime.now(UTC).isoformat(),
        "generator": {
            "script_path": path_for_metadata(script_path, repository_root=repository_root),
            "command": [
                command_argument_for_metadata(argument, repository_root=repository_root)
                for argument in command
            ],
            "python_version": sys.version,
            "platform": platform.platform(),
            "git_commit": _git_commit(),
        },
        "inputs": inputs,
        "outputs": outputs,
        "parameters": parameters,
        "data_summary": data_summary,
        "plot_summary": plot_summary,
    }

    if notes is not None:
        metadata["notes"] = notes

    return metadata


def write_metadata_json(path: Path, metadata: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)

    path.write_text(
        json.dumps(_json_ready(metadata), indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def path_for_metadata(path: Path, *, repository_root: Optional[Path] = None) -> str:
    if repository_root is None:
        return str(path)

    resolved_path = path.resolve()
    resolved_repository_root = repository_root.resolve()

    try:
        return resolved_path.relative_to(resolved_repository_root).as_posix()
    except ValueError:
        if path.is_absolute():
            return path.name

        return path.as_posix()


def command_argument_for_metadata(
    argument: str,
    *,
    repository_root: Optional[Path] = None,
) -> str:
    if repository_root is None:
        return argument

    argument_path = Path(argument)
    if not argument_path.is_absolute():
        return argument

    return path_for_metadata(argument_path, repository_root=repository_root)


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()

    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)

    return digest.hexdigest()


def _git_commit() -> Optional[str]:
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
    except FileNotFoundError:
        return None
    except subprocess.CalledProcessError:
        return None

    return result.stdout.strip()


def _json_ready(value: Any) -> Any:
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, dict):
        return {str(key): _json_ready(nested_value) for key, nested_value in value.items()}
    if isinstance(value, list):
        return [_json_ready(nested_value) for nested_value in value]
    if isinstance(value, tuple):
        return [_json_ready(nested_value) for nested_value in value]

    return value
