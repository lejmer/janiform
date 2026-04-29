use janiform_terminal_ui::CompactTable;
use janiform_terminal_ui::TerminalRenderer;
use serde_json::Map;
use serde_json::Value;

use crate::error::CliError;

use super::super::payload::malformed_payload;
use super::super::payload::required_object_string;
use super::super::payload::required_value_array;
use super::super::payload::required_value_object;
use super::super::payload::required_value_string;

pub(super) fn render_expanded_details(
    category: &str,
    configuration: &Map<String, Value>,
    renderer: &TerminalRenderer,
) -> Result<Vec<String>, CliError> {
    match category {
        "known_news" => Ok(vec![render_known_news_events(
            required_configuration_value(configuration, "events")?,
            renderer,
        )?]),
        "session" => render_session_details(configuration, renderer),
        _ => Ok(Vec::new()),
    }
}

fn render_known_news_events(
    events: &Value,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let events = required_value_array(events, "configuration.events", "presets.show")?;

    if events.is_empty() {
        return Ok("Events\n\nNo events".to_string());
    }

    let rows = events
        .iter()
        .enumerate()
        .map(|(event_position, event)| event_table_row(event, event_position))
        .collect::<Result<Vec<_>, CliError>>()?;

    let table = CompactTable {
        headers: vec![
            "Scheduled At".to_string(),
            "Event ID".to_string(),
            "Name".to_string(),
            "Tags".to_string(),
        ],
        rows,
    }
    .render(renderer);

    Ok(format!("Events\n\n{table}"))
}

fn render_session_details(
    configuration: &Map<String, Value>,
    renderer: &TerminalRenderer,
) -> Result<Vec<String>, CliError> {
    let regular_windows = required_configuration_value(configuration, "regular_session_windows")?;
    let closed_windows = required_configuration_value(configuration, "recurring_closed_windows")?;

    Ok(vec![
        render_regular_session_windows(regular_windows, renderer)?,
        render_recurring_closed_windows(closed_windows, renderer)?,
    ])
}

fn render_regular_session_windows(
    windows: &Value,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let windows = required_value_array(
        windows,
        "configuration.regular_session_windows",
        "presets.show",
    )?;

    if windows.is_empty() {
        return Ok("Regular Session Windows\n\nNo windows".to_string());
    }

    let rows = windows
        .iter()
        .enumerate()
        .map(|(window_position, window)| regular_session_window_row(window, window_position))
        .collect::<Result<Vec<_>, CliError>>()?;

    let table = CompactTable {
        headers: vec![
            "Weekday".to_string(),
            "Open".to_string(),
            "Close".to_string(),
            "Label".to_string(),
        ],
        rows,
    }
    .render(renderer);

    Ok(format!("Regular Session Windows\n\n{table}"))
}

fn regular_session_window_row(
    window: &Value,
    window_position: usize,
) -> Result<Vec<String>, CliError> {
    let command_path = "presets.show";
    let field_path = format!("configuration.regular_session_windows[{window_position}]");

    let window = required_value_object(window, &field_path, command_path)?;

    Ok(vec![
        required_object_string(window, "weekday", command_path)?.to_string(),
        required_object_string(window, "open_time", command_path)?.to_string(),
        required_object_string(window, "close_time", command_path)?.to_string(),
        required_object_string(window, "label", command_path)?.to_string(),
    ])
}

fn render_recurring_closed_windows(
    windows: &Value,
    renderer: &TerminalRenderer,
) -> Result<String, CliError> {
    let windows = required_value_array(
        windows,
        "configuration.recurring_closed_windows",
        "presets.show",
    )?;

    if windows.is_empty() {
        return Ok("Recurring Closed Windows\n\nNo windows".to_string());
    }

    let rows = windows
        .iter()
        .enumerate()
        .map(|(window_position, window)| recurring_closed_window_row(window, window_position))
        .collect::<Result<Vec<_>, CliError>>()?;

    let table = CompactTable {
        headers: vec![
            "Start Weekday".to_string(),
            "Start".to_string(),
            "End Weekday".to_string(),
            "End".to_string(),
            "Reason".to_string(),
        ],
        rows,
    }
    .render(renderer);

    Ok(format!("Recurring Closed Windows\n\n{table}"))
}

fn recurring_closed_window_row(
    window: &Value,
    window_position: usize,
) -> Result<Vec<String>, CliError> {
    let command_path = "presets.show";
    let field_path = format!("configuration.recurring_closed_windows[{window_position}]");

    let window = required_value_object(window, &field_path, command_path)?;

    Ok(vec![
        required_object_string(window, "start_weekday", command_path)?.to_string(),
        required_object_string(window, "start_time", command_path)?.to_string(),
        required_object_string(window, "end_weekday", command_path)?.to_string(),
        required_object_string(window, "end_time", command_path)?.to_string(),
        required_object_string(window, "reason", command_path)?.to_string(),
    ])
}

fn event_table_row(event: &Value, event_position: usize) -> Result<Vec<String>, CliError> {
    let command_path = "presets.show";
    let field_path = format!("configuration.events[{event_position}]");

    let event = required_value_object(event, &field_path, command_path)?;

    Ok(vec![
        required_object_string(event, "scheduled_at", command_path)?.to_string(),
        required_object_string(event, "event_id", command_path)?.to_string(),
        nullable_event_name_label(event, &field_path)?,
        event_tags_label(event, &field_path)?,
    ])
}

fn nullable_event_name_label(
    event: &Map<String, Value>,
    field_path: &str,
) -> Result<String, CliError> {
    let command_path = "presets.show";
    match event.get("event_name") {
        Some(Value::String(event_name)) => Ok(event_name.to_string()),
        Some(Value::Null) | None => Ok("-".to_string()),
        Some(_) => Err(malformed_payload(
            command_path,
            &format!("{field_path}.event_name must be a string when provided"),
        )),
    }
}

fn event_tags_label(event: &Map<String, Value>, field_path: &str) -> Result<String, CliError> {
    let command_path = "presets.show";
    let tags = event
        .get("tags")
        .ok_or_else(|| malformed_payload(command_path, &format!("{field_path}.tags is required")))
        .and_then(|tags| required_value_array(tags, &format!("{field_path}.tags"), command_path))?;

    let mut tag_labels = Vec::new();
    for (tag_position, tag) in tags.iter().enumerate() {
        let tag = required_value_string(
            tag,
            &format!("{field_path}.tags[{tag_position}]"),
            command_path,
        )?;
        tag_labels.push(tag);
    }

    Ok(tag_labels.join(", "))
}

fn required_configuration_value<'a>(
    configuration: &'a Map<String, Value>,
    field_name: &str,
) -> Result<&'a Value, CliError> {
    configuration.get(field_name).ok_or_else(|| {
        malformed_payload(
            "presets.show",
            &format!("configuration.{field_name} is required"),
        )
    })
}
