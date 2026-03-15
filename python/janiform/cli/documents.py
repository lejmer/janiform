import json
from pathlib import Path
from typing import Any

from .errors import _cli_validation_error
from .paths import _resolve_path_from_repository_root


def _load_json_document(path: Path, context: str) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(payload, dict):
        raise ValueError(f"{context} at {path} must decode to a JSON object")

    return payload


def _load_json_document_for_cli_field(
    *,
    repository_root: Path,
    path: Path,
    context: str,
    field_path: str,
    error_id: str,
    entrypoint: str,
) -> dict[str, Any]:
    resolved_path = _resolve_path_from_repository_root(repository_root, path)

    try:
        return _load_json_document(resolved_path, context)
    except FileNotFoundError as e:
        raise _cli_validation_error(
            field_path,
            "input file could not be found",
            error_id=error_id,
            entrypoint=entrypoint,
            context={"path": str(resolved_path)},
        ) from e
    except OSError as e:
        raise _cli_validation_error(
            field_path,
            f"input file could not be read: {e}",
            error_id=error_id,
            entrypoint=entrypoint,
            context={"path": str(resolved_path)},
        ) from e
    except json.JSONDecodeError as e:
        raise _cli_validation_error(
            field_path,
            "input file must contain valid JSON",
            error_id=error_id,
            entrypoint=entrypoint,
            context={
                "column_number": e.colno,
                "line_number": e.lineno,
                "path": str(resolved_path),
            },
        ) from e

