use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use janiform::calibration::artifacts::CalibrationExecutionMode;
use janiform::calibration::market_data::{
    MarketDataPreparationProgressError, MarketDataPreparationProgressEvent,
    OptionForwardParityWarningPolicy, PreparedMarketDataCalibrationPlan,
    infer_market_data_run_id_from_path, prepare_market_data_calibration_plan,
    prepare_market_data_calibration_plan_with_observer,
};
use janiform::calibration::presets::ResolvedMarketProfile;
use janiform::calibration::staged::{
    CalibrationDiagnosticsRequest, CalibrationExtendedDiagnosticKind,
    CalibrationMarketDataCarryInput, CalibrationMarketDataMeanReversionConfiguration,
    CalibrationMarketDataOutputPreferences, CalibrationMarketDataPreparationError,
    CalibrationMarketDataPreparationRequest, CalibrationMarketDataWorkflowScope,
};
use janiform_terminal_ui::{ProgressAdapter, ProgressEvent, ProgressMeasure, format_path};
use janiform_workflows::fit::{
    FitWorkflowPaths, fit_preparation_input_count, fit_preparation_start_summary,
    fit_workflow_paths, market_data_file_format_label, tick_price_policy_label,
    workflow_scope_label,
};
use janiform_workflows::market_data_progress::{
    MarketDataLoadingProgressGate, market_data_loading_progress_measure,
    market_data_loading_progress_signal, market_data_preparation_progress_signal,
};

use super::labels::workflow_scope_argument_label;
use crate::commands::options;
use crate::commands::options::CalibrationDiagnosticOption;
use crate::commands::options::TickDataOptions;
use crate::commands::options::WorkflowScope;
use crate::commands::shared::calibration::price_coordinate_policy;
use crate::commands::shared::calibration::tick_objective_data_usage_policy;
use crate::commands::shared::market_data::prepare_market_data_command_inputs;
use crate::commands::shared::output::write_pretty_json_file;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::commands::shared::terminal::terminal_progress_adapter_for_context;
use crate::context::{CommandContext, OutputMode};
use crate::error::CliError;

const FIT_PREPARATION_REQUEST_SCHEMA_VERSION: &str = "calibration.market_data.request.v1";

