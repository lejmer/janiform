use janiform::simulation::{
    SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickEventRecord,
    SeededTickEventSide, SeededTickEventStateSnapshot, SeededTickSimulationNewsEventMarketState,
    SeededTickSimulationRunResult,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::mem::size_of;

#[pyclass(module = "janiform._native", name = "SeededFinePathChunkColumns")]
pub(crate) struct PySeededFinePathChunkColumns {
    #[pyo3(get)]
    transition_index: Vec<u64>,
    #[pyo3(get)]
    timestamp_utc_nanos: Vec<i64>,
    #[pyo3(get)]
    transition_kind: Vec<String>,
    #[pyo3(get)]
    side: Vec<Option<String>>,
    #[pyo3(get)]
    news_arrival_component: Vec<Option<String>>,
    #[pyo3(get)]
    market_state: Vec<String>,
    #[pyo3(get)]
    buy_side_return: Vec<f64>,
    #[pyo3(get)]
    sell_side_return: Vec<f64>,
    #[pyo3(get)]
    trading_time_return: Vec<f64>,
    #[pyo3(get)]
    open_jump_return: Vec<f64>,
    #[pyo3(get)]
    closed_gap_return: Vec<f64>,
    #[pyo3(get)]
    total_return: Vec<f64>,
    #[pyo3(get)]
    cumulative_log_return: Vec<f64>,
    #[pyo3(get)]
    state_snapshot_y_plus_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_plus_minus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_minus_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_minus_minus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_deseasonalized_lambda_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_deseasonalized_lambda_minus: Vec<Option<f64>>,
}

impl PySeededFinePathChunkColumns {
    fn from_records(records: &[SeededFinePathRecord]) -> Self {
        let mut transition_index = Vec::with_capacity(records.len());
        let mut timestamp_utc_nanos = Vec::with_capacity(records.len());
        let mut transition_kind = Vec::with_capacity(records.len());
        let mut side = Vec::with_capacity(records.len());
        let mut news_arrival_component = Vec::with_capacity(records.len());
        let mut market_state = Vec::with_capacity(records.len());
        let mut buy_side_return = Vec::with_capacity(records.len());
        let mut sell_side_return = Vec::with_capacity(records.len());
        let mut trading_time_return = Vec::with_capacity(records.len());
        let mut open_jump_return = Vec::with_capacity(records.len());
        let mut closed_gap_return = Vec::with_capacity(records.len());
        let mut total_return = Vec::with_capacity(records.len());
        let mut cumulative_log_return = Vec::with_capacity(records.len());
        let mut state_snapshot_y_plus_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_plus_minus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_minus_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_minus_minus = Vec::with_capacity(records.len());
        let mut state_snapshot_deseasonalized_lambda_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_deseasonalized_lambda_minus = Vec::with_capacity(records.len());

        for record in records {
            transition_index.push(record.transition_index);
            timestamp_utc_nanos.push(record.timestamp_utc_nanos);
            transition_kind.push(seeded_fine_path_transition_kind_label(record.transition_kind));
            side.push(record.side.map(seeded_tick_event_side_label));

            news_arrival_component.push(
                record
                    .news_arrival_component
                    .map(seeded_news_arrival_component_label),
            );

            market_state.push(seeded_market_state_label(record.market_state));
            buy_side_return.push(record.return_components.buy_side_return);
            sell_side_return.push(record.return_components.sell_side_return);
            trading_time_return.push(record.return_components.trading_time_return);
            open_jump_return.push(record.return_components.open_jump_return);
            closed_gap_return.push(record.return_components.closed_gap_return);
            total_return.push(record.return_components.total_return);
            cumulative_log_return.push(record.cumulative_log_return);

            push_state_snapshot_columns(
                &mut state_snapshot_y_plus_plus,
                &mut state_snapshot_y_plus_minus,
                &mut state_snapshot_y_minus_plus,
                &mut state_snapshot_y_minus_minus,
                &mut state_snapshot_deseasonalized_lambda_plus,
                &mut state_snapshot_deseasonalized_lambda_minus,
                record.state_snapshot,
            );
        }

        Self {
            transition_index,
            timestamp_utc_nanos,
            transition_kind,
            side,
            news_arrival_component,
            market_state,
            buy_side_return,
            sell_side_return,
            trading_time_return,
            open_jump_return,
            closed_gap_return,
            total_return,
            cumulative_log_return,
            state_snapshot_y_plus_plus,
            state_snapshot_y_plus_minus,
            state_snapshot_y_minus_plus,
            state_snapshot_y_minus_minus,
            state_snapshot_deseasonalized_lambda_plus,
            state_snapshot_deseasonalized_lambda_minus,
        }
    }
}

#[pymethods]
impl PySeededFinePathChunkColumns {
    #[getter]
    fn estimated_transport_bytes(&self) -> usize {
        self.transition_index.len().saturating_mul(size_of::<u64>())
            .saturating_add(self.timestamp_utc_nanos.len().saturating_mul(size_of::<i64>()))
            .saturating_add(estimated_string_column_bytes(self.transition_kind.as_slice()))
            .saturating_add(estimated_optional_string_column_bytes(self.side.as_slice()))
            .saturating_add(estimated_optional_string_column_bytes(
                self.news_arrival_component.as_slice(),
            ))
            .saturating_add(estimated_string_column_bytes(self.market_state.as_slice()))
            .saturating_add(self.buy_side_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.sell_side_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.trading_time_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.open_jump_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.closed_gap_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.total_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(self.cumulative_log_return.len().saturating_mul(size_of::<f64>()))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_plus_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_plus_minus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_minus_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_minus_minus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_deseasonalized_lambda_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_deseasonalized_lambda_minus.as_slice(),
            ))
    }
}

