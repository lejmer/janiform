use serde::Serialize;

use janiform_affine_oracle::affine_oracle::{
    ExponentialAffineTransformEquationSpecification, ExponentialAffineTransformNewsIntensityInput,
};
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

use super::constants::{
    DEFAULT_INTRADAY_RISK_INTERVAL_DELTA, NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE,
    SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY,
    SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION,
};
use super::errors::{
    SufficientStatisticsRiskIntervalFactorsError, SufficientStatisticsRiskModelInputsError,
    SufficientStatisticsRiskRunRequestError,
};
use super::validation::validate_excitation_state;

/// Per-interval deterministic factors used by the sufficient-statistics risk sampler.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskIntervalFactors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_delta: Option<f64>,
    pub s_h: Box<[f64]>,
    #[serde(skip_serializing)]
    pub nu_h: Box<[f64]>,
    pub nu_h_known: Box<[f64]>,
    pub nu_h_surprise: Box<[f64]>,
}

impl SufficientStatisticsRiskIntervalFactors {
    pub fn try_new(
        s_h: Vec<f64>,
        nu_h_known: Vec<f64>,
        nu_h_surprise: Vec<f64>,
        interval_delta: Option<f64>,
    ) -> Result<Self, SufficientStatisticsRiskIntervalFactorsError> {
        if s_h.is_empty() {
            return Err(SufficientStatisticsRiskIntervalFactorsError::MissingIntervals);
        }
        if nu_h_known.len() != s_h.len() {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: s_h.len(),
                    actual_length: nu_h_known.len(),
                },
            );
        }
        if nu_h_surprise.len() != s_h.len() {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: s_h.len(),
                    actual_length: nu_h_surprise.len(),
                },
            );
        }

        let nu_h: Vec<f64> = nu_h_known
            .iter()
            .zip(nu_h_surprise.iter())
            .map(|(known_nu_h, surprise_nu_h)| known_nu_h + surprise_nu_h)
            .collect();
        let interval_factors = Self {
            interval_delta,
            s_h: s_h.into_boxed_slice(),
            nu_h: nu_h.into_boxed_slice(),
            nu_h_known: nu_h_known.into_boxed_slice(),
            nu_h_surprise: nu_h_surprise.into_boxed_slice(),
        };
        interval_factors.validate()?;
        Ok(interval_factors)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskIntervalFactorsError> {
        let interval_count = self.s_h.len();
        if interval_count == 0 {
            return Err(SufficientStatisticsRiskIntervalFactorsError::MissingIntervals);
        }

        for (interval_index, value) in self.s_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidSeasonalityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        for (interval_index, value) in self.nu_h.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidNewsIntensityFactor {
                        interval_index,
                        value,
                    },
                );
            }
        }

        if self.nu_h_known.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_known",
                    expected_length: interval_count,
                    actual_length: self.nu_h_known.len(),
                },
            );
        }
        if self.nu_h_surprise.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h_surprise",
                    expected_length: interval_count,
                    actual_length: self.nu_h_surprise.len(),
                },
            );
        }
        if self.nu_h.len() != interval_count {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::FieldLengthMismatch {
                    field: "nu_h",
                    expected_length: interval_count,
                    actual_length: self.nu_h.len(),
                },
            );
        }

        for interval_index in 0..interval_count {
            let known_nu_h = self.nu_h_known[interval_index];
            if !known_nu_h.is_finite() || known_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidKnownNewsIntensityFactor {
                        interval_index,
                        value: known_nu_h,
                    },
                );
            }

            let surprise_nu_h = self.nu_h_surprise[interval_index];
            if !surprise_nu_h.is_finite() || surprise_nu_h < 0.0 {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InvalidSurpriseNewsIntensityFactor {
                        interval_index,
                        value: surprise_nu_h,
                    },
                );
            }

            let total_nu_h = self.nu_h[interval_index];
            let component_sum = known_nu_h + surprise_nu_h;
            if !component_sum.is_finite()
                || (component_sum - total_nu_h).abs() > NEWS_INTENSITY_COMPONENT_SUM_TOLERANCE
            {
                return Err(
                    SufficientStatisticsRiskIntervalFactorsError::InconsistentNewsIntensityAggregation {
                        interval_index,
                        nu_h_total: total_nu_h,
                        nu_h_known: known_nu_h,
                        nu_h_surprise: surprise_nu_h,
                    },
                );
            }
        }

        if let Some(interval_delta) = self.interval_delta
            && (!interval_delta.is_finite() || interval_delta <= 0.0)
        {
            return Err(
                SufficientStatisticsRiskIntervalFactorsError::InvalidIntervalDelta {
                    value: interval_delta,
                },
            );
        }

        Ok(())
    }

    pub const fn policy() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_INTERVAL_FACTOR_POLICY
    }

    pub fn resolved_interval_delta(&self) -> f64 {
        self.interval_delta
            .unwrap_or(DEFAULT_INTRADAY_RISK_INTERVAL_DELTA)
    }

    pub fn news_intensity_input_at(
        &self,
        interval_index: usize,
    ) -> ExponentialAffineTransformNewsIntensityInput {
        ExponentialAffineTransformNewsIntensityInput::try_new(
            self.nu_h_known[interval_index],
            self.nu_h_surprise[interval_index],
        )
        .expect("validated interval factors must produce valid news intensity inputs")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskModelInputs {
    pub equation_specification: ExponentialAffineTransformEquationSpecification,
    pub nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    pub initial_excitation_state: HawkesExcitationState,
}

impl SufficientStatisticsRiskModelInputs {
    pub fn try_new(
        equation_specification: ExponentialAffineTransformEquationSpecification,
        nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
        initial_excitation_state: HawkesExcitationState,
    ) -> Result<Self, SufficientStatisticsRiskModelInputsError> {
        let model_inputs = Self {
            equation_specification,
            nig_shared_plus_shift_configuration,
            initial_excitation_state,
        };
        model_inputs.validate()?;
        Ok(model_inputs)
    }

    pub fn validate(self) -> Result<(), SufficientStatisticsRiskModelInputsError> {
        self.equation_specification.validate().map_err(|source| {
            SufficientStatisticsRiskModelInputsError::InvalidEquationSpecification { source }
        })?;

        self.nig_shared_plus_shift_configuration
            .derive_driver_parameters()
            .map_err(|source| {
                SufficientStatisticsRiskModelInputsError::InvalidNigSharedPlusShiftConfiguration {
                    source,
                }
            })?;

        validate_excitation_state(self.initial_excitation_state).map_err(|(field, value)| {
            SufficientStatisticsRiskModelInputsError::InvalidInitialState { field, value }
        })?;

        Ok(())
    }
}

/// Public run request contract for sufficient-statistics risk simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SufficientStatisticsRiskRunRequest {
    pub seed: u64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: SufficientStatisticsRiskModelInputs,
    pub interval_factors: SufficientStatisticsRiskIntervalFactors,
}

