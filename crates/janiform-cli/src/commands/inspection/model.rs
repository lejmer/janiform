use std::path::Path;

use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StageChecklist;
use janiform_terminal_ui::StageChecklistItem;
use janiform_terminal_ui::StageChecklistStatus;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::WarningBlock;
use serde_json::Value;

use crate::commands::inspection::InspectModelCommand;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::commands::shared::artifacts::require_json_object;
use crate::commands::shared::output::write_json_payload;
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;
use janiform_foundations::json::display::compact_value_string;
use janiform_foundations::json::query::find_value_by_key;
use janiform_foundations::json::query::nested_string;
use janiform_foundations::json::query::nested_value;
use janiform_foundations::json::query::string_field;
use janiform_workflows::fit::stage_label_from_token;
use janiform_workflows::fit::workflow_scope_label_from_token;

pub(crate) fn run_inspect_model(
    context: &CommandContext,
    command: InspectModelCommand,
) -> Result<(), CliError> {
    let model_path = context.paths().resolve_process_path(&command.model_path);
    let model = load_json_artifact(&model_path, "model")?;
    require_json_object(&model, "model", &model_path)?;

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&model),
        OutputMode::Silent => Ok(()),
        OutputMode::Terminal | OutputMode::Quiet => {
            let rendered = render_model(
                &model,
                &model_path,
                command.parameters,
                &context.stdout_renderer(),
            );
            context
                .stdout_renderer()
                .write_stdout(&rendered)
                .map_err(|source| CliError::OutputWrite { source })
        }
    }
}

fn render_model(
    model: &Value,
    model_path: &Path,
    include_parameters: bool,
    renderer: &TerminalRenderer,
) -> String {
    let mut sections = vec![render_model_summary(model, model_path, renderer)];

    sections.push(render_fitted_components(model, renderer));
    sections.push(render_main_parameters(model, renderer));

    if include_parameters {
        sections.push(render_parameter_details(model, renderer));
    }

    sections.push(render_capabilities(model, renderer));
    sections.push(render_training_data(model, renderer));

    if let Some(warnings) = model_warnings(model) {
        sections.push(warnings.render(renderer));
    }

    sections.join("\n\n")
}

