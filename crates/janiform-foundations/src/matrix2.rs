//! Shared deterministic `2x2` matrix helpers.

/// Closed-form coefficient expansion for a centered `2x2` matrix exponential and
/// its first two time integrals over one interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoByTwoMatrixExpansion {
    pub exponential_identity_coefficient: f64,
    pub exponential_centered_matrix_coefficient: f64,
    pub integrated_exponential_identity_coefficient: f64,
    pub integrated_exponential_centered_matrix_coefficient: f64,
    pub double_integrated_exponential_identity_coefficient: f64,
    pub double_integrated_exponential_centered_matrix_coefficient: f64,
}

/// Closed-form solution summary for one affine `2x2` linear system with constant
/// source term over a finite interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TwoByTwoLinearSourceSystemSolution {
    pub first_component_after_interval: f64,
    pub second_component_after_interval: f64,
    pub integrated_first_component: f64,
    pub integrated_second_component: f64,
}

/// Evaluate the scalar coefficient expansion for a centered `2x2` matrix
/// exponential over interval length `delta`.
///
/// The underlying matrix is represented in trace/centered form
/// `A = trace_half * I + C`, with `C^2 = discriminant * I`.
pub fn centered_two_by_two_matrix_expansion(
    trace_half: f64,
    discriminant: f64,
    delta: f64,
) -> TwoByTwoMatrixExpansion {
    if discriminant > 0.0 {
        let root_discriminant = discriminant.sqrt();

        let positive_rate = trace_half + root_discriminant;
        let negative_rate = trace_half - root_discriminant;

        let exp_positive = (positive_rate * delta).exp();
        let exp_negative = (negative_rate * delta).exp();

        TwoByTwoMatrixExpansion {
            exponential_identity_coefficient: 0.5 * (exp_positive + exp_negative),
            exponential_centered_matrix_coefficient: 0.5 * (exp_positive - exp_negative)
                / root_discriminant,
            integrated_exponential_identity_coefficient: 0.5
                * (scaled_exponential_minus_one(positive_rate, delta)
                    + scaled_exponential_minus_one(negative_rate, delta)),
            integrated_exponential_centered_matrix_coefficient: 0.5
                * (scaled_exponential_minus_one(positive_rate, delta)
                    - scaled_exponential_minus_one(negative_rate, delta))
                / root_discriminant,
            double_integrated_exponential_identity_coefficient: 0.5
                * (scaled_exponential_minus_one_minus_linear_term(positive_rate, delta)
                    + scaled_exponential_minus_one_minus_linear_term(negative_rate, delta)),
            double_integrated_exponential_centered_matrix_coefficient: 0.5
                * (scaled_exponential_minus_one_minus_linear_term(positive_rate, delta)
                    - scaled_exponential_minus_one_minus_linear_term(negative_rate, delta))
                / root_discriminant,
        }
    } else if discriminant < 0.0 {
        let oscillation_rate = (-discriminant).sqrt();
        let exp_trace = (trace_half * delta).exp();
        let angle = oscillation_rate * delta;

        let cosine = angle.cos();
        let sine = angle.sin();

        let denominator = trace_half * trace_half + oscillation_rate * oscillation_rate;

        TwoByTwoMatrixExpansion {
            exponential_identity_coefficient: exp_trace * cosine,
            exponential_centered_matrix_coefficient: exp_trace * sine / oscillation_rate,
            integrated_exponential_identity_coefficient: (exp_trace
                * (trace_half * cosine + oscillation_rate * sine)
                - trace_half)
                / denominator,
            integrated_exponential_centered_matrix_coefficient: (exp_trace
                * (trace_half * sine - oscillation_rate * cosine)
                + oscillation_rate)
                / (oscillation_rate * denominator),
            double_integrated_exponential_identity_coefficient: f64::NAN,
            double_integrated_exponential_centered_matrix_coefficient: f64::NAN,
        }
    } else {
        let exp_trace = (trace_half * delta).exp();

        TwoByTwoMatrixExpansion {
            exponential_identity_coefficient: exp_trace,
            exponential_centered_matrix_coefficient: exp_trace * delta,
            integrated_exponential_identity_coefficient: scaled_exponential_minus_one(
                trace_half, delta,
            ),
            integrated_exponential_centered_matrix_coefficient: if trace_half == 0.0 {
                0.5 * delta * delta
            } else {
                (exp_trace * (trace_half * delta - 1.0) + 1.0) / (trace_half * trace_half)
            },
            double_integrated_exponential_identity_coefficient: 0.5 * delta * delta,
            double_integrated_exponential_centered_matrix_coefficient: delta * delta * delta / 6.0,
        }
    }
}

