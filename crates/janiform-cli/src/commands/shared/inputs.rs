use std::path::Path;
use std::path::PathBuf;

use janiform::calibration::MarketDataInputFileReference;
use janiform_foundations::identifiers::sanitize_ascii_identifier_component;

use crate::commands::options::InputFileFormat;
use crate::commands::shared::calibration::market_data_file_format;
use crate::context::CommandContext;
use crate::error::CliError;

pub(crate) fn required_input_file(
    context: &CommandContext,
    argument_name: &'static str,
    path: &Option<PathBuf>,
) -> Result<PathBuf, CliError> {
    let path = path
        .as_ref()
        .ok_or(CliError::MissingRequiredArgument { argument_name })?;

    context.paths().input_file(argument_name, path)
}

pub(crate) fn optional_input_file(
    context: &CommandContext,
    argument_name: &'static str,
    path: Option<&PathBuf>,
) -> Result<Option<PathBuf>, CliError> {
    path.map(|path| context.paths().input_file(argument_name, path))
        .transpose()
}

pub(crate) fn market_data_input_file_reference(
    argument_name: &'static str,
    path: &Path,
    explicit_format: Option<InputFileFormat>,
    explicit_provenance_label: Option<&str>,
    default_provenance_role: &str,
) -> Result<MarketDataInputFileReference, CliError> {
    Ok(MarketDataInputFileReference {
        path: path.display().to_string(),
        file_format: market_data_file_format(
            explicit_format.unwrap_or(infer_input_file_format(argument_name, path)?),
        ),
        provenance_label: explicit_provenance_label
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| provenance_label_from_path(path, default_provenance_role)),
    })
}

pub(crate) fn infer_input_file_format(
    argument_name: &'static str,
    path: &Path,
) -> Result<InputFileFormat, CliError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("csv") => Ok(InputFileFormat::Csv),
        Some(extension) if extension.eq_ignore_ascii_case("parquet") => {
            Ok(InputFileFormat::Parquet)
        }
        Some(extension) if extension.eq_ignore_ascii_case("json") => Ok(InputFileFormat::Json),
        _ => Err(CliError::InvalidCommandArgument {
            argument_name,
            message: "file format could not be inferred from the extension".to_string(),
        }),
    }
}

pub(crate) fn provenance_label_from_path(path: &Path, default_role: &str) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(sanitize_ascii_identifier_component)
        .unwrap_or_else(|| default_role.to_string())
}
