import re
from pathlib import Path
from typing import Final

from tests.support.rust_workspace import load_workspace_package_names


CI_RUST_PACKAGE_ARGUMENT_PATTERN: Final = re.compile(r"^\s+-p\s+(?P<package_name>[A-Za-z0-9_-]+)\s*$")


def test_multiplatform_rust_shards_cover_workspace_crates(
    project_root: Path,
) -> None:
    """The manual multiplatform workflow runs the CI Rust verification shards."""
    workspace_package_names = load_workspace_package_names(project_root)
    workflow_path = project_root / ".github" / "workflows" / "multiplatform-verification.yml"
    package_names: list[str] = []

    for line in workflow_path.read_text(encoding="utf-8").splitlines():
        package_match = CI_RUST_PACKAGE_ARGUMENT_PATTERN.match(line)
        if package_match is not None:
            package_names.append(package_match.group("package_name"))

    assert sorted(package_names) == sorted(workspace_package_names), workflow_path