/// Apply a coefficient expansion of the form `alpha * I + beta * C` to a vector,
/// where `C = [[centered_a, matrix_b], [matrix_c, -centered_a]]`.
pub fn apply_centered_two_by_two_expansion(
    identity_coefficient: f64,
    centered_matrix_coefficient: f64,
    centered_a: f64,
    matrix_b: f64,
    matrix_c: f64,
    vector: [f64; 2],
) -> [f64; 2] {
    let centered_matrix_times_vector = [
        centered_a * vector[0] + matrix_b * vector[1],
        matrix_c * vector[0] - centered_a * vector[1],
    ];

    [
        identity_coefficient * vector[0]
            + centered_matrix_coefficient * centered_matrix_times_vector[0],
        identity_coefficient * vector[1]
            + centered_matrix_coefficient * centered_matrix_times_vector[1],
    ]
}

/// Solve one `2x2` linear system. Returns `None` when the matrix is singular.
pub fn solve_two_by_two_linear_system(
    matrix_a: f64,
    matrix_b: f64,
    matrix_c: f64,
    matrix_d: f64,
    right_hand_side: [f64; 2],
) -> Option<[f64; 2]> {
    let determinant = matrix_a * matrix_d - matrix_b * matrix_c;
    if determinant == 0.0 {
        return None;
    }

    Some([
        (matrix_d * right_hand_side[0] - matrix_b * right_hand_side[1]) / determinant,
        (-matrix_c * right_hand_side[0] + matrix_a * right_hand_side[1]) / determinant,
    ])
}

