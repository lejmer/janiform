use janiform::simulation::{
    SeededFinePathRecord, SeededFinePathTransitionKind, SeededTickEventRecord, SeededTickEventSide,
    SeededTickEventStateSnapshot, SeededTickSimulationNewsEventMarketState,
    SeededTickSimulationRunResult,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use std::mem::size_of;

use crate::SimulationExecutionError;
use crate::panic_boundary::panic_to_pyresult;

#[pyclass(module = "janiform._native", name = "SeededFinePathChunkColumns")]
pub(crate) struct PySeededFinePathChunkColumns {
    transition_index: Vec<u64>,
    timestamp_utc_nanos: Vec<i64>,
    transition_kind: Vec<String>,
    side: Vec<Option<String>>,
    news_arrival_component: Vec<Option<String>>,
    market_state: Vec<String>,
    buy_side_return: Vec<f64>,
    sell_side_return: Vec<f64>,
    trading_time_return: Vec<f64>,
    open_jump_return: Vec<f64>,
    closed_gap_return: Vec<f64>,
    total_return: Vec<f64>,
    cumulative_log_return: Vec<f64>,
    state_snapshot_y_plus_plus: Vec<Option<f64>>,
    state_snapshot_y_plus_minus: Vec<Option<f64>>,
    state_snapshot_y_minus_plus: Vec<Option<f64>>,
    state_snapshot_y_minus_minus: Vec<Option<f64>>,
    state_snapshot_deseasonalized_lambda_plus: Vec<Option<f64>>,
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
            transition_kind.push(seeded_fine_path_transition_kind_label(
                record.transition_kind,
            ));
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
    fn transition_index(&self) -> PyResult<Vec<u64>> {
        simulation_chunk_getter(
            "reading seeded fine-path transition indexes",
            &self.transition_index,
        )
    }

    #[getter]
    fn timestamp_utc_nanos(&self) -> PyResult<Vec<i64>> {
        simulation_chunk_getter(
            "reading seeded fine-path timestamps",
            &self.timestamp_utc_nanos,
        )
    }

    #[getter]
    fn transition_kind(&self) -> PyResult<Vec<String>> {
        simulation_chunk_getter(
            "reading seeded fine-path transition kinds",
            &self.transition_kind,
        )
    }

    #[getter]
    fn side(&self) -> PyResult<Vec<Option<String>>> {
        simulation_chunk_getter("reading seeded fine-path sides", &self.side)
    }

    #[getter]
    fn news_arrival_component(&self) -> PyResult<Vec<Option<String>>> {
        simulation_chunk_getter(
            "reading seeded fine-path news-arrival components",
            &self.news_arrival_component,
        )
    }

    #[getter]
    fn market_state(&self) -> PyResult<Vec<String>> {
        simulation_chunk_getter("reading seeded fine-path market states", &self.market_state)
    }

    #[getter]
    fn buy_side_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path buy-side returns",
            &self.buy_side_return,
        )
    }

    #[getter]
    fn sell_side_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path sell-side returns",
            &self.sell_side_return,
        )
    }

    #[getter]
    fn trading_time_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path trading-time returns",
            &self.trading_time_return,
        )
    }

    #[getter]
    fn open_jump_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path open-jump returns",
            &self.open_jump_return,
        )
    }

    #[getter]
    fn closed_gap_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path closed-gap returns",
            &self.closed_gap_return,
        )
    }

    #[getter]
    fn total_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter("reading seeded fine-path total returns", &self.total_return)
    }

    #[getter]
    fn cumulative_log_return(&self) -> PyResult<Vec<f64>> {
        simulation_chunk_getter(
            "reading seeded fine-path cumulative log returns",
            &self.cumulative_log_return,
        )
    }

    #[getter]
    fn state_snapshot_y_plus_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path y_plus_plus snapshots",
            &self.state_snapshot_y_plus_plus,
        )
    }

    #[getter]
    fn state_snapshot_y_plus_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path y_plus_minus snapshots",
            &self.state_snapshot_y_plus_minus,
        )
    }

    #[getter]
    fn state_snapshot_y_minus_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path y_minus_plus snapshots",
            &self.state_snapshot_y_minus_plus,
        )
    }

    #[getter]
    fn state_snapshot_y_minus_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path y_minus_minus snapshots",
            &self.state_snapshot_y_minus_minus,
        )
    }

    #[getter]
    fn state_snapshot_deseasonalized_lambda_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path deseasonalized lambda-plus snapshots",
            &self.state_snapshot_deseasonalized_lambda_plus,
        )
    }

    #[getter]
    fn state_snapshot_deseasonalized_lambda_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded fine-path deseasonalized lambda-minus snapshots",
            &self.state_snapshot_deseasonalized_lambda_minus,
        )
    }

    #[getter]
    fn estimated_transport_bytes(&self) -> PyResult<usize> {
        panic_to_pyresult(
            "reading seeded fine-path transport-byte estimate",
            SimulationExecutionError::new_err,
            || {
                Ok(self
                    .transition_index
                    .len()
                    .saturating_mul(size_of::<u64>())
                    .saturating_add(
                        self.timestamp_utc_nanos
                            .len()
                            .saturating_mul(size_of::<i64>()),
                    )
                    .saturating_add(estimated_string_column_bytes(
                        self.transition_kind.as_slice(),
                    ))
                    .saturating_add(estimated_optional_string_column_bytes(self.side.as_slice()))
                    .saturating_add(estimated_optional_string_column_bytes(
                        self.news_arrival_component.as_slice(),
                    ))
                    .saturating_add(estimated_string_column_bytes(self.market_state.as_slice()))
                    .saturating_add(self.buy_side_return.len().saturating_mul(size_of::<f64>()))
                    .saturating_add(self.sell_side_return.len().saturating_mul(size_of::<f64>()))
                    .saturating_add(
                        self.trading_time_return
                            .len()
                            .saturating_mul(size_of::<f64>()),
                    )
                    .saturating_add(self.open_jump_return.len().saturating_mul(size_of::<f64>()))
                    .saturating_add(
                        self.closed_gap_return
                            .len()
                            .saturating_mul(size_of::<f64>()),
                    )
                    .saturating_add(self.total_return.len().saturating_mul(size_of::<f64>()))
                    .saturating_add(
                        self.cumulative_log_return
                            .len()
                            .saturating_mul(size_of::<f64>()),
                    )
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
                    )))
            },
        )
    }
}

