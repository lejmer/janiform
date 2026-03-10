use janiform_core::hawkes::{HawkesExcitationState, HawkesKernelLoadings, HawkesKernelParameters};
use janiform_core::simulation::{
    CoarseSimulationRunRequest, SeededTickEventStateSnapshotCapture,
    SeededTickSimulationDebugTraceOptions,
    SeededTickSimulationModelInputs, SeededTickSimulationNewsMarkScales,
    SeededTickSimulationRunConfig, SeededTickSimulationRunOptions,
    run_coarse_simulation as run_coarse_simulation_core,
    run_seeded_tick_event_simulation as run_seeded_tick_event_simulation_core,
};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyMapping, PyModule};

use super::calendar::parse_timestamp_utc;
use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::types::PyHourlyFactorContractResult;

#[pyfunction]
fn run_coarse_simulation(run_request_json: &str) -> PyResult<String> {
    let run_request: CoarseSimulationRunRequest =
        serde_json::from_str(run_request_json).map_err(|source| {
            SimulationExecutionError::new_err(format!(
                "failed to deserialize coarse simulation run request: {source}"
            ))
        })?;
    let coarse_path = run_coarse_simulation_core(run_request)
        .map_err(|source| SimulationExecutionError::new_err(source.to_string()))?;

    serde_json::to_string(&coarse_path).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "failed to serialize coarse simulation path payload: {source}"
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (seed, start_time_utc, horizon_interval_count, kernel_parameters, initial_excitation_state, hourly_factor_contract, *, debug_trace_enabled = false, sample_every_nth_transition = 1, max_records_per_chunk = 1024, state_snapshot_capture = "disabled"))]
fn run_seeded_tick_event_simulation(
    py: Python<'_>,
    seed: u64,
    start_time_utc: &str,
    horizon_interval_count: usize,
    kernel_parameters: &Bound<'_, PyAny>,
    initial_excitation_state: &Bound<'_, PyAny>,
    hourly_factor_contract: &PyHourlyFactorContractResult,
    debug_trace_enabled: bool,
    sample_every_nth_transition: u64,
    max_records_per_chunk: usize,
    state_snapshot_capture: &str,
) -> PyResult<String> {
    let parsed_start_time_utc = parse_timestamp_utc(start_time_utc)?;
    let parsed_kernel_parameters = parse_seeded_simulation_kernel_parameters(kernel_parameters)?;
    let parsed_initial_excitation_state =
        parse_seeded_simulation_initial_excitation_state(initial_excitation_state)?;
    let parsed_state_snapshot_capture =
        parse_seeded_simulation_state_snapshot_capture(state_snapshot_capture)?;

    let run_options = SeededTickSimulationRunOptions {
        debug_trace_enabled,
        debug_trace_options: SeededTickSimulationDebugTraceOptions {
            sample_every_nth_transition,
            max_records_per_chunk,
        },
        state_snapshot_capture: parsed_state_snapshot_capture,
    };

    let run_config = SeededTickSimulationRunConfig::try_new(
        seed,
        parsed_start_time_utc,
        horizon_interval_count,
        SeededTickSimulationModelInputs {
            kernel_parameters: parsed_kernel_parameters,
            initial_excitation_state: parsed_initial_excitation_state,
            hourly_factor_contract: hourly_factor_contract.output.clone(),
            calendar_registry: hourly_factor_contract.calendar_registry.clone(),
            known_scheduled_news_event_timestamps_utc_nanos: Vec::new(),
            known_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
            surprise_news_mark_scales: SeededTickSimulationNewsMarkScales::default(),
        },
        Some(run_options),
    )
    .map_err(map_seeded_tick_simulation_run_error)?;

    let run_result = py
        .allow_threads(|| run_seeded_tick_event_simulation_core(run_config))
        .map_err(map_seeded_tick_simulation_run_error)?;

    run_result.serialize_event_tape_json().map_err(|_| {
        SeasonalityInputValidationError::new_err(
            "failed to serialize seeded tick event tape payload to json",
        )
    })
}

fn parse_seeded_simulation_kernel_parameters(
    raw_kernel_parameters: &Bound<'_, PyAny>,
) -> PyResult<HawkesKernelParameters> {
    let kernel_parameters_mapping =
        raw_kernel_parameters.downcast::<PyMapping>().map_err(|_| {
            SeasonalityInputValidationError::new_err("kernel_parameters must be a mapping")
        })?;

    let lambda_bar = parse_required_float_field_from_mapping(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "lambda_bar",
    )?;
    let beta = parse_required_float_field_from_mapping(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "beta",
    )?;
    let alpha = parse_required_hawkes_kernel_loadings_field(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "alpha",
    )?;
    let rho = parse_required_hawkes_kernel_loadings_field(
        &kernel_parameters_mapping,
        "kernel_parameters",
        "rho",
    )?;

    HawkesKernelParameters::try_new(lambda_bar, beta, alpha, rho).map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "kernel_parameters payload failed Hawkes validation: {source}"
        ))
    })
}

fn parse_seeded_simulation_initial_excitation_state(
    raw_initial_excitation_state: &Bound<'_, PyAny>,
) -> PyResult<HawkesExcitationState> {
    let state_mapping = raw_initial_excitation_state
        .downcast::<PyMapping>()
        .map_err(|_| {
            SeasonalityInputValidationError::new_err("initial_excitation_state must be a mapping")
        })?;

    Ok(HawkesExcitationState {
        y_plus_plus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_plus_plus",
        )?,
        y_plus_minus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_plus_minus",
        )?,
        y_minus_plus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_minus_plus",
        )?,
        y_minus_minus: parse_required_float_field_from_mapping(
            &state_mapping,
            "initial_excitation_state",
            "y_minus_minus",
        )?,
    })
}

fn parse_seeded_simulation_state_snapshot_capture(
    raw_state_snapshot_capture: &str,
) -> PyResult<SeededTickEventStateSnapshotCapture> {
    match raw_state_snapshot_capture {
        "disabled" => Ok(SeededTickEventStateSnapshotCapture::Disabled),
        "post_event_transition" => Ok(SeededTickEventStateSnapshotCapture::PostEventTransition),
        _ => Err(SeasonalityInputValidationError::new_err(format!(
            "state_snapshot_capture must be either 'disabled' or 'post_event_transition', received '{raw_state_snapshot_capture}'"
        ))),
    }
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
    let field_value = parent_mapping
        .contains(field_name)?
        .then(|| parent_mapping.get_item(field_name))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "{parent_context} is missing required field '{field_name}'"
            ))
        })??;

    let field_mapping = field_value.downcast::<PyMapping>().map_err(|_| {
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
    let field_value = parent_mapping
        .contains(field_name)?
        .then(|| parent_mapping.get_item(field_name))
        .ok_or_else(|| {
            SeasonalityInputValidationError::new_err(format!(
                "{parent_context} is missing required field '{field_name}'"
            ))
        })??;

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

fn map_seeded_tick_simulation_run_error(
    error: janiform_core::simulation::SeededTickSimulationRunError,
) -> PyErr {
    SeasonalityInputValidationError::new_err(error.to_string())
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_coarse_simulation, module)?)?;
    module.add_function(wrap_pyfunction!(run_seeded_tick_event_simulation, module)?)?;
    Ok(())
}
