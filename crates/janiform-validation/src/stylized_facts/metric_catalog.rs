mod builders;
mod constants;
mod core;
mod errors;

pub use builders::{
    build_stylized_fact_metric_catalog, load_stylized_fact_metric_catalog_from_json,
    serialize_stylized_fact_metric_catalog_to_json,
};
pub use constants::{
    STYLIZED_FACT_METRIC_CATALOG_FORWARD_EXTENSION_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_HORIZON_LABEL_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_DECISION_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_INSUFFICIENT_DATA_STATUS_CODE,
    STYLIZED_FACT_METRIC_CATALOG_NAMING_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_OUTPUT_SCHEMA_METADATA_VERSION,
    STYLIZED_FACT_METRIC_CATALOG_SCHEMA_FAMILY, STYLIZED_FACT_METRIC_CATALOG_SCHEMA_GENERATION,
    STYLIZED_FACT_METRIC_CATALOG_SCHEMA_VERSION,
    STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_OVERRIDE_APPROVAL_POLICY,
    STYLIZED_FACT_METRIC_CATALOG_UNCERTAINTY_POLICY_ID,
};
pub use core::{
    StylizedFactEstimatorMetadata, StylizedFactInsufficientDataSemantics,
    StylizedFactMetricCatalog, StylizedFactMetricCatalogOutputSchemaMetadata,
    StylizedFactMetricCatalogSchemaMetadata, StylizedFactMetricCatalogUncertaintyPolicy,
    StylizedFactMetricClass, StylizedFactMetricDefinition, StylizedFactMetricDomain,
    StylizedFactMetricFamily, StylizedFactMetricHorizonApplicability,
    StylizedFactMetricNamingConventions, StylizedFactMetricUncertaintyConfiguration,
    StylizedFactMinimumSampleSufficiencyRequirements, StylizedFactSampleDependence,
    StylizedFactSecondaryMomentVariant, StylizedFactUncertaintyMethod,
    StylizedFactUncertaintyOverridePolicy,
};
pub use errors::{
    StylizedFactMetricCatalogLoadError, StylizedFactMetricCatalogSerializeError,
    StylizedFactMetricCatalogValidationError,
};
