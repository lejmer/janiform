import argparse
import os
import shutil
import tarfile
import tempfile
import zipfile
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Package a Janiform standalone CLI archive.",
    )

    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)

    args = parser.parse_args()

    if not args.binary.exists():
        raise SystemExit(f"CLI binary does not exist: {args.binary}")

    args.output_dir.mkdir(parents=True, exist_ok=True)

    archive_stem = f"janiform-cli-{args.version}-{args.target}"
    executable_name = "janiform.exe" if args.binary.suffix.lower() == ".exe" else "janiform"

    with tempfile.TemporaryDirectory(prefix="janiform-cli-archive-") as temporary_directory:
        staging_root = Path(temporary_directory) / archive_stem
        staging_root.mkdir(parents=True)

        shutil.copy2(args.binary, staging_root / executable_name)
        shutil.copy2(REPOSITORY_ROOT / "LICENSE", staging_root / "LICENSE")
        shutil.copy2(
            REPOSITORY_ROOT / "THIRD_PARTY_NOTICES.txt",
            staging_root / "THIRD_PARTY_NOTICES.txt",
        )

        (staging_root / "README.txt").write_text(
            "\n".join(
                [
                    f"Janiform CLI {args.version}",
                    "",
                    "Run `janiform --help` to inspect available commands.",
                    "This archive contains the standalone Rust CLI executable.",
                    "",
                ]
            ),
            encoding="utf-8",
        )

        if os.name == "nt":
            archive_path = args.output_dir / f"{archive_stem}.zip"
            _write_zip(archive_path, staging_root)
        else:
            archive_path = args.output_dir / f"{archive_stem}.tar.gz"
            _write_tar_gz(archive_path, staging_root)

    print(archive_path)
    return 0


def _write_zip(archive_path: Path, staging_root: Path) -> None:
    with zipfile.ZipFile(archive_path, mode="w", compression=zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(staging_root.rglob("*")):
            if path.is_file():
                archive.write(path, path.relative_to(staging_root.parent))


def _write_tar_gz(archive_path: Path, staging_root: Path) -> None:
    with tarfile.open(archive_path, mode="w:gz") as archive:
        archive.add(staging_root, arcname=staging_root.name)


if __name__ == "__main__":
    raise SystemExit(main())
