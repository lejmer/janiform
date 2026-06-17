use janiform::model::affine_oracle::{
    EndogenousAnchorIntervalFactors, ExponentialAffineTransformEquationSpecification,
    materialize_endogenous_anchor_path,
};
use janiform::model::long_variance::LongVarianceParameters;
use janiform::model::news::NewsProcessConfiguration;
use janiform::simulation::{
    materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments,
    run_coarse_simulation as run_coarse_simulation_core,
    run_coarse_simulation_with_observer as run_coarse_simulation_core_with_observer,
    run_seeded_tick_event_simulation as run_seeded_tick_event_simulation_core,
    run_seeded_tick_event_simulation_with_observer as run_seeded_tick_event_simulation_core_with_observer,
};
use janiform_workflows::simulation::{
    run_coarse_simulation_with_terminal_progress,
    run_seeded_tick_event_simulation_with_terminal_progress,
    run_simulation_from_model_artifact as run_simulation_from_model_artifact_core,
    run_simulation_from_model_artifact_with_terminal_progress,
};
use janiform_workflows::terminal::TerminalProgressMode;
use pyo3::prelude::{Bound, Py, PyErr, PyResult, Python};
use pyo3::types::{PyAny, PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use super::events::run_simulation_with_optional_progress_events;
use super::model_inputs::{
    parse_required_numeric_sequence, parse_seeded_simulation_initial_excitation_state,
    parse_seeded_simulation_kernel_parameters,
    parse_seeded_simulation_mean_reversion_fitted_parameters,
    parse_seeded_simulation_nig_shared_plus_shift_configuration,
};
use super::output::{
    map_seeded_tick_simulation_run_error, simulation_from_model_artifact_output_to_py_object,
    workflow_error_to_simulation_error,
};
use super::request::{
    CoarseSimulationRunRequestPayload, ModelArtifactSimulationWorkflowPayload,
    SeededTickSimulationRunConfigPayload,
};
use crate::SeasonalityInputValidationError;
use crate::SimulationExecutionError;
use crate::native_interrupt::{NativeInterruptMode, run_interruptible_native};
use crate::panic_boundary::panic_to_pyresult;
use crate::python_value::{json_value_to_py_object, py_any_to_json_value};
use crate::types::PySeededTickSimulationNativeResult;

#[pyfunction]
#[pyo3(signature = (request, event_callback = None, progress_mode = "none"))]
fn run_simulation_from_model_artifact(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
    progress_mode: &str,
) -> PyResult<Py<PyAny>> {
    let request = ModelArtifactSimulationWorkflowPayload::from_py(request)?.into_workflow_request();
    let progress_mode = parse_simulation_progress_mode(progress_mode)?;

    let output = if event_callback.is_some() {
        run_simulation_with_optional_progress_events(
            py,
            "running simulation from model artifact",
            request,
            event_callback,
            |request, cancellation_token| {
                run_simulation_from_model_artifact_core(request, cancellation_token, &mut |_| {})
                    .map_err(workflow_error_to_simulation_error)
            },
            |request, cancellation_token, observer| {
                run_simulation_from_model_artifact_core(request, cancellation_token, observer)
                    .map_err(workflow_error_to_simulation_error)
            },
        )?
    } else {
        panic_to_pyresult(
            "running simulation from model artifact",
            SimulationExecutionError::new_err,
            || {
                run_interruptible_native(
                    py,
                    NativeInterruptMode::ImportedApi,
                    move |cancellation_token| {
                        run_simulation_from_model_artifact_with_terminal_progress(
                            request,
                            cancellation_token,
                            progress_mode,
                        )
                        .map_err(workflow_error_to_simulation_error)
                    },
                )
            },
        )?
    };

    simulation_from_model_artifact_output_to_py_object(py, output)
}

#[pyfunction]
#[pyo3(signature = (kernel_parameters, nig_shared_plus_shift_configuration, news_process_configuration, long_variance_parameters, initial_excitation_state, mean_reversion_fitted_parameters, interval_alignment_policy, start_anchor_value, seasonality_factor, known_news_intensity, surprise_news_intensity, interval_deltas))]
#[expect(
    clippy::too_many_arguments,
    reason = "PyO3 boundary mirrors the public Python keyword API."
)]
fn materialize_endogenous_mean_reversion_runtime_interval_anchors(
    py: Python<'_>,
    kernel_parameters: &Bound<'_, PyAny>,
    nig_shared_plus_shift_configuration: &Bound<'_, PyAny>,
    news_process_configuration: &Bound<'_, PyAny>,
    long_variance_parameters: &Bound<'_, PyAny>,
    initial_excitation_state: &Bound<'_, PyAny>,
    mean_reversion_fitted_parameters: &Bound<'_, PyAny>,
    interval_alignment_policy: &str,
    start_anchor_value: f64,
    seasonality_factor: &Bound<'_, PyAny>,
    known_news_intensity: &Bound<'_, PyAny>,
    surprise_news_intensity: &Bound<'_, PyAny>,
    interval_deltas: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    let parsed_kernel_parameters = parse_seeded_simulation_kernel_parameters(kernel_parameters)?;

    let parsed_nig_shared_plus_shift_configuration =
        parse_seeded_simulation_nig_shared_plus_shift_configuration(
            nig_shared_plus_shift_configuration,
        )?;

    let parsed_news_process_configuration = serde_json::from_value::<NewsProcessConfiguration>(
        py_any_to_json_value(news_process_configuration)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "news_process_configuration failed to deserialize: {source}"
        ))
    })?;

    let parsed_long_variance_parameters = serde_json::from_value::<LongVarianceParameters>(
        py_any_to_json_value(long_variance_parameters)?,
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "long_variance_parameters failed to deserialize: {source}"
        ))
    })?;

    let parsed_initial_excitation_state =
        parse_seeded_simulation_initial_excitation_state(initial_excitation_state)?;

    let parsed_mean_reversion_fitted_parameters =
        parse_seeded_simulation_mean_reversion_fitted_parameters(mean_reversion_fitted_parameters)?;

    let parsed_seasonality_factor =
        parse_required_numeric_sequence(seasonality_factor, "seasonality_factor")?;

    let parsed_known_news_intensity =
        parse_required_numeric_sequence(known_news_intensity, "known_news_intensity")?;

    let parsed_surprise_news_intensity =
        parse_required_numeric_sequence(surprise_news_intensity, "surprise_news_intensity")?;

    let parsed_interval_deltas =
        parse_required_numeric_sequence(interval_deltas, "interval_deltas")?;

    let interval_count = parsed_seasonality_factor.len();
    if parsed_known_news_intensity.len() != interval_count
        || parsed_surprise_news_intensity.len() != interval_count
        || parsed_interval_deltas.len() != interval_count
    {
        return Err(SeasonalityInputValidationError::new_err(
            "endogenous mean-reversion runtime anchor materialization requires `seasonality_factor`, `known_news_intensity`, `surprise_news_intensity`, and `interval_deltas` to have identical lengths",
        ));
    }

    let interval_factor_grids: Result<Vec<EndogenousAnchorIntervalFactors>, PyErr> =
        parsed_seasonality_factor
            .iter()
            .zip(parsed_known_news_intensity.iter())
            .zip(parsed_surprise_news_intensity.iter())
            .zip(parsed_interval_deltas.iter())
            .map(
                |(((seasonality_factor, known_intensity), surprise_intensity), interval_delta)| {
                    EndogenousAnchorIntervalFactors::try_new(
                        *seasonality_factor,
                        *known_intensity,
                        *surprise_intensity,
                        *interval_delta,
                    )
                    .map_err(|source| {
                        SeasonalityInputValidationError::new_err(format!(
                            "endogenous mean-reversion interval factors failed validation: {source}"
                        ))
                    })
                },
            )
            .collect();

    let interval_factor_grids = interval_factor_grids?;

    let materialization = materialize_endogenous_anchor_path(
        interval_alignment_policy,
        start_anchor_value,
        ExponentialAffineTransformEquationSpecification::new(
            parsed_kernel_parameters,
            parsed_news_process_configuration
                .shared_mark_law
                .gamma_severity_marks,
            parsed_long_variance_parameters,
        ),
        parsed_nig_shared_plus_shift_configuration,
        &parsed_news_process_configuration,
        parsed_initial_excitation_state,
        interval_factor_grids.as_slice(),
    )
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "endogenous mean-reversion runtime anchor materialization failed: {source}"
        ))
    })?;

    let core_expected_increments: Vec<f64> = materialization
        .interval_expectations
        .iter()
        .map(|expectation| expectation.core_expected_increment)
        .collect();

    let runtime_interval_anchors =
        materialize_mean_reversion_runtime_interval_anchors_from_core_expected_increments(
            interval_alignment_policy,
            start_anchor_value,
            0.0,
            &parsed_mean_reversion_fitted_parameters.anchor_model,
            core_expected_increments.as_slice(),
            &parsed_interval_deltas,
        )
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "endogenous mean-reversion runtime anchor materialization failed: {source}"
            ))
        })?;

    let serialized_runtime_interval_anchors = serde_json::to_value(&runtime_interval_anchors)
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "failed to serialize endogenous mean-reversion runtime anchors: {source}"
            ))
        })?;

    json_value_to_py_object(py, &serialized_runtime_interval_anchors)
}

