use std::path::Path;

use janiform_terminal_ui::CompactTable;
use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::StartSummary;
use janiform_terminal_ui::TerminalRenderer;
use janiform_terminal_ui::format_artifact_path;
use janiform_terminal_ui::format_token_as_kebab;
use serde_json::Value;

use super::super::payload::malformed_payload;
use super::super::payload::optional_object_string;
use super::super::payload::optional_string;
use super::super::payload::required_array;
use super::super::payload::required_object;
use super::super::payload::required_object_string;
use super::super::payload::required_string;
use super::super::payload::required_usize;
use super::super::payload::required_value_object;
use super::details;
use crate::error::CliError;

pub(crate) fn render_presets_list(
    payload: &Value,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let presets = required_array(payload, "presets", "presets.list")?;

    let table_rows = presets
        .iter()
        .map(|entry| {
            Ok(vec![
                format_token_as_kebab(required_string(entry, "category", "presets.list")?),
                format_token_as_kebab(required_string(entry, "registry_scope", "presets.list")?),
                required_string(entry, "preset_name", "presets.list")?.to_string(),
                required_string(entry, "version_label", "presets.list")?.to_string(),
            ])
        })
        .collect::<Result<Vec<_>, CliError>>()?;

    let mut sections = vec![
        StartSummary {
            title: "Preset registry".to_string(),
            rows: vec![
                KeyValueRow::new(
                    "Presets",
                    required_usize(payload, "preset_count", "presets.list")?.to_string(),
                ),
                KeyValueRow::new(
                    "Category filter",
                    optional_string(payload, "category_filter")
                        .unwrap_or("all")
                        .to_string(),
                ),
                KeyValueRow::new(
                    "Scope filter",
                    optional_string(payload, "scope_filter")
                        .unwrap_or("all")
                        .to_string(),
                ),
            ],
        }
        .render(renderer),
    ];

    sections.push(
        CompactTable {
            headers: vec![
                "Category".to_string(),
                "Scope".to_string(),
                "Name".to_string(),
                "Version".to_string(),
            ],
            rows: table_rows,
        }
        .render(renderer),
    );

    Ok(sections.join("\n\n"))
}

pub(crate) fn render_presets_show(
    artifact: &Value,
    resolved_scope: &str,
    expanded: bool,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let metadata = required_object(artifact, "metadata", "presets.show")?;
    let configuration = required_object(artifact, "configuration", "presets.show")?;
    let category = required_string(artifact, "category", "presets.show")?;

    let mut rows = vec![
        KeyValueRow::new("Category", format_token_as_kebab(category)),
        KeyValueRow::new(
            "Name",
            required_string(artifact, "preset_name", "presets.show")?,
        ),
        KeyValueRow::new("Resolved scope", format_token_as_kebab(resolved_scope)),
        KeyValueRow::new(
            "Version",
            required_object_string(metadata, "version_label", "presets.show")?,
        ),
    ];

    push_reference_row(&mut rows, configuration, "session_reference", "Session")?;
    push_reference_row(
        &mut rows,
        configuration,
        "known_news_reference",
        "Known news",
    )?;

    if let Some(exchange_code) = optional_object_string(configuration, "exchange_code") {
        rows.push(KeyValueRow::new("Exchange", exchange_code));
    }

    if let Some(events) = configuration.get("events").and_then(Value::as_array) {
        rows.push(KeyValueRow::new("Events", events.len().to_string()));
    }

    if let Some(description) = optional_object_string(metadata, "description") {
        rows.push(KeyValueRow::block("Description", description));
    }

    let mut sections = vec![
        StartSummary {
            title: "Preset".to_string(),
            rows,
        }
        .render(renderer),
    ];

    if expanded {
        sections.extend(details::render_expanded_details(
            category,
            configuration,
            renderer,
        )?);
    }

    Ok(sections.join("\n\n"))
}

pub(crate) fn render_presets_export(
    payload: &Value,
    output_path: Option<&Path>,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let mut rows = vec![
        KeyValueRow::new(
            "Requested category",
            format_token_as_kebab(required_string(
                payload,
                "requested_category",
                "presets.export",
            )?),
        ),
        KeyValueRow::new(
            "Preset",
            required_string(payload, "preset_name", "presets.export")?,
        ),
        KeyValueRow::new(
            "Resolved scope",
            format_token_as_kebab(required_string(
                payload,
                "requested_scope",
                "presets.export",
            )?),
        ),
        KeyValueRow::new(
            "Market profile",
            included_preset_label(payload, "market_profile_preset", "presets.export")?,
        ),
        KeyValueRow::new(
            "Session",
            included_preset_label(payload, "session_preset", "presets.export")?,
        ),
        KeyValueRow::new(
            "Known news",
            included_preset_label(payload, "known_news_preset", "presets.export")?,
        ),
    ];

    if let Some(output_path) = output_path {
        rows.push(KeyValueRow::new(
            "Output",
            format_artifact_path(output_path),
        ));
    }

    Ok(StartSummary {
        title: "Preset export".to_string(),
        rows,
    }
    .render(renderer))
}

fn push_reference_row(
    rows: &mut Vec<KeyValueRow>,
    configuration: &serde_json::Map<String, Value>,
    field_name: &str,
    label: &str,
) -> Result<(), CliError> {
    let Some(reference) = configuration.get(field_name) else {
        return Ok(());
    };
    if reference.is_null() {
        return Ok(());
    }

    let reference = required_value_object(reference, field_name, "presets.show")?;
    let usage_mode = required_object_string(reference, "usage_mode", "presets.show")?;

    match usage_mode {
        "named" => {
            let preset_name = required_object_string(reference, "preset_name", "presets.show")?;
            let scope = required_object_string(reference, "registry_scope", "presets.show")?;

            rows.push(KeyValueRow::new(
                label,
                format!("{} ({})", preset_name, format_token_as_kebab(scope)),
            ));
        }
        "explicit" => rows.push(KeyValueRow::new(label, "explicit configuration")),
        "automatic" => rows.push(KeyValueRow::new(label, "automatic")),
        _ => {
            return Err(malformed_payload(
                "presets.show",
                &format!("{field_name}.usage_mode has unsupported value `{usage_mode}`"),
            ));
        }
    }

    Ok(())
}

fn included_preset_label(
    payload: &Value,
    field_name: &str,
    command_path: &'static str,
) -> Result<String, CliError> {
    let Some(artifact) = payload.get(field_name) else {
        return Ok("not included".to_string());
    };
    if artifact.is_null() {
        return Ok("not included".to_string());
    }

    let category = required_string(artifact, "category", command_path)?;
    let preset_name = required_string(artifact, "preset_name", command_path)?;

    Ok(format!(
        "{}:{}",
        format_token_as_kebab(category),
        preset_name
    ))
}