#[pyclass(module = "janiform._native", name = "SeededTickEventTapeChunkColumns")]
pub(crate) struct PySeededTickEventTapeChunkColumns {
    event_index: Vec<u64>,
    timestamp_utc_nanos: Vec<i64>,
    side: Vec<String>,
    state_snapshot_y_plus_plus: Vec<Option<f64>>,
    state_snapshot_y_plus_minus: Vec<Option<f64>>,
    state_snapshot_y_minus_plus: Vec<Option<f64>>,
    state_snapshot_y_minus_minus: Vec<Option<f64>>,
    state_snapshot_deseasonalized_lambda_plus: Vec<Option<f64>>,
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
    fn event_index(&self) -> PyResult<Vec<u64>> {
        simulation_chunk_getter("reading seeded event-tape indexes", &self.event_index)
    }

    #[getter]
    fn timestamp_utc_nanos(&self) -> PyResult<Vec<i64>> {
        simulation_chunk_getter(
            "reading seeded event-tape timestamps",
            &self.timestamp_utc_nanos,
        )
    }

    #[getter]
    fn side(&self) -> PyResult<Vec<String>> {
        simulation_chunk_getter("reading seeded event-tape sides", &self.side)
    }

    #[getter]
    fn state_snapshot_y_plus_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape y_plus_plus snapshots",
            &self.state_snapshot_y_plus_plus,
        )
    }

    #[getter]
    fn state_snapshot_y_plus_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape y_plus_minus snapshots",
            &self.state_snapshot_y_plus_minus,
        )
    }

    #[getter]
    fn state_snapshot_y_minus_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape y_minus_plus snapshots",
            &self.state_snapshot_y_minus_plus,
        )
    }

    #[getter]
    fn state_snapshot_y_minus_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape y_minus_minus snapshots",
            &self.state_snapshot_y_minus_minus,
        )
    }

    #[getter]
    fn state_snapshot_deseasonalized_lambda_plus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape deseasonalized lambda-plus snapshots",
            &self.state_snapshot_deseasonalized_lambda_plus,
        )
    }

    #[getter]
    fn state_snapshot_deseasonalized_lambda_minus(&self) -> PyResult<Vec<Option<f64>>> {
        simulation_chunk_getter(
            "reading seeded event-tape deseasonalized lambda-minus snapshots",
            &self.state_snapshot_deseasonalized_lambda_minus,
        )
    }

    #[getter]
    fn estimated_transport_bytes(&self) -> PyResult<usize> {
        panic_to_pyresult(
            "reading seeded event-tape transport-byte estimate",
            SimulationExecutionError::new_err,
            || {
                Ok(self
                    .event_index
                    .len()
                    .saturating_mul(size_of::<u64>())
                    .saturating_add(
                        self.timestamp_utc_nanos
                            .len()
                            .saturating_mul(size_of::<i64>()),
                    )
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
                    )))
            },
        )
    }
}

