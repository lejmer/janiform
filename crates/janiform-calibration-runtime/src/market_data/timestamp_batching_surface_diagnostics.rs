use janiform_calibration_artifacts::CalibrationStage;
use janiform_model_kernels::timestamp_batching_surface::{
    TimestampBatchingMode, TimestampBatchingSurface,
};
use serde::Serialize;

/// Schema version for market-data preparation timestamp batching diagnostics.
pub const CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION: &str =
    "calibration.market_data.timestamp_batching_surface_diagnostic.v1";

/// Diagnostic policy for reporting the fitted timestamp batching surface during preparation.
pub const CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY: &str =
    "return_components_timestamp_batching_surface_visibility.v1";

/// Preparation diagnostic state for timestamp batching surface inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationTimestampBatchingSurfaceDiagnosticState {
    /// A fixed timestamp bucket was inferred from open-session ticks.
    InferredFixedBucket,
    /// Return-components calibration ran and no supported timestamp batching was detectable.
    DisabledNoDetectableBatching,
    /// The selected calibration scope does not include return-components calibration.
    NotEvaluatedReturnComponentsNotSelected,
    /// Return-components calibration was selected but did not produce an input payload.
    NotEvaluatedReturnComponentsInputUnavailable,
}

impl CalibrationTimestampBatchingSurfaceDiagnosticState {
    /// Stable state identifier used by diagnostics and machine-readable progress.
    pub const fn state_id(self) -> &'static str {
        match self {
            Self::InferredFixedBucket => "inferred_fixed_bucket",
            Self::DisabledNoDetectableBatching => "disabled_no_detectable_batching",
            Self::NotEvaluatedReturnComponentsNotSelected => {
                "not_evaluated_return_components_not_selected"
            }
            Self::NotEvaluatedReturnComponentsInputUnavailable => {
                "not_evaluated_return_components_input_unavailable"
            }
        }
    }
}

/// Timestamp batching surface diagnostic included in market-data preparation outputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CalibrationTimestampBatchingSurfaceDiagnostic {
    /// Contract schema version.
    pub schema_version: String,
    /// Diagnostic policy that determines how this visibility record is reported.
    pub diagnostic_policy: String,
    /// Human and machine-readable state of the preparation-time inference.
    pub state: CalibrationTimestampBatchingSurfaceDiagnosticState,
    /// Whether the selected workflow includes return-components calibration.
    pub return_components_stage_selected: bool,
    /// Preparation-time visibility snapshot.
    pub timestamp_batching_surface: TimestampBatchingSurface,
    /// Short user-facing summary suitable for progress displays.
    pub message: String,
}

pub(crate) fn timestamp_batching_surface_diagnostic(
    selected_stages: &[CalibrationStage],
    timestamp_batching_surface: Option<&TimestampBatchingSurface>,
) -> CalibrationTimestampBatchingSurfaceDiagnostic {
    let return_components_stage_selected =
        selected_stages.contains(&CalibrationStage::ReturnComponents);

    let state = timestamp_batching_surface_diagnostic_state(
        return_components_stage_selected,
        timestamp_batching_surface,
    );

    // The preparation diagnostic owns a snapshot while the execution input retains
    // the original calibration model for later stages.
    let timestamp_batching_surface = timestamp_batching_surface
        .cloned()
        .unwrap_or_else(TimestampBatchingSurface::disabled);

    CalibrationTimestampBatchingSurfaceDiagnostic {
        schema_version: CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_SCHEMA_VERSION
            .to_string(),
        diagnostic_policy: CALIBRATION_TIMESTAMP_BATCHING_SURFACE_DIAGNOSTIC_POLICY.to_string(),
        state,
        return_components_stage_selected,
        message: timestamp_batching_surface_message(state, &timestamp_batching_surface),
        timestamp_batching_surface,
    }
}

fn timestamp_batching_surface_diagnostic_state(
    return_components_stage_selected: bool,
    timestamp_batching_surface: Option<&TimestampBatchingSurface>,
) -> CalibrationTimestampBatchingSurfaceDiagnosticState {
    match (
        return_components_stage_selected,
        timestamp_batching_surface
            .map(|model| model.mode)
            .unwrap_or(TimestampBatchingMode::Disabled),
        timestamp_batching_surface.is_some(),
    ) {
        (false, _, _) => CalibrationTimestampBatchingSurfaceDiagnosticState::NotEvaluatedReturnComponentsNotSelected,
        (true, TimestampBatchingMode::FixedBucketFloor, true) => {
            CalibrationTimestampBatchingSurfaceDiagnosticState::InferredFixedBucket
        }
        (true, TimestampBatchingMode::Disabled, true) => {
            CalibrationTimestampBatchingSurfaceDiagnosticState::DisabledNoDetectableBatching
        }
        (true, _, false) => {
            CalibrationTimestampBatchingSurfaceDiagnosticState::NotEvaluatedReturnComponentsInputUnavailable
        }
    }
}

