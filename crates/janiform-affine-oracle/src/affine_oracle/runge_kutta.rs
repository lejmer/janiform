use janiform_foundations::complex_math::ComplexScalar;

use super::contracts::ExponentialAffineTransformDVector;

pub(super) fn combine_d_vector_runge_kutta(
    k1: ExponentialAffineTransformDVector,
    k2: ExponentialAffineTransformDVector,
    k3: ExponentialAffineTransformDVector,
    k4: ExponentialAffineTransformDVector,
) -> ExponentialAffineTransformDVector {
    k1.plus(k2.scale(2.0))
        .plus(k3.scale(2.0))
        .plus(k4)
        .scale(1.0 / 6.0)
}

pub(super) fn combine_complex_runge_kutta(
    k1: ComplexScalar,
    k2: ComplexScalar,
    k3: ComplexScalar,
    k4: ComplexScalar,
) -> ComplexScalar {
    k1.plus(k2.scale(2.0))
        .plus(k3.scale(2.0))
        .plus(k4)
        .scale(1.0 / 6.0)
}
