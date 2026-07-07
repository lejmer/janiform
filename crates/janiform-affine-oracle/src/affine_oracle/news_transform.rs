use janiform_foundations::complex_math::{
    ComplexScalar, complex_log_principal, complex_multiply, complex_pow_real_exponent_principal,
};
use janiform_model_kernels::news::{
    GammaSeverityMarkConfiguration, NewsComponentMarkScaleConfiguration,
};

use super::contracts::ExponentialAffineTransformNewsIntensityInput;

#[derive(Debug, Clone, Copy)]
pub(crate) struct GammaNormalNewsTransformComponents {
    pub(crate) gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    pub(crate) news_intensity: ExponentialAffineTransformNewsIntensityInput,
    pub(crate) known_component_mark_scales: NewsComponentMarkScaleConfiguration,
    pub(crate) surprise_component_mark_scales: NewsComponentMarkScaleConfiguration,
    pub(crate) activity_loading: ComplexScalar,
    pub(crate) return_loading: ComplexScalar,
}

pub(crate) fn evaluate_gamma_normal_news_transform(
    components: GammaNormalNewsTransformComponents,
) -> ComplexScalar {
    evaluate_component_log_transform(
        components.gamma_severity_mark_configuration,
        components.known_component_mark_scales,
        components.activity_loading,
        components.return_loading,
    )
    .scale(components.news_intensity.known_news_intensity)
    .plus(
        evaluate_component_transform(
            components.gamma_severity_mark_configuration,
            components.surprise_component_mark_scales,
            components.activity_loading,
            components.return_loading,
        )
        .subtract(ComplexScalar::new(1.0, 0.0))
        .scale(components.news_intensity.surprise_news_intensity),
    )
}

pub(crate) fn evaluate_gamma_normal_news_transform_tangent(
    components: GammaNormalNewsTransformComponents,
    activity_loading_tangent: ComplexScalar,
    return_loading_tangent: ComplexScalar,
) -> ComplexScalar {
    evaluate_component_log_transform_tangent(
        components.gamma_severity_mark_configuration,
        components.known_component_mark_scales,
        components.activity_loading,
        components.return_loading,
        activity_loading_tangent,
        return_loading_tangent,
    )
    .scale(components.news_intensity.known_news_intensity)
    .plus(
        evaluate_component_transform_tangent(
            components.gamma_severity_mark_configuration,
            components.surprise_component_mark_scales,
            components.activity_loading,
            components.return_loading,
            activity_loading_tangent,
            return_loading_tangent,
        )
        .scale(components.news_intensity.surprise_news_intensity),
    )
}

pub(crate) fn gamma_normal_news_domain_metric(
    components: GammaNormalNewsTransformComponents,
) -> ComplexScalar {
    let known_metric = component_domain_metric(
        components.known_component_mark_scales,
        components.activity_loading,
        components.return_loading,
    );

    let surprise_metric = component_domain_metric(
        components.surprise_component_mark_scales,
        components.activity_loading,
        components.return_loading,
    );

    match (
        components.news_intensity.known_news_intensity > 0.0,
        components.news_intensity.surprise_news_intensity > 0.0,
    ) {
        (true, true) if surprise_metric.real > known_metric.real => surprise_metric,
        (true, true) => known_metric,
        (true, false) => known_metric,
        (false, true) => surprise_metric,
        (false, false) => ComplexScalar::new(0.0, 0.0),
    }
}

fn evaluate_component_transform(
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    activity_loading: ComplexScalar,
    return_loading: ComplexScalar,
) -> ComplexScalar {
    let component_metric =
        component_domain_metric(component_mark_scales, activity_loading, return_loading);

    let base = ComplexScalar::new(1.0, 0.0).subtract(
        component_metric.scale(gamma_severity_mark_configuration.derived_unit_mean_scale()),
    );

    complex_pow_real_exponent_principal(base, -gamma_severity_mark_configuration.shape_k)
}

fn evaluate_component_log_transform(
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    activity_loading: ComplexScalar,
    return_loading: ComplexScalar,
) -> ComplexScalar {
    let component_metric =
        component_domain_metric(component_mark_scales, activity_loading, return_loading);

    let base = ComplexScalar::new(1.0, 0.0).subtract(
        component_metric.scale(gamma_severity_mark_configuration.derived_unit_mean_scale()),
    );

    complex_log_principal(base).scale(-gamma_severity_mark_configuration.shape_k)
}

fn evaluate_component_transform_tangent(
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    activity_loading: ComplexScalar,
    return_loading: ComplexScalar,
    activity_loading_tangent: ComplexScalar,
    return_loading_tangent: ComplexScalar,
) -> ComplexScalar {
    let component_metric =
        component_domain_metric(component_mark_scales, activity_loading, return_loading);

    let component_metric_tangent = component_domain_metric_tangent(
        component_mark_scales,
        return_loading,
        activity_loading_tangent,
        return_loading_tangent,
    );

    let theta_u = gamma_severity_mark_configuration.derived_unit_mean_scale();
    let base = ComplexScalar::new(1.0, 0.0).subtract(component_metric.scale(theta_u));

    let gamma_power = complex_pow_real_exponent_principal(
        base,
        -(gamma_severity_mark_configuration.shape_k + 1.0),
    );

    let prefactor = gamma_severity_mark_configuration.shape_k * theta_u;

    complex_multiply(gamma_power.scale(prefactor), component_metric_tangent)
}

