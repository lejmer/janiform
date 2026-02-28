#![allow(dead_code)]

use std::collections::BTreeMap;

use janiform_core::nig::ComplexScalar;
use janiform_core::return_decomposition::ReturnDecompositionEdgeMode;
use serde::Deserialize;
use serde::Serialize;

pub const RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION: &str =
    "return_decomposition.failure_diagnostic_report.v1";
pub const RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_DETERMINISM_POLICY: &str =
    "stable_category_classification_and_sorted_violation_output.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnDecompositionFailureDiagnosticCategory {
    Mapping,
    Sign,
    ComponentDisableMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionDiagnosticComplexScalar {
    pub real: f64,
    pub imaginary: f64,
}

impl ReturnDecompositionDiagnosticComplexScalar {
    pub fn into_core(self) -> ComplexScalar {
        ComplexScalar::new(self.real, self.imaginary)
    }
}

impl From<ComplexScalar> for ReturnDecompositionDiagnosticComplexScalar {
    fn from(value: ComplexScalar) -> Self {
        Self {
            real: value.real,
            imaginary: value.imaginary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionDiagnosticContributions {
    pub buy_side_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar,
    pub sell_side_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar,
    pub combined_weighted_exponent: ReturnDecompositionDiagnosticComplexScalar,
    pub trading_time_characteristic_factor: ReturnDecompositionDiagnosticComplexScalar,
    pub combined_characteristic_factor: ReturnDecompositionDiagnosticComplexScalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnDecompositionScenarioDiagnosticContext {
    pub scenario_id: String,
    pub scenario_intent: String,
    pub seed: u64,
    pub edge_mode: ReturnDecompositionEdgeMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReturnDecompositionFailureClassificationHints {
    pub wrong_positive_u_sell_side_weighted_exponent: ComplexScalar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionFailureDiagnosticSummary {
    pub scenario_count: usize,
    pub failure_count: usize,
    pub category_counts: Vec<ReturnDecompositionFailureDiagnosticCategoryCount>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionFailureDiagnosticCategoryCount {
    pub category: ReturnDecompositionFailureDiagnosticCategory,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionFailureDiagnosticViolation {
    pub category: ReturnDecompositionFailureDiagnosticCategory,
    pub check_id: String,
    pub scenario_id: String,
    pub scenario_intent: String,
    pub seed: u64,
    pub edge_mode: ReturnDecompositionEdgeMode,
    pub detail: String,
    pub absolute_distance: f64,
    pub tolerance: f64,
    pub observed: ReturnDecompositionDiagnosticContributions,
    pub expected: ReturnDecompositionDiagnosticContributions,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnDecompositionFailureDiagnosticReport {
    pub schema_version: String,
    pub determinism_policy: String,
    pub suite_id: String,
    pub summary: ReturnDecompositionFailureDiagnosticSummary,
    pub violations: Vec<ReturnDecompositionFailureDiagnosticViolation>,
}

pub fn build_return_decomposition_failure_diagnostic_violation(
    context: &ReturnDecompositionScenarioDiagnosticContext,
    check_id: &str,
    expected: ReturnDecompositionDiagnosticContributions,
    observed: ReturnDecompositionDiagnosticContributions,
    classification_hints: ReturnDecompositionFailureClassificationHints,
    tolerance: f64,
) -> ReturnDecompositionFailureDiagnosticViolation {
    let category = classify_return_decomposition_failure_category(
        context,
        &expected,
        &observed,
        classification_hints,
        tolerance,
    );
    let absolute_distance = distance_for_check_id(check_id, &expected, &observed);

    ReturnDecompositionFailureDiagnosticViolation {
        category,
        check_id: check_id.to_string(),
        scenario_id: context.scenario_id.clone(),
        scenario_intent: context.scenario_intent.clone(),
        seed: context.seed,
        edge_mode: context.edge_mode,
        detail: format!(
            "check `{check_id}` mismatch with absolute_distance={absolute_distance} tolerance={tolerance}"
        ),
        absolute_distance,
        tolerance,
        observed,
        expected,
    }
}

pub fn build_return_decomposition_failure_diagnostic_report(
    suite_id: &str,
    scenario_count: usize,
    mut violations: Vec<ReturnDecompositionFailureDiagnosticViolation>,
) -> ReturnDecompositionFailureDiagnosticReport {
    violations.sort_by(|left_violation, right_violation| {
        left_violation
            .category
            .cmp(&right_violation.category)
            .then_with(|| left_violation.scenario_id.cmp(&right_violation.scenario_id))
            .then_with(|| left_violation.check_id.cmp(&right_violation.check_id))
    });

    let mut category_count_lookup: BTreeMap<ReturnDecompositionFailureDiagnosticCategory, usize> =
        BTreeMap::new();
    for violation in &violations {
        let current_count = category_count_lookup.entry(violation.category).or_insert(0);
        *current_count = current_count.saturating_add(1);
    }

    let category_counts: Vec<ReturnDecompositionFailureDiagnosticCategoryCount> =
        category_count_lookup
            .into_iter()
            .map(
                |(category, count)| ReturnDecompositionFailureDiagnosticCategoryCount {
                    category,
                    count,
                },
            )
            .collect();

    ReturnDecompositionFailureDiagnosticReport {
        schema_version: RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_REPORT_SCHEMA_VERSION.to_string(),
        determinism_policy: RETURN_DECOMPOSITION_FAILURE_DIAGNOSTIC_DETERMINISM_POLICY.to_string(),
        suite_id: suite_id.to_string(),
        summary: ReturnDecompositionFailureDiagnosticSummary {
            scenario_count,
            failure_count: violations.len(),
            category_counts,
        },
        violations,
    }
}

pub fn render_return_decomposition_failure_diagnostic_report(
    report: &ReturnDecompositionFailureDiagnosticReport,
) -> String {
    serde_json::to_string_pretty(report)
        .expect("return decomposition failure diagnostic report serialization should succeed")
}

pub fn classify_return_decomposition_failure_category(
    context: &ReturnDecompositionScenarioDiagnosticContext,
    expected: &ReturnDecompositionDiagnosticContributions,
    observed: &ReturnDecompositionDiagnosticContributions,
    classification_hints: ReturnDecompositionFailureClassificationHints,
    tolerance: f64,
) -> ReturnDecompositionFailureDiagnosticCategory {
    if has_component_disable_mode_signature(context, observed, tolerance) {
        return ReturnDecompositionFailureDiagnosticCategory::ComponentDisableMode;
    }

    if context.edge_mode.sell_component_enabled() {
        let observed_sell_side = observed.sell_side_weighted_exponent.into_core();
        let expected_sell_side = expected.sell_side_weighted_exponent.into_core();
        let observed_buy_side = observed.buy_side_weighted_exponent.into_core();
        let expected_buy_side = expected.buy_side_weighted_exponent.into_core();
        let buy_side_distance = observed_buy_side.subtract(expected_buy_side).norm();
        let distance_to_expected = observed_sell_side.subtract(expected_sell_side).norm();
        let distance_to_wrong_positive_u = observed_sell_side
            .subtract(classification_hints.wrong_positive_u_sell_side_weighted_exponent)
            .norm();
        if buy_side_distance <= tolerance
            && distance_to_wrong_positive_u + tolerance < distance_to_expected
        {
            return ReturnDecompositionFailureDiagnosticCategory::Sign;
        }
    }

    ReturnDecompositionFailureDiagnosticCategory::Mapping
}

fn has_component_disable_mode_signature(
    context: &ReturnDecompositionScenarioDiagnosticContext,
    observed: &ReturnDecompositionDiagnosticContributions,
    tolerance: f64,
) -> bool {
    if !context.edge_mode.buy_component_enabled()
        && observed.buy_side_weighted_exponent.into_core().norm() > tolerance
    {
        return true;
    }

    if !context.edge_mode.sell_component_enabled()
        && observed.sell_side_weighted_exponent.into_core().norm() > tolerance
    {
        return true;
    }

    if !context.edge_mode.open_jump_component_enabled()
        && !context.edge_mode.closed_gap_component_enabled()
    {
        let trading_factor = observed.trading_time_characteristic_factor.into_core();
        let combined_factor = observed.combined_characteristic_factor.into_core();
        if combined_factor.subtract(trading_factor).norm() > tolerance {
            return true;
        }
    }

    false
}

fn distance_for_check_id(
    check_id: &str,
    expected: &ReturnDecompositionDiagnosticContributions,
    observed: &ReturnDecompositionDiagnosticContributions,
) -> f64 {
    match check_id {
        "buy_side_weighted_exponent" => observed
            .buy_side_weighted_exponent
            .into_core()
            .subtract(expected.buy_side_weighted_exponent.into_core())
            .norm(),
        "sell_side_weighted_exponent" => observed
            .sell_side_weighted_exponent
            .into_core()
            .subtract(expected.sell_side_weighted_exponent.into_core())
            .norm(),
        "combined_weighted_exponent" => observed
            .combined_weighted_exponent
            .into_core()
            .subtract(expected.combined_weighted_exponent.into_core())
            .norm(),
        "trading_time_characteristic_factor" => observed
            .trading_time_characteristic_factor
            .into_core()
            .subtract(expected.trading_time_characteristic_factor.into_core())
            .norm(),
        "combined_characteristic_factor" => observed
            .combined_characteristic_factor
            .into_core()
            .subtract(expected.combined_characteristic_factor.into_core())
            .norm(),
        _ => observed
            .combined_characteristic_factor
            .into_core()
            .subtract(expected.combined_characteristic_factor.into_core())
            .norm(),
    }
}