#[derive(Clone, Debug, Default)]
pub(crate) struct FitPreparationOverrides {
    pub(crate) mean_reversion_grid_interval_hours: Option<f64>,
    pub(crate) dynamics_search_restart_candidate_budget: Option<usize>,
    pub(crate) option_refinement_candidate_count: Option<usize>,
    pub(crate) diagnostics: Vec<CalibrationDiagnosticOption>,
    pub(crate) treat_calibration_warnings_as_errors: bool,
    pub(crate) stylized_fact_huber_cutoff: Option<f64>,
    pub(crate) stylized_fact_admissibility_z_threshold: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FitPreviewOptions {
    pub(crate) workflow_scope: Option<WorkflowScope>,
    pub(crate) execution_mode: Option<CalibrationExecutionMode>,
    pub(crate) cpu_count: Option<usize>,
    pub(crate) overrides: FitPreparationOverrides,
    pub(crate) no_persist: bool,
}

#[derive(Debug)]
pub(crate) struct PreparedFitPreview {
    pub(crate) request: CalibrationMarketDataPreparationRequest,
    pub(crate) plan: PreparedMarketDataCalibrationPlan,
    pub(crate) paths: FitWorkflowPaths,
    pub(crate) inputs: FitPreviewInputs,
    pub(crate) start_summary_emitted: bool,
    pub(crate) explicit_values: Vec<(String, String)>,
    pub(crate) inferred_values: Vec<(String, String)>,
    pub(crate) preset_defaults: Vec<(String, String)>,
    pub(crate) recommended_defaults: Vec<(String, String)>,
    pub(crate) derived_values: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct FitPreviewInputs {
    pub(crate) tick_data_path: PathBuf,
    pub(crate) coarse_data_path: Option<PathBuf>,
    pub(crate) option_surface_path: Option<PathBuf>,
    pub(crate) known_news_path: Option<PathBuf>,
    pub(crate) session_definition_path: Option<PathBuf>,
    pub(crate) market_profile: Option<ResolvedMarketProfile>,
}

fn calibration_diagnostics_request(
    diagnostics: &[CalibrationDiagnosticOption],
) -> CalibrationDiagnosticsRequest {
    CalibrationDiagnosticsRequest {
        extended: diagnostics
            .iter()
            .map(|diagnostic| match diagnostic {
                CalibrationDiagnosticOption::OptionSurfaceShape => {
                    CalibrationExtendedDiagnosticKind::OptionSurfaceShape
                }
            })
            .collect(),
    }
}

pub(crate) fn prepare_fit_preview(
    context: &CommandContext,
    inputs: &TickDataOptions,
    run: &options::RunIdentityOptions,
    preview_options: FitPreviewOptions,
) -> Result<PreparedFitPreview, CliError> {
    let effective_workflow_scope = resolve_workflow_scope(
        preview_options.workflow_scope,
        inputs.option_surface_path.is_some(),
    )?;

    let prepared_inputs =
        prepare_market_data_command_inputs(context, inputs, effective_workflow_scope)?;

    let run_id = match &run.run_id {
        Some(run_id) => run_id.clone(),
        None => infer_market_data_run_id_from_path(None, &prepared_inputs.tick_data_path).map_err(
            |source| CliError::InvalidCommandArgument {
                argument_name: "--tick-data-path",
                message: source.to_string(),
            },
        )?,
    };

    let output_directory = if preview_options.no_persist {
        std::env::temp_dir()
            .join("janiform-no-persist")
            .join(format!("{}-{}", run_id, std::process::id()))
    } else {
        match &run.output_dir {
            Some(path) => context.paths().output_path(path),
            None => default_run_output_directory(context, &run_id),
        }
    };

    let paths = fit_workflow_paths(output_directory.clone());

    let explicit_option_surface_value = if inputs.option_surface_path.is_some() {
        Some(
            prepared_inputs
                .option_surface_path
                .as_ref()
                .map(|path| format_path(path))
                .unwrap_or_else(|| "none".to_string()),
        )
    } else {
        None
    };

    let preview_inputs = FitPreviewInputs {
        tick_data_path: prepared_inputs.tick_data_path,
        coarse_data_path: prepared_inputs.coarse_data_path,
        option_surface_path: prepared_inputs.option_surface_path,
        known_news_path: prepared_inputs.known_news_path,
        session_definition_path: prepared_inputs.session_definition_path,
        market_profile: prepared_inputs.market_profile,
    };

    let execution_mode = if preview_options.no_persist {
        CalibrationExecutionMode::Memory
    } else {
        preview_options
            .execution_mode
            .unwrap_or(CalibrationExecutionMode::Disk)
    };

    let request = CalibrationMarketDataPreparationRequest {
        schema_version: FIT_PREPARATION_REQUEST_SCHEMA_VERSION.to_string(),
        run_id: run_id.clone(),
        tick_data_source: prepared_inputs.tick_data_source,
        coarse_data_source: prepared_inputs.coarse_data_source,
        session_configuration: prepared_inputs.session_configuration,
        column_mapping: prepared_inputs.column_mapping,
        workflow_scope: effective_workflow_scope,
        normalization_policy: prepared_inputs.normalization_policy,
        output_preferences: CalibrationMarketDataOutputPreferences {
            execution_mode,
            disk_checkpoint_directory: match execution_mode {
                CalibrationExecutionMode::Disk => {
                    Some(paths.output_directory.display().to_string())
                }
                CalibrationExecutionMode::Memory => None,
            },
            emit_input_provenance_report: true,
        },
        optimizer_settings_by_stage: Vec::new(),
        preprocessing_settings: None,
        option_surface_source: prepared_inputs.option_surface_source,
        option_surface_valuation_time_utc: inputs.option_surface_valuation_time_utc.clone(),
        carry: Some(CalibrationMarketDataCarryInput {
            discount_rate: 0.0,
            carry_rate: 0.0,
        }),
        price_coordinate_policy: inputs
            .price_coordinate_policy
            .map(price_coordinate_policy)
            .unwrap_or_default(),
        option_forward_parity_warning_policy: inputs
            .option_forward_parity_warning_threshold
            .map(|relative_threshold| OptionForwardParityWarningPolicy { relative_threshold })
            .unwrap_or_default(),
        pricing_interval_schedule_policy: Default::default(),
        tick_objective_data_usage_policy: inputs
            .tick_objective_data_usage_policy
            .map(tick_objective_data_usage_policy)
            .unwrap_or_default(),
        dynamics_search_restart_candidate_budget: preview_options
            .overrides
            .dynamics_search_restart_candidate_budget,
        calibration_effort: None,
        treat_calibration_warnings_as_errors: preview_options
            .overrides
            .treat_calibration_warnings_as_errors,
        joint_refinement_option_refinement_candidate_count_override: preview_options
            .overrides
            .option_refinement_candidate_count,
        joint_refinement_option_refinement_wall_clock_budget_millis: None,
        joint_refinement_audit_option_pricing_enabled: false,
        diagnostics: calibration_diagnostics_request(
            preview_options.overrides.diagnostics.as_slice(),
        ),
        stylized_fact_huber_cutoff: preview_options.overrides.stylized_fact_huber_cutoff,
        stylized_fact_admissibility_z_threshold: preview_options
            .overrides
            .stylized_fact_admissibility_z_threshold,
        known_news_inputs: prepared_inputs.known_news_inputs,
        mean_reversion: preview_options
            .overrides
            .mean_reversion_grid_interval_hours
            .map(|hours| CalibrationMarketDataMeanReversionConfiguration {
                calibration_grid_interval_hours: Some(hours),
            }),
    };

    let start_summary_emitted = emit_start_summary_before_loading(context, &request, &paths)?;

    let plan = prepare_market_data_calibration_plan_for_cli(
        context,
        request.clone(),
        preview_options.cpu_count,
        paths.output_directory.as_path(),
    )?;

    let mut explicit_values = Vec::new();

    push_if_present(&mut explicit_values, "run id", run.run_id.as_deref());
    push_if_present(
        &mut explicit_values,
        "workflow scope",
        preview_options
            .workflow_scope
            .map(workflow_scope_argument_label)
            .as_deref(),
    );
    push_if_present(
        &mut explicit_values,
        "exchange code",
        inputs.exchange_code.as_deref(),
    );
    push_if_present(
        &mut explicit_values,
        "tick timestamp column",
        inputs.tick_timestamp_column.as_deref(),
    );
    push_if_present(
        &mut explicit_values,
        "tick price column",
        inputs.tick_price_column.as_deref(),
    );

    if inputs.coarse_data_path.is_some() {
        explicit_values.push((
            "coarse data".to_string(),
            preview_inputs
                .coarse_data_path
                .as_ref()
                .map(|path| format_path(path))
                .unwrap_or_else(|| "none".to_string()),
        ));
    }

    if inputs.option_surface_path.is_some() {
        explicit_values.push((
            "option surface".to_string(),
            explicit_option_surface_value.unwrap_or_else(|| "none".to_string()),
        ));
    }

    let mut inferred_values = vec![
        ("run id".to_string(), run_id),
        (
            "tick format".to_string(),
            market_data_file_format_label(request.tick_data_source.file_format).to_string(),
        ),
        (
            "tick price policy".to_string(),
            request
                .normalization_policy
                .tick_price_input_policy
                .map(tick_price_policy_label)
                .unwrap_or("not required")
                .to_string(),
        ),
        (
            "workflow scope".to_string(),
            workflow_scope_label(request.workflow_scope).to_string(),
        ),
    ];
    if inputs.exchange_code.is_none() {
        inferred_values.push((
            "exchange code".to_string(),
            request.session_configuration.exchange_code.clone(),
        ));
    }
    if let Some(market_profile) = &preview_inputs.market_profile {
        inferred_values.push((
            "market profile".to_string(),
            market_profile.profile_name.clone(),
        ));
    }

    let mut preset_defaults = vec![(
        "session source".to_string(),
        session_source_label(&request, &preview_inputs),
    )];
    if let Some(market_profile) = &preview_inputs.market_profile {
        preset_defaults.push((
            "session preset".to_string(),
            market_profile.session_preset_name.clone(),
        ));
    }

    let mut recommended_defaults = vec![(
        "known news".to_string(),
        if request.known_news_inputs.is_some() {
            "explicit source".to_string()
        } else {
            "none".to_string()
        },
    )];
    if let Some(market_profile) = &preview_inputs.market_profile
        && let Some(known_news_preset_name) = &market_profile.known_news_preset_name
    {
        recommended_defaults.push((
            "known-news preset".to_string(),
            market_profile
                .known_news_policy
                .as_ref()
                .map(|policy| format!("{known_news_preset_name} ({policy})"))
                .unwrap_or_else(|| known_news_preset_name.clone()),
        ));
    }

    let derived_values = vec![
        (
            "output directory".to_string(),
            format_path(&paths.output_directory),
        ),
        ("model path".to_string(), format_path(&paths.model_path)),
        ("report path".to_string(), format_path(&paths.report_path)),
    ];

    Ok(PreparedFitPreview {
        request,
        plan,
        paths,
        inputs: preview_inputs,
        start_summary_emitted,
        explicit_values,
        inferred_values,
        preset_defaults,
        recommended_defaults,
        derived_values,
    })
}

fn emit_start_summary_before_loading(
    context: &CommandContext,
    request: &CalibrationMarketDataPreparationRequest,
    paths: &FitWorkflowPaths,
) -> Result<bool, CliError> {
    if context.output_mode() != OutputMode::Terminal {
        return Ok(false);
    }

    let rendered = fit_preparation_start_summary(request, paths).render(&context.stdout_renderer());

    context
        .stdout_renderer()
        .write_stdout(&format!("\n{rendered}"))
        .map_err(|source| CliError::OutputWrite { source })?;

    Ok(true)
}

fn prepare_market_data_calibration_plan_for_cli(
    context: &CommandContext,
    request: CalibrationMarketDataPreparationRequest,
    cpu_count: Option<usize>,
    output_directory: &Path,
) -> Result<PreparedMarketDataCalibrationPlan, CliError> {
    let mut progress_adapter = terminal_progress_adapter_for_context(context);

    let plan_result = match progress_adapter.as_mut() {
        Some(progress_adapter) => prepare_market_data_calibration_plan_with_terminal_progress(
            progress_adapter,
            request,
            cpu_count,
            output_directory,
        ),
        None => prepare_market_data_calibration_plan(request, cpu_count),
    };

    if let Some(progress_adapter) = progress_adapter.as_mut() {
        match &plan_result {
            Ok(_) => progress_adapter
                .finish_and_keep()
                .map_err(|source| CliError::OutputWrite { source })?,
            Err(_) => progress_adapter.finish_and_clear(),
        }
    }

    let plan = plan_result.map_err(|source| CliError::FitPreparationFailed {
        source: Box::new(source),
    })?;

    Ok(plan)
}

fn prepare_market_data_calibration_plan_with_terminal_progress(
    progress_adapter: &mut ProgressAdapter,
    request: CalibrationMarketDataPreparationRequest,
    cpu_count: Option<usize>,
    output_directory: &Path,
) -> Result<PreparedMarketDataCalibrationPlan, CalibrationMarketDataPreparationError> {
    let mut progress_gate = MarketDataLoadingProgressGate::default();
    let loading_started_at = Instant::now();
    // Intentional copy because the preparation request is consumed by the loader
    // after the progress run has been initialized.
    let progress_run_id = request.run_id.clone();

    progress_adapter.observe(ProgressEvent::RunStarted {
        run_id: progress_run_id,
        scope: "Preparing fit".to_string(),
        input_count: fit_preparation_input_count(&request),
        output_path: Some(format_path(output_directory)),
        stage_sequence: vec!["Loading market data".to_string()],
        completed_stage_count: 0,
    });

    progress_adapter.observe(ProgressEvent::StageStarted {
        stage: "Loading market data".to_string(),
        stage_number: 1,
        stage_count: 1,
    });

    progress_adapter.draw_latest().map_err(|source| {
        CalibrationMarketDataPreparationError::from(format!(
            "failed to render loading progress: {source}"
        ))
    })?;

    let mut preparation_progress = |event| {
        let (phase, measure, latest_signal) = match event {
            MarketDataPreparationProgressEvent::Loading(loading_event) => {
                if !progress_gate.should_emit(&loading_event) {
                    return Ok(());
                }

                (
                    "Loading market data".to_string(),
                    market_data_loading_progress_measure(&loading_event).map_err(|source| {
                        MarketDataPreparationProgressError::observer(source.to_string())
                    })?,
                    market_data_loading_progress_signal(&loading_event),
                )
            }
            MarketDataPreparationProgressEvent::Preparing(preparation_phase) => (
                "Preparing market data".to_string(),
                ProgressMeasure::Unbounded,
                market_data_preparation_progress_signal(preparation_phase),
            ),
        };

        progress_adapter.observe(ProgressEvent::StageProgress {
            phase,
            measure,
            work_item_label: None,
            work_item_progress_percent: None,
            work_item_status: None,
            planned_work_item_labels: Vec::new(),
            work_item_columns: Vec::new(),
            work_item_metrics: Vec::new(),
            latest_signal: Some(latest_signal),
        });

        progress_adapter
            .draw_latest()
            .map_err(|source| MarketDataPreparationProgressError::observer(source.to_string()))
    };

    let plan = prepare_market_data_calibration_plan_with_observer(
        request,
        cpu_count,
        &mut preparation_progress,
    )?;

    progress_adapter.observe(ProgressEvent::StageCompleted {
        stage: "Loading market data".to_string(),
        status: "completed".to_string(),
        runtime_seconds: loading_started_at.elapsed().as_secs_f64(),
    });

    progress_adapter.observe(ProgressEvent::RunCompleted {
        status: "prepared".to_string(),
    });

    Ok(plan)
}

pub(crate) fn write_staged_execution_input(
    context: &CommandContext,
    output_path: Option<&Path>,
    plan: &PreparedMarketDataCalibrationPlan,
) -> Result<Option<PathBuf>, CliError> {
    let Some(output_path) = output_path else {
        return Ok(None);
    };

    let resolved_path = context.paths().resolve_process_path(output_path);

    write_pretty_json_file(&resolved_path, &plan.execution_input, |_, source| {
        CliError::StagedExecutionInputWrite {
            path: resolved_path.clone(),
            source,
        }
    })?;

    Ok(Some(resolved_path))
}

fn resolve_workflow_scope(
    workflow_scope: Option<WorkflowScope>,
    option_surface_present: bool,
) -> Result<CalibrationMarketDataWorkflowScope, CliError> {
    match workflow_scope {
        Some(WorkflowScope::SeasonalityOnly) => {
            Ok(CalibrationMarketDataWorkflowScope::SeasonalityOnly)
        }
        Some(WorkflowScope::FullWithoutOptionSurface) => {
            if option_surface_present {
                return Err(CliError::ConflictingCommandArguments {
                    first_argument: "--workflow-scope",
                    second_argument: "--option-surface-path",
                    message: "full-without-option-surface cannot include an option surface"
                        .to_string(),
                });
            }

            Ok(CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface)
        }
        Some(WorkflowScope::FullWithOptionSurface) => {
            if !option_surface_present {
                return Err(CliError::MissingRequiredArgument {
                    argument_name: "--option-surface-path",
                });
            }

            Ok(CalibrationMarketDataWorkflowScope::FullWithOptionSurface)
        }
        Some(WorkflowScope::Full) | None if option_surface_present => {
            Ok(CalibrationMarketDataWorkflowScope::FullWithOptionSurface)
        }
        Some(WorkflowScope::Full) | None => {
            Ok(CalibrationMarketDataWorkflowScope::FullWithoutOptionSurface)
        }
    }
}

fn session_source_label(
    request: &CalibrationMarketDataPreparationRequest,
    inputs: &FitPreviewInputs,
) -> String {
    if request
        .session_configuration
        .session_definition_source
        .is_some()
    {
        return "explicit session definition".to_string();
    }

    if let Some(market_profile) = &inputs.market_profile {
        return format!("market profile {}", market_profile.profile_name);
    }

    "bundled session registry by exchange code".to_string()
}

pub(crate) fn fit_warnings(prepared: &PreparedFitPreview) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(warning) = &prepared.plan.fit_warning {
        warnings.push(warning.clone());
    }

    warnings.extend(prepared.plan.warnings.iter().cloned());

    for warning in &prepared.plan.session_resolution.ambiguity_warnings {
        warnings.push(warning.clone());
    }

    warnings
}

fn push_if_present(values: &mut Vec<(String, String)>, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        values.push((label.to_string(), value.to_string()));
    }
}
