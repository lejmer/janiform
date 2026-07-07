use janiform::validation::stylized_facts::{
    StylizedFactHorizonComparisonReportGeneratorRequest,
    run_stylized_fact_horizon_comparison_report_generator as run_stylized_fact_horizon_comparison_report_generator_core,
};
use pyo3::prelude::{Bound, PyResult, Python};
use pyo3::types::{PyModule, PyModuleMethods};
use pyo3::{pyfunction, wrap_pyfunction};

use crate::SeasonalityInputValidationError;

#[pyfunction]
#[pyo3(signature = (generator_request_json))]
fn run_stylized_fact_horizon_comparison_report_generator(
    py: Python<'_>,
    generator_request_json: &str,
) -> PyResult<String> {
    let generator_request = serde_json::from_str::<
        StylizedFactHorizonComparisonReportGeneratorRequest,
    >(generator_request_json)
    .map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "stylized fact report generator request must be valid JSON: {source}"
        ))
    })?;

    let report_bundle = py
        .detach(|| run_stylized_fact_horizon_comparison_report_generator_core(generator_request))
        .map_err(|source| {
            SeasonalityInputValidationError::new_err(format!(
                "stylized fact report generation failed: {source}"
            ))
        })?;

    serde_json::to_string(&report_bundle).map_err(|source| {
        SeasonalityInputValidationError::new_err(format!(
            "failed to serialize stylized fact report bundle to JSON: {source}"
        ))
    })
}

pub(super) fn register_functions(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(
        run_stylized_fact_horizon_comparison_report_generator,
        module
    )?)?;
    Ok(())
}
