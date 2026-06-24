use std::error::Error;
use std::fmt::{Display, Formatter};

use janiform_affine_oracle::sufficient_statistics::{
    SufficientStatisticsIntervalFactors, SufficientStatisticsIntervalFactorsError,
};
use janiform_foundations::time::utc_datetime_from_nanos;
use serde::{Deserialize, Serialize};

use super::constants::COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION;
use super::model_inputs::{CoarseSimulationModelInputs, CoarseSimulationModelInputsError};
use super::time_index::{
    CoarseSimulationTimeIndexError, IntervalDeltaGridError, compute_horizon_end_time_utc_nanos,
    interval_delta_to_nanos, validate_exact_string, validate_horizon_end_timestamp,
};
use crate::core::compute_max_effective_endogenous_branching_ratio;

/// Typed coarse-interval simulation run request anchored to a real UTC start timestamp.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoarseSimulationRunRequest {
    pub schema_version: String,
    pub seed: u64,
    pub start_time_utc_nanos: i64,
    pub horizon_interval_count: usize,
    pub interval_delta: f64,
    pub model_inputs: CoarseSimulationModelInputs,
    pub interval_factor_grids: SufficientStatisticsIntervalFactors,
}

impl CoarseSimulationRunRequest {
    pub fn try_new(
        seed: u64,
        start_time_utc_nanos: i64,
        horizon_interval_count: usize,
        interval_delta: f64,
        model_inputs: CoarseSimulationModelInputs,
        interval_factor_grids: SufficientStatisticsIntervalFactors,
    ) -> Result<Self, CoarseSimulationRunRequestError> {
        let request = Self {
            schema_version: COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION.to_string(),
            seed,
            start_time_utc_nanos,
            horizon_interval_count,
            interval_delta,
            model_inputs,
            interval_factor_grids,
        };

        request.validate()?;

        Ok(request)
    }

