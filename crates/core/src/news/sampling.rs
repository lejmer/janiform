use chrono::{DateTime, Utc};

use crate::randomness::SimulationRandomnessStream;

use super::interface::ONE_SECOND_NANOS;

pub(crate) fn sample_gamma_shape_scale<R>(
    shape_k: f64,
    scale_theta: f64,
    randomness_stream: &mut R,
) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    sample_gamma_unit_scale(shape_k, randomness_stream) * scale_theta
}

fn sample_gamma_unit_scale<R>(shape_k: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    if shape_k < 1.0 {
        let boosted_sample = sample_gamma_shape_at_least_one(shape_k + 1.0, randomness_stream);
        let uniform_draw = randomness_stream
            .next_unit_interval_f64()
            .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);
        return boosted_sample * uniform_draw.powf(1.0 / shape_k);
    }

    sample_gamma_shape_at_least_one(shape_k, randomness_stream)
}

fn sample_gamma_shape_at_least_one<R>(shape_k: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let d = shape_k - (1.0 / 3.0);
    let c = (9.0 * d).sqrt().recip();

    loop {
        let standard_normal_draw = sample_standard_normal_draw(randomness_stream);
        let one_plus_cx = 1.0 + c * standard_normal_draw;
        if one_plus_cx <= 0.0 {
            continue;
        }

        let v = one_plus_cx * one_plus_cx * one_plus_cx;
        let uniform_draw = randomness_stream
            .next_unit_interval_f64()
            .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);

        if uniform_draw < 1.0 - 0.0331 * standard_normal_draw.powi(4) {
            return d * v;
        }

        if uniform_draw.ln()
            < 0.5 * standard_normal_draw * standard_normal_draw + d * (1.0 - v + v.ln())
        {
            return d * v;
        }
    }
}

fn sample_standard_normal_draw<R>(randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
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

pub(crate) fn sample_exponential_with_rate<R>(rate: f64, randomness_stream: &mut R) -> f64
where
    R: SimulationRandomnessStream + ?Sized,
{
    let uniform_draw = randomness_stream
        .next_unit_interval_f64()
        .clamp(f64::MIN_POSITIVE, 1.0 - f64::EPSILON);

    -uniform_draw.ln() / rate
}

pub(crate) fn utc_timestamp_from_nanos(timestamp_utc_nanos: i64) -> Option<DateTime<Utc>> {
    let seconds = timestamp_utc_nanos.div_euclid(ONE_SECOND_NANOS);
    let subsecond_nanos = timestamp_utc_nanos.rem_euclid(ONE_SECOND_NANOS) as u32;

    DateTime::from_timestamp(seconds, subsecond_nanos)
        .map(|timestamp| timestamp.with_timezone(&Utc))
}
