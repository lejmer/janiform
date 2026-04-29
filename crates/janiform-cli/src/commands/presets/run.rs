use std::fs;
use std::path::Path;
use std::path::PathBuf;

use janiform_foundations::json_payload;
use janiform_terminal_ui::KeyValueRow;
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
use crate::context::CommandContext;
use crate::context::OutputMode;
use crate::error::CliError;

const PRESETS_LIST_SCHEMA_VERSION: &str = "cli.presets.list.result.v1";
const PRESETS_LIST_COMMAND_ID: &str = "presets.list";
const PRESETS_SHOW_COMMAND_ID: &str = "presets.show";
const PRESETS_EXPORT_COMMAND_ID: &str = "presets.export";
const USER_PRESET_REGISTRY_SCHEMA_VERSION: &str = "preset.user_registry.v1";
const ALL_FILTER_LABEL: &str = "all";
const AUTOMATIC_SCOPE_LABEL: &str = "automatic";
const NO_PRESET_NAME: &str = "-";

pub(crate) fn run_presets_list(
    context: &CommandContext,
    command: PresetsListCommand,
) -> Result<(), CliError> {
    let registry = user_preset_registry_payload(context, &command.registry)?;
    let index_payload = janiform::calibration::preset_registry_index(json!({
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
        OutputMode::Human => write_terminal_output(
            context,
            &rendering::render_presets_list(&payload, &context.stdout_renderer())?,
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &rendering::render_presets_completion(
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

    let resolved = janiform::calibration::resolve_preset_artifact(request).map_err(|source| {
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
        OutputMode::Human => write_terminal_output(
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
            &rendering::render_presets_completion(
                "Preset shown",
                vec![
                    KeyValueRow::new("Preset", preset_name),
                    KeyValueRow::new("Scope", resolved_scope.replace('_', "-")),
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

    let mut payload = janiform::calibration::export_resolved_preset(request).map_err(|source| {
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
            "resolved preset export must be a JSON object",
        )
    })?;

    payload_object.insert(
        "command_id".to_string(),
        Value::String(PRESETS_EXPORT_COMMAND_ID.to_string()),
    );

    let output_path = match &command.output_path {
        Some(path) => Some(context.paths().resolve_repository_path(path)?),
        None => None,
    };

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
        OutputMode::Human => write_terminal_output(
            context,
            &rendering::render_presets_export(
                &payload,
                output_path.as_deref(),
                &context.stdout_renderer(),
            )?,
        ),
        OutputMode::Quiet => write_terminal_output(
            context,
            &rendering::render_presets_completion(
                "Preset exported",
                export_completion_rows(&payload, output_path.as_deref())?,
                &context.stdout_renderer(),
            ),
        ),
        OutputMode::Silent => Ok(()),
    }
}

fn user_preset_registry_payload(
    context: &CommandContext,
    registry_args: &PresetRegistryArgs,
) -> Result<Value, CliError> {
    let mut user_registered_presets = Vec::new();

    for path in &registry_args.preset_registry {
        let resolved_path = context.paths().user_preset_registry_file(path)?;
        append_registry_manifest_presets(context, &mut user_registered_presets, &resolved_path)?;
    }

    for path in &registry_args.user_preset {
        let resolved_path = context.paths().user_preset_file(path)?;
        append_user_preset_artifact(&mut user_registered_presets, resolved_path)?;
    }

    Ok(json!({
        "user_registered_presets": user_registered_presets,
    }))
}

fn append_registry_manifest_presets(
    context: &CommandContext,
    user_registered_presets: &mut Vec<Value>,
    registry_path: &Path,
) -> Result<(), CliError> {
    let registry_json =
        fs::read_to_string(registry_path).map_err(|source| CliError::ArtifactRead {
            artifact_name: "preset registry",
            path: registry_path.to_path_buf(),
            source,
        })?;
    let registry_payload = serde_json::from_str::<Value>(&registry_json).map_err(|source| {
        CliError::ArtifactParse {
            artifact_name: "preset registry",
            path: registry_path.to_path_buf(),
            source,
        }
    })?;

    let registry = registry_manifest_object(&registry_payload, registry_path)?;
    let schema_version = registry_manifest_string(registry, "schema_version", registry_path)?;
    if schema_version != USER_PRESET_REGISTRY_SCHEMA_VERSION {
        return Err(CliError::ArtifactMalformed {
            artifact_name: "preset registry",
            path: registry_path.to_path_buf(),
            message: format!("schema_version must be `{USER_PRESET_REGISTRY_SCHEMA_VERSION}`"),
        });
    }

    let artifact_paths = registry_manifest_array(registry, "preset_artifact_paths", registry_path)?;
    let base_directory = registry_path.parent().unwrap_or_else(|| Path::new(""));

    for (artifact_position, artifact_path) in artifact_paths.iter().enumerate() {
        let field_path = format!("preset_artifact_paths[{artifact_position}]");
        let artifact_path =
            registry_manifest_value_string(artifact_path, &field_path, registry_path)?;
        let resolved_path = context.paths().input_file_relative_to(
            "--preset-registry",
            base_directory,
            Path::new(artifact_path),
        )?;

        append_user_preset_artifact(user_registered_presets, resolved_path)?;
    }

    Ok(())
}

fn append_user_preset_artifact(
    user_registered_presets: &mut Vec<Value>,
    resolved_path: PathBuf,
) -> Result<(), CliError> {
    let preset_json =
        fs::read_to_string(&resolved_path).map_err(|source| CliError::PresetUserFileRead {
            path: resolved_path.clone(),
            source,
        })?;
    let artifact = janiform::calibration::deserialize_preset_artifact_json(&preset_json).map_err(
        |source| CliError::PresetUserFileInvalid {
            path: resolved_path,
            source,
        },
    )?;

    user_registered_presets.push(artifact);

    Ok(())
}

fn registry_manifest_object<'a>(
    value: &'a Value,
    registry_path: &Path,
) -> Result<&'a serde_json::Map<String, Value>, CliError> {
    json_payload::object_value(value, "preset_registry")
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_array<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
    registry_path: &Path,
) -> Result<&'a Vec<Value>, CliError> {
    json_payload::array_field(value, field_name, field_name)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
    registry_path: &Path,
) -> Result<&'a str, CliError> {
    json_payload::string_field(value, field_name, field_name)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_value_string<'a>(
    value: &'a Value,
    field_path: &str,
    registry_path: &Path,
) -> Result<&'a str, CliError> {
    json_payload::string_value(value, field_path)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn preset_registry_manifest_malformed(
    registry_path: &Path,
    source: json_payload::JsonPayloadError,
) -> CliError {
    CliError::ArtifactMalformed {
        artifact_name: "preset registry",
        path: registry_path.to_path_buf(),
        message: format!("{} {}", source.field_path(), source.message()),
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
    source: janiform::calibration::PresetWorkflowResolutionError,
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
        source,
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
            required_string(payload, "requested_scope", PRESETS_EXPORT_COMMAND_ID)?
                .replace('_', "-"),
        ),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new(
            "Output",
            output_path.display().to_string(),
        ));
    }

    Ok(rows)
}

fn write_terminal_output(context: &CommandContext, output: &str) -> Result<(), CliError> {
    context
        .stdout_renderer()
        .write_stdout(output)
        .map_err(|source| CliError::OutputWrite { source })
}

impl PresetCategory {
    fn registry_value(self) -> &'static str {
        match self {
            Self::MarketProfile => "market_profile",
            Self::Session => "session",
            Self::KnownNews => "known_news",
        }
    }
}

impl PresetRegistryScope {
    fn registry_value(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::BuiltIn => Some("built_in"),
            Self::UserRegistered => Some("user_registered"),
        }
    }
}
