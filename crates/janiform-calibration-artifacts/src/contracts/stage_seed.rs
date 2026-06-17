use janiform_foundations::splitmix64_output_mixer;

use super::{
    CalibrationOptimizerSettings, CalibrationResolvedStageSeed, CalibrationRunSpec,
    CalibrationStage, CalibrationStageSeedSource,
};

pub fn resolve_calibration_stage_seed(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> u64 {
    resolve_calibration_stage_seed_metadata(run_spec, stage).stage_seed
}

pub fn resolve_calibration_stage_seed_metadata(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> CalibrationResolvedStageSeed {
    if let Some(stage_seed_override) = run_spec
        .seed_settings
        .stage_seed_overrides
        .iter()
        .find(|stage_seed_override| stage_seed_override.stage == stage)
    {
        return CalibrationResolvedStageSeed {
            run_global_seed: run_spec.seed_settings.global_seed,
            stage_seed: stage_seed_override.seed,
            stage_seed_source: CalibrationStageSeedSource::StageOverride,
        };
    }

    CalibrationResolvedStageSeed {
        run_global_seed: run_spec.seed_settings.global_seed,
        stage_seed: derive_calibration_stage_seed_from_global_seed(
            run_spec.seed_settings.global_seed,
            stage,
        ),
        stage_seed_source: CalibrationStageSeedSource::DerivedFromGlobalSeed,
    }
}

pub fn derive_calibration_stage_seed_from_global_seed(
    global_seed: u64,
    stage: CalibrationStage,
) -> u64 {
    splitmix64_output_mixer(global_seed ^ calibration_stage_seed_derivation_nonce(stage))
}

const fn calibration_stage_seed_derivation_nonce(stage: CalibrationStage) -> u64 {
    match stage {
        CalibrationStage::Seasonality => 0x5E45_0A3D_B51F_4E91,
        CalibrationStage::Hawkes => 0xA67B_3D2C_17F8_94E0,
        CalibrationStage::ObservationModels => 0x9F27_6C31_D4B8_A05E,
        CalibrationStage::ReturnComponents => 0x3CF2_D8A1_6B94_C705,
        CalibrationStage::LongVariance => 0xF1C4_729B_0E6D_38A5,
        CalibrationStage::NewsShocks => 0x86A4_D371_2FB0_9C5E,
        CalibrationStage::ReturnBackfit => 0xB3F8_94C2_1D6E_A705,
        CalibrationStage::MeanReversion => 0x6D28_A14C_09F3_B572,
        CalibrationStage::JointRefinement => 0x9B5D_3F41_7A2C_E811,
        CalibrationStage::OptionRefinement => 0xD4E1_72A9_3C0B_6F58,
        CalibrationStage::Finalization => 0x42B7_C9E6_81D5_0A33,
    }
}

pub fn stage_solver_settings_for_stage(
    run_spec: &CalibrationRunSpec,
    stage: CalibrationStage,
) -> Option<CalibrationOptimizerSettings> {
    run_spec
        .optimizer_settings_by_stage
        .iter()
        .find(|stage_solver_settings| stage_solver_settings.stage == stage)
        .map(|stage_solver_settings| stage_solver_settings.optimizer_settings.clone())
}
