use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

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
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| map_io_error(parent.to_path_buf(), source))?;
    }

    let mut file =
        fs::File::create(path).map_err(|source| map_io_error(path.to_path_buf(), source))?;

    serde_json::to_writer_pretty(&mut file, payload).map_err(|source| {
        CliError::OutputSerialize {
            source: Box::new(source),
        }
    })?;

    writeln!(file).map_err(|source| map_io_error(path.to_path_buf(), source))
}