/// Solve one constant-source `2x2` linear system over interval length `delta`.
///
/// This returns both the terminal state and the time-integrated state under the
/// dynamics
/// `x'(t) = A x(t) + b`
/// with fixed `A`, fixed `b`, and the provided initial state.
#[allow(clippy::too_many_arguments)]
pub fn solve_two_by_two_linear_source_system(
    matrix_a: f64,
    matrix_b: f64,
    matrix_c: f64,
    matrix_d: f64,
    first_component_start: f64,
    second_component_start: f64,
    first_component_source: f64,
    second_component_source: f64,
    delta: f64,
) -> TwoByTwoLinearSourceSystemSolution {
    let trace_half = 0.5 * (matrix_a + matrix_d);
    let centered_a = 0.5 * (matrix_a - matrix_d);
    let discriminant = centered_a * centered_a + matrix_b * matrix_c;

    let matrix_expansion = centered_two_by_two_matrix_expansion(trace_half, discriminant, delta);

    let start_vector = [first_component_start, second_component_start];
    let source_vector = [first_component_source, second_component_source];

    let after_interval_from_start = apply_centered_two_by_two_expansion(
        matrix_expansion.exponential_identity_coefficient,
        matrix_expansion.exponential_centered_matrix_coefficient,
        centered_a,
        matrix_b,
        matrix_c,
        start_vector,
    );
    let after_interval_from_source = apply_centered_two_by_two_expansion(
        matrix_expansion.integrated_exponential_identity_coefficient,
        matrix_expansion.integrated_exponential_centered_matrix_coefficient,
        centered_a,
        matrix_b,
        matrix_c,
        source_vector,
    );
    let after_interval = [
        after_interval_from_start[0] + after_interval_from_source[0],
        after_interval_from_start[1] + after_interval_from_source[1],
    ];

    let integrated_state = if let Some(solved_state) = solve_two_by_two_linear_system(
        matrix_a,
        matrix_b,
        matrix_c,
        matrix_d,
        [
            after_interval[0] - first_component_start - first_component_source * delta,
            after_interval[1] - second_component_start - second_component_source * delta,
        ],
    ) {
        solved_state
    } else {
        let integrated_state_from_start = apply_centered_two_by_two_expansion(
            matrix_expansion.integrated_exponential_identity_coefficient,
            matrix_expansion.integrated_exponential_centered_matrix_coefficient,
            centered_a,
            matrix_b,
            matrix_c,
            start_vector,
        );
        let integrated_state_from_source = apply_centered_two_by_two_expansion(
            matrix_expansion.double_integrated_exponential_identity_coefficient,
            matrix_expansion.double_integrated_exponential_centered_matrix_coefficient,
            centered_a,
            matrix_b,
            matrix_c,
            source_vector,
        );
        [
            integrated_state_from_start[0] + integrated_state_from_source[0],
            integrated_state_from_start[1] + integrated_state_from_source[1],
        ]
    };

    TwoByTwoLinearSourceSystemSolution {
        first_component_after_interval: after_interval[0],
        second_component_after_interval: after_interval[1],
        integrated_first_component: integrated_state[0],
        integrated_second_component: integrated_state[1],
    }
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
    use super::{
        TwoByTwoLinearSourceSystemSolution, TwoByTwoMatrixExpansion,
        apply_centered_two_by_two_expansion, centered_two_by_two_matrix_expansion,
        solve_two_by_two_linear_source_system, solve_two_by_two_linear_system,
    };

    #[test]
    fn centered_two_by_two_matrix_expansion_handles_zero_discriminant() {
        let expansion = centered_two_by_two_matrix_expansion(0.0, 0.0, 2.0);

        assert_eq!(
            expansion,
            TwoByTwoMatrixExpansion {
                exponential_identity_coefficient: 1.0,
                exponential_centered_matrix_coefficient: 2.0,
                integrated_exponential_identity_coefficient: 2.0,
                integrated_exponential_centered_matrix_coefficient: 2.0,
                double_integrated_exponential_identity_coefficient: 2.0,
                double_integrated_exponential_centered_matrix_coefficient: 4.0 / 3.0,
            }
        );
    }

    #[test]
    fn apply_centered_two_by_two_expansion_preserves_identity_action() {
        let applied = apply_centered_two_by_two_expansion(1.0, 0.0, 3.0, 4.0, 5.0, [2.0, -1.0]);

        assert_eq!(applied, [2.0, -1.0]);
    }

    #[test]
    fn solve_two_by_two_linear_system_solves_non_singular_system() {
        let solution = solve_two_by_two_linear_system(2.0, 1.0, 1.0, 3.0, [5.0, 7.0]);

        assert_eq!(solution, Some([1.6, 1.8]));
    }

    #[test]
    fn solve_two_by_two_linear_system_rejects_singular_system() {
        let solution = solve_two_by_two_linear_system(1.0, 2.0, 2.0, 4.0, [3.0, 6.0]);

        assert_eq!(solution, None);
    }

    #[test]
    fn solve_two_by_two_linear_source_system_preserves_constant_state_without_flow() {
        let solution =
            solve_two_by_two_linear_source_system(0.0, 0.0, 0.0, 0.0, 1.5, -2.0, 0.0, 0.0, 3.0);

        assert_eq!(
            solution,
            TwoByTwoLinearSourceSystemSolution {
                first_component_after_interval: 1.5,
                second_component_after_interval: -2.0,
                integrated_first_component: 4.5,
                integrated_second_component: -6.0,
            }
        );
    }

    #[test]
    fn solve_two_by_two_linear_source_system_accumulates_constant_source_without_dynamics() {
        let solution =
            solve_two_by_two_linear_source_system(0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 0.5, -1.0, 4.0);

        assert_eq!(
            solution,
            TwoByTwoLinearSourceSystemSolution {
                first_component_after_interval: 3.0,
                second_component_after_interval: -2.0,
                integrated_first_component: 8.0,
                integrated_second_component: 0.0,
            }
        );
    }
}
