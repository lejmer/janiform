import argparse
import os
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Smoke test a Janiform standalone CLI archive.",
    )
    parser.add_argument("archive", type=Path)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="janiform-cli-smoke-") as temporary_directory:
        extraction_root = Path(temporary_directory)

        _extract_archive(args.archive, extraction_root)
        executable = _find_executable(extraction_root)

        if os.name != "nt":
            executable.chmod(executable.stat().st_mode | 0o111)

        subprocess.run([str(executable), "--help"], check=True)

    print(f"CLI archive smoke passed for {args.archive}")
    return 0


def _extract_archive(archive_path: Path, extraction_root: Path) -> None:
    if archive_path.suffix == ".zip":
        with zipfile.ZipFile(archive_path) as archive:
            archive.extractall(extraction_root)

        return

    if archive_path.name.endswith(".tar.gz"):
        with tarfile.open(archive_path, mode="r:gz") as archive:
            archive.extractall(extraction_root)

        return

    raise SystemExit(f"unsupported archive format: {archive_path}")


def _find_executable(extraction_root: Path) -> Path:
    executable_name = "janiform.exe" if os.name == "nt" else "janiform"
    matches = sorted(extraction_root.rglob(executable_name))
    if len(matches) != 1:
        raise SystemExit(f"expected exactly one {executable_name} in {extraction_root}, found {len(matches)}")

    return matches[0]


if __name__ == "__main__":
    raise SystemExit(main())
