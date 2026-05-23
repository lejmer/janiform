//! Shared `2x2` linear algebra helpers.

use nalgebra::{Matrix2, Vector2};

fn centered_two_by_two_matrix_expansion(
    trace_half: f64,
    discriminant: f64,
    delta: f64,
) -> (Vector2<f64>, Vector2<f64>, Vector2<f64>) {
    if discriminant > 0.0 {
        let root_discriminant = discriminant.sqrt();

        let positive_rate = trace_half + root_discriminant;
        let negative_rate = trace_half - root_discriminant;

        let exp_positive = (positive_rate * delta).exp();
        let exp_negative = (negative_rate * delta).exp();

        (
            Vector2::new(
                0.5 * (exp_positive + exp_negative),
                0.5 * (exp_positive - exp_negative) / root_discriminant,
            ),
            Vector2::new(
                0.5 * (scaled_exponential_minus_one(positive_rate, delta)
                    + scaled_exponential_minus_one(negative_rate, delta)),
                0.5 * (scaled_exponential_minus_one(positive_rate, delta)
                    - scaled_exponential_minus_one(negative_rate, delta))
                    / root_discriminant,
            ),
            Vector2::new(
                0.5 * (scaled_exponential_minus_one_minus_linear_term(positive_rate, delta)
                    + scaled_exponential_minus_one_minus_linear_term(negative_rate, delta)),
                0.5 * (scaled_exponential_minus_one_minus_linear_term(positive_rate, delta)
                    - scaled_exponential_minus_one_minus_linear_term(negative_rate, delta))
                    / root_discriminant,
            ),
        )
    } else if discriminant < 0.0 {
        let oscillation_rate = (-discriminant).sqrt();
        let exp_trace = (trace_half * delta).exp();

        let angle = oscillation_rate * delta;

        let cosine = angle.cos();
        let sine = angle.sin();

        let denominator = trace_half * trace_half + oscillation_rate * oscillation_rate;

        (
            Vector2::new(exp_trace * cosine, exp_trace * sine / oscillation_rate),
            Vector2::new(
                (exp_trace * (trace_half * cosine + oscillation_rate * sine) - trace_half)
                    / denominator,
                (exp_trace * (trace_half * sine - oscillation_rate * cosine) + oscillation_rate)
                    / (oscillation_rate * denominator),
            ),
            Vector2::new(f64::NAN, f64::NAN),
        )
    } else {
        let exp_trace = (trace_half * delta).exp();

        (
            Vector2::new(exp_trace, exp_trace * delta),
            Vector2::new(
                scaled_exponential_minus_one(trace_half, delta),
                if trace_half == 0.0 {
                    0.5 * delta * delta
                } else {
                    (exp_trace * (trace_half * delta - 1.0) + 1.0) / (trace_half * trace_half)
                },
            ),
            Vector2::new(0.5 * delta * delta, delta * delta * delta / 6.0),
        )
    }
}

fn apply_centered_two_by_two_expansion(
    identity_coefficient: f64,
    centered_matrix_coefficient: f64,
    centered_matrix: Matrix2<f64>,
    vector: Vector2<f64>,
) -> Vector2<f64> {
    vector * identity_coefficient + (centered_matrix * vector) * centered_matrix_coefficient
}