fn timestamp_batching_surface_message(
    state: CalibrationTimestampBatchingSurfaceDiagnosticState,
    timestamp_batching_surface: &TimestampBatchingSurface,
) -> String {
    match state {
        CalibrationTimestampBatchingSurfaceDiagnosticState::InferredFixedBucket => {
            let bucket_width_nanos = timestamp_batching_surface
                .bucket_width_nanos
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let same_timestamp_continuation_count = timestamp_batching_surface
                .same_timestamp_continuation_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let same_session_observation_transition_count = timestamp_batching_surface
                .same_session_observation_transition_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let continuation_probability = timestamp_batching_surface
                .same_timestamp_continuation_probability()
                .map(|value| format!("{value:.6}"))
                .unwrap_or_else(|| "unavailable".to_string());

            format!(
                "timestamp batching surface inferred: fixed_bucket_floor bucket_width_nanos={bucket_width_nanos} same_timestamp_continuation_count={same_timestamp_continuation_count} same_session_observation_transition_count={same_session_observation_transition_count} continuation_probability={continuation_probability}"
            )
        }
        CalibrationTimestampBatchingSurfaceDiagnosticState::DisabledNoDetectableBatching => {
            "timestamp batching surface evaluated: disabled because no same-session duplicate timestamp quantum was detected"
                .to_string()
        }
        CalibrationTimestampBatchingSurfaceDiagnosticState::NotEvaluatedReturnComponentsNotSelected => {
            "timestamp batching surface not evaluated because return components are not selected"
                .to_string()
        }
        CalibrationTimestampBatchingSurfaceDiagnosticState::NotEvaluatedReturnComponentsInputUnavailable => {
            "timestamp batching surface not evaluated because return-components calibration input is unavailable"
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationStage;
    use janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface;

    use super::{
        CalibrationTimestampBatchingSurfaceDiagnosticState, timestamp_batching_surface_diagnostic,
    };

    #[test]
    fn timestamp_batching_diagnostic_reports_inferred_fixed_bucket() {
        let timestamp_batching_surface =
            TimestampBatchingSurface::fixed_bucket_floor(1_000, "test_source", 2, 10).unwrap();

        let diagnostic = timestamp_batching_surface_diagnostic(
            &[CalibrationStage::ReturnComponents],
            Some(&timestamp_batching_surface),
        );

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingSurfaceDiagnosticState::InferredFixedBucket
        );
        assert_eq!(
            diagnostic.timestamp_batching_surface,
            timestamp_batching_surface
        );
        assert_eq!(
            diagnostic.message,
            "timestamp batching surface inferred: fixed_bucket_floor bucket_width_nanos=1000 same_timestamp_continuation_count=2 same_session_observation_transition_count=10 continuation_probability=0.200000"
        );
    }

    #[test]
    fn timestamp_batching_diagnostic_reports_unselected_return_components() {
        let diagnostic =
            timestamp_batching_surface_diagnostic(&[CalibrationStage::Seasonality], None);

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingSurfaceDiagnosticState::NotEvaluatedReturnComponentsNotSelected
        );
        assert_eq!(
            diagnostic.timestamp_batching_surface,
            TimestampBatchingSurface::disabled()
        );
    }

    #[test]
    fn timestamp_batching_diagnostic_reports_disabled_no_detectable_batching() {
        let timestamp_batching_surface = TimestampBatchingSurface::disabled();

        let diagnostic = timestamp_batching_surface_diagnostic(
            &[CalibrationStage::ReturnComponents],
            Some(&timestamp_batching_surface),
        );

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingSurfaceDiagnosticState::DisabledNoDetectableBatching
        );
        assert_eq!(
            diagnostic.timestamp_batching_surface,
            timestamp_batching_surface
        );
        assert_eq!(
            diagnostic.message,
            "timestamp batching surface evaluated: disabled because no same-session duplicate timestamp quantum was detected"
        );
    }
}
