use std::fs;
use std::path::Path;

use janiform_terminal_ui::KeyValueRow;
use janiform_terminal_ui::format_artifact_path;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::commands::reporting::ReportFitCommand;
use crate::commands::shared::terminal::emit_json_or_success_summary;
use crate::context::CommandContext;
use crate::error::CliError;

use super::FIT_REPORT_HTML_EXTENSION;
use super::FitReportContext;
use super::parameter_fits::report_with_parameter_fits;
use super::report_run_id;
use super::report_success;

type JsonFieldRows<'a> = Vec<(&'a str, &'a Value)>;

#[derive(Clone, Debug, Serialize)]
struct FitReportExportPayload {
    status: &'static str,
    rendered_report_path: String,
    source_report_path: String,
}

pub(super) fn export_fit_report(
    context: &CommandContext,
    command: ReportFitCommand,
    report_context: &FitReportContext,
    report: &Value,
    model: Option<&Value>,
) -> Result<(), CliError> {
    let output_path = match command.output_path.output_path {
        Some(path) => context.paths().resolve_process_path(&path),
        None => report_context
            .report_path
            .with_extension(FIT_REPORT_HTML_EXTENSION),
    };

    let enriched_report = report_with_parameter_fits(report, model);

    write_html_report(&output_path, &enriched_report)?;

    let payload = FitReportExportPayload {
        status: "written",
        rendered_report_path: output_path.display().to_string(),
        source_report_path: report_context.report_path.display().to_string(),
    };

    emit_json_or_success_summary(
        context,
        &payload,
        "Fit report written",
        vec![KeyValueRow::new("HTML", format_artifact_path(&output_path))],
        vec![KeyValueRow::new("HTML", format_artifact_path(&output_path))],
    )
}

fn write_html_report(output_path: &Path, report: &Value) -> Result<(), CliError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|source| CliError::ArtifactWrite {
            artifact_name: "rendered fit report",
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let html = render_html_report(report);

    fs::write(output_path, html).map_err(|source| CliError::ArtifactWrite {
        artifact_name: "rendered fit report",
        path: output_path.to_path_buf(),
        source,
    })
}

fn render_html_report(report: &Value) -> String {
    let title = format!("Fit report: {}", report_run_id(report));
    let outcome_class = if report_success(report) {
        "badge-success"
    } else {
        "badge-failure"
    };
    let outcome_label = if report_success(report) {
        "Completed"
    } else {
        "Needs attention"
    };

    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>{}</style>\n</head>\n<body>\n<main>\n<header class=\"report-header\">\n<p class=\"eyebrow\">Fit report</p>\n<h1>{}</h1>\n<div class=\"badge-row\"><span class=\"badge {}\">{}</span></div>\n</header>\n{}\n</main>\n</body>\n</html>\n",
        escape_html(&title),
        html_styles(),
        escape_html(&title),
        outcome_class,
        outcome_label,
        render_report_sections(report)
    )
}

fn render_report_sections(report: &Value) -> String {
    let Some(report_fields) = report.as_object() else {
        return render_section("Report", &render_value(report, 2), 2);
    };

    let (overview_fields, detail_fields) = split_object_fields(report_fields);
    let mut html = String::new();

    if !overview_fields.is_empty() {
        html.push_str(&render_section(
            "Overview",
            &render_field_table(&overview_fields),
            2,
        ));
    }

    for (key, value) in detail_fields {
        html.push_str(&render_section(
            &humanize_key(key),
            &render_value(value, 2),
            2,
        ));
    }

    html
}

fn render_value(value: &Value, depth: usize) -> String {
    match value {
        Value::Object(fields) => render_object(fields, depth),
        Value::Array(values) => render_array(values, depth),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            render_scalar_value(value)
        }
    }
}

fn render_object(fields: &Map<String, Value>, depth: usize) -> String {
    if fields.is_empty() {
        return "<p class=\"empty\">None recorded</p>".to_string();
    }

    let (scalar_fields, detail_fields) = split_object_fields(fields);
    let mut html = String::new();

    if !scalar_fields.is_empty() {
        html.push_str(&render_field_table(&scalar_fields));
    }

    for (key, value) in detail_fields {
        html.push_str(&render_nested_group(key, value, depth + 1));
    }

    html
}

fn render_array(values: &[Value], depth: usize) -> String {
    if values.is_empty() {
        return "<p class=\"empty\">None recorded</p>".to_string();
    }

    if let Some(table) = render_object_array_table(values) {
        return table;
    }

    if values.iter().all(is_scalar_value) {
        return render_scalar_list(values);
    }

    let mut html = String::from("<ol class=\"object-list\">");

    for value in values {
        html.push_str("<li>");
        html.push_str(&render_value(value, depth + 1));
        html.push_str("</li>");
    }

    html.push_str("</ol>");
    html
}

fn render_nested_group(key: &str, value: &Value, depth: usize) -> String {
    let title = humanize_key(key);
    render_section(&title, &render_value(value, depth), depth)
}

fn render_section(title: &str, body: &str, depth: usize) -> String {
    let heading_level = heading_level(depth);

    format!(
        "<section class=\"report-section depth-{}\"><h{}>{}</h{}>{}</section>\n",
        depth,
        heading_level,
        escape_html(title),
        heading_level,
        body
    )
}

fn render_field_table(fields: &[(&str, &Value)]) -> String {
    let mut html = String::from("<table class=\"field-table\"><tbody>");

    for (key, value) in fields {
        html.push_str("<tr><th scope=\"row\">");
        html.push_str(&escape_html(&humanize_key(key)));
        html.push_str("</th><td>");
        html.push_str(&render_scalar_value(value));
        html.push_str("</td></tr>");
    }

    html.push_str("</tbody></table>");
    html
}

