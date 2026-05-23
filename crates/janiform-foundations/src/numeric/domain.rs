use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for FiniteNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite, found {}",
            self.field, self.value
        )
    }
}

impl Error for FiniteNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositiveNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for PositiveNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and strictly positive, found {}",
            self.field, self.value
        )
    }
}

impl Error for PositiveNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NonNegativeNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for NonNegativeNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and non-negative, found {}",
            self.field, self.value
        )
    }
}

impl Error for NonNegativeNumberValidationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NegativeNumberValidationError {
    pub field: &'static str,
    pub value: f64,
}

impl Display for NegativeNumberValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} must be finite and strictly negative, found {}",
            self.field, self.value
        )
    }
}

impl Error for NegativeNumberValidationError {}

/// Return the smallest strictly positive normal `f64` value.
pub fn strictly_positive_machine_value() -> f64 {
    f64::MIN_POSITIVE
}

/// Return the widest finite log span representable by strictly positive `f64` values.
pub fn representable_positive_f64_log_span() -> f64 {
    f64::MAX.ln() - f64::MIN_POSITIVE.ln()
}

/// Validate that a numeric field is finite.
pub fn validate_finite_number(
    value: f64,
    field: &'static str,
) -> Result<(), FiniteNumberValidationError> {
    if !value.is_finite() {
        return Err(FiniteNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and strictly positive.
pub fn validate_positive_number(
    value: f64,
    field: &'static str,
) -> Result<(), PositiveNumberValidationError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(PositiveNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and non-negative.
pub fn validate_non_negative_number(
    value: f64,
    field: &'static str,
) -> Result<(), NonNegativeNumberValidationError> {
    if !value.is_finite() || value < 0.0 {
        return Err(NonNegativeNumberValidationError { field, value });
    }

    Ok(())
}

/// Validate that a numeric field is finite and strictly negative.
pub fn validate_negative_number(
    value: f64,
    field: &'static str,
) -> Result<(), NegativeNumberValidationError> {
    if !value.is_finite() || value >= 0.0 {
        return Err(NegativeNumberValidationError { field, value });
    }

    Ok(())
}

/// Clamp a finite value into the closed unit interval.
pub fn clamp_to_unit_interval(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

/// Clamp a finite value into the open unit interval using machine-adjacent bounds.
pub fn clamp_to_open_unit_interval(value: f64) -> f64 {
    value.clamp(0.0f64.next_up(), 1.0f64.next_down())
}

/// Compute the fraction of covered intervals within a horizon.
pub fn coverage_ratio(covered_interval_count: usize, horizon_interval_count: usize) -> f64 {
    if horizon_interval_count == 0 {
        return 0.0;
    }

    covered_interval_count as f64 / horizon_interval_count as f64
}

/// Compute the logistic sigmoid in a numerically stable piecewise form.
pub fn logistic(value: f64) -> f64 {
    if value >= 0.0 {
        let exp_neg = (-value).exp();
        1.0 / (1.0 + exp_neg)
    } else {
        let exp_pos = value.exp();
        exp_pos / (1.0 + exp_pos)
    }
}

/// Round to a fixed number of decimal places.
pub fn round_to(value: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    (value * scale).round() / scale
}

/// Compute the integer square root by flooring the real square root.
pub fn integer_square_root(value: usize) -> usize {
    (value as f64).sqrt().floor() as usize
}

/// Compute the integer square root by ceiling the real square root.
pub fn ceil_integer_square_root(value: usize) -> usize {
    let root = integer_square_root(value);
    if root.saturating_mul(root) == value {
        root
    } else {
        root + 1
    }
}

/// Round while preserving a strictly positive domain after quantization.
pub fn round_positive_to_domain(value: f64, digits: i32) -> f64 {
    round_to(value.max(strictly_positive_machine_value()), digits)
        .max(strictly_positive_machine_value())
}

/// Use a data-normalized smoothing pseudocount rather than a fixed constant.
pub fn normalized_pseudocount(event_count: usize) -> f64 {
    1.0 / event_count.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::{
        ceil_integer_square_root, clamp_to_open_unit_interval, clamp_to_unit_interval,
        coverage_ratio, integer_square_root, logistic, normalized_pseudocount,
        representable_positive_f64_log_span, round_positive_to_domain, round_to,
        strictly_positive_machine_value, validate_finite_number, validate_negative_number,
        validate_non_negative_number, validate_positive_number,
    };

    #[test]
    fn open_unit_interval_clamp_preserves_strict_bounds() {
        let lower = clamp_to_open_unit_interval(0.0);
        let upper = clamp_to_open_unit_interval(1.0);

        assert!(lower > 0.0);
        assert!(upper < 1.0);
    }

    #[test]
    fn positive_rounding_preserves_domain_after_quantization() {
        let rounded = round_positive_to_domain(1.0e-300, 12);

        assert!(rounded >= strictly_positive_machine_value());
    }

    #[test]
    fn domain_helpers_are_deterministic() {
        assert_eq!(clamp_to_unit_interval(-1.0), 0.0);
        assert_eq!(coverage_ratio(2, 5), 0.4);
        assert_eq!(coverage_ratio(0, 0), 0.0);
        assert_eq!(validate_finite_number(1.0, "field"), Ok(()));

        let error = validate_finite_number(f64::NAN, "field")
            .expect_err("non-finite values must fail validation");

        assert_eq!(error.field, "field");
        assert!(error.value.is_nan());
        assert_eq!(validate_positive_number(1.0, "field"), Ok(()));

        let error = validate_positive_number(0.0, "field")
            .expect_err("non-positive values must fail validation");

        assert_eq!(error.field, "field");
        assert_eq!(error.value, 0.0);
        assert_eq!(validate_non_negative_number(0.0, "field"), Ok(()));

        let error = validate_non_negative_number(-1.0, "field")
            .expect_err("negative values must fail validation");

        assert_eq!(error.field, "field");
        assert_eq!(error.value, -1.0);
        assert_eq!(validate_negative_number(-1.0, "field"), Ok(()));

        let error = validate_negative_number(0.0, "field")
            .expect_err("non-negative values must fail validation");

        assert_eq!(error.field, "field");
        assert_eq!(error.value, 0.0);
        assert_eq!(round_to(1.23456, 3), 1.235);
        assert_eq!(integer_square_root(15), 3);
        assert_eq!(integer_square_root(16), 4);
        assert_eq!(ceil_integer_square_root(15), 4);
        assert_eq!(ceil_integer_square_root(16), 4);
        assert_eq!(normalized_pseudocount(4), 0.25);
        assert_eq!(logistic(0.0), 0.5);
        assert_eq!(
            representable_positive_f64_log_span(),
            f64::MAX.ln() - f64::MIN_POSITIVE.ln()
        );
    }
}