/// Solve one constant-source `2x2` linear system over interval length `delta`.
///
/// Returns `(terminal_state, integrated_state)` under the dynamics
/// `x'(t) = A x(t) + b` with fixed `A`, fixed `b`, and the provided initial
/// state.
pub fn solve_two_by_two_linear_source_system(
    matrix: Matrix2<f64>,
    initial_state: Vector2<f64>,
    source: Vector2<f64>,
    delta: f64,
) -> (Vector2<f64>, Vector2<f64>) {
    let matrix_a = matrix[(0, 0)];
    let matrix_b = matrix[(0, 1)];
    let matrix_c = matrix[(1, 0)];
    let matrix_d = matrix[(1, 1)];

    let trace_half = 0.5 * (matrix_a + matrix_d);
    let centered_a = 0.5 * (matrix_a - matrix_d);
    let centered_matrix = Matrix2::new(centered_a, matrix_b, matrix_c, -centered_a);

    let discriminant = centered_a * centered_a + matrix_b * matrix_c;

    let (exponential_coefficients, integrated_coefficients, double_integrated_coefficients) =
        centered_two_by_two_matrix_expansion(trace_half, discriminant, delta);

    let after_interval_from_start = apply_centered_two_by_two_expansion(
        exponential_coefficients[0],
        exponential_coefficients[1],
        centered_matrix,
        initial_state,
    );

    let after_interval_from_source = apply_centered_two_by_two_expansion(
        integrated_coefficients[0],
        integrated_coefficients[1],
        centered_matrix,
        source,
    );

    let after_interval = after_interval_from_start + after_interval_from_source;

    let integrated_state_rhs = after_interval - initial_state - source * delta;

    let integrated_state = if let Some(solved_state) = matrix.lu().solve(&integrated_state_rhs) {
        solved_state
    } else {
        let integrated_state_from_start = apply_centered_two_by_two_expansion(
            integrated_coefficients[0],
            integrated_coefficients[1],
            centered_matrix,
            initial_state,
        );
        let integrated_state_from_source = apply_centered_two_by_two_expansion(
            double_integrated_coefficients[0],
            double_integrated_coefficients[1],
            centered_matrix,
            source,
        );

        integrated_state_from_start + integrated_state_from_source
    };

    (after_interval, integrated_state)
}

fn scaled_exponential_minus_one(rate: f64, delta: f64) -> f64 {
    if rate == 0.0 {
        delta
    } else {
        (rate * delta).exp_m1() / rate
    }
}

fn scaled_exponential_minus_one_minus_linear_term(rate: f64, delta: f64) -> f64 {
    if rate == 0.0 {
        0.5 * delta * delta
    } else {
        ((rate * delta).exp_m1() - rate * delta) / (rate * rate)
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::{Matrix2, Vector2};

    use super::{
        apply_centered_two_by_two_expansion, centered_two_by_two_matrix_expansion,
        solve_two_by_two_linear_source_system,
    };

    #[test]
    fn centered_two_by_two_matrix_expansion_handles_zero_discriminant() {
        let expansion = centered_two_by_two_matrix_expansion(0.0, 0.0, 2.0);

        assert_eq!(
            expansion,
            (
                Vector2::new(1.0, 2.0),
                Vector2::new(2.0, 2.0),
                Vector2::new(2.0, 4.0 / 3.0)
            )
        );
    }

    #[test]
    fn apply_centered_two_by_two_expansion_preserves_identity_action() {
        let applied = apply_centered_two_by_two_expansion(
            1.0,
            0.0,
            Matrix2::new(3.0, 4.0, 5.0, -3.0),
            Vector2::new(2.0, -1.0),
        );

        assert_eq!(applied, Vector2::new(2.0, -1.0));
    }

    #[test]
    fn solve_two_by_two_linear_source_system_preserves_constant_state_without_flow() {
        let solution = solve_two_by_two_linear_source_system(
            Matrix2::zeros(),
            Vector2::new(1.5, -2.0),
            Vector2::zeros(),
            3.0,
        );

        assert_eq!(solution, (Vector2::new(1.5, -2.0), Vector2::new(4.5, -6.0)));
    }

    #[test]
    fn solve_two_by_two_linear_source_system_accumulates_constant_source_without_dynamics() {
        let solution = solve_two_by_two_linear_source_system(
            Matrix2::zeros(),
            Vector2::new(1.0, 2.0),
            Vector2::new(0.5, -1.0),
            4.0,
        );

        assert_eq!(solution, (Vector2::new(3.0, -2.0), Vector2::new(8.0, 0.0)));
    }
}