fn evaluate_component_log_transform_tangent(
    gamma_severity_mark_configuration: GammaSeverityMarkConfiguration,
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    activity_loading: ComplexScalar,
    return_loading: ComplexScalar,
    activity_loading_tangent: ComplexScalar,
    return_loading_tangent: ComplexScalar,
) -> ComplexScalar {
    let component_metric =
        component_domain_metric(component_mark_scales, activity_loading, return_loading);

    let component_metric_tangent = component_domain_metric_tangent(
        component_mark_scales,
        return_loading,
        activity_loading_tangent,
        return_loading_tangent,
    );

    let theta_u = gamma_severity_mark_configuration.derived_unit_mean_scale();
    let base = ComplexScalar::new(1.0, 0.0).subtract(component_metric.scale(theta_u));

    component_metric_tangent
        .divide(base)
        .unwrap_or(ComplexScalar::new(f64::NAN, f64::NAN))
        .scale(gamma_severity_mark_configuration.shape_k * theta_u)
}

fn component_domain_metric(
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    activity_loading: ComplexScalar,
    return_loading: ComplexScalar,
) -> ComplexScalar {
    activity_loading
        .scale(component_mark_scales.activity_coupling_scale)
        .plus(return_loading.scale(component_mark_scales.return_jump_drift))
        .plus(
            return_loading
                .square()
                .scale(0.5 * component_mark_scales.return_jump_volatility.powi(2)),
        )
}

fn component_domain_metric_tangent(
    component_mark_scales: NewsComponentMarkScaleConfiguration,
    return_loading: ComplexScalar,
    activity_loading_tangent: ComplexScalar,
    return_loading_tangent: ComplexScalar,
) -> ComplexScalar {
    activity_loading_tangent
        .scale(component_mark_scales.activity_coupling_scale)
        .plus(return_loading_tangent.scale(component_mark_scales.return_jump_drift))
        .plus(
            complex_multiply(return_loading, return_loading_tangent)
                .scale(component_mark_scales.return_jump_volatility.powi(2)),
        )
}

#[cfg(test)]
mod tests {
    use janiform_model_kernels::news::GammaSeverityMarkConfiguration;

    use super::{
        GammaNormalNewsTransformComponents, evaluate_gamma_normal_news_transform,
        evaluate_gamma_normal_news_transform_tangent,
    };
    use crate::affine_oracle::ExponentialAffineTransformNewsIntensityInput;
    use janiform_foundations::complex_math::ComplexScalar;
    use janiform_model_kernels::news::NewsComponentMarkScaleConfiguration;

    #[test]
    fn known_news_uses_deterministic_arrival_log_transform() {
        let components = test_components(2.0, 0.0);
        let transform = evaluate_gamma_normal_news_transform(components);

        let component_metric = 0.5 * 0.2;
        let expected_log_transform = -2.0_f64 * (1.0_f64 - 0.5_f64 * component_metric).ln();

        assert_eq!(
            transform,
            ComplexScalar::new(2.0 * expected_log_transform, 0.0)
        );
    }

    #[test]
    fn surprise_news_uses_compound_poisson_transform() {
        let components = test_components(0.0, 2.0);
        let transform = evaluate_gamma_normal_news_transform(components);

        let component_metric = 0.5 * 0.2;
        let expected_transform = (1.0_f64 - 0.5_f64 * component_metric).powf(-2.0);

        assert_eq!(
            transform,
            ComplexScalar::new(2.0 * (expected_transform - 1.0), 0.0)
        );
    }

    #[test]
    fn known_news_tangent_uses_log_transform_derivative() {
        let components = test_components(2.0, 0.0);
        let tangent = evaluate_gamma_normal_news_transform_tangent(
            components,
            ComplexScalar::new(0.3, 0.0),
            ComplexScalar::new(0.0, 0.0),
        );

        let denominator = 1.0 - 0.5 * 0.5 * 0.2;
        let expected_tangent = 2.0 * (2.0 * 0.5 * 0.5 * 0.3 / denominator);

        assert_eq!(tangent, ComplexScalar::new(expected_tangent, 0.0));
    }

    fn test_components(
        known_news_intensity: f64,
        surprise_news_intensity: f64,
    ) -> GammaNormalNewsTransformComponents {
        GammaNormalNewsTransformComponents {
            gamma_severity_mark_configuration: GammaSeverityMarkConfiguration { shape_k: 2.0 },
            news_intensity: ExponentialAffineTransformNewsIntensityInput {
                known_news_intensity,
                surprise_news_intensity,
            },
            known_component_mark_scales: test_component_mark_scales(),
            surprise_component_mark_scales: test_component_mark_scales(),
            activity_loading: ComplexScalar::new(0.2, 0.0),
            return_loading: ComplexScalar::new(0.0, 0.0),
        }
    }

    fn test_component_mark_scales() -> NewsComponentMarkScaleConfiguration {
        NewsComponentMarkScaleConfiguration {
            activity_coupling_scale: 0.5,
            return_jump_drift: 0.0,
            return_jump_volatility: 0.0,
        }
    }
}
