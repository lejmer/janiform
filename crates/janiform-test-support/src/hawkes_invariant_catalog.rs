#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use serde::Deserialize;
use serde::Serialize;

use crate::validation_fixture::fixtures_root_path;

pub const HAWKES_INVARIANT_CATALOG_SCHEMA_VERSION: &str = "hawkes.invariant.catalog.v1";
pub const HAWKES_INVARIANT_CATALOG_ID: &str = "affine_hawkes_invariant_catalog_v1";
pub const HAWKES_INVARIANT_CHECK_RULE_DETERMINISTIC_ID: &str = "deterministic_equation_rule_v1";
pub const HAWKES_INVARIANT_CHECK_RULE_STATISTICAL_ID: &str = "statistical_property_rule_v1";

pub const HAWKES_INVARIANT_CATEGORY_CONSTRAINTS: &str = "constraints";
pub const HAWKES_INVARIANT_CATEGORY_DYNAMICS: &str = "dynamics";
pub const HAWKES_INVARIANT_CATEGORY_BOUNDARY_BEHAVIOR: &str = "boundary_behavior";
pub const HAWKES_INVARIANT_CATEGORY_SAMPLING_PROPERTIES: &str = "sampling_properties";
pub const HAWKES_INVARIANT_CATEGORY_SCALE_CONSISTENCY: &str = "scale_consistency";

pub const HAWKES_REQUIRED_INVARIANT_CLASS_NON_GRID_EVENT_TIME_SEMANTICS: &str =
    "non_grid_event_time_semantics";
