use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use janiform_foundations::json::document::{JsonDocumentWriteError, write_pretty_json_document};
use serde::Serialize;

use crate::error::CliError;

pub(crate) fn write_json_payload<T>(payload: &T) -> Result<(), CliError>
where
    T: Serialize + ?Sized,
{
    let mut stdout = std::io::stdout().lock();

    serde_json::to_writer(&mut stdout, payload).map_err(|source| CliError::OutputSerialize {
        source: Box::new(source),
    })?;

    writeln!(stdout).map_err(|source| CliError::OutputWrite { source })
}

pub(crate) fn write_pretty_json_file<T, F>(
    path: &Path,
    payload: &T,
    map_io_error: F,
) -> Result<(), CliError>
where
    T: Serialize + ?Sized,
    F: Fn(PathBuf, io::Error) -> CliError,
{
    write_pretty_json_document(path, payload).map_err(|source| match source {
        JsonDocumentWriteError::CreateDirectory { path, source }
        | JsonDocumentWriteError::Create { path, source }
        | JsonDocumentWriteError::Write { path, source } => map_io_error(path, source),
        JsonDocumentWriteError::Serialize { source } => CliError::OutputSerialize {
            source: Box::new(source),
        },
    })
}
