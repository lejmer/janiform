use janiform_foundations::numeric::usize_to_f64_exact;

/// Return the Bayesian information criterion for a fitted likelihood model.
pub(crate) fn bayesian_information_criterion(
    sample_count: usize,
    parameter_count: usize,
    log_likelihood: f64,
) -> f64 {
    let Ok(sample_count_float) = usize_to_f64_exact(sample_count, "model_selection_sample_count")
    else {
        return f64::INFINITY;
    };

    bayesian_information_criterion_for_effective_sample_count(
        sample_count_float,
        parameter_count,
        log_likelihood,
    )
}

pub(crate) fn bayesian_information_criterion_for_effective_sample_count(
    effective_sample_count: f64,
    parameter_count: usize,
    log_likelihood: f64,
) -> f64 {
    if log_likelihood.is_infinite() && log_likelihood.is_sign_positive() {
        return f64::NEG_INFINITY;
    }

    if !effective_sample_count.is_finite() || effective_sample_count <= 0.0 {
        return f64::INFINITY;
    }

    let Ok(parameter_count_float) =
        usize_to_f64_exact(parameter_count, "model_selection_parameter_count")
    else {
        return f64::INFINITY;
    };

    parameter_count_float * effective_sample_count.ln() - 2.0 * log_likelihood
}

pub(crate) fn bayesian_information_score_for_effective_sample_count(
    effective_sample_count: f64,
    parameter_count: usize,
    log_likelihood: f64,
) -> f64 {
    -0.5 * bayesian_information_criterion_for_effective_sample_count(
        effective_sample_count,
        parameter_count,
        log_likelihood,
    )
}
