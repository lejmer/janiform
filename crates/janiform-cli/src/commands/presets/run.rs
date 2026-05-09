use std::path::Path;

use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::format_token_as_kebab;
use serde_json::Value;
use serde_json::json;

use super::PresetCategory;
use super::PresetRegistryArgs;
use super::PresetRegistryScope;
use super::PresetsExportCommand;
use super::PresetsListCommand;
use super::PresetsShowCommand;
use super::PresetsShowSubcommand;
use super::payload::malformed_payload;
use super::payload::required_array;
use super::payload::required_string;
use super::payload::required_value;
use super::rendering;
use crate::commands::shared::output::write_json_payload;
use crate::commands::shared::output::write_pretty_json_file;
use crate::commands::shared::presets::user_preset_registry_payload;
use crate::commands::shared::terminal::{
    append_output_path_row, render_success_completion_summary, write_terminal_output,
};
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

const PRESETS_LIST_SCHEMA_VERSION: &str = "cli.presets.list.result.v1";
const PRESETS_LIST_COMMAND_ID: &str = "presets.list";
const PRESETS_SHOW_COMMAND_ID: &str = "presets.show";
const PRESETS_EXPORT_COMMAND_ID: &str = "presets.export";
const ALL_FILTER_LABEL: &str = "all";
const AUTOMATIC_SCOPE_LABEL: &str = "automatic";
const NO_PRESET_NAME: &str = "-";

pub(crate) fn run_presets_list(
    context: &CommandContext,
    command: PresetsListCommand,
) -> Result<(), CliError> {
    let registry = user_preset_registry_payload(context, &command.registry)?;
    let index_payload = janiform::calibration::presets::preset_registry_index(json!({
        "registry": registry,
    }))
    .map_err(|source| {
        preset_workflow_error(
            PRESETS_LIST_COMMAND_ID,
            command.category,
            command.scope,
            None,
            source,
        )
    })?;

    let presets = filtered_index_entries(
        &index_payload,
        command.category,
        command.scope,
        PRESETS_LIST_COMMAND_ID,
    )?;

    let preset_count = presets.len();

    let payload = json!({
        "schema_version": PRESETS_LIST_SCHEMA_VERSION,
        "command_id": PRESETS_LIST_COMMAND_ID,
        "category_filter": command.category.map(PresetCategory::registry_value),
        "scope_filter": command.scope.and_then(PresetRegistryScope::registry_value),
        "preset_count": preset_count,
        "presets": presets,
    });

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&payload),
        OutputMode::Terminal => write_terminal_output(
            context,
            &rendering::render_presets_list(&payload, &context.stdout_renderer())?,
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &render_success_completion_summary(
                "Presets listed",
                vec![KeyValueRow::new("Presets", preset_count.to_string())],
                &context.stdout_renderer(),
            ),
        ),
        OutputMode::Silent => Ok(()),
    }
}

pub(crate) fn run_presets_show(
    context: &CommandContext,
    command: PresetsShowCommand,
) -> Result<(), CliError> {
    let show_request = show_request(command);
    let preset_name = require_preset_name(show_request.name, PRESETS_SHOW_COMMAND_ID)?;

    let registry = user_preset_registry_payload(context, &show_request.registry)?;

    let request = preset_request(
        registry,
        show_request.category,
        &preset_name,
        show_request
            .scope
            .and_then(PresetRegistryScope::registry_value),
    );

    let resolved =
        janiform::calibration::presets::resolve_preset_artifact(request).map_err(|source| {
            preset_workflow_error(
                PRESETS_SHOW_COMMAND_ID,
                Some(show_request.category),
                show_request.scope,
                Some(&preset_name),
                source,
            )
        })?;

    let resolved_scope = required_string(&resolved, "registry_scope", PRESETS_SHOW_COMMAND_ID)?;
    let artifact = required_value(&resolved, "artifact", PRESETS_SHOW_COMMAND_ID)?.clone();

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&artifact),
        OutputMode::Terminal => write_terminal_output(
            context,
            &rendering::render_presets_show(
                &artifact,
                resolved_scope,
                show_request.expanded,
                &context.stdout_renderer(),
            )?,
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &render_success_completion_summary(
                "Preset shown",
                vec![
                    KeyValueRow::new("Preset", preset_name),
                    KeyValueRow::new("Scope", format_token_as_kebab(resolved_scope)),
                ],
                &context.stdout_renderer(),
            ),
        ),
        OutputMode::Silent => Ok(()),
    }
}

struct PresetsShowRequest {
    category: PresetCategory,
    name: String,
    scope: Option<PresetRegistryScope>,
    registry: PresetRegistryArgs,
    expanded: bool,
}

fn show_request(command: PresetsShowCommand) -> PresetsShowRequest {
    match command.command {
        PresetsShowSubcommand::MarketProfile(command) => PresetsShowRequest {
            category: PresetCategory::MarketProfile,
            name: command.name,
            scope: command.registry.scope,
            registry: command.registry.registry,
            expanded: false,
        },
        PresetsShowSubcommand::Session(command) => PresetsShowRequest {
            category: PresetCategory::Session,
            name: command.name,
            scope: command.registry.scope,
            registry: command.registry.registry,
            expanded: command.expanded,
        },
        PresetsShowSubcommand::KnownNews(command) => PresetsShowRequest {
            category: PresetCategory::KnownNews,
            name: command.name,
            scope: command.registry.scope,
            registry: command.registry.registry,
            expanded: command.expanded,
        },
    }
}

