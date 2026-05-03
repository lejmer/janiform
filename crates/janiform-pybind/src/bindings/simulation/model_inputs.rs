use janiform::model::hawkes::{
    HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters,
};
use janiform::model::nig::NigSharedPlusShiftConfiguration;
use janiform::simulation::{
    MeanReversionBoundaryState, MeanReversionFittedParameters, MeanReversionRuntimeIntervalAnchors,
};
use pyo3::prelude::{Bound, PyResult};
use pyo3::types::{PyAny, PyAnyMethods, PyMapping};

use super::fields::parse_required_field;
use crate::SeasonalityInputValidationError;
use crate::python_value::py_any_to_json_value;

pub(super) fn parse_seeded_simulation_mean_reversion_fitted_parameters(
    raw_mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionFittedParameters> {
    let parsed_parameters = serde_json::from_value::<MeanReversionFittedParameters>(
        py_any_to_json_value(raw_mean_reversion_fitted_parameters)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "mean_reversion_fitted_parameters failed to deserialize: {source}"
        ))
    })?;

    parsed_parameters.validate().map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "mean_reversion_fitted_parameters failed validation: {source}"
        ))
    })?;

    Ok(parsed_parameters)
}

pub(super) fn parse_seeded_simulation_mean_reversion_runtime_interval_anchors(
    raw_mean_reversion_runtime_interval_anchors: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionRuntimeIntervalAnchors> {
    let parsed_runtime_interval_anchors =
        serde_json::from_value::<MeanReversionRuntimeIntervalAnchors>(py_any_to_json_value(
            raw_mean_reversion_runtime_interval_anchors,
        )?)
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "mean_reversion_runtime_interval_anchors failed to deserialize: {source}"
            ))
        })?;

    parsed_runtime_interval_anchors
        .validate()
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "mean_reversion_runtime_interval_anchors failed validation: {source}"
            ))
        })?;

    Ok(parsed_runtime_interval_anchors)
}

pub(super) fn parse_seeded_simulation_mean_reversion_boundary_state(
    raw_initial_mean_reversion_boundary_state: &Bound<'_, PyAny>,
) -> PyResult<MeanReversionBoundaryState> {
    let parsed_boundary_state = serde_json::from_value::<MeanReversionBoundaryState>(
        py_any_to_json_value(raw_initial_mean_reversion_boundary_state)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "initial_mean_reversion_boundary_state failed to deserialize: {source}"
        ))
    })?;

    parsed_boundary_state.validate().map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "initial_mean_reversion_boundary_state failed validation: {source}"
        ))
    })?;

    Ok(parsed_boundary_state)
}

pub(super) fn parse_required_numeric_sequence(
    raw_sequence: &Bound<'_, PyAny>,
    context: &str,
) -> PyResult<Vec<f64>> {
    let parsed_sequence = serde_json::from_value::<Vec<f64>>(py_any_to_json_value(raw_sequence)?)
        .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "{context} failed to deserialize as a numeric sequence: {source}"
        ))
    })?;

    if parsed_sequence.iter().any(|value| !value.is_finite()) {
        return Err(SeasonalityInputValidationError::new_err(format!(
            "{context} must contain only finite numeric values"
        )));
    }

    Ok(parsed_sequence)
}

pub(super) fn parse_seeded_simulation_kernel_parameters(
    raw_kernel_parameters: &Bound<'_, PyAny>,
) -> PyResult<HawkesKernelParameters> {
    let kernel_parameters_mapping = raw_kernel_parameters.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err("kernel_parameters must be a mapping")
    })?;

    let lambda_bar = parse_required_float_field_from_mapping(
        kernel_parameters_mapping,
        "kernel_parameters",
        "lambda_bar",
    )?;

    let beta = parse_required_float_field_from_mapping(
        kernel_parameters_mapping,
        "kernel_parameters",
        "beta",
    )?;

    let alpha = parse_required_hawkes_kernel_loadings_field(
        kernel_parameters_mapping,
        "kernel_parameters",
        "alpha",
    )?;

    let rho = parse_required_hawkes_kernel_loadings_field(
        kernel_parameters_mapping,
        "kernel_parameters",
        "rho",
    )?;

    HawkesKernelParameters::try_new(lambda_bar, beta, alpha, rho).map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "kernel_parameters payload failed Hawkes validation: {source}"
        ))
    })
}

