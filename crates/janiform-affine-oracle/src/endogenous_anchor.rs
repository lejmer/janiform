use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::affine_oracle::{
    ExponentialAffineTransformEquationSpecification,
    ExponentialAffineTransformEquationSpecificationError,
    ExponentialAffineTransformNewsIntensityInput,
    ExponentialAffineTransformNewsIntensityInputError,
};
use crate::sufficient_statistics::{
    SufficientStatisticsExpectedIntervalStatistics, SufficientStatisticsIntervalStatisticsError,
    SufficientStatisticsTransformSliceSamplingError, SufficientStatisticsTransformSliceSolveError,
};
use janiform_foundations::linear_algebra::solve_two_by_two_linear_source_system;
use janiform_model_kernels::hawkes::HawkesExcitationState;
use janiform_model_kernels::mean_reversion::MeanReversionRuntimeIntervalAnchors;
use janiform_model_kernels::news::{NewsProcessConfiguration, NewsProcessConfigurationError};
use janiform_model_kernels::nig::{
    NigIncrementSamplingError, NigSharedPlusShiftConfiguration,
    NigSharedPlusShiftConfigurationError,
};
use nalgebra::{Matrix2, Vector2};

/// One interval's structural factors used by endogenous-anchor recursion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EndogenousAnchorIntervalFactors {
    pub s_h: f64,
    pub known_nu_h: f64,
    pub surprise_nu_h: f64,
    pub delta: f64,
}

impl EndogenousAnchorIntervalFactors {
    pub fn try_new(
        s_h: f64,
        known_nu_h: f64,
        surprise_nu_h: f64,
        delta: f64,
    ) -> Result<Self, EndogenousAnchorMaterializationError> {
        let factors = Self {
            s_h,
            known_nu_h,
            surprise_nu_h,
            delta,
        };

        factors.validate()?;

        Ok(factors)
    }

    pub fn validate(self) -> Result<(), EndogenousAnchorMaterializationError> {
        if !self.s_h.is_finite() || self.s_h < 0.0 {
            return Err(
                EndogenousAnchorMaterializationError::InvalidSeasonalityFactor { value: self.s_h },
            );
        }

        ExponentialAffineTransformNewsIntensityInput::try_new(self.known_nu_h, self.surprise_nu_h)
            .map_err(
                |source| EndogenousAnchorMaterializationError::InvalidNewsIntensity { source },
            )?;

        if !self.delta.is_finite() || self.delta <= 0.0 {
            return Err(EndogenousAnchorMaterializationError::InvalidIntervalDelta {
                value: self.delta,
            });
        }

        Ok(())
    }

    pub fn news_intensity_input(
        self,
    ) -> Result<ExponentialAffineTransformNewsIntensityInput, EndogenousAnchorMaterializationError>
    {
        ExponentialAffineTransformNewsIntensityInput::try_new(self.known_nu_h, self.surprise_nu_h)
            .map_err(|source| EndogenousAnchorMaterializationError::InvalidNewsIntensity { source })
    }

    pub fn expected_known_arrival_count(self) -> f64 {
        self.known_nu_h * self.delta
    }

    pub fn expected_surprise_arrival_count(self) -> f64 {
        self.surprise_nu_h * self.delta
    }
}

/// One interval's endogenous-anchor recursion outputs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EndogenousAnchorIntervalExpectation {
    pub interval_index: usize,
    pub factors: EndogenousAnchorIntervalFactors,
    pub structural_state_before: HawkesExcitationState,
    pub structural_state_after: HawkesExcitationState,
    pub expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    pub expected_direct_news_return_increment: f64,
    pub core_expected_increment: f64,
    pub anchor_value_after_interval: f64,
}

/// Shared endogenous-anchor materialization output.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EndogenousAnchorMaterialization {
    pub runtime_interval_anchors: MeanReversionRuntimeIntervalAnchors,
    pub expected_hawkes_state_path: Vec<HawkesExcitationState>,
    pub interval_expectations: Vec<EndogenousAnchorIntervalExpectation>,
}

