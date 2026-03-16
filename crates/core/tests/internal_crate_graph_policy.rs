use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct InternalCrateGraphPolicy {
    schema_version: String,
    document_version: String,
    policy_document: String,
    umbrella_facade_crate: String,
    pybind_boundary_crate: String,
    curated_facade_module_roots: Vec<String>,
    transitional_facade_module_roots: Vec<String>,
    crates: BTreeMap<String, InternalCratePolicyEntry>,
}

#[derive(Debug, Deserialize)]
struct InternalCratePolicyEntry {
    status: String,
    kind: String,
    layer: String,
    allowed_internal_dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    dependencies: Vec<CargoDependency>,
}

#[derive(Debug, Deserialize)]
struct CargoDependency {
    name: String,
}

#[test]
fn rust_internal_crate_graph_policy_document_contains_required_sections_and_rules() {
    let document = fs::read_to_string(policy_document_path())
        .expect("Rust internal crate graph policy document should exist");

    let required_sections = [
        "## Purpose",
        "## Problem Statement",
        "## Current Module Map To Target Ownership",
        "## Target Workspace Crate Graph",
        "## Umbrella Facade Policy",
        "## Dependency Direction Rules",
        "## Review And CI Enforcement",
    ];
    for required_section in required_sections {
        assert!(
            document.contains(required_section),
            "policy document must contain section `{required_section}`",
        );
    }

    let lowercase_document = document.to_lowercase();
    let required_snippets = [
        "janiform-foundations",
        "janiform-model-kernels",
        "janiform-affine-oracle",
        "janiform-market-time",
        "janiform-engine-pricing",
        "janiform-engine-risk",
        "janiform-engine-simulation",
        "janiform-calibration-artifacts",
        "janiform-calibration-runtime",
        "janiform-analytics-stylized-facts",
        "janiform-validation",
        "janiform-governance",
        "janiform-governance-bench",
        "janiform-core",
        "janiform-pybind",
        "engine crates must not depend on `janiform-calibration-runtime`",
        "engine crates must not depend on `janiform-validation`",
        "engine crates must not depend on `janiform-governance`",
        "governance may depend on engines and validation outputs, but not the reverse",
        "benchmark-only support must not grow production crate surfaces",
        "`janiform-pybind` should continue depending on `janiform-core`",
    ];
    for required_snippet in required_snippets {
        assert!(
            lowercase_document.contains(&required_snippet.to_lowercase()),
            "policy document must contain snippet `{required_snippet}`",
        );
    }
}

