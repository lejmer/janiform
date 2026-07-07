use std::error::Error;
use std::fmt::Display;

use serde_json::Map;
use serde_json::Value;

use super::payload::{payload_error, string_field, string_value};

pub(super) const DEFAULT_HIGH_LEVEL_KNOWN_NEWS_PRESET_NAME: &str = "none";
pub(super) const RESOLVED_PRESET_WORKFLOW_CONFIGURATION_SCHEMA_VERSION: &str =
    "calibration.preset_resolution.resolved_configuration.v1";

pub(super) type PresetResolutionResult<T> = Result<T, PresetWorkflowResolutionError>;

/// Errors raised while resolving high-level market-data preset references.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PresetWorkflowResolutionError {
    /// A request or preset payload did not match the expected JSON object shape.
    InvalidPayload { field_path: String, message: String },
    /// A named preset reference could not be found in the selected registry scope.
    UnknownPreset {
        category: &'static str,
        registry_scope: &'static str,
        preset_name: String,
    },
}

impl Display for PresetWorkflowResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPayload {
                field_path,
                message,
            } => write!(formatter, "{field_path}: {message}"),
            Self::UnknownPreset {
                category,
                registry_scope,
                preset_name,
            } => write!(
                formatter,
                "unknown {registry_scope} {category} preset `{preset_name}`"
            ),
        }
    }
}

impl Error for PresetWorkflowResolutionError {}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PresetCategory {
    Session,
    KnownNews,
    MarketProfile,
}

impl PresetCategory {
    pub(super) fn from_str(raw_value: &str, field_path: &str) -> PresetResolutionResult<Self> {
        match raw_value {
            "session" => Ok(Self::Session),
            "known_news" => Ok(Self::KnownNews),
            "market_profile" => Ok(Self::MarketProfile),
            _ => Err(payload_error(
                field_path,
                "must be one of: session, known_news, market_profile",
            )),
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::KnownNews => "known_news",
            Self::MarketProfile => "market_profile",
        }
    }

    pub(super) fn index_sort_rank(self) -> u8 {
        match self {
            Self::MarketProfile => 0,
            Self::KnownNews => 1,
            Self::Session => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum PresetRegistryScope {
    BuiltIn,
    UserRegistered,
}

impl PresetRegistryScope {
    pub(super) fn from_optional_value(
        raw_value: Option<&Value>,
        field_path: &str,
    ) -> PresetResolutionResult<Option<Self>> {
        let Some(raw_value) = raw_value else {
            return Ok(None);
        };
        if raw_value.is_null() {
            return Ok(None);
        }

        let raw_scope = string_value(raw_value, field_path)?;

        Ok(Some(match raw_scope {
            "built_in" => Self::BuiltIn,
            "user_registered" => Self::UserRegistered,
            _ => {
                return Err(payload_error(
                    field_path,
                    "must be one of: built_in, user_registered",
                ));
            }
        }))
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::BuiltIn => "built_in",
            Self::UserRegistered => "user_registered",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PresetReferenceUsageMode {
    Automatic,
    Named,
    Explicit,
}

impl PresetReferenceUsageMode {
    pub(super) fn from_reference(
        reference: &Map<String, Value>,
        field_path: &str,
    ) -> PresetResolutionResult<Self> {
        match string_field(reference, "usage_mode", &format!("{field_path}.usage_mode"))? {
            "automatic" => Ok(Self::Automatic),
            "named" => Ok(Self::Named),
            "explicit" => Ok(Self::Explicit),
            _ => Err(payload_error(
                &format!("{field_path}.usage_mode"),
                "must be one of: automatic, named, explicit",
            )),
        }
    }
}
