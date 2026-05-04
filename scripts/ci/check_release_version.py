import argparse
import json
import re
import subprocess
import sys
import tomllib
import zipfile
from pathlib import Path
from typing import Any, Optional


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
TAG_PATTERN = re.compile(r"^v(?P<version>[0-9]+\.[0-9]+\.[0-9]+(?:[a-zA-Z0-9.+-]*)?)$")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate Janiform release version consistency.",
    )

    parser.add_argument(
        "--version",
        help="Expected release version without the leading v.",
    )
    parser.add_argument(
        "--tag",
        help="Release tag, for example v0.1.0.",
    )
    parser.add_argument(
        "--wheel",
        type=Path,
        action="append",
        default=[],
        help="Wheel file whose metadata version must match.",
    )
    parser.add_argument(
        "--check-installed",
        action="store_true",
        help="Check janiform.__version__ in the active Python environment.",
    )

    args = parser.parse_args()

    expected_version = _expected_version(args.version, args.tag)

    errors: list[str] = []

    workspace_version = _workspace_version()
    if workspace_version != expected_version:
        errors.append(
            f"workspace.package.version is {workspace_version}, expected {expected_version}",
        )

    errors.extend(_workspace_package_version_errors(expected_version))

    for wheel_path in args.wheel:
        wheel_version = _wheel_metadata_version(wheel_path)
        if wheel_version != expected_version:
            errors.append(f"{wheel_path} metadata version is {wheel_version}, expected {expected_version}")

    if args.check_installed:
        installed_version = _installed_python_version()
        if installed_version != expected_version:
            errors.append(
                f"installed janiform.__version__ is {installed_version}, expected {expected_version}",
            )

    if errors:
        for error in errors:
            print(f"release version check failed: {error}", file=sys.stderr)

        return 1

    print(f"release version check passed for {expected_version}")
    return 0


def _expected_version(raw_version: Optional[str], raw_tag: Optional[str]) -> str:
    if raw_tag is None and raw_version is None:
        raise SystemExit("Either --version or --tag is required.")

    if raw_tag is None:
        return _require_non_empty(raw_version, "--version")

    match = TAG_PATTERN.fullmatch(raw_tag)
    if match is None:
        raise SystemExit(f"--tag must look like vX.Y.Z, found {raw_tag!r}")

    tag_version = match.group("version")
    if raw_version is not None and raw_version != tag_version:
        raise SystemExit(f"--version {raw_version!r} does not match --tag {raw_tag!r}")

    return tag_version


def _workspace_version() -> str:
    root_manifest = tomllib.loads((REPOSITORY_ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    return str(root_manifest["workspace"]["package"]["version"])


def _workspace_package_version_errors(expected_version: str) -> list[str]:
    metadata = _cargo_metadata()
    workspace_member_ids = set(metadata["workspace_members"])
    packages_by_id = {package["id"]: package for package in metadata["packages"]}

    errors: list[str] = []
    for package_id in sorted(workspace_member_ids):
        package = packages_by_id[package_id]

        package_version = str(package["version"])
        if package_version != expected_version:
            errors.append(
                f"{package['name']} package version is {package_version}, expected {expected_version}",
            )

    return errors


def _cargo_metadata() -> dict[str, Any]:
    try:
        result = subprocess.run(
            [
                "cargo",
                "metadata",
                "--locked",
                "--format-version",
                "1",
            ],
            check=True,
            capture_output=True,
            cwd=REPOSITORY_ROOT,
            text=True,
        )
    except FileNotFoundError as error:
        raise SystemExit("cargo is required for release version checks") from error

    return json.loads(result.stdout)


def _wheel_metadata_version(wheel_path: Path) -> str:
    with zipfile.ZipFile(wheel_path) as wheel:
        metadata_names = [
            name
            for name in wheel.namelist()
            if name.endswith(".dist-info/METADATA")
        ]
        if len(metadata_names) != 1:
            raise SystemExit(f"{wheel_path} must contain exactly one dist-info/METADATA file")

        metadata_text = wheel.read(metadata_names[0]).decode("utf-8")

    for line in metadata_text.splitlines():
        if line.startswith("Version: "):
            return line.removeprefix("Version: ").strip()

    raise SystemExit(f"{wheel_path} metadata does not contain a Version field")


def _installed_python_version() -> str:
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import janiform; print(janiform.__version__)",
        ],
        check=True,
        capture_output=True,
        text=True,
    )

    return result.stdout.strip()


def _require_non_empty(value: Optional[str], field_name: str) -> str:
    if value is None or not value.strip():
        raise SystemExit(f"{field_name} is required")

    return value


if __name__ == "__main__":
    raise SystemExit(main())