fn render_object_array_table(values: &[Value]) -> Option<String> {
    let mut objects = Vec::new();

    for value in values {
        let object = value.as_object()?;

        if object.values().any(|value| !is_scalar_value(value)) {
            return None;
        }

        objects.push(object);
    }

    if objects.is_empty() {
        return None;
    }

    let mut keys = Vec::new();

    for object in &objects {
        for key in object.keys() {
            if !keys.contains(&key.as_str()) {
                keys.push(key.as_str());
            }
        }
    }

    if keys.is_empty() {
        return None;
    }

    let mut html =
        String::from("<div class=\"table-scroll\"><table class=\"data-table\"><thead><tr>");

    for key in &keys {
        html.push_str("<th scope=\"col\">");
        html.push_str(&escape_html(&humanize_key(key)));
        html.push_str("</th>");
    }

    html.push_str("</tr></thead><tbody>");

    for object in objects {
        html.push_str("<tr>");

        for key in &keys {
            html.push_str("<td>");

            if let Some(value) = object.get(*key) {
                html.push_str(&render_scalar_value(value));
            } else {
                html.push_str("<span class=\"empty\">not recorded</span>");
            }

            html.push_str("</td>");
        }

        html.push_str("</tr>");
    }

    html.push_str("</tbody></table></div>");
    Some(html)
}

fn render_scalar_list(values: &[Value]) -> String {
    let mut html = String::from("<ul class=\"value-list\">");

    for value in values {
        html.push_str("<li>");
        html.push_str(&render_scalar_value(value));
        html.push_str("</li>");
    }

    html.push_str("</ul>");
    html
}

fn render_scalar_value(value: &Value) -> String {
    match value {
        Value::Null => "<span class=\"empty\">not recorded</span>".to_string(),
        Value::Bool(value) => {
            let label = if *value { "yes" } else { "no" };
            format!("<span class=\"scalar scalar-bool\">{label}</span>")
        }
        Value::Number(value) => format!("<span class=\"scalar\">{value}</span>"),
        Value::String(value) => format!(
            "<span class=\"scalar scalar-string\">{}</span>",
            escape_html(value)
        ),
        Value::Array(_) | Value::Object(_) => render_value(value, 2),
    }
}

fn split_object_fields<'a>(
    fields: &'a Map<String, Value>,
) -> (JsonFieldRows<'a>, JsonFieldRows<'a>) {
    let mut scalar_fields = Vec::new();
    let mut detail_fields = Vec::new();

    for (key, value) in fields {
        if is_scalar_value(value) {
            scalar_fields.push((key.as_str(), value));
        } else {
            detail_fields.push((key.as_str(), value));
        }
    }

    (scalar_fields, detail_fields)
}

fn is_scalar_value(value: &Value) -> bool {
    matches!(
        value,
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
    )
}

fn heading_level(depth: usize) -> usize {
    depth.clamp(2, 4)
}

fn humanize_key(key: &str) -> String {
    let label = key.replace(['_', '-'], " ");
    let mut characters = label.chars();

    let Some(first_character) = characters.next() else {
        return String::new();
    };

    format!("{}{}", first_character.to_uppercase(), characters.as_str())
}

fn html_styles() -> &'static str {
    concat!(
        "body{font-family:system-ui,-apple-system,Segoe UI,sans-serif;margin:0;",
        "background:#f4f5f2;color:#242424}",
        "main{max-width:1120px;margin:0 auto;padding:40px 24px 56px}",
        ".report-header{border-bottom:1px solid #d8dcd3;margin-bottom:28px;",
        "padding-bottom:22px}",
        ".eyebrow{color:#65705f;font-size:13px;font-weight:700;letter-spacing:.06em;",
        "margin:0 0 8px;text-transform:uppercase}",
        "h1{font-size:32px;line-height:1.15;margin:0 0 18px}",
        "h2{font-size:22px;margin:0 0 16px}",
        "h3{font-size:17px;margin:26px 0 10px}",
        "h4{font-size:15px;margin:22px 0 8px}",
        ".badge-row{display:flex;gap:8px;flex-wrap:wrap}",
        ".badge{border-radius:999px;display:inline-flex;font-size:13px;",
        "font-weight:700;padding:5px 10px}",
        ".badge-success{background:#dfeee1;color:#1d5f30}",
        ".badge-failure{background:#f4dedb;color:#8f2a1f}",
        ".report-section{background:#fff;border:1px solid #d8dcd3;border-radius:8px;",
        "margin:18px 0;padding:18px}",
        ".report-section .report-section{background:#fafbf8;margin:16px 0 0}",
        ".field-table,.data-table{border-collapse:collapse;width:100%}",
        ".field-table th{color:#65705f;font-weight:650;text-align:left;",
        "vertical-align:top;width:220px}",
        ".field-table th,.field-table td,.data-table th,.data-table td{",
        "border-bottom:1px solid #e5e7e0;padding:8px 10px}",
        ".field-table tr:last-child th,.field-table tr:last-child td,",
        ".data-table tr:last-child td{border-bottom:0}",
        ".data-table th{background:#eef1ea;color:#4e5948;font-size:13px;",
        "text-align:left}",
        ".table-scroll{overflow-x:auto}",
        ".value-list,.object-list{margin:0;padding-left:22px}",
        ".value-list li,.object-list li{margin:6px 0}",
        ".scalar-string{overflow-wrap:anywhere}",
        ".empty{color:#7a8174;font-style:italic}",
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