#[pyfunction]
#[pyo3(signature = (request, event_callback = None, progress_mode = "none"))]
fn run_coarse_simulation(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
    progress_mode: &str,
) -> PyResult<Py<PyAny>> {
    let run_request = CoarseSimulationRunRequestPayload::from_py(request)?.into_run_request()?;
    let progress_mode = parse_simulation_progress_mode(progress_mode)?;

    let coarse_path = if event_callback.is_some() {
        run_simulation_with_optional_progress_events(
            py,
            "running coarse simulation",
            run_request,
            event_callback,
            |run_request, cancellation_token| {
                run_coarse_simulation_core(run_request, cancellation_token)
                    .map_err(|source| SimulationExecutionError::new_err(source.to_string()))
            },
            |run_request, cancellation_token, observer| {
                run_coarse_simulation_core_with_observer(run_request, cancellation_token, observer)
                    .map_err(|source| SimulationExecutionError::new_err(source.to_string()))
            },
        )?
    } else {
        panic_to_pyresult(
            "running coarse simulation",
            SimulationExecutionError::new_err,
            || {
                run_interruptible_native(
                    py,
                    NativeInterruptMode::ImportedApi,
                    move |cancellation_token| {
                        run_coarse_simulation_with_terminal_progress(
                            run_request,
                            cancellation_token,
                            progress_mode,
                        )
                        .map_err(workflow_error_to_simulation_error)
                    },
                )
            },
        )?
    };

    let serialized_coarse_path = serde_json::to_value(&coarse_path).map_err(|source| {
        SimulationExecutionError::new_err(format!(
            "failed to serialize coarse simulation path payload: {source}"
        ))
    })?;

    json_value_to_py_object(py, &serialized_coarse_path)
}

