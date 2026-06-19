use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, RecordBatch, TimestampNanosecondArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{SecondsFormat, Utc};
use parquet::arrow::ArrowWriter;

use crate::artifact_io::{ArtifactWriteKind, write_pretty_json_file};
use crate::error::WorkflowError;

use super::output::{
    ForecastBundleManifest, ForecastExpectedPathRow, ForecastOutput,
    ForecastTerminalDistributionRow,
};
use super::request::ForecastTableFormat;

const FORECAST_MANIFEST_FILE_NAME: &str = "forecast.json";
const EXPECTED_PATH_TABLE_STEM: &str = "expected_path";
const TERMINAL_DISTRIBUTION_TABLE_STEM: &str = "terminal_distribution";
pub(super) const FORECAST_BUNDLE_ARTIFACT_COUNT: usize = 3;

/// Save a forecast output as a directory bundle and return the resolved bundle path.
pub fn save_forecast_bundle(
    forecast: &ForecastOutput,
    target_path: &Path,
    table_format: ForecastTableFormat,
) -> Result<PathBuf, WorkflowError> {
    save_forecast_bundle_with_progress(forecast, target_path, table_format, |_| {})
}

pub(super) fn save_forecast_bundle_with_progress(
    forecast: &ForecastOutput,
    target_path: &Path,
    table_format: ForecastTableFormat,
    mut observe_written_artifact_count: impl FnMut(usize),
) -> Result<PathBuf, WorkflowError> {
    let bundle_path = resolve_bundle_path(target_path)?;

    fs::create_dir_all(&bundle_path)
        .map_err(|source| artifact_write_error(bundle_path.as_path(), source))?;

    let expected_path_file_name = table_file_name(EXPECTED_PATH_TABLE_STEM, table_format);

    let terminal_distribution_file_name =
        table_file_name(TERMINAL_DISTRIBUTION_TABLE_STEM, table_format);

    write_expected_path_table(
        bundle_path.join(&expected_path_file_name).as_path(),
        &forecast.expected_path,
        table_format,
    )?;

    observe_written_artifact_count(1);

    write_terminal_distribution_table(
        bundle_path.join(&terminal_distribution_file_name).as_path(),
        &forecast.terminal_distribution,
        table_format,
    )?;

    observe_written_artifact_count(2);

    // Save may override table file names without mutating the in-memory result.
    let mut manifest = forecast.manifest.clone();

    manifest.expected_path_table.file_name = expected_path_file_name;
    manifest.expected_path_table.table_format = table_format;
    manifest.terminal_distribution_table.file_name = terminal_distribution_file_name;
    manifest.terminal_distribution_table.table_format = table_format;

    write_pretty_json_file(
        bundle_path.join(FORECAST_MANIFEST_FILE_NAME).as_path(),
        &manifest,
        ArtifactWriteKind::Forecast,
    )?;

    observe_written_artifact_count(FORECAST_BUNDLE_ARTIFACT_COUNT);

    Ok(bundle_path)
}

/// Load the manifest from a forecast bundle directory.
pub fn load_forecast_bundle_manifest(
    bundle_path: &Path,
) -> Result<ForecastBundleManifest, WorkflowError> {
    if !bundle_path.is_dir() {
        return Err(WorkflowError::ForecastArtifactMalformed {
            path: bundle_path.to_path_buf(),
            message: "forecast load path must be a bundle directory".to_string(),
        });
    }

    let manifest_path = bundle_path.join(FORECAST_MANIFEST_FILE_NAME);

    let payload = fs::read_to_string(manifest_path.as_path()).map_err(|source| {
        WorkflowError::ForecastArtifactRead {
            path: manifest_path.clone(),
            source,
        }
    })?;

    serde_json::from_str(&payload).map_err(|source| WorkflowError::ForecastArtifactParse {
        path: manifest_path,
        source,
    })
}

pub(super) fn table_file_name(stem: &str, table_format: ForecastTableFormat) -> String {
    format!("{stem}.{}", table_format.extension())
}

