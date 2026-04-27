use std::fs;
use std::path::Path;
use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, RecordBatch, StringArray, TimestampNanosecondArray};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{DateTime, SecondsFormat, Utc};
use janiform::calendar::{ExchangeCalendarRegistry, MarketState};
use janiform::simulation::{
    CoarseSimulationPath, SeededFinePathRecord, SeededTickEventSide, SeededTickSimulationRunResult,
};
use parquet::arrow::ArrowWriter;

use super::DEFAULT_SIMULATION_EXPORT_BATCH_SIZE;
use crate::error::CliError;

const SIMULATION_OUTPUT_ARTIFACT_NAME: &str = "simulation output";
const STATE_COLUMN_NAMES: [&str; 4] = [
    "observed_log_price",
    "anchor_value",
    "pressure_state",
    "deviation_from_anchor",
];

#[derive(Clone, Debug)]
pub(crate) struct SimulationOutputColumns {
    pub(crate) timestamp_column: String,
    pub(crate) price_column: String,
    pub(crate) side_column: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct SimulationExportSummary {
    pub(crate) record_count: usize,
    pub(crate) chunk_count: usize,
}

#[derive(Clone, Debug)]
struct SimulationOutputRow {
    timestamp_utc_nanos: i64,
    price: f64,
    side: Option<String>,
    observed_log_price: f64,
    anchor_value: f64,
    pressure_state: f64,
    deviation_from_anchor: f64,
}

pub(crate) fn export_seeded_tick_simulation(
    result: &SeededTickSimulationRunResult,
    output_path: &Path,
    reference_price: f64,
    columns: &SimulationOutputColumns,
    batch_size: usize,
) -> Result<SimulationExportSummary, CliError> {
    validate_reference_price(reference_price)?;
    validate_batch_size(batch_size)?;

    let rows = result
        .public_transition_positions
        .iter()
        .copied()
        .map(|position| {
            let record = result.fine_path.get(position).ok_or_else(|| {
                CliError::InvalidCommandArgument {
                    argument_name: "--output-path",
                    message: format!(
                        "seeded simulation public transition position {position} is outside the fine path"
                    ),
                }
            })?;

            seeded_output_row(record, reference_price)
        })
        .collect::<Result<Vec<_>, _>>()?;

    write_simulation_rows(output_path, columns, &rows, batch_size)
}

pub(crate) fn export_coarse_simulation(
    result: &CoarseSimulationPath,
    output_path: &Path,
    reference_price: f64,
    columns: &SimulationOutputColumns,
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    batch_size: usize,
) -> Result<SimulationExportSummary, CliError> {
    validate_reference_price(reference_price)?;
    validate_batch_size(batch_size)?;

    let mut rows = Vec::new();
    for record in &result.records {
        let timestamp = timestamp_from_nanos(record.interval_start_utc_nanos)?;
        if calendar_registry
            .resolve_market_state_at_utc(exchange_code, timestamp)
            .map_err(|source| CliError::InvalidCommandArgument {
                argument_name: "--request-json",
                message: source.to_string(),
            })?
            == MarketState::Closed
        {
            continue;
        }

        rows.push(SimulationOutputRow {
            timestamp_utc_nanos: record.interval_end_utc_nanos,
            price: price_from_log_price(reference_price, record.observed_log_price_end)?,
            side: None,
            observed_log_price: record.observed_log_price_end,
            anchor_value: record.anchor_value_end,
            pressure_state: record.pressure_state_end,
            deviation_from_anchor: record.deviation_from_anchor_end,
        });
    }

    write_simulation_rows(output_path, columns, &rows, batch_size)
}

fn seeded_output_row(
    record: &SeededFinePathRecord,
    reference_price: f64,
) -> Result<SimulationOutputRow, CliError> {
    Ok(SimulationOutputRow {
        timestamp_utc_nanos: record.timestamp_utc_nanos,
        price: price_from_log_price(reference_price, record.observed_log_price)?,
        side: record.side.map(side_label).map(str::to_string),
        observed_log_price: record.observed_log_price,
        anchor_value: record.anchor_value,
        pressure_state: record.pressure_state,
        deviation_from_anchor: record.deviation_from_anchor,
    })
}

fn write_simulation_rows(
    output_path: &Path,
    columns: &SimulationOutputColumns,
    rows: &[SimulationOutputRow],
    batch_size: usize,
) -> Result<SimulationExportSummary, CliError> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|source| CliError::ArtifactWrite {
            artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
            path: parent.to_path_buf(),
            source,
        })?;
    }

    match output_path
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some(extension) if extension.eq_ignore_ascii_case("csv") => {
            write_csv_rows(output_path, columns, rows)?;
        }
        Some(extension) if extension.eq_ignore_ascii_case("parquet") => {
            write_parquet_rows(output_path, columns, rows, batch_size)?;
        }
        _ => {
            return Err(CliError::InvalidCommandArgument {
                argument_name: "--output-path",
                message: "simulation output path must end in .csv or .parquet".to_string(),
            });
        }
    }

    Ok(SimulationExportSummary {
        record_count: rows.len(),
        chunk_count: rows.chunks(batch_size).count(),
    })
}