/// One interval's exact first-moment projection for endogenous Hawkes statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EndogenousAnchorFirstMomentIntervalProjection {
    pub expected_statistics: SufficientStatisticsExpectedIntervalStatistics,
    pub structural_state_after: HawkesExcitationState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EndogenousAnchorMaterializationError {
    InvalidSeasonalityFactor {
        value: f64,
    },
    InvalidNewsIntensity {
        source: ExponentialAffineTransformNewsIntensityInputError,
    },
    InvalidIntervalDelta {
        value: f64,
    },
    InvalidStartAnchorValue {
        value: f64,
    },
    InvalidEquationSpecification {
        source: ExponentialAffineTransformEquationSpecificationError,
    },
    InvalidNewsProcessConfiguration {
        source: NewsProcessConfigurationError,
    },
    InvalidNigSharedPlusShiftConfiguration {
        source: NigSharedPlusShiftConfigurationError,
    },
    InvalidExpectedIntervalStatistics {
        interval_index: usize,
        source: SufficientStatisticsIntervalStatisticsError,
    },
    TransformSliceSolveFailure {
        interval_index: usize,
        source: SufficientStatisticsTransformSliceSolveError,
    },
    TransformSliceExpectationFailure {
        interval_index: usize,
        source: Box<SufficientStatisticsTransformSliceSamplingError>,
    },
    BuySideExpectationFailure {
        interval_index: usize,
        source: NigIncrementSamplingError,
    },
    SellSideExpectationFailure {
        interval_index: usize,
        source: NigIncrementSamplingError,
    },
    NonFiniteCoreExpectedIncrement {
        interval_index: usize,
        value: f64,
    },
    InvalidRuntimeIntervalAnchors {
        error_message: String,
    },
    InvalidExpectedHawkesState {
        interval_index: usize,
        field: &'static str,
        value: f64,
    },
}

impl Display for EndogenousAnchorMaterializationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSeasonalityFactor { value } => write!(
                formatter,
                "endogenous-anchor materialization requires finite non-negative s_h, found {value}"
            ),
            Self::InvalidNewsIntensity { source } => write!(
                formatter,
                "endogenous-anchor materialization received invalid news intensity: {source}"
            ),
            Self::InvalidIntervalDelta { value } => write!(
                formatter,
                "endogenous-anchor materialization requires finite strictly positive delta, found {value}"
            ),
            Self::InvalidStartAnchorValue { value } => write!(
                formatter,
                "endogenous-anchor materialization requires finite start_anchor_value, found {value}"
            ),
            Self::InvalidEquationSpecification { source } => write!(
                formatter,
                "endogenous-anchor equation specification is invalid: {source}"
            ),
            Self::InvalidNewsProcessConfiguration { source } => write!(
                formatter,
                "endogenous-anchor news process configuration is invalid: {source}"
            ),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => write!(
                formatter,
                "endogenous-anchor NIG shared-plus-shift configuration is invalid: {source}"
            ),
            Self::InvalidExpectedIntervalStatistics {
                interval_index,
                source,
            } => write!(
                formatter,
                "endogenous-anchor first-moment interval statistics are invalid at interval {interval_index}: {source}"
            ),
            Self::TransformSliceSolveFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "endogenous-anchor transform-slice solve failed at interval {interval_index}: {source}"
            ),
            Self::TransformSliceExpectationFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "endogenous-anchor expected sufficient-statistics extraction failed at interval {interval_index}: {source}"
            ),
            Self::BuySideExpectationFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "endogenous-anchor buy-side expected increment failed at interval {interval_index}: {source}"
            ),
            Self::SellSideExpectationFailure {
                interval_index,
                source,
            } => write!(
                formatter,
                "endogenous-anchor sell-side expected increment failed at interval {interval_index}: {source}"
            ),
            Self::NonFiniteCoreExpectedIncrement {
                interval_index,
                value,
            } => write!(
                formatter,
                "endogenous-anchor core expected increment must be finite at interval {interval_index}, found {value}"
            ),
            Self::InvalidRuntimeIntervalAnchors { error_message } => write!(
                formatter,
                "endogenous-anchor materialization produced invalid runtime interval anchors: {error_message}"
            ),
            Self::InvalidExpectedHawkesState {
                interval_index,
                field,
                value,
            } => write!(
                formatter,
                "endogenous-anchor expected Hawkes state field {field} is invalid at interval {interval_index}: {value}"
            ),
        }
    }
}

impl Error for EndogenousAnchorMaterializationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidNewsIntensity { source } => Some(source),
            Self::InvalidEquationSpecification { source } => Some(source),
            Self::InvalidNewsProcessConfiguration { source } => Some(source),
            Self::InvalidNigSharedPlusShiftConfiguration { source } => Some(source),
            Self::InvalidExpectedIntervalStatistics { source, .. } => Some(source),
            Self::TransformSliceSolveFailure { source, .. } => Some(source),
            Self::TransformSliceExpectationFailure { source, .. } => Some(source.as_ref()),
            Self::BuySideExpectationFailure { source, .. } => Some(source),
            Self::SellSideExpectationFailure { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Materialize interval-aligned endogenous anchor values from structural recursion under `P`.
pub fn materialize_endogenous_anchor_path(
    interval_alignment_policy: impl Into<String>,
    start_anchor_value: f64,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    nig_shared_plus_shift_configuration: NigSharedPlusShiftConfiguration,
    news_process_configuration: &NewsProcessConfiguration,
    initial_excitation_state: HawkesExcitationState,
    interval_factors: &[EndogenousAnchorIntervalFactors],
) -> Result<EndogenousAnchorMaterialization, EndogenousAnchorMaterializationError> {
    if !start_anchor_value.is_finite() {
        return Err(
            EndogenousAnchorMaterializationError::InvalidStartAnchorValue {
                value: start_anchor_value,
            },
        );
    }

    equation_specification.validate().map_err(|source| {
        EndogenousAnchorMaterializationError::InvalidEquationSpecification { source }
    })?;

    news_process_configuration.validate().map_err(|source| {
        EndogenousAnchorMaterializationError::InvalidNewsProcessConfiguration { source }
    })?;

    let driver_parameters = nig_shared_plus_shift_configuration
        .derive_driver_parameters()
        .map_err(|source| {
            EndogenousAnchorMaterializationError::InvalidNigSharedPlusShiftConfiguration { source }
        })?;

    let expected_severity_mark = news_process_configuration
        .shared_mark_law
        .gamma_severity_marks
        .expected_severity_mark()
        .map_err(|source| {
            EndogenousAnchorMaterializationError::InvalidNewsProcessConfiguration {
                source: NewsProcessConfigurationError::InvalidGammaSeverityMarkConfiguration {
                    source,
                },
            }
        })?;

    let mut expected_hawkes_state_path = Vec::with_capacity(interval_factors.len() + 1);

    expected_hawkes_state_path.push(initial_excitation_state);

    let mut end_anchor_values = Vec::with_capacity(interval_factors.len());
    let mut interval_deltas = Vec::with_capacity(interval_factors.len());
    let mut interval_expectations = Vec::with_capacity(interval_factors.len());

    let mut current_anchor_value = start_anchor_value;

    for (interval_index, factors) in interval_factors.iter().copied().enumerate() {
        factors.validate()?;

        let structural_state_before = *expected_hawkes_state_path
            .last()
            .unwrap_or(&initial_excitation_state);

        let expected_activity_loading_rate = expected_scaled_news_loading_rate(
            factors,
            news_process_configuration
                .known_component_mark_scales
                .activity_coupling_scale,
            news_process_configuration
                .surprise_component_mark_scales
                .activity_coupling_scale,
            expected_severity_mark,
        );

        let first_moment_projection = expected_interval_projection_from_first_moment_dynamics(
            interval_index,
            equation_specification,
            factors,
            structural_state_before,
            expected_activity_loading_rate,
        )?;

        let expected_statistics = first_moment_projection.expected_statistics;

        let expected_buy_increment = driver_parameters
            .buy_side_parameters
            .expected_mean_compensated_increment(expected_statistics.expected_a_plus)
            .map_err(
                |source| EndogenousAnchorMaterializationError::BuySideExpectationFailure {
                    interval_index,
                    source,
                },
            )?;

        let expected_sell_increment = driver_parameters
            .sell_side_parameters
            .expected_mean_compensated_increment(expected_statistics.expected_a_minus)
            .map_err(
                |source| EndogenousAnchorMaterializationError::SellSideExpectationFailure {
                    interval_index,
                    source,
                },
            )?;

        let expected_direct_news_return_increment = expected_scaled_news_arrival_contribution(
            factors,
            news_process_configuration
                .known_component_mark_scales
                .return_jump_drift,
            news_process_configuration
                .surprise_component_mark_scales
                .return_jump_drift,
        );

        let core_expected_increment = expected_buy_increment - expected_sell_increment
            + expected_direct_news_return_increment;
        if !core_expected_increment.is_finite() {
            return Err(
                EndogenousAnchorMaterializationError::NonFiniteCoreExpectedIncrement {
                    interval_index,
                    value: core_expected_increment,
                },
            );
        }

        current_anchor_value += core_expected_increment;

        end_anchor_values.push(current_anchor_value);
        interval_deltas.push(factors.delta);

        let structural_state_after = first_moment_projection.structural_state_after;

        expected_hawkes_state_path.push(structural_state_after);
        interval_expectations.push(EndogenousAnchorIntervalExpectation {
            interval_index,
            factors,
            structural_state_before,
            structural_state_after,
            expected_statistics,
            expected_direct_news_return_increment,
            core_expected_increment,
            anchor_value_after_interval: current_anchor_value,
        });
    }

    let runtime_interval_anchors = MeanReversionRuntimeIntervalAnchors::try_new(
        interval_alignment_policy,
        start_anchor_value,
        0.0,
        end_anchor_values,
        vec![0.0; interval_deltas.len()],
        interval_deltas,
    )
    .map_err(
        |source| EndogenousAnchorMaterializationError::InvalidRuntimeIntervalAnchors {
            error_message: source.to_string(),
        },
    )?;

    Ok(EndogenousAnchorMaterialization {
        runtime_interval_anchors,
        expected_hawkes_state_path,
        interval_expectations,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HawkesTargetSumFirstMoment {
    target_plus_after: f64,
    target_minus_after: f64,
    integrated_target_plus: f64,
    integrated_target_minus: f64,
}

/// Derive sufficient-statistics first moments from structural dynamics without
/// solving transform slices.
pub fn derive_expected_interval_statistics_from_first_moment_dynamics(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    expected_severity_mark: f64,
    factors: EndogenousAnchorIntervalFactors,
    structural_state_before: HawkesExcitationState,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError,
> {
    expected_interval_statistics_from_first_moment_dynamics(
        equation_specification,
        expected_severity_mark,
        factors,
        structural_state_before,
    )
}

/// Derive one interval's exact first-moment statistics and terminal target state.
pub fn derive_expected_interval_projection_from_first_moment_dynamics(
    interval_index: usize,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    expected_severity_mark: f64,
    factors: EndogenousAnchorIntervalFactors,
    structural_state_before: HawkesExcitationState,
) -> Result<EndogenousAnchorFirstMomentIntervalProjection, EndogenousAnchorMaterializationError> {
    let expected_activity_loading_rate =
        (factors.known_nu_h + factors.surprise_nu_h) * expected_severity_mark;

    expected_interval_projection_from_first_moment_dynamics(
        interval_index,
        equation_specification,
        factors,
        structural_state_before,
        expected_activity_loading_rate,
    )
}

fn expected_interval_projection_from_first_moment_dynamics(
    interval_index: usize,
    equation_specification: ExponentialAffineTransformEquationSpecification,
    factors: EndogenousAnchorIntervalFactors,
    structural_state_before: HawkesExcitationState,
    expected_activity_loading_rate: f64,
) -> Result<EndogenousAnchorFirstMomentIntervalProjection, EndogenousAnchorMaterializationError> {
    let target_sum_first_moment = integrate_expected_target_sums(
        equation_specification,
        factors,
        structural_state_before,
        expected_activity_loading_rate,
    );

    let expected_statistics = expected_interval_statistics_from_target_sum_first_moment(
        equation_specification,
        factors,
        target_sum_first_moment,
    )
    .map_err(|source| {
        EndogenousAnchorMaterializationError::InvalidExpectedIntervalStatistics {
            interval_index,
            source,
        }
    })?;

    let structural_state_after =
        expected_hawkes_state_after_first_moment_interval(target_sum_first_moment).map_err(
            |(field, value)| EndogenousAnchorMaterializationError::InvalidExpectedHawkesState {
                interval_index,
                field,
                value,
            },
        )?;

    Ok(EndogenousAnchorFirstMomentIntervalProjection {
        expected_statistics,
        structural_state_after,
    })
}

fn expected_interval_statistics_from_first_moment_dynamics(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    expected_severity_mark: f64,
    factors: EndogenousAnchorIntervalFactors,
    structural_state_before: HawkesExcitationState,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError,
> {
    let target_sum_first_moment = integrate_expected_target_sums(
        equation_specification,
        factors,
        structural_state_before,
        (factors.known_nu_h + factors.surprise_nu_h) * expected_severity_mark,
    );

    expected_interval_statistics_from_target_sum_first_moment(
        equation_specification,
        factors,
        target_sum_first_moment,
    )
}

fn expected_interval_statistics_from_target_sum_first_moment(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    factors: EndogenousAnchorIntervalFactors,
    target_sum_first_moment: HawkesTargetSumFirstMoment,
) -> Result<
    SufficientStatisticsExpectedIntervalStatistics,
    SufficientStatisticsIntervalStatisticsError,
> {
    let baseline_expected_business_time =
        equation_specification.hawkes_kernel_parameters.lambda_bar * factors.s_h * factors.delta;

    SufficientStatisticsExpectedIntervalStatistics::try_new(
        baseline_expected_business_time,
        target_sum_first_moment.integrated_target_plus,
        baseline_expected_business_time,
        target_sum_first_moment.integrated_target_minus,
        baseline_expected_business_time + target_sum_first_moment.integrated_target_plus,
        baseline_expected_business_time + target_sum_first_moment.integrated_target_minus,
    )
}

fn integrate_expected_target_sums(
    equation_specification: ExponentialAffineTransformEquationSpecification,
    factors: EndogenousAnchorIntervalFactors,
    structural_state_before: HawkesExcitationState,
    expected_activity_loading_rate: f64,
) -> HawkesTargetSumFirstMoment {
    let kernel_parameters = equation_specification.hawkes_kernel_parameters;

    let alpha = kernel_parameters.alpha;
    let rho = kernel_parameters.rho;

    let target_plus_before = structural_state_before.target_plus_sum();
    let target_minus_before = structural_state_before.target_minus_sum();

    let system_matrix = Matrix2::new(
        -kernel_parameters.beta + alpha.plus_plus,
        alpha.plus_minus,
        alpha.minus_plus,
        -kernel_parameters.beta + alpha.minus_minus,
    );
    let initial_state = Vector2::new(target_plus_before, target_minus_before);

    let source_plus =
        factors.s_h * kernel_parameters.lambda_bar * (alpha.plus_plus + alpha.plus_minus)
            + expected_activity_loading_rate * (rho.plus_plus + rho.plus_minus);
    let source_minus =
        factors.s_h * kernel_parameters.lambda_bar * (alpha.minus_plus + alpha.minus_minus)
            + expected_activity_loading_rate * (rho.minus_plus + rho.minus_minus);

    let (target_after, integrated_target) = solve_two_by_two_linear_source_system(
        system_matrix,
        initial_state,
        Vector2::new(source_plus, source_minus),
        factors.delta,
    );

    HawkesTargetSumFirstMoment {
        target_plus_after: target_after[0],
        target_minus_after: target_after[1],
        integrated_target_plus: integrated_target[0],
        integrated_target_minus: integrated_target[1],
    }
}

fn expected_hawkes_state_after_first_moment_interval(
    target_sum_first_moment: HawkesTargetSumFirstMoment,
) -> Result<HawkesExcitationState, (&'static str, f64)> {
    let target_plus_after = target_sum_first_moment.target_plus_after;
    if !target_plus_after.is_finite() || target_plus_after < 0.0 {
        return Err(("target_plus_after", target_plus_after));
    }

    let target_minus_after = target_sum_first_moment.target_minus_after;
    if !target_minus_after.is_finite() || target_minus_after < 0.0 {
        return Err(("target_minus_after", target_minus_after));
    }

    // The first-moment dynamics are closed in side-level target sums, so the
    // aggregate target is carried in a canonical component for the next step.
    Ok(HawkesExcitationState::from_target_sums(
        target_plus_after,
        target_minus_after,
    ))
}

fn expected_scaled_news_loading_rate(
    factors: EndogenousAnchorIntervalFactors,
    known_component_scale: f64,
    surprise_component_scale: f64,
    expected_mark: f64,
) -> f64 {
    let expected_known_loading_rate = factors.known_nu_h * known_component_scale * expected_mark;

    let expected_surprise_loading_rate =
        factors.surprise_nu_h * surprise_component_scale * expected_mark;

    expected_known_loading_rate + expected_surprise_loading_rate
}

fn expected_scaled_news_arrival_contribution(
    factors: EndogenousAnchorIntervalFactors,
    known_component_expected_return_jump: f64,
    surprise_component_expected_return_jump: f64,
) -> f64 {
    let expected_known_contribution =
        factors.expected_known_arrival_count() * known_component_expected_return_jump;

    let expected_surprise_contribution =
        factors.expected_surprise_arrival_count() * surprise_component_expected_return_jump;

    expected_known_contribution + expected_surprise_contribution
}

#[cfg(test)]
fn zero_hawkes_excitation_state() -> HawkesExcitationState {
    HawkesExcitationState {
        y_plus_plus: 0.0,
        y_plus_minus: 0.0,
        y_minus_plus: 0.0,
        y_minus_minus: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EndogenousAnchorIntervalFactors, materialize_endogenous_anchor_path,
        zero_hawkes_excitation_state,
    };
    use crate::affine_oracle::ExponentialAffineTransformEquationSpecification;
    use janiform_model_kernels::hawkes::{HawkesKernelLoadings, HawkesKernelParameters};
    use janiform_model_kernels::long_variance::LongVarianceParameters;
    use janiform_model_kernels::news::{
        GammaSeverityMarkConfiguration, KnownNewsArrivalConfiguration, NewsArrivalConfiguration,
        NewsComponentMarkScaleConfiguration, NewsProcessConfiguration,
        SharedNewsMarkLawConfiguration, SurpriseNewsArrivalConfiguration,
    };
    use janiform_model_kernels::nig::NigSharedPlusShiftConfiguration;

    #[test]
    fn endogenous_anchor_path_stays_flat_without_structural_or_news_flow() {
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                4.0,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.1,
                    plus_minus: 0.05,
                    minus_plus: 0.04,
                    minus_minus: 0.08,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.2,
                    plus_minus: 0.1,
                    minus_plus: 0.1,
                    minus_minus: 0.2,
                },
            )
            .expect("hawkes parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(2.0)
                .expect("gamma configuration should construct"),
            LongVarianceParameters::neutral(),
        )
        .expect("equation specification should construct");

        let nig_configuration = NigSharedPlusShiftConfiguration::try_new(2.5, 0.0, 0.8, 0.0, -0.2)
            .expect("nig configuration should construct");

        let news_process_configuration = NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.0,
                    closed_baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(2.0)
                    .expect("gamma configuration should construct"),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_drift: 0.0,
                return_jump_volatility: 0.0,
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 1.0,
                return_jump_drift: 0.0,
                return_jump_volatility: 0.0,
            },
        )
        .expect("news process configuration should construct");

        let materialization = materialize_endogenous_anchor_path(
            "explicit_runtime_interval_grid_alignment.v1",
            1.25,
            equation_specification,
            nig_configuration,
            &news_process_configuration,
            zero_hawkes_excitation_state(),
            &[
                EndogenousAnchorIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                    .expect("interval factors should construct"),
                EndogenousAnchorIntervalFactors::try_new(0.0, 0.0, 0.0, 1.0)
                    .expect("interval factors should construct"),
            ],
        )
        .expect("endogenous anchor materialization should succeed");

        assert_eq!(
            materialization.runtime_interval_anchors.end_anchor_values,
            vec![1.25, 1.25]
        );
    }

    #[test]
    fn endogenous_anchor_path_accumulates_direct_news_return_expectation() {
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                4.0,
                1.5,
                HawkesKernelLoadings {
                    plus_plus: 0.1,
                    plus_minus: 0.05,
                    minus_plus: 0.04,
                    minus_minus: 0.08,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.2,
                    plus_minus: 0.1,
                    minus_plus: 0.1,
                    minus_minus: 0.2,
                },
            )
            .expect("hawkes parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(4.0)
                .expect("gamma configuration should construct"),
            LongVarianceParameters::neutral(),
        )
        .expect("equation specification should construct");

        let nig_configuration = NigSharedPlusShiftConfiguration::try_new(2.5, 0.0, 0.8, 0.0, -0.2)
            .expect("nig configuration should construct");

        let news_process_configuration = NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.0,
                    closed_baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(4.0)
                    .expect("gamma configuration should construct"),
            },
            NewsComponentMarkScaleConfiguration {
                activity_coupling_scale: 0.5,
                return_jump_drift: 1.2,
                return_jump_volatility: 0.0,
            },
            NewsComponentMarkScaleConfiguration::neutral(),
        )
        .expect("news process configuration should construct");

        let interval_factors = EndogenousAnchorIntervalFactors::try_new(0.0, 2.0, 0.0, 0.5)
            .expect("interval factors should construct");

        let expected_anchor_increment = interval_factors.expected_known_arrival_count() * 1.2;

        let materialization = materialize_endogenous_anchor_path(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            equation_specification,
            nig_configuration,
            &news_process_configuration,
            zero_hawkes_excitation_state(),
            &[interval_factors],
        )
        .expect("endogenous anchor materialization should succeed");

        assert!(
            (materialization.runtime_interval_anchors.end_anchor_values[0]
                - expected_anchor_increment)
                .abs()
                <= 1.0e-12
        );
    }

    #[test]
    fn endogenous_anchor_uses_seasonality_for_baseline_only() {
        let equation_specification = ExponentialAffineTransformEquationSpecification::try_new(
            HawkesKernelParameters::try_new(
                4784.844063542383,
                1758.535955776643,
                HawkesKernelLoadings {
                    plus_plus: 1007.730876687951,
                    plus_minus: 111.970097409772,
                    minus_plus: 112.136913128064,
                    minus_minus: 1009.23221815258,
                },
                HawkesKernelLoadings {
                    plus_plus: 0.0,
                    plus_minus: 0.0,
                    minus_plus: 0.0,
                    minus_minus: 0.0,
                },
            )
            .expect("hawkes parameters should construct"),
            GammaSeverityMarkConfiguration::try_new(2.0)
                .expect("gamma configuration should construct"),
            LongVarianceParameters::neutral(),
        )
        .expect("equation specification should construct");

        let nig_configuration = NigSharedPlusShiftConfiguration::try_new(2.5, 0.0, 0.8, 0.0, -0.2)
            .expect("nig configuration should construct");

        let news_process_configuration = NewsProcessConfiguration::try_new(
            NewsArrivalConfiguration {
                known: KnownNewsArrivalConfiguration {
                    scheduled_event_timestamps_utc_nanos: Vec::new(),
                },
                surprise: SurpriseNewsArrivalConfiguration {
                    open_baseline_intensity: 0.0,
                    closed_baseline_intensity: 0.0,
                },
            },
            SharedNewsMarkLawConfiguration {
                gamma_severity_marks: GammaSeverityMarkConfiguration::try_new(2.0)
                    .expect("gamma configuration should construct"),
            },
            NewsComponentMarkScaleConfiguration::neutral(),
            NewsComponentMarkScaleConfiguration::neutral(),
        )
        .expect("news process configuration should construct");

        let materialization = materialize_endogenous_anchor_path(
            "explicit_runtime_interval_grid_alignment.v1",
            0.0,
            equation_specification,
            nig_configuration,
            &news_process_configuration,
            zero_hawkes_excitation_state(),
            &[
                EndogenousAnchorIntervalFactors::try_new(1.571_170_932_355_796_6, 0.0, 0.0, 1.0)
                    .expect("interval factors should construct"),
            ],
        )
        .expect("endogenous anchor materialization should succeed");

        assert!(
            materialization.interval_expectations[0]
                .expected_statistics
                .expected_a_plus
                .is_finite()
        );
        assert!(
            materialization.interval_expectations[0]
                .expected_statistics
                .expected_a_minus
                .is_finite()
        );
    }
}
