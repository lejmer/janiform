use std::error::Error;
use std::fmt::{Display, Formatter};

/// Project a scalar exogenous-news loading onto the admissible non-negative
/// one-dimensional response family induced by the Hawkes target equations.
///
/// The local-measure recursion provides two target equations and one scalar
/// loading variable. The mathematically stable solution is the non-negative
/// least-squares projection of the target residual onto the unit-loading
/// response vector.
pub(crate) fn project_nonnegative_exogenous_news_loading(
    observed_response_plus: f64,
    observed_response_minus: f64,
    base_response_plus: f64,
    base_response_minus: f64,
    unit_loading_response_plus: f64,
    unit_loading_response_minus: f64,
) -> Result<f64, ExogenousNewsLoadingProjectionError> {
    let projection_input = [
        ("observed_response_plus", observed_response_plus),
        ("observed_response_minus", observed_response_minus),
        ("base_response_plus", base_response_plus),
        ("base_response_minus", base_response_minus),
        ("unit_loading_response_plus", unit_loading_response_plus),
        ("unit_loading_response_minus", unit_loading_response_minus),
    ];

    for (field_name, value) in projection_input {
        if !value.is_finite() {
            return Err(ExogenousNewsLoadingProjectionError::NonFiniteInput { field_name, value });
        }
    }

    let target_response_plus = observed_response_plus - base_response_plus;
    let target_response_minus = observed_response_minus - base_response_minus;
    if !target_response_plus.is_finite() || !target_response_minus.is_finite() {
        return Err(
            ExogenousNewsLoadingProjectionError::NonFiniteTargetResidual {
                target_response_plus,
                target_response_minus,
            },
        );
    }

    let projection_numerator = target_response_plus.mul_add(
        unit_loading_response_plus,
        target_response_minus * unit_loading_response_minus,
    );
    let projection_denominator = unit_loading_response_plus.mul_add(
        unit_loading_response_plus,
        unit_loading_response_minus * unit_loading_response_minus,
    );
    if !projection_numerator.is_finite() || !projection_denominator.is_finite() {
        return Err(
            ExogenousNewsLoadingProjectionError::NonFiniteProjectionMoments {
                projection_numerator,
                projection_denominator,
            },
        );
    }

    if projection_denominator == 0.0 {
        return Ok(0.0);
    }

    let projected_loading = (projection_numerator / projection_denominator).max(0.0);
    if !projected_loading.is_finite() {
        return Err(
            ExogenousNewsLoadingProjectionError::NonFiniteProjectedLoading {
                projection_numerator,
                projection_denominator,
                projected_loading,
            },
        );
    }

    Ok(projected_loading)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ExogenousNewsLoadingProjectionError {
    NonFiniteInput {
        field_name: &'static str,
        value: f64,
    },
    NonFiniteTargetResidual {
        target_response_plus: f64,
        target_response_minus: f64,
    },
    NonFiniteProjectionMoments {
        projection_numerator: f64,
        projection_denominator: f64,
    },
    NonFiniteProjectedLoading {
        projection_numerator: f64,
        projection_denominator: f64,
        projected_loading: f64,
    },
}

impl Display for ExogenousNewsLoadingProjectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteInput { field_name, value } => write!(
                formatter,
                "exogenous-news loading projection requires finite {field_name}, found {value}"
            ),
            Self::NonFiniteTargetResidual {
                target_response_plus,
                target_response_minus,
            } => write!(
                formatter,
                "exogenous-news loading projection produced non-finite target residuals: plus={target_response_plus}, minus={target_response_minus}"
            ),
            Self::NonFiniteProjectionMoments {
                projection_numerator,
                projection_denominator,
            } => write!(
                formatter,
                "exogenous-news loading projection produced non-finite projection moments: numerator={projection_numerator}, denominator={projection_denominator}"
            ),
            Self::NonFiniteProjectedLoading {
                projection_numerator,
                projection_denominator,
                projected_loading,
            } => write!(
                formatter,
                "exogenous-news loading projection produced non-finite loading from numerator={projection_numerator} and denominator={projection_denominator}: {projected_loading}"
            ),
        }
    }
}

impl Error for ExogenousNewsLoadingProjectionError {}