#[pyclass(module = "janiform._native", name = "SeededTickSimulationNativeResult")]
pub(crate) struct PySeededTickSimulationNativeResult {
    pub(crate) output: SeededTickSimulationRunResult,
}

#[pymethods]
impl PySeededTickSimulationNativeResult {
    #[getter]
    fn schema_version(&self) -> PyResult<&str> {
        panic_to_pyresult(
            "reading seeded simulation schema version",
            SimulationExecutionError::new_err,
            || Ok(self.output.schema_version),
        )
    }

    #[getter]
    fn transition_count(&self) -> PyResult<usize> {
        panic_to_pyresult(
            "reading seeded simulation transition count",
            SimulationExecutionError::new_err,
            || Ok(self.output.public_transition_count()),
        )
    }

    #[getter]
    fn event_count(&self) -> PyResult<usize> {
        panic_to_pyresult(
            "reading seeded simulation event count",
            SimulationExecutionError::new_err,
            || Ok(self.output.event_count()),
        )
    }

    #[getter]
    fn estimated_native_retained_bytes(&self) -> PyResult<usize> {
        panic_to_pyresult(
            "reading seeded simulation retained-byte estimate",
            SimulationExecutionError::new_err,
            || {
                Ok(self
                    .output
                    .native_diagnostics()
                    .estimated_total_retained_bytes)
            },
        )
    }

    fn fine_path_metadata_json(&self) -> PyResult<String> {
        panic_to_pyresult(
            "serializing seeded fine-path metadata",
            SimulationExecutionError::new_err,
            || {
                serde_json::to_string(&self.output.fine_path_metadata).map_err(|source| {
                    PyRuntimeError::new_err(format!(
                        "failed to serialize seeded fine-path metadata payload: {source}"
                    ))
                })
            },
        )
    }

    fn event_tape_metadata_json(&self) -> PyResult<String> {
        panic_to_pyresult(
            "serializing seeded event-tape metadata",
            SimulationExecutionError::new_err,
            || {
                serde_json::to_string(&self.output.event_tape_metadata).map_err(|source| {
                    PyRuntimeError::new_err(format!(
                        "failed to serialize seeded event-tape metadata payload: {source}"
                    ))
                })
            },
        )
    }

    fn get_fine_path_chunk_columns(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> PyResult<PySeededFinePathChunkColumns> {
        panic_to_pyresult(
            "projecting seeded fine-path chunk columns",
            SimulationExecutionError::new_err,
            || {
                let records = self.output.public_fine_path_chunk(start_index, max_records);
                Ok(PySeededFinePathChunkColumns::from_records(
                    records.as_slice(),
                ))
            },
        )
    }

    fn get_event_tape_chunk_columns(
        &self,
        start_index: usize,
        max_records: usize,
    ) -> PyResult<PySeededTickEventTapeChunkColumns> {
        panic_to_pyresult(
            "projecting seeded event-tape chunk columns",
            SimulationExecutionError::new_err,
            || {
                let records = self.output.event_chunk(start_index, max_records);
                Ok(PySeededTickEventTapeChunkColumns::from_records(
                    records.as_slice(),
                ))
            },
        )
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

fn simulation_chunk_getter<T>(context: &'static str, value: &T) -> PyResult<T>
where
    T: Clone,
{
    panic_to_pyresult(context, SimulationExecutionError::new_err, || {
        Ok(value.clone())
    })
}

fn estimated_string_column_bytes(values: &[String]) -> usize {
    values.iter().map(|value| value.len()).sum()
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
