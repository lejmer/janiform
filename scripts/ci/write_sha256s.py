import argparse
import hashlib
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Write SHA256SUMS for release artifacts.",
    )

    parser.add_argument("--artifact-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)

    args = parser.parse_args()

    artifact_paths = [
        path
        for path in sorted(args.artifact_dir.iterdir())
        if path.is_file() and path.resolve() != args.output.resolve()
    ]

    lines = [
        f"{_sha256(path)}  {path.name}"
        for path in artifact_paths
    ]
    args.output.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(args.output)
    return 0


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)

    return digest.hexdigest()


if __name__ == "__main__":
    raise SystemExit(main())
