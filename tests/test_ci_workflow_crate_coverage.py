import re
from pathlib import Path

from tests.support.rust_workspace import load_workspace_package_names


CI_RUST_PACKAGE_ARGUMENT_PATTERN = re.compile(r"^\s+-p\s+(?P<package_name>[A-Za-z0-9_-]+)\s*$")


def test_ci_rust_shards_cover_workspace_crates(
    project_root: Path,
) -> None:
    workspace_package_names = load_workspace_package_names(project_root)
    package_names_by_workflow = _ci_rust_shard_package_names_by_workflow(project_root)

    for workflow_path, package_names in package_names_by_workflow.items():
        assert sorted(package_names) == sorted(workspace_package_names), workflow_path


def _ci_rust_shard_package_names_by_workflow(project_root: Path) -> dict[Path, list[str]]:
    workflow_paths = (
        project_root / ".github" / "workflows" / "ci-pr-test-rust.yml",
        project_root / ".github" / "workflows" / "ci-release-candidate.yml",
        project_root / ".github" / "workflows" / "multiplatform-verification.yml",
    )
    package_names_by_workflow: dict[Path, list[str]] = {}
    for workflow_path in workflow_paths:
        package_names: list[str] = []
        for line in workflow_path.read_text(encoding="utf-8").splitlines():
            package_match = CI_RUST_PACKAGE_ARGUMENT_PATTERN.match(line)
            if package_match is not None:
                package_names.append(package_match.group("package_name"))

        package_names_by_workflow[workflow_path] = package_names

    return package_names_by_workflow
