use std::time::Instant;

use chrono::Utc;
use janiform_foundations::cancellation::CancellationToken;
use janiform_foundations::parallelism::resolved_cpu_count;
use janiform_terminal_ui::{ProgressAdapter, TerminalSummaryPolicy};
use serde_json::json;

use crate::error::WorkflowError;
use crate::model::persisted_model_run_id;
use crate::progress::VolatileProgressGate;

use super::artifacts::{
    FORECAST_BUNDLE_ARTIFACT_COUNT, save_forecast_bundle_with_progress, table_file_name,
};
use super::distribution::{
    DistributionProjection, build_distribution_projection, solve_terminal_law,
};
use super::expected_path::build_expected_path;
use super::output::{
    FORECAST_SCHEMA_VERSION, ForecastBundleManifest, ForecastExpectedPathRow, ForecastOutput,
    ForecastScalarSummary, ForecastWarning, expected_path_table_schema,
    terminal_distribution_table_schema,
};
use super::progress::{
    ForecastProgressEvent, ForecastProgressPhase, complete_forecast_progress,
    complete_forecast_stage_with_warnings, emit_forecast_completion_summary,
    emit_forecast_start_summary, fail_forecast_progress, observe_gated_forecast_terminal_event,
    start_forecast_progress,
};
use super::request::{ForecastWorkflowRequest, validation_error};
use super::runtime::{
    ForecastRuntime, ResolvedForecastRequest, build_runtime, resolve_horizon, validate_request,
};

/// Observer sinks for forecast workflow execution.
#[derive(Default)]
pub struct ForecastWorkflowObserver<'a> {
    /// Cooperative cancellation token for long-running workflow work.
    pub cancellation_token: CancellationToken,
    /// Terminal progress adapter.
    pub progress_adapter: Option<&'a mut ProgressAdapter>,
    /// Terminal output for run start and completion summaries.
    pub terminal_output: Option<TerminalSummaryPolicy>,
    /// Optional structured progress event observer.
    pub progress_event_observer: Option<&'a mut dyn FnMut(ForecastProgressEvent)>,
}

