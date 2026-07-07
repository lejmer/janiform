use janiform_model_kernels::hawkes::HawkesExcitationState;

pub fn validate_excitation_state(state: HawkesExcitationState) -> Result<(), (&'static str, f64)> {
    validate_non_negative_finite_component("y_plus_plus", state.y_plus_plus)?;
    validate_non_negative_finite_component("y_plus_minus", state.y_plus_minus)?;
    validate_non_negative_finite_component("y_minus_plus", state.y_minus_plus)?;
    validate_non_negative_finite_component("y_minus_minus", state.y_minus_minus)?;

    Ok(())
}

fn validate_non_negative_finite_component(
    field: &'static str,
    value: f64,
) -> Result<(), (&'static str, f64)> {
    if !value.is_finite() || value < 0.0 {
        return Err((field, value));
    }
    Ok(())
}
