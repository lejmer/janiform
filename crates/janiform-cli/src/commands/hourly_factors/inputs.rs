use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use janiform::hourly_factors::DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD;
use janiform_foundations::time::parse_utc_datetime;
use serde_json::{Map, Value};

use super::args::HourlyFactorCommonOptions;
use crate::commands::shared::artifacts::load_json_artifact;
use crate::commands::shared::inputs::{
    parse_exact_positive_interval_duration_nanos, parse_utc_timestamp_argument,
    required_input_file, required_non_empty_argument,
};
use crate::context::CommandContext;
use crate::error::CliError;

pub(super) fn resolve_common_options(
    context: &CommandContext,
    options: &HourlyFactorCommonOptions,
) -> Result<ResolvedHourlyFactorCommonOptions, CliError> {
    let calendar_registry_path = required_input_file(
        context,
        "--calendar-registry-json",
        &options.calendar_registry_json,
    )?;

    let exchange_code =
        required_non_empty_argument("--exchange-code", options.exchange_code.clone())?;

    let range_start_utc = parse_utc_timestamp_argument(
        "--range-start-utc",
        options
            .range_start_utc
            .as_deref()
            .ok_or(CliError::MissingRequiredArgument {
                argument_name: "--range-start-utc",
            })?,
    )?;

    let range_end_utc = parse_utc_timestamp_argument(
        "--range-end-utc",
        options
            .range_end_utc
            .as_deref()
            .ok_or(CliError::MissingRequiredArgument {
                argument_name: "--range-end-utc",
            })?,
    )?;

    let interval_duration_nanos =
        parse_exact_positive_interval_duration_nanos("--interval", &options.interval)?;

    Ok(ResolvedHourlyFactorCommonOptions {
        calendar_registry_path,
        exchange_code,
        range_start_utc,
        range_end_utc,
        interval_duration_nanos,
    })
}

pub(super) fn load_factor_source(
    context: &CommandContext,
    explicit_path: Option<&PathBuf>,
    materialized_path: Option<&PathBuf>,
    source_kind: SourceKind,
    resolved: &ResolvedHourlyFactorCommonOptions,
) -> Result<(Vec<f64>, String), CliError> {
    match (explicit_path, materialized_path) {
        (Some(path), None) => {
            let path = context
                .paths()
                .input_file(source_kind.explicit_argument(), path)?;

            let document = load_json_artifact(&path, source_kind.artifact_name())?;

            let fields = object_document(&document, source_kind.artifact_name(), &path)?;

            let factors =
                f64_array_field(fields, "hourly_factors", source_kind.artifact_name(), &path)?;

            let materialization_method = optional_string_field(fields, "materialization_method")
                .unwrap_or(DEFAULT_HOURLY_FACTOR_MATERIALIZATION_METHOD)
                .to_string();

            Ok((factors, materialization_method))
        }
        (None, Some(path)) => {
            let path = context
                .paths()
                .input_file(source_kind.materialized_argument(), path)?;

            let document = load_json_artifact(&path, source_kind.artifact_name())?;

            let fields = object_document(&document, source_kind.artifact_name(), &path)?;

            validate_materialized_source_compatibility(fields, source_kind, resolved, &path)?;

            let factors = f64_array_field(
                fields,
                source_kind.materialized_factor_field(),
                source_kind.artifact_name(),
                &path,
            )?;

            let materialization_method =
                string_field(fields, "schema_version", source_kind.artifact_name(), &path)?
                    .to_string();

            Ok((factors, materialization_method))
        }
        _ => Err(CliError::InvalidCommandArgument {
            argument_name: source_kind.argument_group_name(),
            message: "provide exactly one source document".to_string(),
        }),
    }
}

pub(super) fn object_document<'a>(
    document: &'a Value,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a Map<String, Value>, CliError> {
    document
        .as_object()
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: "artifact root must be a JSON object".to_string(),
        })
}

