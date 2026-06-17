use janiform_calibration_artifacts::CalibrationStage;
use janiform_model_kernels::timestamp_observation::{
    TimestampBatchingObservationMode, TimestampBatchingObservationModel,
};
use serde::Serialize;

/// Schema version for market-data preparation timestamp batching diagnostics.
pub const CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_SCHEMA_VERSION: &str =
    "calibration.market_data.timestamp_batching_observation_diagnostic.v1";

/// Diagnostic policy for reporting the fitted timestamp observation layer during preparation.
pub const CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_POLICY: &str =
    "return_components_timestamp_batching_observation_visibility.v1";

/// Preparation diagnostic state for timestamp batching observation inference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationTimestampBatchingObservationDiagnosticState {
    /// A fixed timestamp bucket was inferred from open-session ticks.
    InferredFixedBucket,
    /// Return-components calibration ran and no supported timestamp batching was detectable.
    DisabledNoDetectableBatching,
    /// Fitting is owned by the observation-models calibration stage.
    DeferredToObservationModelsStage,
    /// The selected calibration scope does not include return-components calibration.
    NotEvaluatedReturnComponentsNotSelected,
    /// Return-components calibration was selected but did not produce an input payload.
    NotEvaluatedReturnComponentsInputUnavailable,
}

impl CalibrationTimestampBatchingObservationDiagnosticState {
    /// Stable state identifier used by diagnostics and machine-readable progress.
    pub const fn state_id(self) -> &'static str {
        match self {
            Self::InferredFixedBucket => "inferred_fixed_bucket",
            Self::DisabledNoDetectableBatching => "disabled_no_detectable_batching",
            Self::DeferredToObservationModelsStage => "deferred_to_observation_models_stage",
            Self::NotEvaluatedReturnComponentsNotSelected => {
                "not_evaluated_return_components_not_selected"
            }
            Self::NotEvaluatedReturnComponentsInputUnavailable => {
                "not_evaluated_return_components_input_unavailable"
            }
        }
    }
}

/// Timestamp batching observation diagnostic included in market-data preparation outputs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CalibrationTimestampBatchingObservationDiagnostic {
    /// Contract schema version.
    pub schema_version: String,
    /// Diagnostic policy that determines how this visibility record is reported.
    pub diagnostic_policy: String,
    /// Human and machine-readable state of the preparation-time inference.
    pub state: CalibrationTimestampBatchingObservationDiagnosticState,
    /// Whether the selected workflow includes return-components calibration.
    pub return_components_stage_selected: bool,
    /// Preparation-time visibility snapshot. When fitting is deferred, the stage output owns the model.
    pub observation_model: TimestampBatchingObservationModel,
    /// Short user-facing summary suitable for progress displays.
    pub message: String,
}

pub(crate) fn timestamp_batching_observation_diagnostic(
    selected_stages: &[CalibrationStage],
    timestamp_batching_observation_model: Option<&TimestampBatchingObservationModel>,
) -> CalibrationTimestampBatchingObservationDiagnostic {
    let return_components_stage_selected =
        selected_stages.contains(&CalibrationStage::ReturnComponents);

    let state = timestamp_batching_observation_diagnostic_state(
        return_components_stage_selected,
        timestamp_batching_observation_model,
    );

    // The preparation diagnostic owns a snapshot while the execution input retains
    // the original calibration model for later stages.
    let observation_model = timestamp_batching_observation_model
        .cloned()
        .unwrap_or_else(TimestampBatchingObservationModel::disabled);

    CalibrationTimestampBatchingObservationDiagnostic {
        schema_version: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_SCHEMA_VERSION
            .to_string(),
        diagnostic_policy: CALIBRATION_TIMESTAMP_BATCHING_OBSERVATION_DIAGNOSTIC_POLICY.to_string(),
        state,
        return_components_stage_selected,
        message: timestamp_batching_observation_message(state, &observation_model),
        observation_model,
    }
}

fn timestamp_batching_observation_diagnostic_state(
    return_components_stage_selected: bool,
    timestamp_batching_observation_model: Option<&TimestampBatchingObservationModel>,
) -> CalibrationTimestampBatchingObservationDiagnosticState {
    if return_components_stage_selected && timestamp_batching_observation_model.is_none() {
        return CalibrationTimestampBatchingObservationDiagnosticState::DeferredToObservationModelsStage;
    }

    match (
        return_components_stage_selected,
        timestamp_batching_observation_model
            .map(|model| model.mode)
            .unwrap_or(TimestampBatchingObservationMode::Disabled),
        timestamp_batching_observation_model.is_some(),
    ) {
        (false, _, _) => {
            CalibrationTimestampBatchingObservationDiagnosticState::NotEvaluatedReturnComponentsNotSelected
        }
        (true, TimestampBatchingObservationMode::FixedBucketFloor, true) => {
            CalibrationTimestampBatchingObservationDiagnosticState::InferredFixedBucket
        }
        (true, TimestampBatchingObservationMode::Disabled, true) => {
            CalibrationTimestampBatchingObservationDiagnosticState::DisabledNoDetectableBatching
        }
        (true, _, false) => {
            CalibrationTimestampBatchingObservationDiagnosticState::NotEvaluatedReturnComponentsInputUnavailable
        }
    }
}

