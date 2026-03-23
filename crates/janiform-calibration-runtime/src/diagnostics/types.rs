use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationDiagnosticEvaluationTier {
    ExactBlock,
    CoarseJoint,
    TickJoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationDiagnosticMetricEligibility {
    ExactEligible,
    CoarseEligible,
    TickRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationDiagnosticMetricRole {
    Admissibility,
    Ranking,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticObservedTargetEntry {
    pub target_id: String,
    pub target_family: String,
    pub definition: String,
    pub estimation_method: String,
    pub effective_sample_size: f64,
    pub uncertainty_scale: f64,
    pub stability_score: f64,
    pub evaluation_eligibility: CalibrationDiagnosticMetricEligibility,
    pub admissibility_role: CalibrationDiagnosticMetricRole,
    pub ranking_role: CalibrationDiagnosticMetricRole,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizon_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditioning_label: Option<String>,
    pub units: String,
    pub observed_value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticObservedTargetsArtifact {
    pub schema_version: &'static str,
    pub target_builder_version: &'static str,
    pub targets: Vec<CalibrationDiagnosticObservedTargetEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticFamilyMetric {
    pub metric_id: String,
    pub target_id: String,
    pub target_family: String,
    pub evaluation_tier: CalibrationDiagnosticEvaluationTier,
    pub normalized_distance: f64,
    pub admissible: bool,
    pub simulated_value: f64,
    pub observed_value: f64,
    pub uncertainty_scale: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticCandidateFrontierEntry {
    pub candidate_id: String,
    pub stage: String,
    pub evaluation_tier: CalibrationDiagnosticEvaluationTier,
    pub valid: bool,
    pub admissible: bool,
    pub pareto_rank: usize,
    pub lexicographic_rank: usize,
    pub scalar_tie_break: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
    pub family_metrics: Vec<CalibrationDiagnosticFamilyMetric>,
    pub canonical_coordinates: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticCandidateFrontierArtifact {
    pub schema_version: &'static str,
    pub frontier_entries: Vec<CalibrationDiagnosticCandidateFrontierEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSelectionDecision {
    pub selection_stage: String,
    pub selected_candidate_id: String,
    pub selection_policy: String,
    pub validity_screened_candidate_count: usize,
    pub admissible_candidate_count: usize,
    pub pareto_frontier_count: usize,
    pub tie_break_candidate_count: usize,
    pub decisive_metric_ids: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSelectionRationaleArtifact {
    pub schema_version: &'static str,
    pub decision: CalibrationDiagnosticSelectionDecision,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticConsistencyMetric {
    pub metric_id: String,
    pub horizon_label: String,
    pub coarse_value: f64,
    pub tick_value: f64,
    pub absolute_delta: f64,
    pub normalized_delta: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticCoarseTickConsistencyArtifact {
    pub schema_version: &'static str,
    pub metrics: Vec<CalibrationDiagnosticConsistencyMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSimulationSummaryEntry {
    pub simulation_seed: u64,
    pub evaluation_tier: CalibrationDiagnosticEvaluationTier,
    pub horizon_label: String,
    pub event_count: usize,
    pub realized_variance: f64,
    pub mean_absolute_return: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSimulationSummaryArtifact {
    pub schema_version: &'static str,
    pub summaries: Vec<CalibrationDiagnosticSimulationSummaryEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSearchSurfacePoint {
    pub surface_id: String,
    pub x_label: String,
    pub x_value: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y_value: Option<f64>,
    pub objective_value: f64,
    pub admissible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticSearchSurfaceManifestArtifact {
    pub schema_version: &'static str,
    pub surfaces: Vec<CalibrationDiagnosticSearchSurfacePoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticRuntimeProfileEntry {
    pub component_id: String,
    pub elapsed_seconds: f64,
    pub evaluation_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticRuntimeProfileArtifact {
    pub schema_version: &'static str,
    pub cpu_count: usize,
    pub entries: Vec<CalibrationDiagnosticRuntimeProfileEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticArtifactBundle {
    pub observed_targets: CalibrationDiagnosticObservedTargetsArtifact,
    pub candidate_frontier: CalibrationDiagnosticCandidateFrontierArtifact,
    pub coarse_tick_consistency: CalibrationDiagnosticCoarseTickConsistencyArtifact,
    pub selection_rationale: CalibrationDiagnosticSelectionRationaleArtifact,
    pub simulation_summary: CalibrationDiagnosticSimulationSummaryArtifact,
    pub search_surface_manifest: CalibrationDiagnosticSearchSurfaceManifestArtifact,
    pub runtime_profile: CalibrationDiagnosticRuntimeProfileArtifact,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CalibrationDiagnosticArtifactReference {
    pub artifact_family: String,
    pub relative_path: String,
}
