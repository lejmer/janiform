use super::contracts::{
    ReturnComponentsKnownNewsArrivalCandidateFamily, ReturnComponentsKnownNewsCalendarInput,
};

#[derive(Debug, Clone)]
pub(crate) struct FittedKnownNewsArrivalLayer {
    pub(crate) minimum_abs_known_return_threshold: f64,
    pub(crate) retained_known_event_timestamps_utc_nanos: Vec<i64>,
}

pub(crate) fn fit_known_news_arrival_layer(
    known_news_calendar: &ReturnComponentsKnownNewsCalendarInput,
) -> FittedKnownNewsArrivalLayer {
    FittedKnownNewsArrivalLayer {
        minimum_abs_known_return_threshold: 0.0,
        retained_known_event_timestamps_utc_nanos: known_news_calendar
            .scheduled_event_timestamps_utc_nanos
            .clone(),
    }
}

impl FittedKnownNewsArrivalLayer {
    pub(crate) fn as_candidate_family(&self) -> ReturnComponentsKnownNewsArrivalCandidateFamily {
        ReturnComponentsKnownNewsArrivalCandidateFamily {
            minimum_abs_known_return_threshold: self.minimum_abs_known_return_threshold,
            retained_known_event_timestamps_utc_nanos: self
                .retained_known_event_timestamps_utc_nanos
                .clone(),
        }
    }
}
