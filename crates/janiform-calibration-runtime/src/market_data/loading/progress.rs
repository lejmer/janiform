use std::error::Error;
use std::fmt::{Display, Formatter};

use super::contracts::{MarketDataLoadingError, MarketDataLoadingRequest};

/// Market-data source currently being loaded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MarketDataLoadingSource {
    /// Cross-source market-data preparation.
    MarketData,
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
            Self::MarketData => "market_data",
            Self::TickData => "tick_data",
            Self::CoarseData => "coarse_data",
            Self::OptionSurface => "option_surface",
            Self::KnownNews => "known_news",
        }
    }

    /// Human-readable source label used by progress displays.
    pub const fn source_label(self) -> &'static str {
        match self {
            Self::MarketData => "market data",
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
    /// Normalized source rows are being sorted, filtered, resolved, and summarized.
    FinalizingRecords,
    /// Cross-source calibration inputs are being assembled.
    BuildingCalibrationInputs,
    /// Timestamp batching surface is being inferred from loaded tick records.
    InferringTimestampBatchingSurface,
}

impl MarketDataLoadingProgressPhase {
    /// Stable phase identifier used by progress events.
    pub const fn phase_id(self) -> &'static str {
        match self {
            Self::ReadingRecords => "reading_records",
            Self::NormalizingRecords => "normalizing_records",
            Self::FinalizingRecords => "finalizing_records",
            Self::BuildingCalibrationInputs => "building_calibration_inputs",
            Self::InferringTimestampBatchingSurface => "inferring_timestamp_batching_surface",
        }
    }

    /// Human-readable phase label used by progress displays.
    pub const fn phase_label(self) -> &'static str {
        match self {
            Self::ReadingRecords => "reading records",
            Self::NormalizingRecords => "normalizing records",
            Self::FinalizingRecords => "finalizing normalized records",
            Self::BuildingCalibrationInputs => "building calibration input candidates",
            Self::InferringTimestampBatchingSurface => "inferring timestamp batching surface",
        }
    }
}

/// One-based loading position for a source within a multi-source dataset sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MarketDataLoadingDatasetPosition {
    /// One-based dataset position.
    pub dataset_index: usize,
    /// Total number of datasets in the loading sequence.
    pub dataset_count: usize,
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
    /// One-based dataset sequence position when more than one dataset is being loaded.
    pub dataset_position: Option<MarketDataLoadingDatasetPosition>,
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
            dataset_position: None,
        }
    }

    /// Return this event with multi-source dataset position metadata attached.
    pub const fn with_dataset_position(
        mut self,
        dataset_position: MarketDataLoadingDatasetPosition,
    ) -> Self {
        self.dataset_position = Some(dataset_position);

        self
    }

    /// Human-readable latest signal for terminal progress displays.
    pub fn latest_signal(self) -> String {
        self.with_dataset_prefix(self.source_record_signal())
    }

    /// Human-readable source and phase signal for progress event metadata.
    pub fn phase_signal(self) -> String {
        self.with_dataset_prefix(format!(
            "{}: {}",
            self.source.source_label(),
            self.phase.phase_label()
        ))
    }

    fn source_record_signal(self) -> String {
        if !self.phase.uses_record_count() {
            return format!(
                "{}: {}",
                self.source.source_label(),
                self.phase.phase_label()
            );
        }

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

    fn with_dataset_prefix(self, source_signal: String) -> String {
        match self.dataset_position {
            Some(position) => format!(
                "dataset {}/{}: {}",
                position.dataset_index, position.dataset_count, source_signal
            ),
            None => source_signal,
        }
    }
}

impl MarketDataLoadingProgressPhase {
    /// Whether terminal adapters should display source-record counts for this phase.
    pub const fn uses_record_count(self) -> bool {
        matches!(self, Self::ReadingRecords | Self::NormalizingRecords)
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
    source_sequence: Vec<MarketDataLoadingSource>,
}

impl<'a> MarketDataLoadingProgressContext<'a> {
    pub(crate) fn disabled() -> Self {
        Self {
            observer: None,
            source_sequence: Vec::new(),
        }
    }

    pub(crate) fn observed(observer: MarketDataLoadingProgressObserver<'a>) -> Self {
        Self {
            observer: Some(observer),
            source_sequence: Vec::new(),
        }
    }

    pub(crate) fn set_source_sequence_if_empty(
        &mut self,
        source_sequence: Vec<MarketDataLoadingSource>,
    ) {
        if self.source_sequence.is_empty() {
            self.source_sequence = source_sequence;
        }
    }

