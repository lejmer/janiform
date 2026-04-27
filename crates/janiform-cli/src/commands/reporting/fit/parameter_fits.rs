use serde_json::Map;
use serde_json::Value;

use crate::commands::shared::artifact_documents::nested_value;
use crate::commands::shared::artifact_documents::stage_label;

pub(super) const PARAMETER_FITS_FIELD_NAME: &str = "parameter_fits";

pub(super) fn report_with_parameter_fits(report: &Value, model: Option<&Value>) -> Value {
    let Some(parameter_fits) = parameter_fits(model) else {
        return report.clone();
    };

    let mut enriched_report = report.clone();

    if let Some(fields) = enriched_report.as_object_mut() {
        fields.insert(PARAMETER_FITS_FIELD_NAME.to_string(), parameter_fits);
    }

    enriched_report
}

fn parameter_fits(model: Option<&Value>) -> Option<Value> {
    let model = model?;

    let parameters = nested_value(model, &["parameters"])?.as_object()?;

    let selected_candidate_ids = nested_value(
        model,
        &[
            "metadata",
            "fitted_parameter_summary",
            "selected_candidate_ids",
        ],
    );

    let mut parameter_fits = Map::new();

    for (stage, stage_payload) in parameters {
        let Some(stage_parameters) = stage_parameters(stage, stage_payload) else {
            continue;
        };

        let mut stage_fit = Map::new();
        stage_fit.insert("stage_label".to_string(), Value::String(stage_label(stage)));

        if let Some(selected_candidate_id) =
            selected_candidate_ids.and_then(|selected| selected.get(stage))
        {
            stage_fit.insert(
                "selected_candidate_id".to_string(),
                selected_candidate_id.clone(),
            );
        }

        stage_fit.insert("parameters".to_string(), stage_parameters.clone());
        parameter_fits.insert(stage.clone(), Value::Object(stage_fit));
    }

    if parameter_fits.is_empty() {
        return None;
    }

    Some(Value::Object(parameter_fits))
}

fn stage_parameters<'a>(stage: &str, stage_payload: &'a Value) -> Option<&'a Value> {
    stage_payload
        .get("fitted_parameters")
        .or_else(|| (stage == "seasonality").then_some(stage_payload))
        .filter(|parameters| parameters.is_object())
}
