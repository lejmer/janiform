use serde::Serialize;

use super::constants::{
    NIG_ALPHA_BETA_MARGIN_WARNING_RELATIVE_THRESHOLD, NIG_RADICAND_WARNING_RELATIVE_THRESHOLD,
};
use janiform_foundations::complex_math::ComplexScalar;

/// Pass, warning, or failure status for numerical guardrails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NigNumericalGuardrailStatus {
    Pass,
    Warning,
    Fail,
}

/// Severity for one numerical guardrail event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NigNumericalGuardrailSeverity {
    Warning,
    Error,
}

/// Machine-readable event code for one guardrail hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NigCharacteristicExponentGuardrailCode {
    NonFiniteInput,
    AlphaBetaBoundaryProximity,
    AlphaBetaDomainViolation,
    TransformRadicandProximity,
    CancellationSensitiveDifference,
    RationalizedDifferenceFallback,
    NonFiniteEvaluationOutput,
}

/// One deterministic guardrail event emitted during exponent evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct NigCharacteristicExponentGuardrailEvent {
    pub code: NigCharacteristicExponentGuardrailCode,
    pub severity: NigNumericalGuardrailSeverity,
    pub metric: &'static str,
    pub observed_value: Option<f64>,
    pub threshold_value: Option<f64>,
    pub detail: &'static str,
}

/// Machine-readable exponent value plus guardrail metadata.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NigCharacteristicExponentEvaluationReport {
    pub schema_version: &'static str,
    pub determinism_policy: &'static str,
    pub stability_policy: &'static str,
    pub status: NigNumericalGuardrailStatus,
    pub argument: ComplexScalar,
    pub alpha_beta_margin: f64,
    pub alpha_beta_margin_warning_threshold: f64,
    pub radicand_norm: f64,
    pub radicand_warning_threshold: f64,
    pub cancellation_ratio: f64,
    pub cancellation_warning_threshold: f64,
    pub used_rationalized_difference: bool,
    pub events: Vec<NigCharacteristicExponentGuardrailEvent>,
    pub value: Option<ComplexScalar>,
}

pub(crate) fn alpha_beta_margin_warning_threshold(alpha: f64, beta: f64) -> f64 {
    let scale = alpha.abs().max(beta.abs()).max(1.0);
    scale * NIG_ALPHA_BETA_MARGIN_WARNING_RELATIVE_THRESHOLD
}

pub(crate) fn compute_radicand_warning_threshold(
    alpha: f64,
    beta_plus_imaginary_argument: ComplexScalar,
) -> f64 {
    let transform_scale = alpha
        .abs()
        .max(beta_plus_imaginary_argument.norm())
        .max(1.0);

    transform_scale * transform_scale * NIG_RADICAND_WARNING_RELATIVE_THRESHOLD
}

pub(crate) fn push_guardrail_event(
    status: &mut NigNumericalGuardrailStatus,
    events: &mut Vec<NigCharacteristicExponentGuardrailEvent>,
    event: NigCharacteristicExponentGuardrailEvent,
) {
    match event.severity {
        NigNumericalGuardrailSeverity::Warning => {
            if *status == NigNumericalGuardrailStatus::Pass {
                *status = NigNumericalGuardrailStatus::Warning;
            }
        }
        NigNumericalGuardrailSeverity::Error => {
            *status = NigNumericalGuardrailStatus::Fail;
        }
    }

    events.push(event);
}

pub(crate) fn collect_non_finite_guardrail_event(
    status: &mut NigNumericalGuardrailStatus,
    metric: &'static str,
    observed_value: f64,
    events: &mut Vec<NigCharacteristicExponentGuardrailEvent>,
    detail: &'static str,
) {
    if !observed_value.is_finite() {
        push_guardrail_event(
            status,
            events,
            NigCharacteristicExponentGuardrailEvent {
                code: NigCharacteristicExponentGuardrailCode::NonFiniteInput,
                severity: NigNumericalGuardrailSeverity::Error,
                metric,
                observed_value: Some(observed_value),
                threshold_value: None,
                detail,
            },
        );
    }
}