/// Run a model-based forecast.
pub fn run_forecast(
    request: ForecastWorkflowRequest,
    mut observer: ForecastWorkflowObserver<'_>,
) -> Result<ForecastOutput, WorkflowError> {
    let request = request.with_boundary_defaults();

    validate_request(&request)?;

    let cancellation_token = observer.cancellation_token.shared_handle();

    let started_at = Instant::now();
    let run_id = request.metadata.run_id();

    emit_forecast_start_summary(
        &observer.terminal_output,
        run_id.as_str(),
        request.measure,
        request.horizon_interval_count,
        request.metadata.output_path.as_deref(),
    )?;

    start_forecast_progress(
        observer.progress_adapter.as_deref_mut(),
        run_id.as_str(),
        1,
        request.metadata.output_path.as_deref(),
    )?;

    let mut progress_gate = VolatileProgressGate::default();
    let mut progress_error = None;

    let output_result: Result<ForecastOutput, WorkflowError> = {
        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(ForecastProgressPhase::ResolvingHorizon, 0, 1),
        );

        cancellation_token.check_cancelled()?;

        let resolved = resolve_horizon(&request)?;

        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(ForecastProgressPhase::ResolvingHorizon, 1, 1),
        );

        fail_if_forecast_warnings_are_errors(
            request.treat_forecast_warnings_as_errors,
            resolved.warnings.as_slice(),
        )?;

        if !resolved.warnings.is_empty() {
            complete_forecast_stage_with_warnings(
                observer.progress_adapter.as_deref_mut(),
                ForecastProgressPhase::ResolvingHorizon,
            )?;
        }

        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(
                ForecastProgressPhase::MaterializingIntervals,
                0,
                resolved.horizon_interval_count,
            ),
        );

        cancellation_token.check_cancelled()?;

        let runtime = build_runtime(&request, &resolved)?;

        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(
                ForecastProgressPhase::MaterializingIntervals,
                resolved.horizon_interval_count,
                resolved.horizon_interval_count,
            ),
        );

        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(
                ForecastProgressPhase::ProjectingExpectedPath,
                0,
                resolved.horizon_interval_count,
            ),
        );

        cancellation_token.check_cancelled()?;

        let expected_path = {
            let mut emit_progress = |event: ForecastProgressEvent| {
                emit_forecast_progress(
                    &mut observer,
                    &mut progress_gate,
                    &mut progress_error,
                    event,
                );
            };

            build_expected_path(
                &request,
                &resolved,
                &runtime,
                cancellation_token.shared_handle(),
                &mut emit_progress,
            )?
        };

        cancellation_token.check_cancelled()?;

        let terminal_law = {
            let mut emit_progress = |event: ForecastProgressEvent| {
                emit_forecast_progress(
                    &mut observer,
                    &mut progress_gate,
                    &mut progress_error,
                    event,
                );
            };

            solve_terminal_law(&request, &resolved, &runtime, &mut emit_progress)?
        };

        if terminal_law.has_warnings() {
            complete_forecast_stage_with_warnings(
                observer.progress_adapter.as_deref_mut(),
                ForecastProgressPhase::SolvingTerminalLaw,
            )?;
        }

        emit_forecast_progress(
            &mut observer,
            &mut progress_gate,
            &mut progress_error,
            ForecastProgressEvent::new(
                ForecastProgressPhase::BuildingDistribution,
                0,
                request.distribution_node_count,
            ),
        );

        cancellation_token.check_cancelled()?;

        let distribution = {
            let mut emit_progress = |event: ForecastProgressEvent| {
                emit_forecast_progress(
                    &mut observer,
                    &mut progress_gate,
                    &mut progress_error,
                    event,
                );
            };

            build_distribution_projection(
                &request,
                &runtime,
                terminal_law,
                cancellation_token.shared_handle(),
                &mut emit_progress,
            )?
        };

        fail_if_forecast_warnings_are_errors(
            request.treat_forecast_warnings_as_errors,
            distribution.warnings.as_slice(),
        )?;

        let output = build_output(
            &request,
            &resolved,
            &runtime,
            run_id,
            expected_path,
            distribution,
            started_at.elapsed(),
        )?;

        let output = if let Some(output_path) = request.metadata.output_path.as_deref() {
            emit_forecast_progress(
                &mut observer,
                &mut progress_gate,
                &mut progress_error,
                ForecastProgressEvent::new(
                    ForecastProgressPhase::WritingForecastBundle,
                    0,
                    FORECAST_BUNDLE_ARTIFACT_COUNT,
                ),
            );

            let bundle_path = save_forecast_bundle_with_progress(
                &output,
                output_path,
                request.table_format,
                |written_artifacts| {
                    emit_forecast_progress(
                        &mut observer,
                        &mut progress_gate,
                        &mut progress_error,
                        ForecastProgressEvent::new(
                            ForecastProgressPhase::WritingForecastBundle,
                            written_artifacts,
                            FORECAST_BUNDLE_ARTIFACT_COUNT,
                        ),
                    );
                },
            )?;

            ForecastOutput {
                bundle_path: Some(bundle_path),
                ..output
            }
        } else {
            output
        };

        Ok(output)
    };

    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            fail_forecast_progress(observer.progress_adapter, error.to_string().as_str())?;

            return Err(error);
        }
    };

    complete_forecast_progress(observer.progress_adapter, progress_error)?;
    emit_forecast_completion_summary(&observer.terminal_output, &output)?;

    Ok(output)
}

/// Run a forecast with terminal progress according to the requested progress mode.
pub fn run_forecast_with_terminal_progress(
    request: ForecastWorkflowRequest,
    cancellation_token: CancellationToken,
    progress_mode: crate::terminal::TerminalProgressMode,
) -> Result<ForecastOutput, WorkflowError> {
    let mut progress_adapter = crate::terminal::stderr_progress_adapter(progress_mode);

    run_forecast(
        request,
        ForecastWorkflowObserver {
            cancellation_token,
            progress_adapter: progress_adapter.as_mut(),
            terminal_output: crate::terminal::terminal_summary_policy(progress_mode),
            progress_event_observer: None,
        },
    )
}