fn write_csv_rows(
    output_path: &Path,
    columns: &SimulationOutputColumns,
    rows: &[SimulationOutputRow],
) -> Result<(), CliError> {
    let mut writer =
        csv::Writer::from_path(output_path).map_err(|source| CliError::ArtifactWrite {
            artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
            path: output_path.to_path_buf(),
            source: std::io::Error::other(source.to_string()),
        })?;

    let headers = output_headers(columns);

    writer
        .write_record(&headers)
        .map_err(|source| CliError::ArtifactWrite {
            artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
            path: output_path.to_path_buf(),
            source: std::io::Error::other(source.to_string()),
        })?;

    for row in rows {
        writer
            .write_record(csv_row(columns, row)?)
            .map_err(|source| CliError::ArtifactWrite {
                artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
                path: output_path.to_path_buf(),
                source: std::io::Error::other(source.to_string()),
            })?;
    }

    writer.flush().map_err(|source| CliError::ArtifactWrite {
        artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
        path: output_path.to_path_buf(),
        source,
    })
}

fn write_parquet_rows(
    output_path: &Path,
    columns: &SimulationOutputColumns,
    rows: &[SimulationOutputRow],
    batch_size: usize,
) -> Result<(), CliError> {
    let schema = Arc::new(parquet_schema(columns));

    let file = fs::File::create(output_path).map_err(|source| CliError::ArtifactWrite {
        artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
        path: output_path.to_path_buf(),
        source,
    })?;

    let mut writer = ArrowWriter::try_new(file, schema.clone(), None).map_err(|source| {
        CliError::ArtifactWrite {
            artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
            path: output_path.to_path_buf(),
            source: std::io::Error::other(source.to_string()),
        }
    })?;

    for chunk in rows.chunks(batch_size) {
        let batch = parquet_batch(schema.clone(), columns, chunk, output_path)?;
        writer
            .write(&batch)
            .map_err(|source| CliError::ArtifactWrite {
                artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
                path: output_path.to_path_buf(),
                source: std::io::Error::other(source.to_string()),
            })?;
    }

    writer.close().map_err(|source| CliError::ArtifactWrite {
        artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
        path: output_path.to_path_buf(),
        source: std::io::Error::other(source.to_string()),
    })?;

    Ok(())
}

fn parquet_schema(columns: &SimulationOutputColumns) -> Schema {
    let mut fields = vec![
        Field::new(
            columns.timestamp_column.as_str(),
            DataType::Timestamp(TimeUnit::Nanosecond, Some("+00:00".into())),
            false,
        ),
        Field::new(columns.price_column.as_str(), DataType::Float64, false),
    ];

    if let Some(side_column) = &columns.side_column {
        fields.push(Field::new(side_column.as_str(), DataType::Utf8, true));
    }

    fields.extend(
        STATE_COLUMN_NAMES
            .iter()
            .map(|column| Field::new(*column, DataType::Float64, false)),
    );

    Schema::new(fields)
}

