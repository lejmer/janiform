use std::fs;
use std::path::Path;
use std::path::PathBuf;

use janiform_foundations::json;
use serde_json::Value;
use serde_json::json;

use crate::commands::presets::PresetRegistryArgs;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::context::CommandContext;
use crate::error::CliError;

const USER_PRESET_REGISTRY_SCHEMA_VERSION: &str = "preset.user_registry.v1";

pub(crate) fn user_preset_registry_payload(
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
    let registry_payload = load_json_artifact(registry_path, "preset registry")?;

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

    let artifact = janiform::calibration::presets::deserialize_preset_artifact_json(&preset_json)
        .map_err(|source| CliError::PresetUserFileInvalid {
        path: resolved_path,
        source: Box::new(source),
    })?;

    user_registered_presets.push(artifact);

    Ok(())
}

fn registry_manifest_object<'a>(
    value: &'a Value,
    registry_path: &Path,
) -> Result<&'a serde_json::Map<String, Value>, CliError> {
    json::payload::object_value(value, "preset_registry")
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_array<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
    registry_path: &Path,
) -> Result<&'a Vec<Value>, CliError> {
    json::payload::array_field(value, field_name, field_name)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_string<'a>(
    value: &'a serde_json::Map<String, Value>,
    field_name: &str,
    registry_path: &Path,
) -> Result<&'a str, CliError> {
    json::payload::string_field(value, field_name, field_name)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn registry_manifest_value_string<'a>(
    value: &'a Value,
    field_path: &str,
    registry_path: &Path,
) -> Result<&'a str, CliError> {
    json::payload::string_value(value, field_path)
        .map_err(|source| preset_registry_manifest_malformed(registry_path, source))
}

fn preset_registry_manifest_malformed(
    registry_path: &Path,
    source: json::payload::JsonPayloadError,
) -> CliError {
    CliError::ArtifactMalformed {
        artifact_name: "preset registry",
        path: registry_path.to_path_buf(),
        message: format!("{} {}", source.field_path(), source.message()),
    }
}
