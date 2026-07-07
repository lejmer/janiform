import tomllib
from pathlib import Path
from typing import Any


def load_workspace_package_names(project_root: Path) -> list[str]:
    workspace_document: dict[str, Any] = tomllib.loads(
        (project_root / "Cargo.toml").read_text(encoding="utf-8")
    )
    workspace_section = workspace_document["workspace"]
    if not isinstance(workspace_section, dict):
        raise AssertionError("workspace Cargo.toml must contain a workspace table")

    workspace_members = workspace_section["members"]
    if not isinstance(workspace_members, list):
        raise AssertionError("workspace members must be a list")

    package_names: list[str] = []
    for workspace_member in workspace_members:
        if not isinstance(workspace_member, str):
            raise AssertionError("workspace member paths must be strings")

        crate_manifest_document: dict[str, Any] = tomllib.loads(
            (project_root / workspace_member / "Cargo.toml").read_text(encoding="utf-8")
        )
        package_section = crate_manifest_document["package"]
        if not isinstance(package_section, dict):
            raise AssertionError(f"{workspace_member} must contain a package table")

        package_name = package_section["name"]
        if not isinstance(package_name, str):
            raise AssertionError(f"{workspace_member} package name must be a string")

        package_names.append(package_name)

    return package_names