fn resolve_bundle_path(target_path: &Path) -> Result<PathBuf, WorkflowError> {
    if target_path.is_file() {
        return Err(WorkflowError::ForecastArtifactMalformed {
            path: target_path.to_path_buf(),
            message: "forecast output path must not be an existing file".to_string(),
        });
    }

    if !target_path.exists() {
        return Ok(target_path.to_path_buf());
    }

    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let base_name = format!("forecast_{timestamp}");

    let mut suffix: Option<usize> = None;

    loop {
        let candidate = match suffix {
            None => target_path.join(&base_name),
            Some(suffix_value) => target_path.join(format!("{base_name}_{suffix_value}")),
        };

        if !candidate.exists() {
            return Ok(candidate);
        }

        suffix = Some(match suffix {
            None => 1,
            Some(suffix_value) => suffix_value.checked_add(1).ok_or_else(|| {
                WorkflowError::ForecastArtifactMalformed {
                    path: target_path.to_path_buf(),
                    message: "could not resolve a unique forecast bundle child directory"
                        .to_string(),
                }
            })?,
        });
    }
}

fn write_expected_path_table(
    output_path: &Path,
    rows: &[ForecastExpectedPathRow],
    table_format: ForecastTableFormat,
) -> Result<(), WorkflowError> {
    match table_format {
        ForecastTableFormat::Parquet => write_expected_path_parquet(output_path, rows),
        ForecastTableFormat::Csv => write_expected_path_csv(output_path, rows),
        ForecastTableFormat::Json => write_json_rows(output_path, rows),
    }
}

fn write_terminal_distribution_table(
    output_path: &Path,
    rows: &[ForecastTerminalDistributionRow],
    table_format: ForecastTableFormat,
) -> Result<(), WorkflowError> {
    match table_format {
        ForecastTableFormat::Parquet => write_terminal_distribution_parquet(output_path, rows),
        ForecastTableFormat::Csv => write_terminal_distribution_csv(output_path, rows),
        ForecastTableFormat::Json => write_json_rows(output_path, rows),
    }
}

fn write_expected_path_csv(
    output_path: &Path,
    rows: &[ForecastExpectedPathRow],
) -> Result<(), WorkflowError> {
    let mut writer =
        csv::Writer::from_path(output_path).map_err(|source| csv_error(output_path, source))?;

    writer
        .write_record([
            "interval_index",
            "timestamp_utc",
            "expected_log_return",
            "expected_log_price",
            "geometric_mean_price_ratio",
            "geometric_mean_price",
            "log_expected_price_ratio",
            "expected_price_ratio",
            "expected_price",
        ])
        .map_err(|source| csv_error(output_path, source))?;

    for row in rows {
        writer
            .write_record([
                row.interval_index.to_string(),
                format_timestamp_utc(row.timestamp_utc),
                row.expected_log_return.to_string(),
                row.expected_log_price.to_string(),
                row.geometric_mean_price_ratio.to_string(),
                row.geometric_mean_price.to_string(),
                row.log_expected_price_ratio.to_string(),
                row.expected_price_ratio.to_string(),
                row.expected_price.to_string(),
            ])
            .map_err(|source| csv_error(output_path, source))?;
    }

    writer
        .flush()
        .map_err(|source| artifact_write_error(output_path, source))
}

fn write_terminal_distribution_csv(
    output_path: &Path,
    rows: &[ForecastTerminalDistributionRow],
) -> Result<(), WorkflowError> {
    let mut writer =
        csv::Writer::from_path(output_path).map_err(|source| csv_error(output_path, source))?;

    writer
        .write_record([
            "node_index",
            "terminal_log_return",
            "terminal_price_ratio",
            "terminal_price",
            "log_return_density",
            "price_density",
            "probability_mass",
            "cumulative_probability",
        ])
        .map_err(|source| csv_error(output_path, source))?;

    for row in rows {
        writer
            .write_record([
                row.node_index.to_string(),
                row.terminal_log_return.to_string(),
                row.terminal_price_ratio.to_string(),
                row.terminal_price.to_string(),
                row.log_return_density.to_string(),
                row.price_density.to_string(),
                row.probability_mass.to_string(),
                row.cumulative_probability.to_string(),
            ])
            .map_err(|source| csv_error(output_path, source))?;
    }

    writer
        .flush()
        .map_err(|source| artifact_write_error(output_path, source))
}

