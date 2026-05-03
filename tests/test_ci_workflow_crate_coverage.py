import re
from pathlib import Path

from tests.support.rust_workspace import load_workspace_package_names


CI_RUST_PACKAGE_ARGUMENT_PATTERN = re.compile(r"^\s+-p\s+(?P<package_name>[A-Za-z0-9_-]+)\s*$")


def test_multiplatform_ci_rust_shards_cover_workspace_crates(
    project_root: Path,
) -> None:
    workspace_package_names = load_workspace_package_names(project_root)
    ci_package_names = _ci_rust_shard_package_names(project_root)

    assert sorted(ci_package_names) == sorted(workspace_package_names)


def _ci_rust_shard_package_names(project_root: Path) -> list[str]:
    workflow_path = project_root / ".github" / "workflows" / "ci-tests-multiplatform.yml"
    package_names: list[str] = []
    for line in workflow_path.read_text(encoding="utf-8").splitlines():
        package_match = CI_RUST_PACKAGE_ARGUMENT_PATTERN.match(line)
        if package_match is not None:
            package_names.append(package_match.group("package_name"))

    return package_names