fn render_model_summary(model: &Value, model_path: &Path, renderer: &TerminalRenderer) -> String {
    let workflow_scope = string_field(model, "workflow_scope")
        .or_else(|| nested_string(model, &["metadata", "workflow_scope"]))
        .map(workflow_scope_label_from_token)
        .unwrap_or_else(|| "not recorded".to_string());

    let mut rows = vec![
        KeyValueRow::new("Model", model_path.display().to_string()),
        KeyValueRow::new("Run", model_run_id(model)),
        KeyValueRow::new("Workflow scope", workflow_scope),
    ];

    if let Some(created_at) = model_created_at(model) {
        rows.push(KeyValueRow::new("Created", created_at.to_string()));
    }

    StartSummary {
        title: "Model".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_fitted_components(model: &Value, renderer: &TerminalRenderer) -> String {
    StageChecklist {
        title: "Fitted components".to_string(),
        stages: fitted_components(model)
            .into_iter()
            .map(|component| StageChecklistItem {
                name: component,
                status: StageChecklistStatus::Completed,
            })
            .collect(),
    }
    .render(renderer)
}

fn render_main_parameters(model: &Value, renderer: &TerminalRenderer) -> String {
    let hawkes = find_object_with_key(model, "kernel_parameters");
    let nig = find_object_with_key(model, "nig_shared_plus_shift_configuration");
    let mean_reversion = find_object_with_key(model, "mean_reversion_fitted_parameters");

    let rows = vec![
        parameter_row(hawkes, "Hawkes lambda bar", &["lambda_bar"]),
        parameter_row(hawkes, "Hawkes beta", &["beta"]),
        parameter_row(
            hawkes,
            "Hawkes branching ratio",
            &["branching_ratio", "rho"],
        ),
        parameter_row(nig, "NIG alpha", &["alpha"]),
        parameter_row(nig, "NIG beta_0", &["beta_0"]),
        parameter_row(nig, "NIG delta", &["delta"]),
        parameter_row(nig, "NIG mu", &["mu"]),
        parameter_row(mean_reversion, "Mean reversion kappa", &["kappa"]),
    ];

    StartSummary {
        title: "Main parameters".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_parameter_details(model: &Value, renderer: &TerminalRenderer) -> String {
    let mut rows = Vec::new();

    if let Some(hawkes) = find_object_with_key(model, "hawkes_fitted_parameters")
        .or_else(|| nested_value(model, &["parameters", "hawkes", "fitted_parameters"]))
    {
        rows.push(KeyValueRow::block("Hawkes", compact_value_string(hawkes)));
    }

    if let Some(nig) = find_object_with_key(model, "nig_shared_plus_shift_configuration") {
        rows.push(KeyValueRow::block("NIG", compact_value_string(nig)));
    }

    if let Some(mean_reversion) = find_object_with_key(model, "mean_reversion_fitted_parameters") {
        rows.push(KeyValueRow::block(
            "Mean reversion",
            compact_value_string(mean_reversion),
        ));
    }

    if let Some(news_process) = find_object_with_key(model, "news_process_configuration") {
        rows.push(KeyValueRow::block(
            "News process",
            compact_value_string(news_process),
        ));
    }

    if let Some(seasonality) = seasonality_samples(model) {
        rows.push(KeyValueRow::block("Seasonality", seasonality));
    }

    if let Some(option_refinement) = find_object_with_key(model, "option_calibration_diagnostics") {
        rows.push(KeyValueRow::block(
            "Option refinement",
            compact_value_string(option_refinement),
        ));
    }

    if rows.is_empty() {
        rows.push(KeyValueRow::new("Parameters", "not recorded"));
    }

    StartSummary {
        title: "Parameter details".to_string(),
        rows,
    }
    .render(renderer)
}

fn render_capabilities(model: &Value, renderer: &TerminalRenderer) -> String {
    StageChecklist {
        title: "Capabilities".to_string(),
        stages: capabilities(model)
            .into_iter()
            .map(|capability| StageChecklistItem {
                name: capability,
                status: StageChecklistStatus::Completed,
            })
            .collect(),
    }
    .render(renderer)
}

fn render_training_data(model: &Value, renderer: &TerminalRenderer) -> String {
    let mut rows = Vec::new();

    if let Some(tick_path) = nested_string(
        model,
        &["metadata", "input_inference", "tick_data_source", "path"],
    ) {
        rows.push(KeyValueRow::new("Ticks", tick_path.to_string()));
    }

    if let Some(option_path) = nested_string(
        model,
        &[
            "metadata",
            "input_inference",
            "option_surface_source",
            "path",
        ],
    ) {
        rows.push(KeyValueRow::new("Options", option_path.to_string()));
    }

    if let Some(market_profile) = nested_string(model, &["metadata", "market_profile"]) {
        rows.push(KeyValueRow::new(
            "Market profile",
            market_profile.to_string(),
        ));
    }

    if let Some(session) = nested_string(model, &["metadata", "session_preset"]) {
        rows.push(KeyValueRow::new("Session", session.to_string()));
    }

    if rows.is_empty() {
        rows.push(KeyValueRow::new("Training data", "not recorded"));
    }

    StartSummary {
        title: "Training data".to_string(),
        rows,
    }
    .render(renderer)
}

fn model_run_id(model: &Value) -> String {
    string_field(model, "run_id")
        .or_else(|| nested_string(model, &["metadata", "run_id"]))
        .unwrap_or("not recorded")
        .to_string()
}

fn model_created_at(model: &Value) -> Option<&str> {
    string_field(model, "created_at_utc")
        .or_else(|| nested_string(model, &["metadata", "created_at_utc"]))
        .or_else(|| nested_string(model, &["metadata", "created_at"]))
}

fn fitted_components(model: &Value) -> Vec<String> {
    let mut components = Vec::new();

    if let Some(selected_stages) =
        nested_value(model, &["metadata", "selected_stages"]).and_then(Value::as_array)
    {
        for stage in selected_stages {
            if let Some(stage) = stage.as_str() {
                components.push(stage_label_from_token(stage));
            }
        }
    }

    if components.is_empty()
        && let Some(parameters) = nested_value(model, &["parameters"]).and_then(Value::as_object)
    {
        for key in parameters.keys() {
            components.push(stage_label_from_token(key));
        }
    }

    if has_option_refinement(model) {
        components.push("Option refinement".to_string());
    }

    components.sort();
    components.dedup();

    if components.is_empty() {
        components.push("not recorded".to_string());
    }

    components
}

fn capabilities(model: &Value) -> Vec<String> {
    let has_dynamics = find_object_with_key(model, "hawkes_fitted_parameters").is_some()
        || nested_value(model, &["parameters", "hawkes"]).is_some()
        || nested_value(model, &["parameters", "joint_refinement"]).is_some();
    let has_return_distribution =
        find_object_with_key(model, "nig_shared_plus_shift_configuration").is_some()
            || nested_value(model, &["parameters", "return_components"]).is_some();
    let has_option_pricing = has_option_refinement(model);

    let mut capabilities = Vec::new();

    if has_dynamics && has_return_distribution {
        capabilities.push("Simulate seeded ticks".to_string());
        capabilities.push("Simulate coarse paths".to_string());
    }

    if has_option_pricing {
        capabilities.push("Price vanilla options".to_string());
        capabilities.push("Price option grids".to_string());
        capabilities.push("Price observed option snapshots".to_string());
    }

    if capabilities.is_empty() {
        capabilities.push("not recorded".to_string());
    }

    capabilities
}

fn has_option_refinement(model: &Value) -> bool {
    let has_pricing_block = find_object_with_key(model, "pricing_block_fitted_parameters")
        .is_some_and(|value| !value.is_null());
    let has_option_diagnostics = find_object_with_key(model, "option_calibration_diagnostics")
        .is_some_and(|value| !value.is_null());
    let option_scope = string_field(model, "workflow_scope")
        .or_else(|| nested_string(model, &["metadata", "workflow_scope"]))
        .is_some_and(|scope| {
            scope == "full_with_option_surface" || scope == "joint_option_calibrated"
        });

    has_pricing_block || has_option_diagnostics || option_scope
}

fn parameter_row(container: Option<&Value>, label: &str, candidate_keys: &[&str]) -> KeyValueRow {
    let value = candidate_keys
        .iter()
        .filter_map(|key| container.and_then(|container| find_value_by_key(container, key)))
        .next()
        .map(compact_value_string)
        .unwrap_or_else(|| "not recorded".to_string());

    KeyValueRow::new(label, value)
}

fn model_warnings(model: &Value) -> Option<WarningBlock> {
    let diagnostics = nested_value(model, &["metadata", "diagnostics"])?.as_array()?;
    let warnings = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let severity = diagnostic.get("severity").and_then(Value::as_str)?;
            if severity == "info" {
                return None;
            }

            diagnostic
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    if warnings.is_empty() {
        return None;
    }

    Some(WarningBlock {
        title: format!("Warnings: {}", warnings.len()),
        warnings,
    })
}

fn seasonality_samples(model: &Value) -> Option<String> {
    let seasonality = nested_value(model, &["parameters", "seasonality"])?;

    let mut sections = Vec::new();

    if let Some(intraday) = seasonality
        .get("intraday_factor_by_bin")
        .and_then(Value::as_array)
    {
        sections.push(format!(
            "Intraday:\n{}",
            hourly_samples(intraday.as_slice()).join("\n")
        ));
    }

    if let Some(day_of_week) = seasonality
        .get("day_of_week_factor")
        .and_then(Value::as_array)
    {
        sections.push(format!(
            "Day of week:\n{}",
            day_of_week_samples(day_of_week)
        ));
    }

    if let Some(day_of_year) = seasonality
        .get("day_of_year_factor")
        .and_then(Value::as_array)
    {
        sections.push(format!(
            "Day of year:\n{}",
            day_of_year_samples(day_of_year)
        ));
    }

    if sections.is_empty() {
        return None;
    }

    Some(sections.join("\n"))
}

fn hourly_samples(values: &[Value]) -> Vec<String> {
    let samples = sample_slots(values, 24);

    samples
        .chunks(6)
        .map(|chunk| {
            chunk
                .iter()
                .map(|(position, value)| format!("{position:02} {}", format_sample_value(*value)))
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect()
}

fn day_of_week_samples(values: &[Value]) -> String {
    let labels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

    labels
        .iter()
        .enumerate()
        .filter_map(|(position, label)| {
            values
                .get(position)
                .and_then(Value::as_f64)
                .map(|value| format!("{label} {}", format_sample_value(value)))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn day_of_year_samples(values: &[Value]) -> String {
    let samples = values
        .iter()
        .step_by(15)
        .take(24)
        .enumerate()
        .filter_map(|(sample_position, value)| {
            let day_of_year = sample_position * 15 + 1;
            value
                .as_f64()
                .map(|value| format!("{day_of_year:03} {}", format_sample_value(value)))
        })
        .collect::<Vec<_>>();

    samples
        .chunks(6)
        .map(|chunk| chunk.join("  "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sample_slots(values: &[Value], sample_count: usize) -> Vec<(usize, f64)> {
    if values.is_empty() {
        return Vec::new();
    }

    (0..sample_count)
        .filter_map(|sample_position| {
            let position = sample_position * values.len() / sample_count;
            values
                .get(position)
                .and_then(Value::as_f64)
                .map(|value| (sample_position, value))
        })
        .collect()
}

fn format_sample_value(value: f64) -> String {
    format!("{value:.6}")
}

fn find_object_with_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    find_value_by_key(value, key)
}