    pub(crate) fn emit(
        &mut self,
        event: MarketDataLoadingProgressEvent,
    ) -> Result<(), MarketDataLoadingError> {
        let Some(observer) = self.observer.as_mut() else {
            return Ok(());
        };

        let event = attach_dataset_position(event, self.source_sequence.as_slice());

        observer(event).map_err(MarketDataLoadingError::Progress)
    }
}

pub(crate) fn market_data_loading_source_sequence(
    request: &MarketDataLoadingRequest,
) -> Vec<MarketDataLoadingSource> {
    let mut source_sequence = vec![MarketDataLoadingSource::TickData];
    push_optional_loading_sources(request, &mut source_sequence);

    source_sequence
}

pub(crate) fn optional_market_data_loading_source_sequence(
    request: &MarketDataLoadingRequest,
) -> Vec<MarketDataLoadingSource> {
    let mut source_sequence = Vec::new();
    push_optional_loading_sources(request, &mut source_sequence);

    source_sequence
}

fn push_optional_loading_sources(
    request: &MarketDataLoadingRequest,
    source_sequence: &mut Vec<MarketDataLoadingSource>,
) {
    if request.known_news_input.is_some() {
        source_sequence.push(MarketDataLoadingSource::KnownNews);
    }

    if request.coarse_data_input.is_some() {
        source_sequence.push(MarketDataLoadingSource::CoarseData);
    }

    if request.option_surface_input.is_some() {
        source_sequence.push(MarketDataLoadingSource::OptionSurface);
    }
}

fn attach_dataset_position(
    event: MarketDataLoadingProgressEvent,
    source_sequence: &[MarketDataLoadingSource],
) -> MarketDataLoadingProgressEvent {
    if event.dataset_position.is_some() || source_sequence.len() <= 1 {
        return event;
    }

    source_sequence
        .iter()
        .position(|source| *source == event.source)
        .map(|source_position| {
            event.with_dataset_position(MarketDataLoadingDatasetPosition {
                dataset_index: source_position + 1,
                dataset_count: source_sequence.len(),
            })
        })
        .unwrap_or(event)
}

#[cfg(test)]
mod tests {
    use super::{
        MarketDataLoadingDatasetPosition, MarketDataLoadingProgressContext,
        MarketDataLoadingProgressEvent, MarketDataLoadingProgressPhase, MarketDataLoadingSource,
    };

    #[test]
    fn progress_context_attaches_dataset_position_for_multi_source_sequence() {
        let mut observed_events = Vec::new();
        let mut observer = |event| {
            observed_events.push(event);
            Ok(())
        };

        {
            let mut progress = MarketDataLoadingProgressContext::observed(&mut observer);
            progress.set_source_sequence_if_empty(vec![
                MarketDataLoadingSource::TickData,
                MarketDataLoadingSource::CoarseData,
                MarketDataLoadingSource::OptionSurface,
            ]);

            progress
                .emit(MarketDataLoadingProgressEvent::new(
                    MarketDataLoadingSource::CoarseData,
                    MarketDataLoadingProgressPhase::ReadingRecords,
                    4,
                    Some(10),
                ))
                .expect("observer should accept progress event");
        }

        assert_eq!(
            observed_events,
            vec![
                MarketDataLoadingProgressEvent::new(
                    MarketDataLoadingSource::CoarseData,
                    MarketDataLoadingProgressPhase::ReadingRecords,
                    4,
                    Some(10),
                )
                .with_dataset_position(MarketDataLoadingDatasetPosition {
                    dataset_index: 2,
                    dataset_count: 3,
                }),
            ]
        );
        assert_eq!(
            observed_events[0].latest_signal(),
            "dataset 2/3: coarse data: reading records 4/10 records"
        );
        assert_eq!(
            observed_events[0].phase_signal(),
            "dataset 2/3: coarse data: reading records"
        );
    }

    #[test]
    fn progress_context_omits_dataset_position_for_single_source_sequence() {
        let mut observed_events = Vec::new();
        let mut observer = |event| {
            observed_events.push(event);
            Ok(())
        };

        {
            let mut progress = MarketDataLoadingProgressContext::observed(&mut observer);
            progress.set_source_sequence_if_empty(vec![MarketDataLoadingSource::TickData]);

            progress
                .emit(MarketDataLoadingProgressEvent::new(
                    MarketDataLoadingSource::TickData,
                    MarketDataLoadingProgressPhase::ReadingRecords,
                    4,
                    Some(10),
                ))
                .expect("observer should accept progress event");
        }

        assert_eq!(observed_events[0].dataset_position, None);
        assert_eq!(
            observed_events[0].latest_signal(),
            "tick data: reading records 4/10 records"
        );
        assert_eq!(
            observed_events[0].phase_signal(),
            "tick data: reading records"
        );
    }

    #[test]
    fn timestamp_batching_inference_progress_uses_active_signal_without_record_count() {
        let event = MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::TickData,
            MarketDataLoadingProgressPhase::InferringTimestampBatchingSurface,
            0,
            None,
        );

        assert_eq!(
            event.phase.phase_id(),
            "inferring_timestamp_batching_surface"
        );
        assert_eq!(
            event.latest_signal(),
            "tick data: inferring timestamp batching surface"
        );
        assert_eq!(
            event.phase_signal(),
            "tick data: inferring timestamp batching surface"
        );
    }

    #[test]
    fn non_record_loading_progress_uses_active_signal_without_record_count() {
        let event = MarketDataLoadingProgressEvent::new(
            MarketDataLoadingSource::MarketData,
            MarketDataLoadingProgressPhase::BuildingCalibrationInputs,
            0,
            Some(12),
        );

        assert_eq!(
            event.latest_signal(),
            "market data: building calibration input candidates"
        );
        assert_eq!(
            event.phase_signal(),
            "market data: building calibration input candidates"
        );
    }
}
