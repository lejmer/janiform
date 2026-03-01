use crate::randomness::SimulationRandomnessStream;

use crate::complex_math::ComplexScalar;

pub(crate) fn stable_gamma(alpha: f64, beta: f64) -> f64 {
    ((alpha - beta.abs()) * (alpha + beta.abs())).sqrt()
}

pub(crate) fn principal_branch_square_root(value: ComplexScalar) -> ComplexScalar {
    if value.imaginary == 0.0 {
        if value.real >= 0.0 {
            return ComplexScalar::new(value.real.sqrt(), 0.0);
        }

        return ComplexScalar::new(0.0, (-value.real).sqrt());
    }

    let magnitude = value.real.hypot(value.imaginary);
    let half_sum = (0.5 * (magnitude + value.real)).max(0.0);
    let half_difference = (0.5 * (magnitude - value.real)).max(0.0);

    let real_component = half_sum.sqrt();
    let imaginary_magnitude = half_difference.sqrt();
    let imaginary_component = if value.imaginary.is_sign_negative() {
        -imaginary_magnitude
    } else {
        imaginary_magnitude
    };

    ComplexScalar::new(real_component, imaginary_component)
}

pub(crate) fn sample_inverse_gaussian_increment<R>(
    mean: f64,
    shape: f64,
    randomness_stream: &mut R,
) -> Option<f64>
where
    R: SimulationRandomnessStream,
{
    if !mean.is_finite() || !shape.is_finite() || mean <= 0.0 || shape <= 0.0 {
        return Some(0.0);
    }

    let standard_normal_draw = sample_standard_normal_draw(randomness_stream);
    if !standard_normal_draw.is_finite() {
        return None;
    }

    let squared_standard_normal_draw = standard_normal_draw * standard_normal_draw;
    if squared_standard_normal_draw <= f64::MIN_POSITIVE {
        return Some(mean);
    }

    let four_shape_over_mean_draw = 4.0 * shape / (mean * squared_standard_normal_draw);
    if !four_shape_over_mean_draw.is_finite() {
        return Some(mean);
    }

    let square_root_term = (1.0 + four_shape_over_mean_draw).sqrt();
    if !square_root_term.is_finite() || square_root_term <= 0.0 {
        return None;
    }

    let candidate_draw = mean * (square_root_term - 1.0) / (square_root_term + 1.0);
    if !candidate_draw.is_finite() || candidate_draw <= 0.0 {
        return Some(mean);
    }

    let acceptance_threshold = mean / (mean + candidate_draw);
    if !acceptance_threshold.is_finite() || acceptance_threshold <= 0.0 {
        return None;
    }

    let acceptance_draw = randomness_stream.next_unit_interval_f64().clamp(0.0, 1.0);
    if acceptance_draw <= acceptance_threshold {
        return Some(candidate_draw);
    }

    let reflected_draw = mean * mean / candidate_draw;
    if reflected_draw.is_finite() && reflected_draw > 0.0 {
        Some(reflected_draw)
    } else {
        None
    }
}

pub(crate) fn sample_standard_normal_draw<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream,
{
    let first_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
    let second_uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(0.0, 1.0 - f64::EPSILON);

    let radius = (-2.0 * first_uniform_draw.ln()).sqrt();
    let angle = 2.0 * std::f64::consts::PI * second_uniform_draw;

    radius * angle.cos()
}