#[pyfunction]
#[pyo3(signature = (request, event_callback = None, progress_mode = "none"))]
fn run_seeded_tick_event_simulation(
    py: Python<'_>,
    request: &Bound<'_, PyAny>,
    event_callback: Option<&Bound<'_, PyAny>>,
    progress_mode: &str,
) -> PyResult<PySeededTickSimulationNativeResult> {
    let run_config = SeededTickSimulationRunConfigPayload::from_py(request)?.into_run_config()?;
    let progress_mode = parse_simulation_progress_mode(progress_mode)?;

    let run_result = if event_callback.is_some() {
        run_simulation_with_optional_progress_events(
            py,
            "running seeded tick-event simulation",
            run_config,
            event_callback,
            |run_config, cancellation_token| {
                run_seeded_tick_event_simulation_core(run_config, cancellation_token)
                    .map_err(map_seeded_tick_simulation_run_error)
            },
            |run_config, cancellation_token, observer| {
                run_seeded_tick_event_simulation_core_with_observer(
                    run_config,
                    cancellation_token,
                    observer,
                )
                .map_err(map_seeded_tick_simulation_run_error)
            },
        )?
    } else {
        panic_to_pyresult(
            "running seeded tick-event simulation",
            SimulationExecutionError::new_err,
            || {
                run_interruptible_native(
                    py,
                    NativeInterruptMode::ImportedApi,
                    move |cancellation_token| {
                        run_seeded_tick_event_simulation_with_terminal_progress(
                            run_config,
                            cancellation_token,
                            progress_mode,
                        )
                        .map_err(workflow_error_to_simulation_error)
                    },
                )
            },
        )?
    };

    Ok(PySeededTickSimulationNativeResult { output: run_result })
}

fn parse_simulation_progress_mode(value: &str) -> PyResult<TerminalProgressMode> {
    TerminalProgressMode::from_boundary_value(value)
        .map_err(|message| SimulationExecutionError::new_err(format!("simulation {message}")))
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(run_coarse_simulation, module)?)?;

    module.add_function(wrap_pyfunction!(
        materialize_endogenous_mean_reversion_runtime_interval_anchors,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(
        run_simulation_from_model_artifact,
        module
    )?)?;

    module.add_function(wrap_pyfunction!(run_seeded_tick_event_simulation, module)?)?;

    Ok(())
}
