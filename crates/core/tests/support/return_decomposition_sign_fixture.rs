#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use janiform_core::nig::ComplexScalar;
use serde::Deserialize;

use crate::support::validation_fixture::fixtures_root_path;

pub const RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SCHEMA_VERSION: &str =
    "return.decomposition.sign_analytic_fixture.v1";
pub const RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SET_ID: &str =
    "return_decomposition_sign_analytic_fixture_v1";
pub const RETURN_DECOMPOSITION_SIGN_ANALYTIC_LOADING_ORDER_POLICY: &str =
    "preserve_declared_case_order.v1";
pub const RETURN_DECOMPOSITION_SIGN_ANALYTIC_CONTENT_HASH_SCOPE_POLICY: &str =
    "document_payload_bytes.v1";
pub const RETURN_DECOMPOSITION_SIGN_ANALYTIC_CONTENT_HASH_ALGORITHM_POLICY: &str =
    "fnv1a_64_hex_utf8.v1";

const RETURN_DECOMPOSITION_SIGN_ANALYTIC_REQUIRED_SCENARIO_INTENTS: [&str; 3] =
    ["buy_only", "sell_only", "combined"];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignAnalyticFixtureDocument {
    pub schema_version: String,
    pub fixture_set_id: String,
    pub fixture_set_version: String,
    pub determinism_metadata: ReturnDecompositionSignAnalyticDeterminismMetadata,
    pub analytic_metadata: ReturnDecompositionSignAnalyticMetadata,
    pub driver_configuration: ReturnDecompositionSignDriverConfiguration,
    pub cases: Vec<ReturnDecompositionSignAnalyticFixtureCase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignAnalyticDeterminismMetadata {
    pub fixture_loading_order: String,
    pub content_hash_scope: String,
    pub content_hash_algorithm: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignAnalyticMetadata {
    pub required_scenario_intents: Vec<String>,
    pub stable_fixture_ids_by_scenario_intent: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignDriverConfiguration {
    pub alpha: f64,
    pub beta_0: f64,
    pub delta: f64,
    pub mu: f64,
    pub delta_beta: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignAnalyticFixtureCase {
    pub fixture_id: String,
    pub scenario_intent: String,
    pub argument: ComplexScalarFixture,
    pub return_decomposition: ReturnDecompositionFixture,
    pub gap_characteristic_factor: ComplexScalarFixture,
    pub expected_output: ReturnDecompositionSignExpectedOutputFixture,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionFixture {
    pub a_plus: f64,
    pub a_minus: f64,
    pub gap: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionSignExpectedOutputFixture {
    pub buy_side_weighted_exponent: ComplexScalarFixture,
    pub sell_side_weighted_exponent: ComplexScalarFixture,
    pub combined_weighted_exponent: ComplexScalarFixture,
    pub trading_time_characteristic_factor: ComplexScalarFixture,
    pub combined_characteristic_factor: ComplexScalarFixture,
    pub wrong_positive_u_sell_side_weighted_exponent: ComplexScalarFixture,
    pub minimum_sell_sign_regression_distance: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComplexScalarFixture {
    pub real: f64,
    pub imaginary: f64,
}

impl ComplexScalarFixture {
    pub fn into_core(self) -> ComplexScalar {
        ComplexScalar::new(self.real, self.imaginary)
    }
}

pub fn load_return_decomposition_sign_analytic_fixture_document()
-> Result<ReturnDecompositionSignAnalyticFixtureDocument, String> {
    let fixture_path = fixtures_root_path()
        .join("return_decomposition")
        .join("return_decomposition_sign_analytic_fixture.json");
    let fixture_raw = fs::read_to_string(&fixture_path).map_err(|source| {
        format!(
            "failed to read return decomposition sign analytic fixture `{}`: {source}",
            fixture_path.display()
        )
    })?;
    let fixture_document: ReturnDecompositionSignAnalyticFixtureDocument =
        serde_json::from_str(&fixture_raw).map_err(|source| {
            format!(
                "failed to parse return decomposition sign analytic fixture `{}`: {source}",
                fixture_path.display()
            )
        })?;

    validate_return_decomposition_sign_analytic_fixture_document(&fixture_document)?;

    Ok(fixture_document)
}

pub fn validate_return_decomposition_sign_analytic_fixture_document(
    fixture_document: &ReturnDecompositionSignAnalyticFixtureDocument,
) -> Result<(), String> {
    if fixture_document.schema_version != RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SCHEMA_VERSION
    {
        return Err(format!(
            "unexpected return decomposition sign analytic schema_version: {}",
            fixture_document.schema_version
        ));
    }
    if fixture_document.fixture_set_id != RETURN_DECOMPOSITION_SIGN_ANALYTIC_FIXTURE_SET_ID {
        return Err(format!(
            "unexpected return decomposition sign analytic fixture_set_id: {}",
            fixture_document.fixture_set_id
        ));
    }
    require_non_empty(&fixture_document.fixture_set_version, "fixture_set_version")?;

    if fixture_document.determinism_metadata.fixture_loading_order
        != RETURN_DECOMPOSITION_SIGN_ANALYTIC_LOADING_ORDER_POLICY
    {
        return Err(format!(
            "unexpected return decomposition sign analytic fixture_loading_order: {}",
            fixture_document.determinism_metadata.fixture_loading_order
        ));
    }
    if fixture_document.determinism_metadata.content_hash_scope
        != RETURN_DECOMPOSITION_SIGN_ANALYTIC_CONTENT_HASH_SCOPE_POLICY
    {
        return Err(format!(
            "unexpected return decomposition sign analytic content_hash_scope: {}",
            fixture_document.determinism_metadata.content_hash_scope
        ));
    }
    if fixture_document.determinism_metadata.content_hash_algorithm
        != RETURN_DECOMPOSITION_SIGN_ANALYTIC_CONTENT_HASH_ALGORITHM_POLICY
    {
        return Err(format!(
            "unexpected return decomposition sign analytic content_hash_algorithm: {}",
            fixture_document.determinism_metadata.content_hash_algorithm
        ));
    }

    if fixture_document
        .analytic_metadata
        .required_scenario_intents
        .is_empty()
    {
        return Err(
            "analytic_metadata.required_scenario_intents must include at least one intent"
                .to_string(),
        );
    }

    let required_scenario_intents: BTreeSet<String> = fixture_document
        .analytic_metadata
        .required_scenario_intents
        .iter()
        .map(|scenario_intent| {
            require_non_empty(
                scenario_intent,
                "analytic_metadata.required_scenario_intents",
            )?;
            Ok(scenario_intent.clone())
        })
        .collect::<Result<BTreeSet<String>, String>>()?;
    if required_scenario_intents.len()
        != fixture_document
            .analytic_metadata
            .required_scenario_intents
            .len()
    {
        return Err(
            "analytic_metadata.required_scenario_intents must not include duplicates".to_string(),
        );
    }

    for required_scenario_intent in RETURN_DECOMPOSITION_SIGN_ANALYTIC_REQUIRED_SCENARIO_INTENTS {
        if !required_scenario_intents.contains(required_scenario_intent) {
            return Err(format!(
                "analytic_metadata.required_scenario_intents must include `{required_scenario_intent}`"
            ));
        }
    }

    if fixture_document
        .analytic_metadata
        .stable_fixture_ids_by_scenario_intent
        .is_empty()
    {
        return Err(
            "analytic_metadata.stable_fixture_ids_by_scenario_intent must not be empty".to_string(),
        );
    }

    if fixture_document.cases.is_empty() {
        return Err("return decomposition sign analytic fixture must include cases".to_string());
    }

    let mut observed_fixture_ids: BTreeSet<String> = BTreeSet::new();
    let mut observed_fixture_ids_by_scenario_intent: BTreeMap<String, BTreeSet<String>> =
        BTreeMap::new();

    for fixture_case in &fixture_document.cases {
        require_non_empty(&fixture_case.fixture_id, "cases.fixture_id")?;
        require_non_empty(&fixture_case.scenario_intent, "cases.scenario_intent")?;
        require_non_empty(&fixture_case.description, "cases.description")?;

        if !required_scenario_intents.contains(&fixture_case.scenario_intent) {
            return Err(format!(
                "fixture case {} uses unsupported scenario_intent `{}`",
                fixture_case.fixture_id, fixture_case.scenario_intent
            ));
        }

        if !observed_fixture_ids.insert(fixture_case.fixture_id.clone()) {
            return Err(format!(
                "duplicate fixture_id in return decomposition sign analytic fixture: {}",
                fixture_case.fixture_id
            ));
        }

        validate_complex_scalar_fixture(fixture_case.argument, "cases.argument")?;
        validate_non_negative_finite(fixture_case.return_decomposition.a_plus, "cases.a_plus")?;
        validate_non_negative_finite(fixture_case.return_decomposition.a_minus, "cases.a_minus")?;
        validate_finite(fixture_case.return_decomposition.gap, "cases.gap")?;
        validate_complex_scalar_fixture(
            fixture_case.gap_characteristic_factor,
            "cases.gap_characteristic_factor",
        )?;

        validate_complex_scalar_fixture(
            fixture_case.expected_output.buy_side_weighted_exponent,
            "cases.expected_output.buy_side_weighted_exponent",
        )?;
        validate_complex_scalar_fixture(
            fixture_case.expected_output.sell_side_weighted_exponent,
            "cases.expected_output.sell_side_weighted_exponent",
        )?;
        validate_complex_scalar_fixture(
            fixture_case.expected_output.combined_weighted_exponent,
            "cases.expected_output.combined_weighted_exponent",
        )?;
        validate_complex_scalar_fixture(
            fixture_case
                .expected_output
                .trading_time_characteristic_factor,
            "cases.expected_output.trading_time_characteristic_factor",
        )?;
        validate_complex_scalar_fixture(
            fixture_case.expected_output.combined_characteristic_factor,
            "cases.expected_output.combined_characteristic_factor",
        )?;
        validate_complex_scalar_fixture(
            fixture_case
                .expected_output
                .wrong_positive_u_sell_side_weighted_exponent,
            "cases.expected_output.wrong_positive_u_sell_side_weighted_exponent",
        )?;
        validate_non_negative_finite(
            fixture_case
                .expected_output
                .minimum_sell_sign_regression_distance,
            "cases.expected_output.minimum_sell_sign_regression_distance",
        )?;

        observed_fixture_ids_by_scenario_intent
            .entry(fixture_case.scenario_intent.clone())
            .or_default()
            .insert(fixture_case.fixture_id.clone());
    }

    for (scenario_intent, stable_fixture_ids) in &fixture_document
        .analytic_metadata
        .stable_fixture_ids_by_scenario_intent
    {
        require_non_empty(
            scenario_intent,
            "analytic_metadata.stable_fixture_ids_by_scenario_intent.key",
        )?;
        if stable_fixture_ids.is_empty() {
            return Err(format!(
                "analytic_metadata.stable_fixture_ids_by_scenario_intent[{scenario_intent}] must not be empty"
            ));
        }
        let stable_fixture_id_set: BTreeSet<String> = stable_fixture_ids
            .iter()
            .map(|fixture_id| {
                require_non_empty(
                    fixture_id,
                    "analytic_metadata.stable_fixture_ids_by_scenario_intent.value",
                )?;
                Ok(fixture_id.clone())
            })
            .collect::<Result<BTreeSet<String>, String>>()?;

        if stable_fixture_id_set.len() != stable_fixture_ids.len() {
            return Err(format!(
                "analytic_metadata.stable_fixture_ids_by_scenario_intent[{scenario_intent}] must not include duplicates"
            ));
        }

        let observed_fixture_id_set = observed_fixture_ids_by_scenario_intent
            .get(scenario_intent)
            .cloned()
            .unwrap_or_default();
        if stable_fixture_id_set != observed_fixture_id_set {
            return Err(format!(
                "analytic_metadata.stable_fixture_ids_by_scenario_intent[{scenario_intent}] does not match observed fixture ids"
            ));
        }
    }

    for required_scenario_intent in required_scenario_intents {
        if !fixture_document
            .analytic_metadata
            .stable_fixture_ids_by_scenario_intent
            .contains_key(&required_scenario_intent)
        {
            return Err(format!(
                "analytic_metadata.stable_fixture_ids_by_scenario_intent is missing required scenario intent `{required_scenario_intent}`"
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

fn validate_non_negative_finite(value: f64, field_name: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 {
        return Err(format!("{field_name} must be finite and non-negative"));
    }
    Ok(())
}

fn validate_finite(value: f64, field_name: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{field_name} must be finite"));
    }
    Ok(())
}

fn validate_complex_scalar_fixture(
    value: ComplexScalarFixture,
    field_name: &str,
) -> Result<(), String> {
    if !value.real.is_finite() || !value.imaginary.is_finite() {
        return Err(format!(
            "{field_name} must contain finite real and imaginary values"
        ));
    }
    Ok(())
}