pub const HAWKES_REQUIRED_INVARIANT_CLASS_COARSE_AGGREGATED_SCALE_CONSISTENCY: &str =
    "coarse_vs_aggregated_scale_consistency";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HawkesInvariantCheckClass {
    DeterministicEquation,
    StatisticalProperty,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesInvariantClassificationRule {
    pub check_class: HawkesInvariantCheckClass,
    pub rule_id: String,
    pub summary: String,
    pub evaluation_guidance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesInvariantCategoryDefinition {
    pub category_id: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesInvariantSuiteReference {
    pub suite_id: String,
    pub test_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesInvariantDefinition {
    pub invariant_id: String,
    pub title: String,
    pub category_id: String,
    pub invariant_class: String,
    pub check_class: HawkesInvariantCheckClass,
    pub rationale: String,
    pub suite_references: Vec<HawkesInvariantSuiteReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HawkesInvariantCatalogDocument {
    pub schema_version: String,
    pub catalog_id: String,
    pub catalog_version: String,
    pub classification_rules: Vec<HawkesInvariantClassificationRule>,
    pub categories: Vec<HawkesInvariantCategoryDefinition>,
    pub invariants: Vec<HawkesInvariantDefinition>,
}

pub fn load_hawkes_invariant_catalog_document() -> Result<HawkesInvariantCatalogDocument, String> {
    let catalog_path = fixtures_root_path()
        .join("hawkes")
        .join("hawkes_invariant_catalog.json");
    let catalog_payload = fs::read_to_string(&catalog_path).map_err(|source| {
        format!(
            "failed to read hawkes invariant catalog fixture `{}`: {source}",
            catalog_path.display()
        )
    })?;
    let catalog_document = serde_json::from_str::<HawkesInvariantCatalogDocument>(&catalog_payload)
        .map_err(|source| {
            format!(
                "failed to parse hawkes invariant catalog fixture `{}`: {source}",
                catalog_path.display()
            )
        })?;

    validate_hawkes_invariant_catalog_document(&catalog_document)?;

    Ok(catalog_document)
}

pub fn validate_hawkes_invariant_catalog_document(
    catalog_document: &HawkesInvariantCatalogDocument,
) -> Result<(), String> {
    if catalog_document.schema_version != HAWKES_INVARIANT_CATALOG_SCHEMA_VERSION {
        return Err(format!(
            "unsupported hawkes invariant catalog schema version: {}",
            catalog_document.schema_version
        ));
    }
    if catalog_document.catalog_id != HAWKES_INVARIANT_CATALOG_ID {
        return Err(format!(
            "unexpected hawkes invariant catalog id: {}",
            catalog_document.catalog_id
        ));
    }

    require_non_empty(
        &catalog_document.catalog_version,
        "hawkes_invariant_catalog.catalog_version",
    )?;

    validate_hawkes_classification_rules(&catalog_document.classification_rules)?;
    validate_hawkes_categories(&catalog_document.categories)?;
    validate_hawkes_invariants(&catalog_document.invariants, &catalog_document.categories)?;

    Ok(())
}

pub fn assert_hawkes_invariant_catalog_ids_have_check_class(
    invariant_ids: &[&str],
    expected_check_class: HawkesInvariantCheckClass,
) {
    assert!(
        !invariant_ids.is_empty(),
        "invariant_ids must not be empty when asserting hawkes catalog references"
    );

    let catalog_document = load_hawkes_invariant_catalog_document()
        .expect("hawkes invariant catalog fixture should load");

    for invariant_id in invariant_ids {
        let matching_invariant = catalog_document
            .invariants
            .iter()
            .find(|invariant| &invariant.invariant_id == invariant_id)
            .unwrap_or_else(|| panic!("unknown hawkes invariant catalog id: {invariant_id}"));

        assert_eq!(
            matching_invariant.check_class, expected_check_class,
            "unexpected check class for invariant_id={invariant_id}"
        );
    }
}

pub fn collect_invariant_ids_for_suite_test(
    catalog_document: &HawkesInvariantCatalogDocument,
    suite_id: &str,
    test_name: &str,
) -> Vec<String> {
    let mut matched_invariant_ids: Vec<String> = catalog_document
        .invariants
        .iter()
        .filter(|invariant| {
            invariant.suite_references.iter().any(|suite_reference| {
                suite_reference.suite_id == suite_id && suite_reference.test_name == test_name
            })
        })
        .map(|invariant| invariant.invariant_id.clone())
        .collect();
    matched_invariant_ids.sort();
    matched_invariant_ids
}

fn validate_hawkes_classification_rules(
    classification_rules: &[HawkesInvariantClassificationRule],
) -> Result<(), String> {
    if classification_rules.is_empty() {
        return Err("hawkes invariant catalog classification_rules must not be empty".to_string());
    }

    let mut observed_check_classes: BTreeSet<HawkesInvariantCheckClass> = BTreeSet::new();
    let mut observed_rule_ids: BTreeSet<String> = BTreeSet::new();
    let mut check_class_rule_lookup: BTreeMap<HawkesInvariantCheckClass, String> = BTreeMap::new();

    for classification_rule in classification_rules {
        require_non_empty(&classification_rule.rule_id, "classification_rules.rule_id")?;
        require_non_empty(&classification_rule.summary, "classification_rules.summary")?;
        require_non_empty(
            &classification_rule.evaluation_guidance,
            "classification_rules.evaluation_guidance",
        )?;

        if !observed_check_classes.insert(classification_rule.check_class) {
            return Err(format!(
                "duplicate classification rule for check_class {:?}",
                classification_rule.check_class
            ));
        }
        if !observed_rule_ids.insert(classification_rule.rule_id.clone()) {
            return Err(format!(
                "duplicate classification rule_id: {}",
                classification_rule.rule_id
            ));
        }

        check_class_rule_lookup.insert(
            classification_rule.check_class,
            classification_rule.rule_id.clone(),
        );
    }

    if check_class_rule_lookup.get(&HawkesInvariantCheckClass::DeterministicEquation)
        != Some(&HAWKES_INVARIANT_CHECK_RULE_DETERMINISTIC_ID.to_string())
    {
        return Err(
            "missing deterministic_equation classification rule or unexpected rule_id".to_string(),
        );
    }

    if check_class_rule_lookup.get(&HawkesInvariantCheckClass::StatisticalProperty)
        != Some(&HAWKES_INVARIANT_CHECK_RULE_STATISTICAL_ID.to_string())
    {
        return Err(
            "missing statistical_property classification rule or unexpected rule_id".to_string(),
        );
    }

    Ok(())
}

fn validate_hawkes_categories(
    categories: &[HawkesInvariantCategoryDefinition],
) -> Result<(), String> {
    if categories.is_empty() {
        return Err("hawkes invariant catalog categories must not be empty".to_string());
    }

    let mut observed_category_ids: BTreeSet<String> = BTreeSet::new();
    for category in categories {
        require_non_empty(&category.category_id, "categories.category_id")?;
        require_non_empty(&category.description, "categories.description")?;

        if !observed_category_ids.insert(category.category_id.clone()) {
            return Err(format!(
                "duplicate hawkes invariant category_id: {}",
                category.category_id
            ));
        }
    }

    for required_category_id in [
        HAWKES_INVARIANT_CATEGORY_CONSTRAINTS,
        HAWKES_INVARIANT_CATEGORY_DYNAMICS,
        HAWKES_INVARIANT_CATEGORY_BOUNDARY_BEHAVIOR,
        HAWKES_INVARIANT_CATEGORY_SAMPLING_PROPERTIES,
        HAWKES_INVARIANT_CATEGORY_SCALE_CONSISTENCY,
    ] {
        if !observed_category_ids.contains(required_category_id) {
            return Err(format!(
                "missing required hawkes invariant category_id: {required_category_id}"
            ));
        }
    }

    Ok(())
}

fn validate_hawkes_invariants(
    invariants: &[HawkesInvariantDefinition],
    categories: &[HawkesInvariantCategoryDefinition],
) -> Result<(), String> {
    if invariants.is_empty() {
        return Err("hawkes invariant catalog invariants must not be empty".to_string());
    }

    let known_category_ids: BTreeSet<String> = categories
        .iter()
        .map(|category| category.category_id.clone())
        .collect();

    let mut observed_invariant_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_required_classes: BTreeSet<String> = BTreeSet::new();

    for invariant in invariants {
        require_non_empty(&invariant.invariant_id, "invariants.invariant_id")?;
        require_non_empty(&invariant.title, "invariants.title")?;
        require_non_empty(&invariant.category_id, "invariants.category_id")?;
        require_non_empty(&invariant.invariant_class, "invariants.invariant_class")?;
        require_non_empty(&invariant.rationale, "invariants.rationale")?;

        if !observed_invariant_ids.insert(invariant.invariant_id.clone()) {
            return Err(format!(
                "duplicate hawkes invariant id: {}",
                invariant.invariant_id
            ));
        }
        if !known_category_ids.contains(&invariant.category_id) {
            return Err(format!(
                "invariant {} references unknown category_id: {}",
                invariant.invariant_id, invariant.category_id
            ));
        }

        observed_required_classes.insert(invariant.invariant_class.clone());

        let mut observed_suite_test_pairs_for_invariant: BTreeSet<(String, String)> =
            BTreeSet::new();
        for suite_reference in &invariant.suite_references {
            require_non_empty(
                &suite_reference.suite_id,
                "invariants.suite_references.suite_id",
            )?;
            require_non_empty(
                &suite_reference.test_name,
                "invariants.suite_references.test_name",
            )?;
            let suite_test_pair = (
                suite_reference.suite_id.clone(),
                suite_reference.test_name.clone(),
            );
            if !observed_suite_test_pairs_for_invariant.insert(suite_test_pair.clone()) {
                return Err(format!(
                    "duplicate suite reference in invariant {} for suite_id={} test_name={}",
                    invariant.invariant_id, suite_test_pair.0, suite_test_pair.1
                ));
            }
        }
    }

    for required_invariant_class in [
        HAWKES_REQUIRED_INVARIANT_CLASS_NON_GRID_EVENT_TIME_SEMANTICS,
        HAWKES_REQUIRED_INVARIANT_CLASS_COARSE_AGGREGATED_SCALE_CONSISTENCY,
    ] {
        if !observed_required_classes.contains(required_invariant_class) {
            return Err(format!(
                "missing required hawkes invariant class: {required_invariant_class}"
            ));
        }
    }

    Ok(())
}

fn require_non_empty(value: &str, field_name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field_name} must not be empty"));
    }
    Ok(())
}