fn emit_forecast_progress(
    observer: &mut ForecastWorkflowObserver<'_>,
    progress_gate: &mut VolatileProgressGate<ForecastProgressPhase>,
    progress_error: &mut Option<WorkflowError>,
    event: ForecastProgressEvent,
) {
    if let Some(progress_event_observer) = observer.progress_event_observer.as_deref_mut() {
        progress_event_observer(event.clone());
    }

    observe_gated_forecast_terminal_event(
        &mut observer.progress_adapter,
        progress_gate,
        progress_error,
        event,
    );
}

fn fail_if_forecast_warnings_are_errors(
    treat_forecast_warnings_as_errors: bool,
    warnings: &[ForecastWarning],
) -> Result<(), WorkflowError> {
    if !treat_forecast_warnings_as_errors || warnings.is_empty() {
        return Ok(());
    }

    Err(validation_error(
        "treat_forecast_warnings_as_errors",
        warnings
            .iter()
            .map(|warning| warning.message.as_str())
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

fn build_output(
    request: &ForecastWorkflowRequest,
    resolved: &ResolvedForecastRequest,
    runtime: &ForecastRuntime,
    run_id: String,
    expected_path: Vec<ForecastExpectedPathRow>,
    distribution: DistributionProjection,
    execution_duration: std::time::Duration,
) -> Result<ForecastOutput, WorkflowError> {
    let expected_path_file_name = table_file_name("expected_path", request.table_format);

    let terminal_distribution_file_name =
        table_file_name("terminal_distribution", request.table_format);

    let expected_terminal = expected_path
        .last()
        .ok_or_else(|| validation_error("expected_path", "expected path must not be empty"))?;

    let manifest = ForecastBundleManifest {
        schema_version: FORECAST_SCHEMA_VERSION.to_string(),
        run_id,
        model_run_id: persisted_model_run_id(&request.model_payload)?,
        created_at_utc: Utc::now(),
        model_path: request.metadata.model_path.clone(),
        original_request: serde_json::to_value(request)
            .map_err(|source| WorkflowError::OutputSerialize { source })?,
        resolved_request: json!({
            "start_date_utc": request.start_date_utc.to_rfc3339(),
            "end_date_utc": resolved.end_date_utc.to_rfc3339(),
            "horizon_interval_count": resolved.horizon_interval_count,
            "interval_duration_nanos": request.interval_duration_nanos,
            "initial_price": runtime.initial_price,
            "measure": request.measure,
            "resolved_cpu_count": resolved_cpu_count(request.cpu_count),
        }),
        measure: request.measure,
        initial_state_policy: runtime.initial_state_policy.to_string(),
        reference_price: runtime.reference_price,
        interval_duration_nanos: request.interval_duration_nanos,
        resolved_horizon_interval_count: resolved.horizon_interval_count,
        start_timestamp_utc: request.start_date_utc,
        end_timestamp_utc: resolved.end_date_utc,
        warnings: resolved
            .warnings
            .iter()
            .cloned()
            .chain(distribution.warnings.iter().cloned())
            .collect(),
        expected_path_table: expected_path_table_schema(
            expected_path_file_name,
            request.table_format,
            expected_path.len(),
        ),
        terminal_distribution_table: terminal_distribution_table_schema(
            terminal_distribution_file_name,
            request.table_format,
            distribution.rows.len(),
        ),
        distribution_support: distribution.support,
        quantiles: distribution.quantiles,
        scalar_summary: ForecastScalarSummary {
            initial_price: runtime.initial_price,
            expected_terminal_price_ratio: expected_terminal.expected_price_ratio,
            expected_terminal_price: expected_terminal.expected_price,
            start_timestamp_utc: request.start_date_utc,
            end_timestamp_utc: resolved.end_date_utc,
        },
    };

    Ok(ForecastOutput {
        manifest,
        expected_path,
        terminal_distribution: distribution.rows,
        bundle_path: None,
        execution_duration,
    })
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::super::runtime::{resolve_horizon, validate_request};
    use crate::forecast::{
        DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT, DEFAULT_FORECAST_INITIAL_PRICE,
        ForecastInitialState, ForecastMeasure, ForecastTableFormat, ForecastWorkflowMetadata,
        ForecastWorkflowRequest,
    };

    #[test]
    fn validate_request_requires_exactly_one_horizon_selector() {
        let mut request = forecast_request();

        request.horizon_interval_count = None;
        request.end_date_utc = None;

        assert!(validate_request(&request).is_err());

        request.horizon_interval_count = Some(1);
        request.end_date_utc = Some(start_date_utc());

        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn validate_request_accepts_omitted_initial_price_without_state() {
        let mut request = forecast_request();

        request.initial_price = None;

        assert!(validate_request(&request).is_ok());
    }

    #[test]
    fn validate_request_rejects_initial_price_with_state() {
        let mut request = forecast_request();

        request.state = Some(forecast_initial_state());

        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn resolve_horizon_by_count_uses_exact_interval_count() {
        let request = forecast_request();
        let resolved = resolve_horizon(&request).expect("horizon should resolve");

        assert_eq!(resolved.horizon_interval_count, 4);
        assert_eq!(
            resolved.end_date_utc,
            Utc.with_ymd_and_hms(2026, 2, 2, 16, 0, 0)
                .single()
                .expect("test timestamp should be valid")
        );
        assert!(resolved.warnings.is_empty());
    }

    #[test]
    fn resolve_horizon_by_end_date_ceil_records_alignment_warning() {
        let mut request = forecast_request();

        request.horizon_interval_count = None;

        request.end_date_utc = Some(
            Utc.with_ymd_and_hms(2026, 2, 2, 15, 20, 0)
                .single()
                .expect("test timestamp should be valid"),
        );

        let resolved = resolve_horizon(&request).expect("horizon should resolve");

        assert_eq!(resolved.horizon_interval_count, 2);
        assert_eq!(
            resolved.end_date_utc,
            Utc.with_ymd_and_hms(2026, 2, 2, 15, 30, 0)
                .single()
                .expect("test timestamp should be valid")
        );
        assert_eq!(resolved.warnings.len(), 1);
    }

    fn forecast_request() -> ForecastWorkflowRequest {
        ForecastWorkflowRequest {
            metadata: ForecastWorkflowMetadata::default(),
            model_payload: json!({}),
            start_date_utc: start_date_utc(),
            initial_price: Some(DEFAULT_FORECAST_INITIAL_PRICE),
            state: None,
            interval_duration_nanos: 15 * 60 * 1_000_000_000,
            horizon_interval_count: Some(4),
            end_date_utc: None,
            measure: ForecastMeasure::Physical,
            risk_free_rate: 0.0,
            dividend_yield: 0.0,
            table_format: ForecastTableFormat::Parquet,
            distribution_node_count: DEFAULT_FORECAST_DISTRIBUTION_NODE_COUNT,
            distribution_log_return_lower: None,
            distribution_log_return_upper: None,
            cpu_count: None,
            treat_forecast_warnings_as_errors: false,
        }
    }

    fn forecast_initial_state() -> ForecastInitialState {
        ForecastInitialState {
            hawkes_excitation_state: janiform::model::hawkes::HawkesExcitationState {
                y_plus_plus: 1.0,
                y_plus_minus: 2.0,
                y_minus_plus: 3.0,
                y_minus_minus: 4.0,
            },
            long_variance_state: janiform::model::long_variance::LongVarianceState::unit(),
            mean_reversion_boundary_state: janiform::simulation::MeanReversionBoundaryState {
                observed_log_price: 0.02,
                anchor_value: 0.015,
                pressure_state: 0.1,
                deviation: 0.005,
            },
        }
    }

    fn start_date_utc() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 2, 15, 0, 0)
            .single()
            .expect("test timestamp should be valid")
    }
}