fn parquet_batch(
    schema: Arc<Schema>,
    columns: &SimulationOutputColumns,
    rows: &[SimulationOutputRow],
    output_path: &Path,
) -> Result<RecordBatch, CliError> {
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(
            TimestampNanosecondArray::from(
                rows.iter()
                    .map(|row| row.timestamp_utc_nanos)
                    .collect::<Vec<_>>(),
            )
            .with_timezone_utc(),
        ),
        Arc::new(Float64Array::from(
            rows.iter().map(|row| row.price).collect::<Vec<_>>(),
        )),
    ];

    if columns.side_column.is_some() {
        arrays.push(Arc::new(StringArray::from(
            rows.iter()
                .map(|row| row.side.as_deref())
                .collect::<Vec<_>>(),
        )));
    }

    arrays.extend([
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.observed_log_price)
                .collect::<Vec<_>>(),
        )) as ArrayRef,
        Arc::new(Float64Array::from(
            rows.iter().map(|row| row.anchor_value).collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.pressure_state)
                .collect::<Vec<_>>(),
        )),
        Arc::new(Float64Array::from(
            rows.iter()
                .map(|row| row.deviation_from_anchor)
                .collect::<Vec<_>>(),
        )),
    ]);

    RecordBatch::try_new(schema, arrays).map_err(|source| CliError::ArtifactWrite {
        artifact_name: SIMULATION_OUTPUT_ARTIFACT_NAME,
        path: output_path.to_path_buf(),
        source: std::io::Error::other(source.to_string()),
    })
}

fn output_headers(columns: &SimulationOutputColumns) -> Vec<String> {
    let mut headers = vec![
        columns.timestamp_column.clone(),
        columns.price_column.clone(),
    ];

    if let Some(side_column) = &columns.side_column {
        headers.push(side_column.clone());
    }

    headers.extend(STATE_COLUMN_NAMES.iter().map(ToString::to_string));

    headers
}

fn csv_row(
    columns: &SimulationOutputColumns,
    row: &SimulationOutputRow,
) -> Result<Vec<String>, CliError> {
    let mut values = vec![
        format_timestamp_utc(row.timestamp_utc_nanos)?,
        row.price.to_string(),
    ];

    if columns.side_column.is_some() {
        values.push(row.side.clone().unwrap_or_default());
    }

    values.extend([
        row.observed_log_price.to_string(),
        row.anchor_value.to_string(),
        row.pressure_state.to_string(),
        row.deviation_from_anchor.to_string(),
    ]);

    Ok(values)
}

fn validate_reference_price(reference_price: f64) -> Result<(), CliError> {
    if !reference_price.is_finite() || reference_price <= 0.0 {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--reference-price",
            message: "reference price must be finite and strictly positive".to_string(),
        });
    }

    Ok(())
}

fn validate_batch_size(batch_size: usize) -> Result<(), CliError> {
    if batch_size == 0 {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--output-batch-size",
            message: format!(
                "output batch size must be at least 1 row, default is {DEFAULT_SIMULATION_EXPORT_BATCH_SIZE}"
            ),
        });
    }

    Ok(())
}

fn price_from_log_price(reference_price: f64, observed_log_price: f64) -> Result<f64, CliError> {
    let price = reference_price * observed_log_price.exp();
    if !price.is_finite() {
        return Err(CliError::InvalidCommandArgument {
            argument_name: "--reference-price",
            message: "converted simulated price must be finite".to_string(),
        });
    }

    Ok(price)
}

fn format_timestamp_utc(timestamp_utc_nanos: i64) -> Result<String, CliError> {
    Ok(timestamp_from_nanos(timestamp_utc_nanos)?.to_rfc3339_opts(SecondsFormat::Nanos, true))
}

fn timestamp_from_nanos(timestamp_utc_nanos: i64) -> Result<DateTime<Utc>, CliError> {
    Ok(DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos))
}

const fn side_label(side: SeededTickEventSide) -> &'static str {
    match side {
        SeededTickEventSide::Buy => "buy",
        SeededTickEventSide::Sell => "sell",
    }
}