pub(super) fn optional_string_field<'a>(
    fields: &'a Map<String, Value>,
    field_name: &str,
) -> Option<&'a str> {
    fields.get(field_name).and_then(Value::as_str)
}

pub(super) fn f64_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<f64, CliError> {
    fields
        .get(field_name)
        .and_then(Value::as_f64)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be numeric"),
        })
}

pub(super) fn optional_usize_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    default_value: usize,
    artifact_name: &'static str,
    path: &Path,
) -> Result<usize, CliError> {
    let Some(value) = fields.get(field_name) else {
        return Ok(default_value);
    };

    let value = value.as_u64().ok_or_else(|| CliError::ArtifactMalformed {
        artifact_name,
        path: path.to_path_buf(),
        message: format!("{field_name} must be a non-negative integer"),
    })?;

    usize::try_from(value).map_err(|source| CliError::ArtifactMalformed {
        artifact_name,
        path: path.to_path_buf(),
        message: format!("{field_name} does not fit on this platform: {source}"),
    })
}

pub(super) fn f64_array_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<f64>, CliError> {
    let values = fields
        .get(field_name)
        .and_then(Value::as_array)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be an array"),
        })?;

    values
        .iter()
        .enumerate()
        .map(|(value_position, value)| {
            value.as_f64().ok_or_else(|| CliError::ArtifactMalformed {
                artifact_name,
                path: path.to_path_buf(),
                message: format!("{field_name}[{value_position}] must be numeric"),
            })
        })
        .collect()
}

pub(super) fn timestamp_nanos_array_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<i64>, CliError> {
    let values = fields
        .get(field_name)
        .and_then(Value::as_array)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be an array"),
        })?;

    parse_timestamp_array_values(values, field_name, artifact_name, path)
}

pub(super) fn optional_timestamp_nanos_array_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<i64>, CliError> {
    let Some(values) = fields.get(field_name) else {
        return Ok(Vec::new());
    };

    let values = values
        .as_array()
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be an array"),
        })?;

    parse_timestamp_array_values(values, field_name, artifact_name, path)
}

pub(super) fn optional_multiplier_entries(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<(u64, f64)>, CliError> {
    let Some(value) = fields.get(field_name) else {
        return Ok(Vec::new());
    };

    let object = value
        .as_object()
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be an object keyed by interval index"),
        })?;

    let mut entries = Vec::with_capacity(object.len());

    for (interval_index, multiplier) in object {
        let interval_index =
            interval_index
                .parse::<u64>()
                .map_err(|source| CliError::ArtifactMalformed {
                    artifact_name,
                    path: path.to_path_buf(),
                    message: format!(
                        "{field_name} key `{interval_index}` must be an integer: {source}"
                    ),
                })?;

        let multiplier = multiplier
            .as_f64()
            .ok_or_else(|| CliError::ArtifactMalformed {
                artifact_name,
                path: path.to_path_buf(),
                message: format!("{field_name}[{interval_index}] must be numeric"),
            })?;

        entries.push((interval_index, multiplier));
    }

    entries.sort_by_key(|(interval_index, _)| *interval_index);

    Ok(entries)
}

fn validate_materialized_source_compatibility(
    fields: &Map<String, Value>,
    source_kind: SourceKind,
    resolved: &ResolvedHourlyFactorCommonOptions,
    path: &Path,
) -> Result<(), CliError> {
    let exchange_code = string_field(fields, "exchange_code", source_kind.artifact_name(), path)?;
    if exchange_code != resolved.exchange_code {
        return Err(CliError::ArtifactMalformed {
            artifact_name: source_kind.artifact_name(),
            path: path.to_path_buf(),
            message: format!(
                "exchange_code `{exchange_code}` does not match requested exchange `{}`",
                resolved.exchange_code
            ),
        });
    }

    let start_nanos = i64_field(
        fields,
        "request_start_utc_nanos",
        source_kind.artifact_name(),
        path,
    )?;

    let end_nanos = i64_field(
        fields,
        "request_end_utc_nanos",
        source_kind.artifact_name(),
        path,
    )?;

    if start_nanos != utc_nanos(resolved.range_start_utc)?
        || end_nanos != utc_nanos(resolved.range_end_utc)?
    {
        return Err(CliError::ArtifactMalformed {
            artifact_name: source_kind.artifact_name(),
            path: path.to_path_buf(),
            message: "request UTC range does not match the build-contract request".to_string(),
        });
    }

    Ok(())
}

