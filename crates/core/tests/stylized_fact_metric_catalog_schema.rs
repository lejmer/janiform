use janiform_core::verification::{
    StylizedFactMetricCatalogValidationError, StylizedFactMetricClass, StylizedFactMetricFamily,
    StylizedFactSampleDependence, StylizedFactUncertaintyMethod,
    build_stylized_fact_metric_catalog, load_stylized_fact_metric_catalog_from_json,
    serialize_stylized_fact_metric_catalog_to_json,
};
use serde_json::Value;

#[test]
fn stylized_fact_metric_catalog_round_trip_is_valid() {
    let catalog = build_stylized_fact_metric_catalog();
    catalog.validate().expect("catalog should validate");

    let payload =
        serialize_stylized_fact_metric_catalog_to_json(&catalog).expect("catalog should serialize");
    let loaded_catalog =
        load_stylized_fact_metric_catalog_from_json(&payload).expect("catalog should load");

    assert_eq!(catalog, loaded_catalog);
}

#[test]
fn stylized_fact_metric_catalog_covers_required_metric_classes_and_families() {
    let catalog = build_stylized_fact_metric_catalog();

    assert!(catalog.metrics.iter().any(|metric| {
        metric.metric_class == StylizedFactMetricClass::PrimaryRobust
            && metric.metric_family == StylizedFactMetricFamily::QuantileShape
    }));
    assert!(catalog.metrics.iter().any(|metric| {
        metric.metric_class == StylizedFactMetricClass::PrimaryRobust
            && metric.metric_family == StylizedFactMetricFamily::TailRisk
    }));
    assert!(catalog.metrics.iter().any(|metric| {
        metric.metric_class == StylizedFactMetricClass::PrimaryRobust
            && metric.metric_family == StylizedFactMetricFamily::DistributionDistance
    }));
    assert!(catalog.metrics.iter().any(|metric| {
        metric.metric_class == StylizedFactMetricClass::SecondaryMoment
            && metric.metric_family == StylizedFactMetricFamily::LMoment
    }));
    assert!(catalog.metrics.iter().any(|metric| {
        metric.metric_class == StylizedFactMetricClass::SecondaryMoment
            && metric.metric_family == StylizedFactMetricFamily::CentralMoment
    }));
}

#[test]
fn stylized_fact_metric_catalog_required_field_lists_match_metric_category_shape() {
    let catalog = build_stylized_fact_metric_catalog();
    let serialized_catalog: Value =
        serde_json::to_value(&catalog).expect("catalog should serialize to json value");

    let output_schema_metadata = serialized_catalog
        .get("output_schema_metadata")
        .and_then(Value::as_object)
        .expect("output_schema_metadata should be present");
    let required_primary_fields =
        read_required_field_list(output_schema_metadata, "required_primary_metric_fields");
    let required_secondary_fields =
        read_required_field_list(output_schema_metadata, "required_secondary_metric_fields");
    let required_uncertainty_fields =
        read_required_field_list(output_schema_metadata, "required_uncertainty_fields");
    let required_sample_sufficiency_fields =
        read_required_field_list(output_schema_metadata, "required_sample_sufficiency_fields");
    let required_estimator_metadata_fields =
        read_required_field_list(output_schema_metadata, "required_estimator_metadata_fields");

    let metrics = serialized_catalog
        .get("metrics")
        .and_then(Value::as_array)
        .expect("metrics should be present");
    for metric in metrics {
        let metric_object = metric
            .as_object()
            .expect("each metric should serialize as an object");
        let metric_class = metric_object
            .get("metric_class")
            .and_then(Value::as_str)
            .expect("metric_class should be present");

        let required_metric_fields = match metric_class {
            "primary_robust" => &required_primary_fields,
            "secondary_moment" => &required_secondary_fields,
            other => panic!("unexpected metric_class in test fixture: {other}"),
        };
        for required_field in required_metric_fields {
            assert!(
                metric_object.contains_key(required_field),
                "metric is missing required `{required_field}` field for class `{metric_class}`",
            );
        }

        let uncertainty_object = metric_object
            .get("uncertainty")
            .and_then(Value::as_object)
            .expect("metric uncertainty should be present");
        for required_field in &required_uncertainty_fields {
            assert!(
                uncertainty_object.contains_key(required_field),
                "uncertainty is missing required `{required_field}` field",
            );
        }

        let sample_sufficiency_object = metric_object
            .get("sample_sufficiency")
            .and_then(Value::as_object)
            .expect("metric sample_sufficiency should be present");
        for required_field in &required_sample_sufficiency_fields {
            assert!(
                sample_sufficiency_object.contains_key(required_field),
                "sample_sufficiency is missing required `{required_field}` field",
            );
        }

        if metric_class == "secondary_moment" {
            let estimator_metadata = metric_object
                .get("estimator_metadata")
                .and_then(Value::as_object)
                .expect("secondary moment metrics require estimator_metadata object");
            for required_field in &required_estimator_metadata_fields {
                assert!(
                    estimator_metadata.contains_key(required_field),
                    "estimator_metadata is missing required `{required_field}` field",
                );
            }
        }
    }
}

