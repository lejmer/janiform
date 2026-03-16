use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::CalibrationStage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationStageSeedOverride {
    pub stage: CalibrationStage,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStageSeedSource {
    DerivedFromGlobalSeed,
    StageOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationResolvedStageSeed {
    pub run_global_seed: u64,
    pub stage_seed: u64,
    pub stage_seed_source: CalibrationStageSeedSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunSeedSettings {
    pub global_seed: u64,
    #[serde(default)]
    pub stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
}

impl CalibrationRunSeedSettings {
    pub fn try_new(
        global_seed: u64,
        stage_seed_overrides: Vec<CalibrationStageSeedOverride>,
    ) -> Result<Self, CalibrationRunSeedSettingsError> {
        let seed_settings = Self {
            global_seed,
            stage_seed_overrides,
        };
        seed_settings.validate()?;
        Ok(seed_settings)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunSeedSettingsError> {
        let mut stage_overrides = BTreeSet::new();
        for stage_seed_override in &self.stage_seed_overrides {
            if !stage_overrides.insert(stage_seed_override.stage) {
                return Err(
                    CalibrationRunSeedSettingsError::DuplicateStageSeedOverride {
                        stage: stage_seed_override.stage,
                    },
                );
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunSeedSettingsError {
    DuplicateStageSeedOverride { stage: CalibrationStage },
}

impl Display for CalibrationRunSeedSettingsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateStageSeedOverride { stage } => write!(
                formatter,
                "seed_settings.stage_seed_overrides contains duplicate stage `{stage:?}`"
            ),
        }
    }
}

impl Error for CalibrationRunSeedSettingsError {}