    pub fn validate(&self) -> Result<(), CoarseSimulationRunRequestError> {
        validate_exact_string(
            "schema_version",
            &self.schema_version,
            COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
        )
        .map_err(
            |source| CoarseSimulationRunRequestError::InvalidSchemaVersion {
                field_path: source.field_path,
                expected: source.expected,
                actual: source.actual,
            },
        )?;

        if utc_datetime_from_nanos(self.start_time_utc_nanos).is_none() {
            return Err(CoarseSimulationRunRequestError::InvalidStartTimeUtcNanos {
                value: self.start_time_utc_nanos,
            });
        }

        if self.horizon_interval_count == 0 {
            return Err(CoarseSimulationRunRequestError::InvalidHorizonIntervalCount { value: 0 });
        }

        let interval_delta_nanos = interval_delta_to_nanos(self.interval_delta)
            .map_err(|source| CoarseSimulationRunRequestError::InvalidIntervalDelta { source })?;

        validate_horizon_end_timestamp(
            self.start_time_utc_nanos,
            interval_delta_nanos,
            self.horizon_interval_count,
        )
        .map_err(|source| CoarseSimulationRunRequestError::InvalidHorizonTimeIndex { source })?;

        self.model_inputs
            .validate()
            .map_err(|source| CoarseSimulationRunRequestError::InvalidModelInputs { source })?;

        self.interval_factor_grids
            .validate()
            .map_err(|source| CoarseSimulationRunRequestError::InvalidIntervalFactors { source })?;

        let resolved_factor_interval_delta = self.interval_factor_grids.resolved_interval_delta();
        if self.interval_delta != resolved_factor_interval_delta {
            return Err(
                CoarseSimulationRunRequestError::IntervalDeltaFactorMismatch {
                    requested_interval_delta: self.interval_delta,
                    available_factor_interval_delta: resolved_factor_interval_delta,
                },
            );
        }

        let available_interval_count = self.interval_factor_grids.seasonality_factor.len();
        if self.horizon_interval_count > available_interval_count {
            return Err(
                CoarseSimulationRunRequestError::HorizonExceedsAvailableIntervals {
                    requested_interval_count: self.horizon_interval_count,
                    available_interval_count,
                },
            );
        }

        let runtime_interval_anchors = &self
            .model_inputs
            .sufficient_statistics_model_inputs
            .mean_reversion_runtime_interval_anchors;

        if runtime_interval_anchors.end_anchor_values.len() != self.horizon_interval_count {
            return Err(
                CoarseSimulationRunRequestError::MeanReversionRuntimeAnchorIntervalCountMismatch {
                    expected_interval_count: self.horizon_interval_count,
                    observed_interval_count: runtime_interval_anchors.end_anchor_values.len(),
                },
            );
        }

        for (interval_index, observed_interval_delta) in runtime_interval_anchors
            .interval_deltas
            .iter()
            .copied()
            .take(self.horizon_interval_count)
            .enumerate()
        {
            let observed_interval_delta_nanos =
                interval_delta_to_nanos(observed_interval_delta).map_err(|_| {
                    CoarseSimulationRunRequestError::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                        interval_index,
                        expected_interval_delta: self.interval_delta,
                        observed_interval_delta,
                    }
                })?;

            if observed_interval_delta_nanos != interval_delta_nanos {
                return Err(
                    CoarseSimulationRunRequestError::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                        interval_index,
                        expected_interval_delta: self.interval_delta,
                        observed_interval_delta,
                    },
                );
            }
        }

        if let Some((spectral_radius_alpha_over_beta, effective_branching_ratio)) =
            compute_max_effective_endogenous_branching_ratio(
                self.model_inputs
                    .sufficient_statistics_model_inputs
                    .equation_specification
                    .hawkes_kernel_parameters,
            )
            && effective_branching_ratio >= 1.0
        {
            return Err(
                CoarseSimulationRunRequestError::SeasonalityScaledEndogenousInstability {
                    spectral_radius_alpha_over_beta,
                    effective_branching_ratio,
                },
            );
        }

        Ok(())
    }

    pub fn interval_delta_nanos(&self) -> Result<i64, IntervalDeltaGridError> {
        interval_delta_to_nanos(self.interval_delta)
    }

    pub fn end_time_utc_nanos(&self) -> Result<i64, CoarseSimulationTimeIndexError> {
        let interval_delta_nanos = interval_delta_to_nanos(self.interval_delta)
            .map_err(|source| CoarseSimulationTimeIndexError::InvalidIntervalDelta { source })?;

        compute_horizon_end_time_utc_nanos(
            self.start_time_utc_nanos,
            interval_delta_nanos,
            self.horizon_interval_count,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoarseSimulationRunRequestError {
    InvalidSchemaVersion {
        field_path: &'static str,
        expected: String,
        actual: String,
    },
    InvalidStartTimeUtcNanos {
        value: i64,
    },
    InvalidHorizonIntervalCount {
        value: usize,
    },
    InvalidIntervalDelta {
        source: IntervalDeltaGridError,
    },
    InvalidHorizonTimeIndex {
        source: CoarseSimulationTimeIndexError,
    },
    InvalidModelInputs {
        source: CoarseSimulationModelInputsError,
    },
    InvalidIntervalFactors {
        source: SufficientStatisticsIntervalFactorsError,
    },
    IntervalDeltaFactorMismatch {
        requested_interval_delta: f64,
        available_factor_interval_delta: f64,
    },
    HorizonExceedsAvailableIntervals {
        requested_interval_count: usize,
        available_interval_count: usize,
    },
    MeanReversionRuntimeAnchorIntervalCountMismatch {
        expected_interval_count: usize,
        observed_interval_count: usize,
    },
    MeanReversionRuntimeAnchorIntervalDeltaMismatch {
        interval_index: usize,
        expected_interval_delta: f64,
        observed_interval_delta: f64,
    },
    SeasonalityScaledEndogenousInstability {
        spectral_radius_alpha_over_beta: f64,
        effective_branching_ratio: f64,
    },
}

impl Display for CoarseSimulationRunRequestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSchemaVersion {
                field_path,
                expected,
                actual,
            } => write!(
                formatter,
                "{field_path} must be {expected} for the coarse simulation run request contract, found {actual}"
            ),
            Self::InvalidStartTimeUtcNanos { value } => write!(
                formatter,
                "start_time_utc_nanos must map to a valid UTC timestamp, found {value}"
            ),
            Self::InvalidHorizonIntervalCount { value } => write!(
                formatter,
                "horizon_interval_count must be strictly positive, found {value}"
            ),
            Self::InvalidIntervalDelta { source } => {
                write!(formatter, "interval_delta is invalid: {source}")
            }
            Self::InvalidHorizonTimeIndex { source } => {
                write!(
                    formatter,
                    "time-indexed horizon construction is invalid: {source}"
                )
            }
            Self::InvalidModelInputs { source } => {
                write!(formatter, "model_inputs payload is invalid: {source}")
            }
            Self::InvalidIntervalFactors { source } => {
                write!(
                    formatter,
                    "interval_factor_grids payload is invalid: {source}"
                )
            }
            Self::IntervalDeltaFactorMismatch {
                requested_interval_delta,
                available_factor_interval_delta,
            } => write!(
                formatter,
                "interval_delta ({requested_interval_delta}) must match interval_factor_grids.interval_delta ({available_factor_interval_delta}) under the coarse simulation factor-alignment policy"
            ),
            Self::HorizonExceedsAvailableIntervals {
                requested_interval_count,
                available_interval_count,
            } => write!(
                formatter,
                "horizon_interval_count ({requested_interval_count}) exceeds available interval_factor_grids entries ({available_interval_count})"
            ),
            Self::MeanReversionRuntimeAnchorIntervalCountMismatch {
                expected_interval_count,
                observed_interval_count,
            } => write!(
                formatter,
                "model_inputs.sufficient_statistics_model_inputs.mean_reversion_runtime_interval_anchors.end_anchor_values must contain {expected_interval_count} entries for the requested horizon, found {observed_interval_count}"
            ),
            Self::MeanReversionRuntimeAnchorIntervalDeltaMismatch {
                interval_index,
                expected_interval_delta,
                observed_interval_delta,
            } => write!(
                formatter,
                "model_inputs.sufficient_statistics_model_inputs.mean_reversion_runtime_interval_anchors.interval_deltas[{interval_index}] must equal interval_delta ({expected_interval_delta}), found {observed_interval_delta}"
            ),
            Self::SeasonalityScaledEndogenousInstability {
                spectral_radius_alpha_over_beta,
                effective_branching_ratio,
            } => write!(
                formatter,
                "endogenous Hawkes branching must remain below 1.0, found spectral_radius_alpha_over_beta={spectral_radius_alpha_over_beta}, effective_branching_ratio={effective_branching_ratio}"
            ),
        }
    }
}