impl SufficientStatisticsRiskRunRequest {
    pub fn try_new(
        seed: u64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: SufficientStatisticsRiskModelInputs,
        interval_factors: SufficientStatisticsRiskIntervalFactors,
    ) -> Result<Self, SufficientStatisticsRiskRunRequestError> {
        let run_request = Self {
            seed,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factors,
        };
        run_request.validate()?;
        Ok(run_request)
    }

    pub fn validate(&self) -> Result<(), SufficientStatisticsRiskRunRequestError> {
        if self.horizon_interval_count == 0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidHorizonIntervalCount { value: 0 },
            );
        }

        if !self.interval_delta.is_finite() || self.interval_delta <= 0.0 {
            return Err(
                SufficientStatisticsRiskRunRequestError::InvalidIntervalDelta {
                    value: self.interval_delta,
                },
            );
        }

        self.model_inputs.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidModelInputs { source }
        })?;

        self.interval_factors.validate().map_err(|source| {
            SufficientStatisticsRiskRunRequestError::InvalidIntervalFactors { source }
        })?;

        let resolved_factor_interval_delta = self.interval_factors.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                SufficientStatisticsRiskRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factors.s_h.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                SufficientStatisticsRiskRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        Ok(())
    }

    pub const fn schema_version() -> &'static str {
        SUFFICIENT_STATISTICS_RISK_RUN_REQUEST_SCHEMA_VERSION
    }

    pub const fn default_interval_delta() -> f64 {
        DEFAULT_INTRADAY_RISK_INTERVAL_DELTA
    }
}
