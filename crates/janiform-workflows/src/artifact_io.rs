use std::fs;
use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::error::WorkflowError;

pub(crate) fn write_pretty_json_file<T>(
    path: &Path,
    payload: &T,
    artifact_write_error: impl Fn(&Path, std::io::Error) -> WorkflowError,
    serialize_error: impl Fn(serde_json::Error) -> WorkflowError,
) -> Result<(), WorkflowError>
where
    T: Serialize + ?Sized,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| artifact_write_error(parent, source))?;
    }

    let mut file = fs::File::create(path).map_err(|source| artifact_write_error(path, source))?;
    serde_json::to_writer_pretty(&mut file, payload).map_err(serialize_error)?;

    writeln!(file).map_err(|source| artifact_write_error(path, source))
}