fn write_expected_path_parquet(
    output_path: &Path,
    rows: &[ForecastExpectedPathRow],
) -> Result<(), WorkflowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("interval_index", DataType::UInt64, false),
        Field::new(
            "timestamp_utc",
            DataType::Timestamp(TimeUnit::Nanosecond, Some("+00:00".into())),
            false,
        ),
        Field::new("expected_log_return", DataType::Float64, false),
        Field::new("expected_log_price", DataType::Float64, false),
        Field::new("geometric_mean_price_ratio", DataType::Float64, false),
        Field::new("geometric_mean_price", DataType::Float64, false),
        Field::new("log_expected_price_ratio", DataType::Float64, false),
        Field::new("expected_price_ratio", DataType::Float64, false),
        Field::new("expected_price", DataType::Float64, false),
    ]));

    let timestamp_nanos = rows
        .iter()
        .map(|row| {
            row.timestamp_utc.timestamp_nanos_opt().ok_or_else(|| {
                WorkflowError::ForecastArtifactMalformed {
                    path: output_path.to_path_buf(),
                    message: "expected_path timestamp is outside the supported nanosecond range"
                        .to_string(),
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from(
            rows.iter()
                .map(|row| table_index_to_u64(row.interval_index, "interval_index", output_path))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Arc::new(TimestampNanosecondArray::from(timestamp_nanos).with_timezone_utc()),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.expected_log_return)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.expected_log_price)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.geometric_mean_price_ratio)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.geometric_mean_price)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.log_expected_price_ratio)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.expected_price_ratio)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.expected_price)
                .collect::<Vec<_>>(),
        )),
    ];

    write_record_batch(output_path, schema, arrays)
}

fn write_terminal_distribution_parquet(
    output_path: &Path,
    rows: &[ForecastTerminalDistributionRow],
) -> Result<(), WorkflowError> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("node_index", DataType::UInt64, false),
        Field::new("terminal_log_return", DataType::Float64, false),
        Field::new("terminal_price_ratio", DataType::Float64, false),
        Field::new("terminal_price", DataType::Float64, false),
        Field::new("log_return_density", DataType::Float64, false),
        Field::new("price_density", DataType::Float64, false),
        Field::new("probability_mass", DataType::Float64, false),
        Field::new("cumulative_probability", DataType::Float64, false),
    ]));

    let arrays: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from(
            rows.iter()
                .map(|row| table_index_to_u64(row.node_index, "node_index", output_path))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.terminal_log_return)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.terminal_price_ratio)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.terminal_price)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.log_return_density)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter().map(|row| row.price_density).collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.probability_mass)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.cumulative_probability)
                .collect::<Vec<_>>(),
        )),
    ];

    write_record_batch(output_path, schema, arrays)
}

fn write_record_batch(
    output_path: &Path,
    schema: Arc<Schema>,
    arrays: Vec<ArrayRef>,
) -> Result<(), WorkflowError> {
    let writer_schema = Arc::clone(&schema);

    let batch = RecordBatch::try_new(schema, arrays)
        .map_err(|source| artifact_write_error(output_path, std::io::Error::other(source)))?;

    let file = fs::File::create(output_path)
        .map_err(|source| artifact_write_error(output_path, source))?;

    let mut writer = ArrowWriter::try_new(file, writer_schema, None)
        .map_err(|source| artifact_write_error(output_path, std::io::Error::other(source)))?;

    writer
        .write(&batch)
        .map_err(|source| artifact_write_error(output_path, std::io::Error::other(source)))?;

    writer
        .close()
        .map_err(|source| artifact_write_error(output_path, std::io::Error::other(source)))?;

    Ok(())
}

fn table_index_to_u64(
    value: usize,
    field_name: &'static str,
    output_path: &Path,
) -> Result<u64, WorkflowError> {
    u64::try_from(value).map_err(|_| WorkflowError::ForecastArtifactMalformed {
        path: output_path.to_path_buf(),
        message: format!("forecast table field `{field_name}` exceeds uint64 range"),
    })
}

fn write_json_rows<T>(output_path: &Path, rows: &[T]) -> Result<(), WorkflowError>
where
    T: serde::Serialize,
{
    let mut file = fs::File::create(output_path)
        .map_err(|source| artifact_write_error(output_path, source))?;

    file.write_all(b"[")
        .map_err(|source| artifact_write_error(output_path, source))?;

    for (row_position, row) in rows.iter().enumerate() {
        if row_position > 0 {
            file.write_all(b",")
                .map_err(|source| artifact_write_error(output_path, source))?;
        }

        serde_json::to_writer(&mut file, row)
            .map_err(|source| WorkflowError::OutputSerialize { source })?;
    }

    file.write_all(b"]")
        .map_err(|source| artifact_write_error(output_path, source))
}

fn format_timestamp_utc(timestamp_utc: chrono::DateTime<Utc>) -> String {
    timestamp_utc.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn artifact_write_error(path: &Path, source: std::io::Error) -> WorkflowError {
    WorkflowError::ForecastArtifactWrite {
        path: path.to_path_buf(),
        source,
    }
}

fn csv_error(path: &Path, source: csv::Error) -> WorkflowError {
    artifact_write_error(path, std::io::Error::other(source))
}
