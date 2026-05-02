import argparse
import json
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

type JsonValue = Optional[bool | int | float | str | list[JsonValue] | dict[str, JsonValue]]

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
POLICY_PATH = (
    REPOSITORY_ROOT
    / "tests"
    / "fixtures"
    / "rust_architecture"
    / "internal_crate_graph_policy.json"
)

SCHEMA_VERSION = "v1"
DOCUMENT_VERSION = "rust.internal_crate_graph_and_umbrella_facade_policy.pre_v0_1.milestone_1"
POLICY_DOCUMENT = "docs/contributing/crates.md"
UMBRELLA_FACADE_CRATE = "janiform"
PYBIND_BOUNDARY_CRATE = "janiform-pybind"
FACADE_ROOT_PATH = REPOSITORY_ROOT / "crates" / UMBRELLA_FACADE_CRATE / "src" / "lib.rs"

TRANSITIONAL_FACADE_MODULE_ROOTS = [
    "benchmarking",
    "verification",
]

CRATE_ORDER = [
    "janiform-foundations",
    "janiform-model-kernels",
    "janiform-affine-oracle",
    "janiform-market-time",
    "janiform-engine-pricing",
    "janiform-engine-sufficient-statistics",
    "janiform-engine-simulation",
    "janiform-calibration-artifacts",
    "janiform-calibration-runtime",
    "janiform-joint-refinement-worker",
    "janiform-validation",
    "janiform-verification",
    "janiform-governance",
    "janiform-governance-bench",
    "janiform-test-support",
    "janiform-event-log",
    "janiform-terminal-ui",
    "janiform-workflows",
    "janiform-cli",
    "janiform",
    "janiform-pybind",
]

CRATE_CLASSIFICATION = {
    "janiform-foundations": ("production", "foundations"),
    "janiform-model-kernels": ("production", "model_kernels"),
    "janiform-affine-oracle": ("production", "shared_engine_infrastructure"),
    "janiform-market-time": ("production", "shared_time_infrastructure"),
    "janiform-engine-pricing": ("production", "engine"),
    "janiform-engine-sufficient-statistics": ("production", "engine"),
    "janiform-engine-simulation": ("production", "engine"),
    "janiform-calibration-artifacts": ("production", "calibration_artifacts"),
    "janiform-calibration-runtime": ("production", "calibration_runtime"),
    "janiform-joint-refinement-worker": ("production", "calibration_worker"),
    "janiform-validation": ("production", "model_validation"),
    "janiform-verification": ("production", "verification"),
    "janiform-governance": ("production", "governance"),
    "janiform-governance-bench": ("benchmark_only", "governance_bench"),
    "janiform-test-support": ("test_only", "test_support"),
    "janiform-event-log": ("production", "event_log"),
    "janiform-terminal-ui": ("production", "terminal_ui"),
    "janiform-workflows": ("production", "workflow_orchestration"),
    "janiform-cli": ("boundary", "cli_boundary"),
    "janiform": ("facade", "umbrella_facade"),
    "janiform-pybind": ("boundary", "python_boundary"),
}


@dataclass(frozen=True)
class WorkspacePackage:
    name: str
    internal_dependencies: tuple[str, ...]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate or validate the internal Rust crate graph policy fixture.",
    )

    parser.add_argument(
        "--check",
        action="store_true",
        help="Exit with a non-zero status when the fixture is out of date.",
    )

    args = parser.parse_args()

    packages = load_workspace_packages()
    facade_module_roots = load_facade_module_roots()
    policy = build_policy(packages, facade_module_roots)
    expected_payload = json.dumps(policy, indent=2) + "\n"

    if args.check:
        actual_payload = POLICY_PATH.read_text(encoding="utf-8") if POLICY_PATH.exists() else ""
        if actual_payload != expected_payload:
            print("Internal crate graph policy fixture is out of date.")
            print("Regenerate it with `python3 scripts/generate_internal_crate_graph_policy.py`.")

            return 1

        return 0

    POLICY_PATH.parent.mkdir(parents=True, exist_ok=True)
    POLICY_PATH.write_text(expected_payload, encoding="utf-8")

    print(f"Generated {POLICY_PATH.relative_to(REPOSITORY_ROOT)}.")

    return 0


def load_workspace_packages() -> tuple[WorkspacePackage, ...]:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        capture_output=True,
        check=True,
        cwd=REPOSITORY_ROOT,
        text=True,
    )

    metadata = json.loads(result.stdout)

    workspace_member_ids = set(metadata["workspace_members"])
    workspace_packages = [
        package for package in metadata["packages"] if package["id"] in workspace_member_ids
    ]
    workspace_names = {package["name"] for package in workspace_packages}
    configured_names = set(CRATE_ORDER)

    missing_classification = sorted(workspace_names - configured_names)
    if missing_classification:
        joined_names = ", ".join(missing_classification)
        raise ValueError(f"Workspace crates missing policy classification: {joined_names}")

    stale_classification = sorted(configured_names - workspace_names)
    if stale_classification:
        joined_names = ", ".join(stale_classification)
        raise ValueError(f"Policy classification references non-workspace crates: {joined_names}")

    packages: list[WorkspacePackage] = []
    for package in workspace_packages:
        dependencies = sorted(
            {
                dependency["name"]
                for dependency in package["dependencies"]
                if dependency["name"] in workspace_names
            },
            key=crate_sort_key,
        )

        packages.append(
            WorkspacePackage(
                name=package["name"],
                internal_dependencies=tuple(dependencies),
            )
        )

    packages.sort(key=lambda package: crate_sort_key(package.name))
    return tuple(packages)


def load_facade_module_roots() -> tuple[str, ...]:
    module_roots: list[str] = []

    for line in FACADE_ROOT_PATH.read_text(encoding="utf-8").splitlines():
        stripped_line = line.strip()
        if not stripped_line.startswith("pub mod ") or not stripped_line.endswith(";"):
            continue

        module_roots.append(stripped_line.removeprefix("pub mod ").removesuffix(";"))

    if not module_roots:
        raise ValueError(f"No public facade module roots found in {FACADE_ROOT_PATH}")

    return tuple(module_roots)


def build_policy(
    packages: tuple[WorkspacePackage, ...],
    facade_module_roots: tuple[str, ...],
) -> dict[str, JsonValue]:
    return {
        "schema_version": SCHEMA_VERSION,
        "document_version": DOCUMENT_VERSION,
        "policy_document": POLICY_DOCUMENT,
        "umbrella_facade_crate": UMBRELLA_FACADE_CRATE,
        "pybind_boundary_crate": PYBIND_BOUNDARY_CRATE,
        "curated_facade_module_roots": list(facade_module_roots),
        "transitional_facade_module_roots": TRANSITIONAL_FACADE_MODULE_ROOTS,
        "crates": {
            package.name: build_crate_entry(package)
            for package in sorted(packages, key=lambda package: crate_sort_key(package.name))
        },
    }


def build_crate_entry(package: WorkspacePackage) -> dict[str, JsonValue]:
    kind, layer = CRATE_CLASSIFICATION[package.name]
    return {
        "status": "existing",
        "kind": kind,
        "layer": layer,
        "allowed_internal_dependencies": list(package.internal_dependencies),
    }


def crate_sort_key(crate_name: str) -> tuple[int, str]:
    try:
        return CRATE_ORDER.index(crate_name), crate_name
    except ValueError:
        return len(CRATE_ORDER), crate_name


if __name__ == "__main__":
    raise SystemExit(main())
