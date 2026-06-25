use std::path::Path;

use janiform_calibration_runtime::CalibrationMarketDataPreparationRequest;
use janiform_foundations::paths::format_path;

use super::preparation::FitPreparationRequest;

/// Return the number of input sources represented by a prepared fit request.
pub fn fit_preparation_input_count(request: &CalibrationMarketDataPreparationRequest) -> usize {
    preparation_request_input_paths(request).len()
}

pub(super) fn fit_preparation_request_input_count(request: &FitPreparationRequest) -> usize {
    fit_preparation_request_input_paths(request).len()
}

pub(super) fn preparation_request_input_paths(
    request: &CalibrationMarketDataPreparationRequest,
) -> Vec<String> {
    let mut input_paths = vec![input_path("ticks", request.tick_data_source.path.as_str())];

    if let Some(coarse_data_source) = &request.coarse_data_source {
        push_input_path(
            &mut input_paths,
            "coarse data",
            coarse_data_source.path.as_str(),
        );
    }

    if let Some(option_surface_source) = &request.option_surface_source {
        push_input_path(
            &mut input_paths,
            "options",
            option_surface_source.path.as_str(),
        );
    }

    if let Some(known_news_inputs) = &request.known_news_inputs {
        push_input_path(
            &mut input_paths,
            "known news",
            known_news_inputs.calendar_source.path.as_str(),
        );
    }

    input_paths
}

fn fit_preparation_request_input_paths(request: &FitPreparationRequest) -> Vec<String> {
    let mut input_paths = vec![input_path("ticks", request.tick_data_path.as_str())];

    push_optional_input_path(
        &mut input_paths,
        "coarse data",
        request.coarse_data_path.as_deref(),
    );

    push_optional_input_path(
        &mut input_paths,
        "options",
        request.option_surface_path.as_deref(),
    );

    if request.configuration.known_news_reference.is_some() {
        input_paths.push(input_reference("known news", "resolved preset"));
    }

    input_paths
}

fn push_optional_input_path(input_paths: &mut Vec<String>, label: &str, path: Option<&str>) {
    if let Some(path) = path {
        push_input_path(input_paths, label, path);
    }
}

fn push_input_path(input_paths: &mut Vec<String>, label: &str, path: &str) {
    input_paths.push(input_path(label, path));
}

fn input_path(label: &str, path: &str) -> String {
    format!("{label}: {}", format_path(Path::new(path)))
}

fn input_reference(label: &str, reference: &str) -> String {
    format!("{label}: {reference}")
}

#[cfg(test)]
mod tests {
    use super::{input_path, input_reference};

    #[test]
    fn input_path_formats_relative_portable_path() {
        let source_path = std::env::current_dir()
            .expect("current directory should be readable")
            .join("scratch")
            .join("data")
            .join("ticks.parquet");

        assert_eq!(
            input_path(
                "ticks",
                source_path
                    .to_str()
                    .expect("test path should be valid Unicode"),
            ),
            "ticks: scratch/data/ticks.parquet"
        );
    }

    #[test]
    fn input_reference_preserves_non_path_reference() {
        assert_eq!(
            input_reference("known news", "resolved preset"),
            "known news: resolved preset"
        );
    }
}