#[pyclass(module = "janiform._native", name = "SeededTickEventTapeChunkColumns")]
pub(crate) struct PySeededTickEventTapeChunkColumns {
    #[pyo3(get)]
    event_index: Vec<u64>,
    #[pyo3(get)]
    timestamp_utc_nanos: Vec<i64>,
    #[pyo3(get)]
    side: Vec<String>,
    #[pyo3(get)]
    state_snapshot_y_plus_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_plus_minus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_minus_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_y_minus_minus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_deseasonalized_lambda_plus: Vec<Option<f64>>,
    #[pyo3(get)]
    state_snapshot_deseasonalized_lambda_minus: Vec<Option<f64>>,
}

impl PySeededTickEventTapeChunkColumns {
    fn from_records(records: &[SeededTickEventRecord]) -> Self {
        let mut event_index = Vec::with_capacity(records.len());
        let mut timestamp_utc_nanos = Vec::with_capacity(records.len());
        let mut side = Vec::with_capacity(records.len());
        let mut state_snapshot_y_plus_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_plus_minus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_minus_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_y_minus_minus = Vec::with_capacity(records.len());
        let mut state_snapshot_deseasonalized_lambda_plus = Vec::with_capacity(records.len());
        let mut state_snapshot_deseasonalized_lambda_minus = Vec::with_capacity(records.len());

        for record in records {
            event_index.push(record.event_index);
            timestamp_utc_nanos.push(record.timestamp_utc_nanos);
            side.push(seeded_tick_event_side_label(record.side));

            push_state_snapshot_columns(
                &mut state_snapshot_y_plus_plus,
                &mut state_snapshot_y_plus_minus,
                &mut state_snapshot_y_minus_plus,
                &mut state_snapshot_y_minus_minus,
                &mut state_snapshot_deseasonalized_lambda_plus,
                &mut state_snapshot_deseasonalized_lambda_minus,
                record.state_snapshot,
            );
        }

        Self {
            event_index,
            timestamp_utc_nanos,
            side,
            state_snapshot_y_plus_plus,
            state_snapshot_y_plus_minus,
            state_snapshot_y_minus_plus,
            state_snapshot_y_minus_minus,
            state_snapshot_deseasonalized_lambda_plus,
            state_snapshot_deseasonalized_lambda_minus,
        }
    }
}

#[pymethods]
impl PySeededTickEventTapeChunkColumns {
    #[getter]
    fn estimated_transport_bytes(&self) -> usize {
        self.event_index.len().saturating_mul(size_of::<u64>())
            .saturating_add(self.timestamp_utc_nanos.len().saturating_mul(size_of::<i64>()))
            .saturating_add(estimated_string_column_bytes(self.side.as_slice()))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_plus_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_plus_minus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_minus_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_y_minus_minus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_deseasonalized_lambda_plus.as_slice(),
            ))
            .saturating_add(estimated_optional_f64_column_bytes(
                self.state_snapshot_deseasonalized_lambda_minus.as_slice(),
            ))
    }
}

#[pyclass(module = "janiform._native", name = "SeededTickSimulationNativeResult")]
pub(crate) struct PySeededTickSimulationNativeResult {
    pub(crate) output: SeededTickSimulationRunResult,
}

