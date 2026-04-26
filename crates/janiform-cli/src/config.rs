use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::context::ColorChoice;
use crate::context::EventStreamFormat;
use crate::error::CliError;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CliConfiguration {
    #[serde(default)]
    pub(crate) output: OutputConfiguration,
}

impl CliConfiguration {
    pub(crate) fn load(path: &Path) -> Result<Self, CliError> {
        let document = fs::read_to_string(path).map_err(|source| CliError::ConfigRead {
            path: PathBuf::from(path),
            source,
        })?;

        toml::from_str(&document).map_err(|source| CliError::ConfigParse {
            path: PathBuf::from(path),
            source,
        })
    }

    pub(crate) fn validate(&self, path: &Path) -> Result<(), CliError> {
        self.output.validate(path)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutputConfiguration {
    pub(crate) color: Option<ColorChoice>,
    pub(crate) silent: Option<bool>,
    pub(crate) quiet: Option<bool>,
    pub(crate) verbose: Option<u8>,
    pub(crate) json: Option<bool>,
    pub(crate) events: Option<EventStreamFormat>,
    pub(crate) trace_events: Option<bool>,
}

impl OutputConfiguration {
    fn validate(&self, path: &Path) -> Result<(), CliError> {
        let json = self.json.unwrap_or(false);
        let silent = self.silent.unwrap_or(false);
        let quiet = self.quiet.unwrap_or(false);
        let verbose = self.verbose.unwrap_or(0);

        if json && silent {
            return Err(config_conflict(path, "output.json", "output.silent"));
        }
        if json && quiet {
            return Err(config_conflict(path, "output.json", "output.quiet"));
        }
        if silent && quiet {
            return Err(config_conflict(path, "output.silent", "output.quiet"));
        }
        if verbose > 0 && silent {
            return Err(config_conflict(path, "output.verbose", "output.silent"));
        }
        if verbose > 0 && quiet {
            return Err(config_conflict(path, "output.verbose", "output.quiet"));
        }
        if self.trace_events.unwrap_or(false) && self.events.is_none() {
            return Err(config_conflict(
                path,
                "output.trace_events",
                "output.events",
            ));
        }

        Ok(())
    }
}

fn config_conflict(path: &Path, first_field: &'static str, second_field: &'static str) -> CliError {
    CliError::ConfigConflict {
        path: PathBuf::from(path),
        first_field,
        second_field,
    }
}
