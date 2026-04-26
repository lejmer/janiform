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
fn rust_internal_crate_graph_policy_fixture_declares_required_crates_and_facade_roots() {
    let policy = load_policy();
    let metadata = load_cargo_metadata();

    assert_eq!(policy.schema_version, "v1");
    assert_eq!(
        policy.document_version,
        "rust.internal_crate_graph_and_umbrella_facade_policy.pre_v0_1.milestone_1"
    );
    assert!(!policy.policy_document.trim().is_empty());
    assert_eq!(policy.umbrella_facade_crate, "janiform");
    assert_eq!(policy.pybind_boundary_crate, "janiform-pybind");

    let workspace_package_names = workspace_package_names(&metadata);
    let policy_package_names: BTreeSet<String> = policy.crates.keys().cloned().collect();
    assert_eq!(
        policy_package_names, workspace_package_names,
        "policy fixture must define exactly the workspace crates from cargo metadata",
    );

    assert_eq!(policy.curated_facade_module_roots, facade_module_roots());
    assert_eq!(
        policy.transitional_facade_module_roots,
        vec!["benchmarking".to_string(), "verification".to_string()]
    );

    assert_eq!(policy.crates["janiform"].status, "existing");
    assert_eq!(policy.crates["janiform"].kind, "facade");
    assert_eq!(policy.crates["janiform"].layer, "umbrella_facade");
    assert_eq!(policy.crates["janiform-foundations"].status, "existing");
    assert_eq!(policy.crates["janiform-model-kernels"].status, "existing");
    assert_eq!(policy.crates["janiform-affine-oracle"].status, "existing");
    assert_eq!(policy.crates["janiform-engine-pricing"].status, "existing");
    assert_eq!(
        policy.crates["janiform-engine-sufficient-statistics"].status,
        "existing"
    );
    assert_eq!(
        policy.crates["janiform-engine-simulation"].status,
        "existing"
    );
    assert_eq!(
        policy.crates["janiform-joint-refinement-worker"].status,
        "existing"
    );
    assert_eq!(policy.crates["janiform-pybind"].status, "existing");
    assert_eq!(policy.crates["janiform-pybind"].kind, "boundary");
    assert_eq!(policy.crates["janiform-pybind"].layer, "python_boundary");
    assert_eq!(
        policy.crates["janiform-pybind"].allowed_internal_dependencies,
        vec!["janiform".to_string()]
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
    let workspace_package_names = workspace_package_names(&metadata);
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

#[test]
fn extracted_low_level_crates_keep_only_their_policy_approved_workspace_dependencies() {
    let policy = load_policy();
    let metadata = load_cargo_metadata();
    let workspace_packages_by_name: BTreeMap<String, &CargoPackage> = metadata
        .packages
        .iter()
        .map(|package| (package.name.clone(), package))
        .collect();

    for package_name in [
        "janiform-foundations",
        "janiform-model-kernels",
        "janiform-affine-oracle",
    ] {
        let package = workspace_packages_by_name[package_name];
        let actual_internal_dependencies: BTreeSet<String> = package
            .dependencies
            .iter()
            .map(|dependency| dependency.name.clone())
            .filter(|dependency| policy.crates.contains_key(dependency))
            .collect();
        let expected_internal_dependencies: BTreeSet<String> = policy.crates[package_name]
            .allowed_internal_dependencies
            .iter()
            .cloned()
            .collect();

        assert_eq!(
            actual_internal_dependencies, expected_internal_dependencies,
            "crate `{package_name}` should expose exactly the dependency-light internal boundary declared by policy",
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

fn workspace_package_names(metadata: &CargoMetadata) -> BTreeSet<String> {
    metadata
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect()
}

fn facade_module_roots() -> Vec<String> {
    let crate_root = fs::read_to_string(
        repository_root()
            .join("crates")
            .join("janiform")
            .join("src")
            .join("lib.rs"),
    )
    .expect("janiform facade crate root should exist");

    crate_root
        .lines()
        .filter_map(|line| {
            let trimmed_line = line.trim();
            trimmed_line
                .strip_prefix("pub mod ")
                .and_then(|module_root| module_root.strip_suffix(';'))
                .map(str::to_string)
        })
        .collect()
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("facade crate should live under crates/")
        .parent()
        .expect("workspace root should be above crates/")
        .to_path_buf()
}

fn policy_fixture_path() -> PathBuf {
    repository_root()
        .join("tests")
        .join("fixtures")
        .join("rust_architecture")
        .join("internal_crate_graph_policy.json")
}