#[pymethods]
impl PySeededTickSimulationNativeResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn transition_count(&self) -> usize {
        self.output.public_transition_count()
    }

    #[getter]
    fn event_count(&self) -> usize {
        self.output.event_count()
    }

    #[getter]
    fn estimated_native_retained_bytes(&self) -> usize {
        self.output
            .native_diagnostics()
            .estimated_total_retained_bytes
    }

    fn fine_path_metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.output.fine_path_metadata).map_err(|source| {
            PyRuntimeError::new_err(format!(
                "failed to serialize seeded fine-path metadata payload: {source}"
            ))
        })
    }

    fn event_tape_metadata_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.output.event_tape_metadata).map_err(|source| {
            PyRuntimeError::new_err(format!(
                "failed to serialize seeded event-tape metadata payload: {source}"
            ))
        })
    }

    fn get_fine_path_chunk_columns(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> PySeededFinePathChunkColumns {
        let records = self.output.public_fine_path_chunk(start_index, max_records);
        PySeededFinePathChunkColumns::from_records(records.as_slice())
    }

    fn get_event_tape_chunk_columns(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> PySeededTickEventTapeChunkColumns {
        let records = self.output.event_chunk(start_index, max_records);
        PySeededTickEventTapeChunkColumns::from_records(records.as_slice())
    }
}

fn push_state_snapshot_columns(
    y_plus_plus: &mut Vec<Option<f64>>,
    y_plus_minus: &mut Vec<Option<f64>>,
    y_minus_plus: &mut Vec<Option<f64>>,
    y_minus_minus: &mut Vec<Option<f64>>,
    deseasonalized_lambda_plus: &mut Vec<Option<f64>>,
    deseasonalized_lambda_minus: &mut Vec<Option<f64>>,
    state_snapshot: Option<SeededTickEventStateSnapshot>,
) {
    y_plus_plus.push(state_snapshot.map(|snapshot| snapshot.y_plus_plus));
    y_plus_minus.push(state_snapshot.map(|snapshot| snapshot.y_plus_minus));
    y_minus_plus.push(state_snapshot.map(|snapshot| snapshot.y_minus_plus));
    y_minus_minus.push(state_snapshot.map(|snapshot| snapshot.y_minus_minus));

    deseasonalized_lambda_plus
        .push(state_snapshot.map(|snapshot| snapshot.deseasonalized_lambda_plus));
    deseasonalized_lambda_minus
        .push(state_snapshot.map(|snapshot| snapshot.deseasonalized_lambda_minus));
}

fn seeded_tick_event_side_label(side: SeededTickEventSide) -> String {
    match side {
        SeededTickEventSide::Buy => "buy".to_string(),
        SeededTickEventSide::Sell => "sell".to_string(),
    }
}

fn seeded_fine_path_transition_kind_label(transition_kind: SeededFinePathTransitionKind) -> String {
    match transition_kind {
        SeededFinePathTransitionKind::BuyTrade => "buy_trade".to_string(),
        SeededFinePathTransitionKind::SellTrade => "sell_trade".to_string(),
        SeededFinePathTransitionKind::ExogenousNews => "exogenous_news".to_string(),
        SeededFinePathTransitionKind::IntervalBoundaryDrift => {
            "interval_boundary_drift".to_string()
        }
    }
}

fn seeded_news_arrival_component_label(
    news_arrival_component: janiform::news::NewsArrivalComponent,
) -> String {
    match news_arrival_component {
        janiform::news::NewsArrivalComponent::Known => "known".to_string(),
        janiform::news::NewsArrivalComponent::Surprise => "surprise".to_string(),
    }
}

fn seeded_market_state_label(market_state: SeededTickSimulationNewsEventMarketState) -> String {
    match market_state {
        SeededTickSimulationNewsEventMarketState::Open => "open".to_string(),
        SeededTickSimulationNewsEventMarketState::Closed => "closed".to_string(),
    }
}

fn estimated_string_column_bytes(values: &[String]) -> usize {
    values
        .iter()
        .map(|value| value.len())
        .sum()
}

fn estimated_optional_string_column_bytes(values: &[Option<String>]) -> usize {
    values
        .iter()
        .map(|value| value.as_ref().map_or(1, |inner_value| inner_value.len()))
        .sum()
}

fn estimated_optional_f64_column_bytes(values: &[Option<f64>]) -> usize {
    values
        .iter()
        .map(|value| if value.is_some() { size_of::<f64>() } else { 1 })
        .sum()
}
