use janiform_calibration_runtime::CalibrationMarketDataPreparationRequest;

pub(super) fn preparation_request_input_paths(
    request: &CalibrationMarketDataPreparationRequest,
) -> Vec<String> {
    let mut input_paths = vec![format!("ticks: {}", request.tick_data_source.path)];

    if let Some(coarse_data_source) = &request.coarse_data_source {
        input_paths.push(format!("coarse data: {}", coarse_data_source.path));
    }

    if let Some(option_surface_source) = &request.option_surface_source {
        input_paths.push(format!("options: {}", option_surface_source.path));
    }

    if let Some(known_news_inputs) = &request.known_news_inputs {
        input_paths.push(format!(
            "known news: {}",
            known_news_inputs.calendar_source.path
        ));
    }

    input_paths
}
