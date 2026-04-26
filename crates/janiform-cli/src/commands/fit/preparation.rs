use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use janiform::calibration::{
    CalibrationExecutionMode, CalibrationMarketDataCarryInput,
    CalibrationMarketDataMeanReversionConfiguration, CalibrationMarketDataOutputPreferences,
    CalibrationMarketDataPreparationRequest, CalibrationMarketDataWorkflowScope,
    PreparedMarketDataCalibrationPlan, prepare_market_data_calibration_plan,
};
use janiform_cli_ui::format_path;
use janiform_foundations::identifiers::sanitize_ascii_identifier_component;

use super::labels::{
    market_data_file_format_label, tick_price_policy_label, workflow_scope_argument_label,
    workflow_scope_label,
};
use crate::commands::options;
use crate::commands::options::TickDataOptions;
use crate::commands::options::WorkflowScope;
use crate::commands::shared::market_data::prepare_market_data_command_inputs;
use crate::commands::shared::output::write_pretty_json_file;
use crate::commands::shared::output_paths::default_run_output_directory;
use crate::context::CommandContext;
use crate::error::CliError;

const FIT_PREPARATION_REQUEST_SCHEMA_VERSION: &str = "calibration.market_data.request.v1";

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FitPreparationOverrides {
    pub(crate) mean_reversion_grid_interval_hours: Option<f64>,
    pub(crate) dynamics_search_restart_candidate_budget: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct PreparedFitPreview {
    pub(crate) request: CalibrationMarketDataPreparationRequest,
    pub(crate) plan: PreparedMarketDataCalibrationPlan,
    pub(crate) paths: FitPreviewPaths,
    pub(crate) inputs: FitPreviewInputs,
    pub(crate) explicit_values: Vec<(String, String)>,
    pub(crate) inferred_values: Vec<(String, String)>,
    pub(crate) preset_defaults: Vec<(String, String)>,
    pub(crate) recommended_defaults: Vec<(String, String)>,
    pub(crate) derived_values: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct FitPreviewPaths {
    pub(crate) output_directory: PathBuf,
    pub(crate) model_path: PathBuf,
    pub(crate) report_path: PathBuf,
    pub(crate) checkpoint_directory: PathBuf,
}

#[derive(Debug)]
pub(crate) struct FitPreviewInputs {
    pub(crate) tick_data_path: PathBuf,
    pub(crate) option_surface_path: Option<PathBuf>,
    pub(crate) known_news_path: Option<PathBuf>,
    pub(crate) session_definition_path: Option<PathBuf>,
}

pub(crate) fn prepare_fit_preview(
    context: &CommandContext,
    inputs: &TickDataOptions,
    run: &options::RunIdentityOptions,
    workflow_scope: Option<WorkflowScope>,
    execution_mode: Option<CalibrationExecutionMode>,
    cpu_count: Option<usize>,
    overrides: FitPreparationOverrides,
) -> Result<PreparedFitPreview, CliError> {
    let prepared_inputs = prepare_market_data_command_inputs(context, inputs)?;

    let run_id = run
        .run_id
        .clone()
        .unwrap_or_else(|| infer_run_id(&prepared_inputs.tick_data_path));

    let output_directory = match &run.output_dir {
        Some(path) => context.paths().output_directory(path),
        None => default_run_output_directory(context, &run_id),
    };

    let paths = FitPreviewPaths {
        output_directory: output_directory.clone(),
        model_path: output_directory.join("artifacts").join("model.jf"),
        report_path: output_directory.join("artifacts").join("report.json"),
        checkpoint_directory: output_directory.join("checkpoints"),
    };

    let option_surface_present = prepared_inputs.option_surface_path.is_some();

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
        option_surface_path: prepared_inputs.option_surface_path,
        known_news_path: prepared_inputs.known_news_path,
        session_definition_path: prepared_inputs.session_definition_path,
    };

    let effective_workflow_scope = resolve_workflow_scope(workflow_scope, option_surface_present)?;

    let execution_mode = execution_mode.unwrap_or(CalibrationExecutionMode::Disk);

    let request = CalibrationMarketDataPreparationRequest {
        schema_version: FIT_PREPARATION_REQUEST_SCHEMA_VERSION.to_string(),
        run_id: run_id.clone(),
        tick_data_source: prepared_inputs.tick_data_source,
        session_configuration: prepared_inputs.session_configuration,
        column_mapping: prepared_inputs.column_mapping,
        workflow_scope: effective_workflow_scope,
        normalization_policy: prepared_inputs.normalization_policy,
        output_preferences: CalibrationMarketDataOutputPreferences {
            execution_mode,
            disk_checkpoint_directory: match execution_mode {
                CalibrationExecutionMode::Disk => {
                    Some(paths.checkpoint_directory.display().to_string())
                }
                CalibrationExecutionMode::Memory => None,
            },
            emit_derived_staged_execution_input: false,
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
        pricing_interval_schedule_policy: Default::default(),
        dynamics_search_restart_candidate_budget: overrides
            .dynamics_search_restart_candidate_budget,
        joint_refinement_option_refinement_candidate_count_override: None,
        joint_refinement_audit_option_pricing_enabled: false,
        known_news_inputs: prepared_inputs.known_news_inputs,
        mean_reversion: overrides.mean_reversion_grid_interval_hours.map(|hours| {
            CalibrationMarketDataMeanReversionConfiguration {
                calibration_grid_interval_hours: Some(hours),
            }
        }),
    };

    let plan = prepare_market_data_calibration_plan(request.clone(), cpu_count)
        .map_err(|source| CliError::FitPreparationFailed { source })?;

    let mut explicit_values = Vec::new();

    push_if_present(&mut explicit_values, "run id", run.run_id.as_deref());
    push_if_present(
        &mut explicit_values,
        "workflow scope",
        workflow_scope.map(workflow_scope_argument_label).as_deref(),
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

    if inputs.option_surface_path.is_some() {
        explicit_values.push((
            "option surface".to_string(),
            explicit_option_surface_value.unwrap_or_else(|| "none".to_string()),
        ));
    }

    let inferred_values = vec![
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

    let preset_defaults = vec![(
        "session source".to_string(),
        request
            .session_configuration
            .session_definition_source
            .as_ref()
            .map_or_else(
                || "bundled session registry by exchange code".to_string(),
                |_| "explicit session definition".to_string(),
            ),
    )];

    let recommended_defaults = vec![(
        "known news".to_string(),
        if request.known_news_inputs.is_some() {
            "explicit source".to_string()
        } else {
            "none".to_string()
        },
    )];

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
        explicit_values,
        inferred_values,
        preset_defaults,
        recommended_defaults,
        derived_values,
    })
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

fn infer_run_id(tick_data_path: &Path) -> String {
    let source_label = tick_data_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(sanitize_ascii_identifier_component)
        .unwrap_or_else(|| "fit".to_string());

    let timestamp_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();

    format!("{source_label}-{timestamp_seconds}")
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

pub(crate) fn fit_warnings(prepared: &PreparedFitPreview) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(warning) = &prepared.plan.fit_warning {
        warnings.push(warning.clone());
    }

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
