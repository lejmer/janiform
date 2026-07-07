use janiform::time::calendar::TimestampEventPreprocessingOutput;
use pyo3::{pyclass, pymethods};

#[pyclass(
    module = "janiform._native",
    name = "TimestampEventPreprocessingResult",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct PyTimestampEventPreprocessingResult {
    pub(crate) output: TimestampEventPreprocessingOutput,
}

#[pymethods]
impl PyTimestampEventPreprocessingResult {
    #[getter]
    fn schema_version(&self) -> &str {
        self.output.schema_version
    }

    #[getter]
    fn input_schema_version(&self) -> &str {
        self.output.input_schema_version
    }

    #[getter]
    fn exchange_code(&self) -> &str {
        &self.output.exchange_code
    }

    #[getter]
    fn exchange_timezone_name(&self) -> &str {
        &self.output.exchange_timezone_name
    }

    #[getter]
    fn source_record_positions(&self) -> Vec<u64> {
        self.output.source_record_positions.clone()
    }

    #[getter]
    fn timestamp_utc_nanos(&self) -> Vec<i64> {
        self.output.timestamp_utc_nanos.clone()
    }

    #[getter]
    fn exchange_local_minute_of_day(&self) -> Vec<u16> {
        self.output.exchange_local_minute_of_day.clone()
    }

    #[getter]
    fn exchange_local_day_of_week(&self) -> Vec<u8> {
        self.output.exchange_local_day_of_week.clone()
    }

    #[getter]
    fn exchange_local_day_of_year(&self) -> Vec<u16> {
        self.output.exchange_local_day_of_year.clone()
    }

    #[getter]
    fn market_state_codes(&self) -> Vec<u8> {
        self.output.market_state_codes.clone()
    }

    #[getter]
    fn missing_hour_bucket_utc_nanos(&self) -> Vec<i64> {
        self.output.missing_hour_bucket_utc_nanos.clone()
    }

    #[getter]
    fn sparse_hour_bucket_utc_nanos(&self) -> Vec<i64> {
        self.output.sparse_hour_bucket_utc_nanos.clone()
    }

    #[getter]
    fn input_record_count(&self) -> usize {
        self.output.diagnostics.input_record_count
    }

    #[getter]
    fn validated_record_count(&self) -> usize {
        self.output.diagnostics.validated_record_count
    }

    #[getter]
    fn accepted_record_count(&self) -> usize {
        self.output.diagnostics.accepted_record_count
    }

    #[getter]
    fn filtered_record_count(&self) -> usize {
        self.output.diagnostics.filtered_record_count
    }

    #[getter]
    fn first_event_timestamp_utc_nanos(&self) -> Option<i64> {
        self.output.diagnostics.first_event_timestamp_utc_nanos
    }

    #[getter]
    fn last_event_timestamp_utc_nanos(&self) -> Option<i64> {
        self.output.diagnostics.last_event_timestamp_utc_nanos
    }

    #[getter]
    fn hour_bucket_count(&self) -> usize {
        self.output.diagnostics.hour_bucket_count
    }

    #[getter]
    fn missing_hour_bucket_count(&self) -> usize {
        self.output.diagnostics.missing_hour_bucket_count
    }

    #[getter]
    fn sparse_hour_bucket_count(&self) -> usize {
        self.output.diagnostics.sparse_hour_bucket_count
    }
}
