use std::error::Error;
use std::fmt::{Display, Formatter};

use super::contracts::MarketDataLoadingError;

/// Market-data source currently being loaded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketDataLoadingSource {
    /// Tick data source.
    TickData,
    /// Coarse data source.
    CoarseData,
    /// Option surface source.
    OptionSurface,
    /// Known-news source.
    KnownNews,
}

impl MarketDataLoadingSource {
    /// Stable source identifier used by progress events.
    pub const fn source_id(self) -> &'static str {
        match self {
            Self::TickData => "tick_data",
            Self::CoarseData => "coarse_data",
            Self::OptionSurface => "option_surface",
            Self::KnownNews => "known_news",
        }
    }

    /// Human-readable source label used by progress displays.
    pub const fn source_label(self) -> &'static str {
        match self {
            Self::TickData => "tick data",
            Self::CoarseData => "coarse data",
            Self::OptionSurface => "option surface",
            Self::KnownNews => "known news",
        }
    }
}

/// Market-data loading phase currently being reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketDataLoadingProgressPhase {
    /// Source rows are being read from disk.
    ReadingRecords,
    /// Source rows are being normalized into the typed runtime contract.
    NormalizingRecords,
    /// Source loading finished.
    Completed,
}

impl MarketDataLoadingProgressPhase {
    /// Stable phase identifier used by progress events.
    pub const fn phase_id(self) -> &'static str {
        match self {
            Self::ReadingRecords => "reading_records",
            Self::NormalizingRecords => "normalizing_records",
            Self::Completed => "completed",
        }
    }

    /// Human-readable phase label used by progress displays.
    pub const fn phase_label(self) -> &'static str {
        match self {
            Self::ReadingRecords => "reading records",
            Self::NormalizingRecords => "normalizing records",
            Self::Completed => "loaded",
        }
    }
}

/// Progress emitted while market-data sources are loaded from disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarketDataLoadingProgressEvent {
    /// Market-data source being loaded.
    pub source: MarketDataLoadingSource,
    /// Current loading phase.
    pub phase: MarketDataLoadingProgressPhase,
    /// Completed source-record count for the current phase.
    pub completed_records: usize,
    /// Total source-record count when the source format makes it available.
    pub total_records: Option<usize>,
}

impl MarketDataLoadingProgressEvent {
    /// Create a progress event from runtime-owned source-record counts.
    pub const fn new(
        source: MarketDataLoadingSource,
        phase: MarketDataLoadingProgressPhase,
        completed_records: usize,
        total_records: Option<usize>,
    ) -> Self {
        Self {
            source,
            phase,
            completed_records,
            total_records,
        }
    }

    /// Human-readable latest signal for terminal progress displays.
    pub fn latest_signal(self) -> String {
        match self.total_records {
            Some(total_records) if total_records > 0 => format!(
                "{}: {} {}/{} records",
                self.source.source_label(),
                self.phase.phase_label(),
                self.completed_records,
                total_records
            ),
            Some(_) => format!(
                "{}: {} 0 records",
                self.source.source_label(),
                self.phase.phase_label()
            ),
            None => format!(
                "{}: {} {} records",
                self.source.source_label(),
                self.phase.phase_label(),
                self.completed_records
            ),
        }
    }
}

/// Error returned when a loading progress observer fails.
#[derive(Debug)]
pub struct MarketDataLoadingProgressError {
    message: String,
}

impl MarketDataLoadingProgressError {
    /// Build an observer error from a boundary-specific failure message.
    pub fn observer(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl Display for MarketDataLoadingProgressError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "progress observer failed: {}", self.message)
    }
}

impl Error for MarketDataLoadingProgressError {}

/// Callback used by loading APIs that expose source progress.
pub type MarketDataLoadingProgressObserver<'a> =
    &'a mut dyn FnMut(MarketDataLoadingProgressEvent) -> Result<(), MarketDataLoadingProgressError>;

pub(crate) struct MarketDataLoadingProgressContext<'a> {
    observer: Option<MarketDataLoadingProgressObserver<'a>>,
}

impl<'a> MarketDataLoadingProgressContext<'a> {
    pub(crate) const fn disabled() -> Self {
        Self { observer: None }
    }

    pub(crate) fn observed(observer: MarketDataLoadingProgressObserver<'a>) -> Self {
        Self {
            observer: Some(observer),
        }
    }

    pub(crate) fn emit(
        &mut self,
        event: MarketDataLoadingProgressEvent,
    ) -> Result<(), MarketDataLoadingError> {
        let Some(observer) = self.observer.as_mut() else {
            return Ok(());
        };

        observer(event).map_err(MarketDataLoadingError::Progress)
    }
}
