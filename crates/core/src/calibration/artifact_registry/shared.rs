use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::benchmarking::fnv1a_64;

use super::super::contracts::{CalibrationExecutionMode, CalibrationStage};

pub(crate) fn calibration_execution_mode_token(
    execution_mode: CalibrationExecutionMode,
) -> &'static str {
    match execution_mode {
        CalibrationExecutionMode::Memory => "memory",
        CalibrationExecutionMode::Disk => "disk",
    }
}

pub(crate) fn calibration_stage_token(stage: CalibrationStage) -> &'static str {
    match stage {
        CalibrationStage::Seasonality => "seasonality",
        CalibrationStage::Hawkes => "hawkes",
        CalibrationStage::ReturnComponents => "return_components",
    }
}

pub(crate) fn compute_reference_checksum_hex(data_reference: &str) -> String {
    format!("{:016x}", fnv1a_64(data_reference.as_bytes()))
}

pub(crate) fn validate_checksum_hex<E>(checksum_hex: &str, error: E) -> Result<(), E> {
    if checksum_hex.len() != 16
        || checksum_hex
            .chars()
            .any(|character| !matches!(character, '0'..='9' | 'a'..='f'))
    {
        return Err(error);
    }

    Ok(())
}

pub(crate) fn write_json_file_atomically<T, E, SerializationFailed, WriteFailed, RenameFailed>(
    target_path: &Path,
    payload: &T,
    serialization_failed: SerializationFailed,
    write_failed: WriteFailed,
    rename_failed: RenameFailed,
) -> Result<(), E>
where
    T: Serialize,
    SerializationFailed: FnOnce(String) -> E,
    WriteFailed: Fn(&Path, String) -> E,
    RenameFailed: Fn(&Path, &Path, String) -> E,
{
    let serialized = serde_json::to_vec_pretty(payload)
        .map_err(|source| serialization_failed(source.to_string()))?;

    let temporary_path = target_path.with_extension("json.tmp");
    let mut temporary_file = File::create(&temporary_path)
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;
    temporary_file
        .write_all(serialized.as_slice())
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;
    temporary_file
        .sync_all()
        .map_err(|source| write_failed(&temporary_path, source.to_string()))?;

    fs::rename(&temporary_path, target_path)
        .map_err(|source| rename_failed(&temporary_path, target_path, source.to_string()))?;

    Ok(())
}