fn string_field<'a>(
    fields: &'a Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<&'a str, CliError> {
    fields
        .get(field_name)
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be a string"),
        })
}

fn i64_field(
    fields: &Map<String, Value>,
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<i64, CliError> {
    fields
        .get(field_name)
        .and_then(Value::as_i64)
        .ok_or_else(|| CliError::ArtifactMalformed {
            artifact_name,
            path: path.to_path_buf(),
            message: format!("{field_name} must be an integer"),
        })
}

fn parse_timestamp_array_values(
    values: &[Value],
    field_name: &'static str,
    artifact_name: &'static str,
    path: &Path,
) -> Result<Vec<i64>, CliError> {
    values
        .iter()
        .enumerate()
        .map(|(value_position, value)| match value {
            Value::String(timestamp) => parse_utc_datetime(timestamp)
                .map(|timestamp| utc_nanos(timestamp))
                .map_err(|source| CliError::ArtifactMalformed {
                    artifact_name,
                    path: path.to_path_buf(),
                    message: format!(
                        "{field_name}[{value_position}] must be a valid RFC 3339 timestamp: {source}"
                    ),
                })?,
            Value::Number(_) => value.as_i64().ok_or_else(|| CliError::ArtifactMalformed {
                artifact_name,
                path: path.to_path_buf(),
                message: format!("{field_name}[{value_position}] must fit in i64"),
            }),
            _ => Err(CliError::ArtifactMalformed {
                artifact_name,
                path: path.to_path_buf(),
                message: format!(
                    "{field_name}[{value_position}] must be a timestamp string or UTC nanoseconds integer"
                ),
            }),
        })
        .collect()
}

fn utc_nanos(timestamp: DateTime<Utc>) -> Result<i64, CliError> {
    timestamp
        .timestamp_nanos_opt()
        .ok_or_else(|| CliError::InvalidCommandArgument {
            argument_name: "timestamp",
            message: "timestamp is outside the supported nanosecond range".to_string(),
        })
}

pub(super) struct ResolvedHourlyFactorCommonOptions {
    pub(super) calendar_registry_path: PathBuf,
    pub(super) exchange_code: String,
    pub(super) range_start_utc: DateTime<Utc>,
    pub(super) range_end_utc: DateTime<Utc>,
    pub(super) interval_duration_nanos: i64,
}

#[derive(Clone, Copy)]
pub(super) enum SourceKind {
    Seasonality,
    NewsIntensity,
}

impl SourceKind {
    fn explicit_argument(self) -> &'static str {
        match self {
            Self::Seasonality => "--seasonality-explicit-source-json",
            Self::NewsIntensity => "--news-intensity-explicit-source-json",
        }
    }

    fn materialized_argument(self) -> &'static str {
        match self {
            Self::Seasonality => "--seasonality-materialized-source-json",
            Self::NewsIntensity => "--news-intensity-materialized-source-json",
        }
    }

    fn argument_group_name(self) -> &'static str {
        match self {
            Self::Seasonality => "seasonality source",
            Self::NewsIntensity => "news-intensity source",
        }
    }

    fn artifact_name(self) -> &'static str {
        match self {
            Self::Seasonality => "hourly seasonality source",
            Self::NewsIntensity => "hourly news-intensity source",
        }
    }

    fn materialized_factor_field(self) -> &'static str {
        match self {
            Self::Seasonality => "s_h",
            Self::NewsIntensity => "nu_h",
        }
    }
}