fn timestamp_batching_observation_message(
    state: CalibrationTimestampBatchingObservationDiagnosticState,
    observation_model: &TimestampBatchingObservationModel,
) -> String {
    match state {
        CalibrationTimestampBatchingObservationDiagnosticState::InferredFixedBucket => {
            let bucket_width_nanos = observation_model
                .bucket_width_nanos
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let same_timestamp_continuation_count = observation_model
                .same_timestamp_continuation_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let same_session_observation_transition_count = observation_model
                .same_session_observation_transition_count
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_string());

            let continuation_probability = observation_model
                .same_timestamp_continuation_probability()
                .map(|value| format!("{value:.6}"))
                .unwrap_or_else(|| "unavailable".to_string());

            format!(
                "timestamp batching observation inferred: fixed_bucket_floor bucket_width_nanos={bucket_width_nanos} same_timestamp_continuation_count={same_timestamp_continuation_count} same_session_observation_transition_count={same_session_observation_transition_count} continuation_probability={continuation_probability}"
            )
        }
        CalibrationTimestampBatchingObservationDiagnosticState::DisabledNoDetectableBatching => {
            "timestamp batching observation evaluated: disabled because no same-session duplicate timestamp quantum was detected"
                .to_string()
        }
        CalibrationTimestampBatchingObservationDiagnosticState::DeferredToObservationModelsStage => {
            "timestamp batching observation fit is deferred to the observation-models calibration stage"
                .to_string()
        }
        CalibrationTimestampBatchingObservationDiagnosticState::NotEvaluatedReturnComponentsNotSelected => {
            "timestamp batching observation not evaluated because return components are not selected"
                .to_string()
        }
        CalibrationTimestampBatchingObservationDiagnosticState::NotEvaluatedReturnComponentsInputUnavailable => {
            "timestamp batching observation not evaluated because return-components calibration input is unavailable"
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use janiform_calibration_artifacts::CalibrationStage;
    use janiform_model_kernels::timestamp_observation::TimestampBatchingObservationModel;

    use super::{
        CalibrationTimestampBatchingObservationDiagnosticState,
        timestamp_batching_observation_diagnostic,
    };

    #[test]
    fn timestamp_batching_diagnostic_reports_inferred_fixed_bucket() {
        let observation_model =
            TimestampBatchingObservationModel::fixed_bucket_floor(1_000, "test_source", 2, 10)
                .unwrap();

        let diagnostic = timestamp_batching_observation_diagnostic(
            &[CalibrationStage::ReturnComponents],
            Some(&observation_model),
        );

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingObservationDiagnosticState::InferredFixedBucket
        );
        assert_eq!(diagnostic.observation_model, observation_model);
        assert_eq!(
            diagnostic.message,
            "timestamp batching observation inferred: fixed_bucket_floor bucket_width_nanos=1000 same_timestamp_continuation_count=2 same_session_observation_transition_count=10 continuation_probability=0.200000"
        );
    }

    #[test]
    fn timestamp_batching_diagnostic_reports_unselected_return_components() {
        let diagnostic =
            timestamp_batching_observation_diagnostic(&[CalibrationStage::Seasonality], None);

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingObservationDiagnosticState::NotEvaluatedReturnComponentsNotSelected
        );
        assert_eq!(
            diagnostic.observation_model,
            TimestampBatchingObservationModel::disabled()
        );
    }

    #[test]
    fn timestamp_batching_diagnostic_reports_disabled_no_detectable_batching() {
        let observation_model = TimestampBatchingObservationModel::disabled();

        let diagnostic = timestamp_batching_observation_diagnostic(
            &[CalibrationStage::ReturnComponents],
            Some(&observation_model),
        );

        assert_eq!(
            diagnostic.state,
            CalibrationTimestampBatchingObservationDiagnosticState::DisabledNoDetectableBatching
        );
        assert_eq!(diagnostic.observation_model, observation_model);
        assert_eq!(
            diagnostic.message,
            "timestamp batching observation evaluated: disabled because no same-session duplicate timestamp quantum was detected"
        );
    }
}