impl Error for CoarseSimulationRunRequestError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidIntervalDelta { source } => Some(source),
            Self::InvalidHorizonTimeIndex { source } => Some(source),
            Self::InvalidModelInputs { source } => Some(source),
            Self::InvalidIntervalFactors { source } => Some(source),
            Self::MeanReversionRuntimeAnchorIntervalCountMismatch { .. }
            | Self::MeanReversionRuntimeAnchorIntervalDeltaMismatch { .. }
            | Self::SeasonalityScaledEndogenousInstability { .. }
            | Self::InvalidSchemaVersion { .. }
            | Self::InvalidStartTimeUtcNanos { .. }
            | Self::InvalidHorizonIntervalCount { .. }
            | Self::IntervalDeltaFactorMismatch { .. }
            | Self::HorizonExceedsAvailableIntervals { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use janiform_affine_oracle::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use janiform_affine_oracle::sufficient_statistics::{
        SufficientStatisticsIntervalFactors, SufficientStatisticsModelInputs,
    };
    use janiform_market_time::calendar::{
        CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
        RegularSessionArtifact, SessionWindowArtifact, TradingWeekday,
    };
    use janiform_model_kernels::hawkes::{
        HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
    };
    use janiform_model_kernels::long_variance::{LongVarianceParameters, LongVarianceState};
    use janiform_model_kernels::market_pressure::MarketPressureParameters;
    use janiform_model_kernels::mean_reversion::{
        MeanReversionBoundaryState, MeanReversionFittedParameters,
        MeanReversionRuntimeIntervalAnchors,
    };
    use janiform_model_kernels::news::{
        GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
        NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
        SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
    };
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    use super::{
        CoarseSimulationRunRequest, CoarseSimulationRunRequestError, IntervalDeltaGridError,
    };
    use crate::coarse::COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION;
    use crate::coarse::contracts::CoarseSimulationModelInputs;

    #[test]
    fn coarse_run_request_round_trip_serialization_preserves_time_anchor_and_interval_semantics() {
        let request = build_valid_run_request();

        let serialized =
            serde_json::to_value(&request).expect("coarse simulation request should serialize");

        let parsed: CoarseSimulationRunRequest =
            serde_json::from_value(serialized.clone()).expect("serialized request should parse");

        parsed
            .validate()
            .expect("deserialized coarse simulation request should remain valid");

        assert_eq!(parsed, request);
        assert_eq!(
            serialized["schema_version"],
            json!(COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION)
        );
        assert_eq!(
            serialized["start_time_utc_nanos"],
            json!(1_704_067_200_000_000_000_i64)
        );
        assert_eq!(serialized["interval_delta"], json!(1.0));
        assert_eq!(serialized["horizon_interval_count"], json!(2));
        assert_eq!(
            serialized["interval_factor_grids"]["known_news_intensity"],
            json!([0.12, 0.03])
        );
        assert_eq!(serialized["model_inputs"]["exchange_code"], json!("XNYS"));
    }

    #[test]
    fn coarse_run_request_rejects_factor_interval_delta_mismatch() {
        let error = CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            0.5,
            build_model_inputs(),
            build_interval_factor_grids(Some(1.0)),
        )
        .expect_err("interval delta mismatch must fail");

        assert!(matches!(
            error,
            CoarseSimulationRunRequestError::IntervalDeltaFactorMismatch {
                requested_interval_delta: 0.5,
                available_factor_interval_delta: 1.0,
            }
        ));
    }

    #[test]
    fn coarse_run_request_rejects_non_positive_interval_delta() {
        let error = CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            0.0,
            build_model_inputs(),
            build_interval_factor_grids(Some(1.0)),
        )
        .expect_err("non-positive interval delta must fail");

        assert!(matches!(
            error,
            CoarseSimulationRunRequestError::InvalidIntervalDelta {
                source: IntervalDeltaGridError::NonPositiveOrNonFinite { value: 0.0 },
            }
        ));
    }

    #[test]
    fn coarse_run_request_accepts_seasonality_scaled_subcritical_hawkes_inputs() {
        let mut model_inputs = build_model_inputs();

        model_inputs
            .sufficient_statistics_model_inputs
            .equation_specification
            .hawkes_kernel_parameters = HawkesKernelParameters::try_new(
            0.8,
            1.0,
            HawkesKernelLoadings {
                plus_plus: 0.9,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.9,
            },
            HawkesKernelLoadings {
                plus_plus: 0.0,
                plus_minus: 0.0,
                minus_plus: 0.0,
                minus_minus: 0.0,
            },
        )
        .expect("supercritical guard fixture should be valid in kernel domain");

        let interval_factor_grids = SufficientStatisticsIntervalFactors::try_new(
            vec![1.2, 1.2],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            Some(1.0),
        )
        .expect("interval factors should construct");

        CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            1.0,
            model_inputs,
            interval_factor_grids,
        )
        .expect(
            "raw-subcritical Hawkes inputs should remain valid when seasonality scales baseline",
        );
    }

    #[test]
    fn coarse_run_request_deserialization_rejects_missing_time_anchor_field() {
        let payload = json!({
            "schema_version": COARSE_SIMULATION_RUN_REQUEST_SCHEMA_VERSION,
            "seed": 17,
            "horizon_interval_count": 2,
            "interval_delta": 1.0,
            "model_inputs": serde_json::to_value(build_model_inputs())
                .expect("model inputs should serialize"),
            "interval_factor_grids": serde_json::to_value(build_interval_factor_grids(Some(1.0)))
                .expect("interval factors should serialize")
        });

        let error = serde_json::from_value::<CoarseSimulationRunRequest>(payload)
            .expect_err("missing time anchor must fail deserialization");

        assert!(error.to_string().contains("start_time_utc_nanos"));
    }

    fn build_valid_run_request() -> CoarseSimulationRunRequest {
        CoarseSimulationRunRequest::try_new(
            17,
            1_704_067_200_000_000_000,
            2,
            1.0,
            build_model_inputs(),
            build_interval_factor_grids(Some(1.0)),
        )
        .expect("valid coarse simulation request should construct")
    }

    fn build_initial_state() -> HawkesExcitationState {
        HawkesExcitationState {
            y_plus_plus: 0.2,
            y_plus_minus: 0.1,
            y_minus_plus: 0.1,
            y_minus_minus: 0.3,
        }
    }

    fn build_model_inputs() -> CoarseSimulationModelInputs {
        let hawkes_kernel_parameters = HawkesKernelParameters::try_new(
            0.8,
            1.6,
            HawkesKernelLoadings {
                plus_plus: 0.2,
                plus_minus: 0.1,
                minus_plus: 0.1,
                minus_minus: 0.25,
            },
            HawkesKernelLoadings {
                plus_plus: 0.08,
                plus_minus: 0.04,
                minus_plus: 0.05,
                minus_minus: 0.1,
            },
        )
        .expect("kernel parameters should construct");

        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            hawkes_kernel_parameters,
            GammaSeverityMarkConfiguration::try_new(1.8).expect("gamma marks should construct"),
            LongVarianceParameters::neutral(),
            MarketPressureParameters::no_anchor_impact(),
        )
        .expect("equation specification should construct");

        let sufficient_statistics_model_inputs =
            SufficientStatisticsModelInputs::try_new(SufficientStatisticsModelInputs {
                equation_specification,
                nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration::try_new(
                    2.3, -0.8, 0.7, 0.1, -0.2,
                )
                .expect("nig configuration should construct"),
                initial_excitation_state: build_initial_state(),
                initial_market_pressure_state:
                    janiform_model_kernels::market_pressure::MarketPressureState::neutral(),
                long_variance_parameters: LongVarianceParameters::neutral(),
                initial_long_variance_state: LongVarianceState::unit(),
                mean_reversion_fitted_parameters: MeanReversionFittedParameters::zero_kappa(),
                mean_reversion_runtime_interval_anchors:
                    MeanReversionRuntimeIntervalAnchors::try_new(
                        "explicit_runtime_interval_grid_alignment.v1",
                        0.0,
                        vec![0.0; 2],
                        vec![1.0; 2],
                    )
                    .expect("disabled mean-reversion runtime anchors should construct"),
                initial_mean_reversion_boundary_state: MeanReversionBoundaryState::try_new(
                    0.0, 0.0, 0.0,
                )
                .expect("disabled mean-reversion boundary state should construct"),
            })
            .expect("sufficient-statistics model inputs should construct");

        CoarseSimulationModelInputs::try_new(CoarseSimulationModelInputs {
            sufficient_statistics_model_inputs,
            news_process_configuration: build_news_process_configuration(),
            return_clock_exposure_parameters:
                janiform_model_kernels::return_clock::ReturnClockExposureParameters::neutral(),
            long_variance_parameters: LongVarianceParameters::neutral(),
            initial_long_variance_state: LongVarianceState::unit(),
            observed_price_tick_size: None,
            timestamp_batching_surface:
                janiform_model_kernels::timestamp_batching_surface::TimestampBatchingSurface::disabled(),
            calendar_registry_artifact: build_calendar_registry_artifact(),
            exchange_code: "XNYS".to_string(),
        })
        .expect("coarse model inputs should construct")
    }

    fn build_interval_factor_grids(
        interval_delta: Option<f64>,
    ) -> SufficientStatisticsIntervalFactors {
        SufficientStatisticsIntervalFactors::try_new(
            vec![1.1, 0.9],
            vec![0.12, 0.03],
            vec![0.08, 0.07],
            interval_delta,
        )
        .expect("interval factors should construct")
    }

    fn build_news_process_configuration() -> NewsProcessConfiguration {
        NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: vec![1_704_069_000_000_000_000],
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.2,
                    closed_baseline_intensity: 0.2,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(1.4)
                    .expect("gamma marks should construct"),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.7,
                return_jump_drift: 0.0,
                return_jump_volatility: 0.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.5,
                return_jump_drift: 0.0,
                return_jump_volatility: 0.0,
            },
        )
        .expect("news process configuration should construct")
    }

    fn build_calendar_registry_artifact() -> ExchangeCalendarRegistryArtifact {
        ExchangeCalendarRegistryArtifact {
            schema_version: "calendar.registry.v1".to_string(),
            artifact_date: "2024-01-02".to_string(),
            exchanges: vec![ExchangeCalendarArtifact {
                schema_version: "calendar.v1".to_string(),
                exchange_code: "XNYS".to_string(),
                exchange_name: "New York Stock Exchange".to_string(),
                timezone_name: Some("America/New_York".to_string()),
                rule_metadata: CalendarRuleMetadataArtifact {
                    holiday_calendar_code: "US_EQUITIES".to_string(),
                    early_close_rule_set: None,
                },
                regular_sessions: vec![
                    RegularSessionArtifact {
                        weekday: TradingWeekday::Monday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    },
                    RegularSessionArtifact {
                        weekday: TradingWeekday::Tuesday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    },
                    RegularSessionArtifact {
                        weekday: TradingWeekday::Wednesday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    },
                    RegularSessionArtifact {
                        weekday: TradingWeekday::Thursday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    },
                    RegularSessionArtifact {
                        weekday: TradingWeekday::Friday,
                        windows: vec![SessionWindowArtifact {
                            label: "regular".to_string(),
                            open_time: "09:30".to_string(),
                            close_time: "16:00".to_string(),
                        }],
                    },
                ],
                holiday_dates: vec![],
                early_close_sessions: vec![],
            }],
        }
    }
}