pub(super) fn parse_seeded_simulation_initial_excitation_state(
    raw_initial_excitation_state: &Bound<'_, PyAny>,
) -> PyResult<HawkesExcitationState> {
    let state_mapping = raw_initial_excitation_state
        .cast::<PyMapping>()
        .map_err(|_| {
            SeasonalityInputValidationError::new_err("initial_excitation_state must be a mapping")
        })?;

    Ok(HawkesExcitationState {
        y_plus_plus: parse_required_float_field_from_mapping(
            state_mapping,
            "initial_excitation_state",
            "y_plus_plus",
        )?,
        y_plus_minus: parse_required_float_field_from_mapping(
            state_mapping,
            "initial_excitation_state",
            "y_plus_minus",
        )?,
        y_minus_plus: parse_required_float_field_from_mapping(
            state_mapping,
            "initial_excitation_state",
            "y_minus_plus",
        )?,
        y_minus_minus: parse_required_float_field_from_mapping(
            state_mapping,
            "initial_excitation_state",
            "y_minus_minus",
        )?,
    })
}

pub(super) fn parse_seeded_simulation_nig_shared_plus_shift_configuration(
    raw_configuration: &Bound<'_, PyAny>,
) -> PyResult<NigSharedPlusShiftConfiguration> {
    let configuration_mapping = raw_configuration.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "nig_shared_plus_shift_configuration must be a mapping",
        )
    })?;

    NigSharedPlusShiftConfiguration::try_new(
        parse_required_float_field_from_mapping(
            configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "alpha",
        )?,
        parse_required_float_field_from_mapping(
            configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "beta_0",
        )?,
        parse_required_float_field_from_mapping(
            configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "delta",
        )?,
        parse_required_float_field_from_mapping(
            configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "mu",
        )?,
        parse_required_float_field_from_mapping(
            configuration_mapping,
            "nig_shared_plus_shift_configuration",
            "delta_beta",
        )?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "nig_shared_plus_shift_configuration payload failed NIG validation: {source}"
        ))
    })
}

fn parse_required_hawkes_kernel_loadings_field(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<HawkesKernelLoadings> {
    let loadings_mapping =
        parse_required_mapping_field(parent_mapping, parent_context, field_name)?;

    let loadings_context = format!("{parent_context}.{field_name}");

    Ok(HawkesKernelLoadings {
        plus_plus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "plus_plus",
        )?,
        plus_minus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "plus_minus",
        )?,
        minus_plus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "minus_plus",
        )?,
        minus_minus: parse_required_float_field_from_mapping(
            &loadings_mapping,
            &loadings_context,
            "minus_minus",
        )?,
    })
}

fn parse_required_mapping_field<'py>(
    parent_mapping: &Bound<'py, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<Bound<'py, PyMapping>> {
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    let field_mapping = field_value.cast::<PyMapping>().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a mapping"
        ))
    })?;

    Ok(field_mapping.clone())
}

fn parse_required_float_field_from_mapping(
    parent_mapping: &Bound<'_, PyMapping>,
    parent_context: &str,
    field_name: &str,
) -> PyResult<f64> {
    let field_value = parse_required_field(parent_mapping, parent_context, field_name)?;

    let parsed_value = field_value.extract::<f64>().map_err(|_| {
        SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a finite number"
        ))
    })?;

    if !parsed_value.is_finite() {
        return Err(SeasonalityInputValidationError::new_err(format!(
            "{parent_context}.{field_name} must be a finite number"
        )));
    }

    Ok(parsed_value)
}