#[test]
fn stylized_fact_metric_catalog_rejects_duplicate_metric_id() {
    let mut catalog = build_stylized_fact_metric_catalog();
    catalog.metrics[1].metric_id = catalog.metrics[0].metric_id.clone();

    match catalog.validate() {
        Err(StylizedFactMetricCatalogValidationError::DuplicateMetricId { .. }) => {}
        other => panic!("expected DuplicateMetricId error, found {other:?}"),
    }
}

#[test]
fn stylized_fact_metric_catalog_rejects_invalid_metric_class_family_pairing() {
    let mut catalog = build_stylized_fact_metric_catalog();
    catalog.metrics[0].metric_family = StylizedFactMetricFamily::LMoment;

    match catalog.validate() {
        Err(StylizedFactMetricCatalogValidationError::InvalidMetricClassFamilyCombination {
            ..
        }) => {}
        other => panic!("expected InvalidMetricClassFamilyCombination error, found {other:?}"),
    }
}

#[test]
fn stylized_fact_metric_catalog_rejects_secondary_metric_missing_estimator_metadata() {
    let mut catalog = build_stylized_fact_metric_catalog();

    let secondary_metric = catalog
        .metrics
        .iter_mut()
        .find(|metric| metric.metric_class == StylizedFactMetricClass::SecondaryMoment)
        .expect("catalog should include secondary metrics");
    secondary_metric.estimator_metadata = None;

    match catalog.validate() {
        Err(StylizedFactMetricCatalogValidationError::MissingEstimatorMetadata { .. }) => {}
        other => panic!("expected MissingEstimatorMetadata error, found {other:?}"),
    }
}

#[test]
fn stylized_fact_metric_catalog_rejects_primary_metric_with_estimator_metadata() {
    let mut catalog = build_stylized_fact_metric_catalog();

    let estimator_metadata = catalog
        .metrics
        .iter()
        .find_map(|metric| metric.estimator_metadata.clone())
        .expect("catalog should include estimator metadata on secondary metrics");
    let primary_metric = catalog
        .metrics
        .iter_mut()
        .find(|metric| metric.metric_class == StylizedFactMetricClass::PrimaryRobust)
        .expect("catalog should include primary metrics");
    primary_metric.estimator_metadata = Some(estimator_metadata);

    match catalog.validate() {
        Err(StylizedFactMetricCatalogValidationError::UnexpectedEstimatorMetadata { .. }) => {}
        other => panic!("expected UnexpectedEstimatorMetadata error, found {other:?}"),
    }
}

#[test]
fn stylized_fact_metric_catalog_rejects_non_block_bootstrap_default_for_dependent_returns() {
    let mut catalog = build_stylized_fact_metric_catalog();

    let dependent_return_metric = catalog
        .metrics
        .iter_mut()
        .find(|metric| {
            metric.sample_dependence == StylizedFactSampleDependence::DependentReturnSeries
        })
        .expect("catalog should include dependent return metrics");
    dependent_return_metric.uncertainty.default_method =
        StylizedFactUncertaintyMethod::StationaryBootstrap;

    match catalog.validate() {
        Err(StylizedFactMetricCatalogValidationError::InvalidMetricUncertaintyConfiguration {
            ..
        }) => {}
        other => {
            panic!("expected InvalidMetricUncertaintyConfiguration error, found {other:?}")
        }
    }
}

#[test]
fn stylized_fact_metric_catalog_loader_rejects_missing_required_metric_field() {
    let catalog = build_stylized_fact_metric_catalog();

    let mut value: Value = serde_json::to_value(catalog).expect("catalog should serialize");
    value["metrics"][0]
        .as_object_mut()
        .expect("metric should be an object")
        .remove("unit");
    let payload = serde_json::to_string(&value).expect("mutated payload should serialize");

    let error = load_stylized_fact_metric_catalog_from_json(&payload)
        .expect_err("payload with missing required field should fail");
    let error_text = error.to_string();
    assert!(
        error_text.contains("valid JSON"),
        "unexpected error: {error_text}"
    );
}

fn read_required_field_list(
    output_schema_metadata: &serde_json::Map<String, Value>,
    field_name: &'static str,
) -> Vec<String> {
    output_schema_metadata
        .get(field_name)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{field_name} should be present as an array"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("{field_name} entries must be strings"))
                .to_string()
        })
        .collect()
}
