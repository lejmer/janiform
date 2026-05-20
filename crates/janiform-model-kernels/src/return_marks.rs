use std::error::Error;
use std::fmt::{Display, Formatter};

pub const RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT: usize = 24 * 7;

#[derive(Debug, Clone, PartialEq)]
pub enum ReturnMarkHourOfWeekFactorsError {
    FactorCountMismatch {
        expected_factor_count: usize,
        observed_factor_count: usize,
    },
    InvalidFactor {
        factor_index: usize,
        value: f64,
    },
}

impl Display for ReturnMarkHourOfWeekFactorsError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FactorCountMismatch {
                expected_factor_count,
                observed_factor_count,
            } => write!(
                formatter,
                "return_mark_hour_of_week_factors length must be {expected_factor_count}, found {observed_factor_count}"
            ),
            Self::InvalidFactor {
                factor_index,
                value,
            } => write!(
                formatter,
                "return_mark_hour_of_week_factors[{factor_index}] must be finite and strictly positive, found {value}"
            ),
        }
    }
}

impl Error for ReturnMarkHourOfWeekFactorsError {}

pub fn validate_return_mark_hour_of_week_factors(
    return_mark_hour_of_week_factors: &[f64],
) -> Result<(), ReturnMarkHourOfWeekFactorsError> {
    if return_mark_hour_of_week_factors.len() != RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT {
        return Err(ReturnMarkHourOfWeekFactorsError::FactorCountMismatch {
            expected_factor_count: RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT,
            observed_factor_count: return_mark_hour_of_week_factors.len(),
        });
    }

    for (factor_index, &factor) in return_mark_hour_of_week_factors.iter().enumerate() {
        if !factor.is_finite() || factor <= 0.0 {
            return Err(ReturnMarkHourOfWeekFactorsError::InvalidFactor {
                factor_index,
                value: factor,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT, ReturnMarkHourOfWeekFactorsError,
        validate_return_mark_hour_of_week_factors,
    };

    #[test]
    fn return_mark_hour_of_week_factors_require_exact_weekly_hour_count() {
        let factors = vec![1.0; RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT - 1];

        assert_eq!(
            validate_return_mark_hour_of_week_factors(factors.as_slice()),
            Err(ReturnMarkHourOfWeekFactorsError::FactorCountMismatch {
                expected_factor_count: RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT,
                observed_factor_count: RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT - 1,
            })
        );
    }

    #[test]
    fn return_mark_hour_of_week_factors_reject_non_positive_values() {
        let mut factors = vec![1.0; RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT];
        factors[7] = 0.0;

        assert_eq!(
            validate_return_mark_hour_of_week_factors(factors.as_slice()),
            Err(ReturnMarkHourOfWeekFactorsError::InvalidFactor {
                factor_index: 7,
                value: 0.0,
            })
        );
    }

    #[test]
    fn return_mark_hour_of_week_factors_accept_positive_finite_week_profile() {
        let factors = vec![1.0; RETURN_MARK_HOUR_OF_WEEK_FACTOR_COUNT];

        assert_eq!(
            validate_return_mark_hour_of_week_factors(factors.as_slice()),
            Ok(())
        );
    }
}
