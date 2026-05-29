use crate::contracts::{CalibrationRunSpec, CalibrationStage};

/// Return the stable artifact and checkpoint token for a calibration stage.
pub fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
        CalibrationStage::LongVariance => "long_variance",
        CalibrationStage::NewsShocks => "news_shocks",
        CalibrationStage::MeanReversion => "mean_reversion",
        CalibrationStage::JointRefinement => "joint_refinement",
        CalibrationStage::OptionRefinement => "option_refinement",
        CalibrationStage::Finalization => "finalization",
    }
}

/// Return the canonical stage-output artifact reference for a run and stage.
pub fn calibration_stage_artifact_reference(run_id: &str, stage: CalibrationStage) -> String {
    format!("artifact://{}/{}", calibration_stage_token(stage), run_id)
}

/// Parse a stable calibration-stage token used in artifact paths and checkpoint references.
pub fn calibration_stage_from_token(token: &str) -> Option<CalibrationStage> {
    match token {
        "seasonality" => Some(CalibrationStage::Seasonality),
        "hawkes" => Some(CalibrationStage::Hawkes),
        "return_components" => Some(CalibrationStage::ReturnComponents),
        "long_variance" => Some(CalibrationStage::LongVariance),
        "news_shocks" => Some(CalibrationStage::NewsShocks),
        "mean_reversion" => Some(CalibrationStage::MeanReversion),
        "joint_refinement" => Some(CalibrationStage::JointRefinement),
        "option_refinement" => Some(CalibrationStage::OptionRefinement),
        "finalization" => Some(CalibrationStage::Finalization),
        _ => None,
    }
}

/// Return the stage-output artifact references expected for a stage.
pub fn stage_output_artifact_references_for_stage(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Vec<String> {
    let mut artifact_references = run_spec
        .stage_handoffs
        .iter()
        .filter(|handoff| handoff.from_stage == stage)
        .filter_map(|handoff| handoff.artifact_reference())
        .filter(|artifact_reference| !artifact_reference.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    artifact_references.sort();
    artifact_references.dedup();

    artifact_references
}
