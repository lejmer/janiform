use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationRunDataInputs {
    pub exchange_code: String,
    pub trade_events_data_reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coarse_data_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_surface_data_reference: Option<String>,
}

impl CalibrationRunDataInputs {
    pub fn try_new(
        exchange_code: impl Into<String>,
        trade_events_data_reference: impl Into<String>,
        coarse_data_reference: Option<String>,
        option_surface_data_reference: Option<String>,
    ) -> Result<Self, CalibrationRunDataInputsError> {
        let data_inputs = Self {
            exchange_code: exchange_code.into(),
            trade_events_data_reference: trade_events_data_reference.into(),
            coarse_data_reference,
            option_surface_data_reference,
        };
        data_inputs.validate()?;
        Ok(data_inputs)
    }

    pub fn validate(&self) -> Result<(), CalibrationRunDataInputsError> {
        if self.exchange_code.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingExchangeCode);
        }

        if self.trade_events_data_reference.trim().is_empty() {
            return Err(CalibrationRunDataInputsError::MissingTradeEventsDataReference);
        }

        if let Some(option_surface_data_reference) = &self.option_surface_data_reference
            && option_surface_data_reference.trim().is_empty()
        {
            return Err(CalibrationRunDataInputsError::EmptyOptionSurfaceDataReference);
        }

        if let Some(coarse_data_reference) = &self.coarse_data_reference
            && coarse_data_reference.trim().is_empty()
        {
            return Err(CalibrationRunDataInputsError::EmptyCoarseDataReference);
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_RUN_DATA_INPUTS_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationRunDataInputsError {
    MissingExchangeCode,
    MissingTradeEventsDataReference,
    EmptyOptionSurfaceDataReference,
    EmptyCoarseDataReference,
}

impl Display for CalibrationRunDataInputsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingExchangeCode => {
                write!(
                    formatter,
                    "data_inputs.exchange_code must be a non-empty string"
                )
            }
            Self::MissingTradeEventsDataReference => write!(
                formatter,
                "data_inputs.trade_events_data_reference must be a non-empty string"
            ),
            Self::EmptyOptionSurfaceDataReference => write!(
                formatter,
                "data_inputs.option_surface_data_reference must be non-empty when provided"
            ),
            Self::EmptyCoarseDataReference => write!(
                formatter,
                "data_inputs.coarse_data_reference must be non-empty when provided"
            ),
        }
    }
}

impl Error for CalibrationRunDataInputsError {}