#[test]
fn rust_internal_crate_graph_policy_fixture_declares_required_crates_and_facade_roots() {
    let policy = load_policy();

    assert_eq!(policy.schema_version, "v1");
    assert_eq!(
        policy.document_version,
        "rust.internal_crate_graph_and_umbrella_facade_policy.pre_v0_1.milestone_1"
    );
    assert_eq!(
        policy.policy_document,
        "docs/development/guides/rust_internal_crate_graph_and_umbrella_facade_policy.md"
    );
    assert_eq!(policy.umbrella_facade_crate, "janiform-core");
    assert_eq!(policy.pybind_boundary_crate, "janiform-pybind");

    let required_crates = [
        "janiform-foundations",
        "janiform-model-kernels",
        "janiform-affine-oracle",
        "janiform-market-time",
        "janiform-engine-pricing",
        "janiform-engine-risk",
        "janiform-engine-simulation",
        "janiform-calibration-artifacts",
        "janiform-calibration-runtime",
        "janiform-analytics-stylized-facts",
        "janiform-validation",
        "janiform-governance",
        "janiform-governance-bench",
        "janiform-core",
        "janiform-pybind",
    ];
    for required_crate in required_crates {
        assert!(
            policy.crates.contains_key(required_crate),
            "policy fixture must define crate `{required_crate}`",
        );
    }

    let expected_facade_roots = vec![
        "affine_oracle",
        "benchmarking",
        "calendar",
        "calibration",
        "complex_math",
        "hawkes",
        "hourly_factors",
        "news",
        "nig",
        "pricing",
        "randomness",
        "return_decomposition",
        "risk",
        "risk_neutral",
        "seasonality",
        "simulation",
        "verification",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    assert_eq!(policy.curated_facade_module_roots, expected_facade_roots);
    assert_eq!(
        policy.transitional_facade_module_roots,
        vec!["benchmarking".to_string(), "verification".to_string()]
    );

    assert_eq!(policy.crates["janiform-core"].status, "existing");
    assert_eq!(policy.crates["janiform-core"].kind, "facade");
    assert_eq!(policy.crates["janiform-core"].layer, "umbrella_facade");
    assert_eq!(policy.crates["janiform-pybind"].status, "existing");
    assert_eq!(policy.crates["janiform-pybind"].kind, "boundary");
    assert_eq!(policy.crates["janiform-pybind"].layer, "python_boundary");
    assert_eq!(
        policy.crates["janiform-pybind"].allowed_internal_dependencies,
        vec!["janiform-core".to_string()]
    );
    assert_eq!(
        policy.crates["janiform-governance-bench"].kind,
        "benchmark_only"
    );
}

#[test]
fn cargo_metadata_workspace_dependencies_must_follow_internal_crate_graph_policy() {
    let policy = load_policy();
    let metadata = load_cargo_metadata();

    let workspace_packages_by_name: BTreeMap<String, &CargoPackage> = metadata
        .packages
        .iter()
        .map(|package| (package.name.clone(), package))
        .collect();
    let workspace_package_names: BTreeSet<String> =
        workspace_packages_by_name.keys().cloned().collect();
    let policy_package_names: BTreeSet<String> = policy.crates.keys().cloned().collect();

    assert!(
        workspace_package_names.is_subset(&policy_package_names),
        "workspace packages must remain within the approved policy graph.\npolicy: {:?}\nactual: {:?}",
        policy_package_names,
        workspace_package_names,
    );

    for (package_name, package) in workspace_packages_by_name {
        let allowed_dependencies: BTreeSet<String> = policy.crates[&package_name]
            .allowed_internal_dependencies
            .iter()
            .cloned()
            .collect();
        let actual_internal_dependencies: BTreeSet<String> = package
            .dependencies
            .iter()
            .filter(|dependency| workspace_package_names.contains(&dependency.name))
            .map(|dependency| dependency.name.clone())
            .collect();

        assert!(
            actual_internal_dependencies.is_subset(&allowed_dependencies),
            "crate `{package_name}` has workspace dependencies outside the approved policy.\nallowed: {:?}\nactual: {:?}",
            allowed_dependencies,
            actual_internal_dependencies,
        );
    }
}

fn load_policy() -> InternalCrateGraphPolicy {
    let policy_payload = fs::read_to_string(policy_fixture_path())
        .expect("internal crate graph policy fixture should exist");

    serde_json::from_str(&policy_payload)
        .expect("internal crate graph policy fixture should deserialize")
}

fn load_cargo_metadata() -> CargoMetadata {
    let command_output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(repository_root())
        .output()
        .expect("cargo metadata command should run");

    assert!(
        command_output.status.success(),
        "cargo metadata failed with status {:?}\nstderr:\n{}",
        command_output.status.code(),
        String::from_utf8_lossy(&command_output.stderr),
    );

    serde_json::from_slice(&command_output.stdout)
        .expect("cargo metadata output should deserialize")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate should live under crates/")
        .parent()
        .expect("workspace root should be above crates/")
        .to_path_buf()
}

fn policy_document_path() -> PathBuf {
    repository_root()
        .join("docs")
        .join("development")
        .join("guides")
        .join("rust_internal_crate_graph_and_umbrella_facade_policy.md")
}

fn policy_fixture_path() -> PathBuf {
    repository_root()
        .join("tests")
        .join("fixtures")
        .join("rust_architecture")
        .join("internal_crate_graph_policy.json")
}