pub(crate) fn run_presets_export(
    context: &CommandContext,
    command: PresetsExportCommand,
) -> Result<(), CliError> {
    let preset_name = require_preset_name(command.name, PRESETS_EXPORT_COMMAND_ID)?;

    let registry = user_preset_registry_payload(context, &command.registry)?;

    let request = preset_request(
        registry,
        command.category,
        &preset_name,
        command.scope.and_then(PresetRegistryScope::registry_value),
    );

    let mut payload =
        janiform::calibration::presets::resolve_preset_snapshot(request).map_err(|source| {
            preset_workflow_error(
                PRESETS_EXPORT_COMMAND_ID,
                Some(command.category),
                command.scope,
                Some(&preset_name),
                source,
            )
        })?;

    let payload_object = payload.as_object_mut().ok_or_else(|| {
        malformed_payload(
            PRESETS_EXPORT_COMMAND_ID,
            "resolved preset snapshot must be a JSON object",
        )
    })?;

    payload_object.insert(
        "command_id".to_string(),
        Value::String(PRESETS_EXPORT_COMMAND_ID.to_string()),
    );

    let output_path = command
        .output_path
        .as_ref()
        .map(|path| context.paths().output_path(path));

    if let Some(output_path) = &output_path {
        write_pretty_json_file(output_path, &payload, |path, source| {
            CliError::ArtifactWrite {
                artifact_name: "preset export",
                path,
                source,
            }
        })?;
    }

    match context.output_mode() {
        OutputMode::Json => write_json_payload(&payload),
        OutputMode::Terminal => write_terminal_output(
            context,
            &rendering::render_presets_export(
                &payload,
                output_path.as_deref(),
                &context.stdout_renderer(),
            )?,
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &render_success_completion_summary(
                "Preset exported",
                export_completion_rows(&payload, output_path.as_deref())?,
                &context.stdout_renderer(),
            ),
        ),
        OutputMode::Silent => Ok(()),
    }
}

fn preset_request(
    registry: Value,
    category: PresetCategory,
    preset_name: &str,
    registry_scope: Option<&'static str>,
) -> Value {
    json!({
        "category": category.registry_value(),
        "preset_name": preset_name,
        "registry_scope": registry_scope,
        "registry": registry,
    })
}

fn filtered_index_entries(
    index_payload: &Value,
    category_filter: Option<PresetCategory>,
    scope_filter: Option<PresetRegistryScope>,
    command_path: &'static str,
) -> Result<Vec<Value>, CliError> {
    let entries = required_array(index_payload, "entries", command_path)?;
    let scope_filter = scope_filter.and_then(PresetRegistryScope::registry_value);

    let mut filtered_entries = Vec::new();
    for entry in entries {
        if index_entry_matches(entry, category_filter, scope_filter, command_path)? {
            filtered_entries.push(entry.clone());
        }
    }

    Ok(filtered_entries)
}

fn index_entry_matches(
    entry: &Value,
    category_filter: Option<PresetCategory>,
    scope_filter: Option<&str>,
    command_path: &'static str,
) -> Result<bool, CliError> {
    if let Some(category_filter) = category_filter {
        let category = required_string(entry, "category", command_path)?;
        if category != category_filter.registry_value() {
            return Ok(false);
        }
    }

    if let Some(scope_filter) = scope_filter {
        let registry_scope = required_string(entry, "registry_scope", command_path)?;
        if registry_scope != scope_filter {
            return Ok(false);
        }
    }

    Ok(true)
}

fn require_preset_name(
    preset_name: String,
    command_path: &'static str,
) -> Result<String, CliError> {
    if preset_name.trim().is_empty() {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "name",
            message: format!("janiform {command_path} requires a non-empty preset name"),
        });
    }

    Ok(preset_name)
}

fn preset_workflow_error(
    command_path: &'static str,
    category: Option<PresetCategory>,
    scope: Option<PresetRegistryScope>,
    preset_name: Option<&str>,
    source: janiform::calibration::presets::PresetWorkflowResolutionError,
) -> CliError {
    CliError::PresetWorkflowFailed {
        command_path,
        category: category
            .map(PresetCategory::registry_value)
            .unwrap_or(ALL_FILTER_LABEL)
            .to_string(),
        scope: scope
            .and_then(PresetRegistryScope::registry_value)
            .unwrap_or(AUTOMATIC_SCOPE_LABEL)
            .to_string(),
        preset_name: preset_name.unwrap_or(NO_PRESET_NAME).to_string(),
        source: Box::new(source),
    }
}

fn export_completion_rows(
    payload: &Value,
    output_path: Option<&Path>,
) -> Result<Vec<KeyValueRow>, CliError> {
    let mut rows = vec![
        KeyValueRow::new(
            "Preset",
            required_string(payload, "preset_name", PRESETS_EXPORT_COMMAND_ID)?,
        ),
        KeyValueRow::new(
            "Scope",
            format_token_as_kebab(required_string(
                payload,
                "requested_scope",
                PRESETS_EXPORT_COMMAND_ID,
            )?),
        ),
    ];

    append_output_path_row(&mut rows, output_path);

    Ok(rows)
}
