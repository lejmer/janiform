import argparse
import hashlib
import json
from datetime import UTC, datetime
from pathlib import Path
from typing import Any


MANIFEST_SCHEMA_VERSION = "janiform.release_manifest.v1"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Build a Janiform release artifact manifest.",
    )

    parser.add_argument("--artifact-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--git-sha", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--output", type=Path, required=True)

    args = parser.parse_args()

    artifacts = [
        _artifact_entry(path)
        for path in sorted(args.artifact_dir.iterdir())
        if path.is_file() and path.resolve() != args.output.resolve()
    ]

    manifest: dict[str, Any] = {
        "schema_version": MANIFEST_SCHEMA_VERSION,
        "version": args.version,
        "tag": args.tag,
        "git_sha": args.git_sha,
        "github_run_id": args.run_id,
        "generated_at_utc": datetime.now(UTC).isoformat(),
        "artifacts": artifacts,
    }

    args.output.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    print(args.output)
    return 0


def _artifact_entry(path: Path) -> dict[str, Any]:
    return {
        "file_name": path.name,
        "artifact_type": _artifact_type(path),
        "sha256": _sha256(path),
        "size_bytes": path.stat().st_size,
    }


def _artifact_type(path: Path) -> str:
    name = path.name
    if name.endswith(".whl"):
        return "python_wheel"
    if name.endswith(".tar.gz") and name.startswith("janiform-cli-"):
        return "standalone_cli_archive"
    if name.endswith(".zip") and name.startswith("janiform-cli-"):
        return "standalone_cli_archive"
    if name.endswith(".tar.gz"):
        return "python_sdist"
    if name == "SHA256SUMS":
        return "checksums"

    return "release_support"


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)

    return digest.hexdigest()


if __name__ == "__main__":
    raise SystemExit(main())
