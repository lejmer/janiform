use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use janiform_calibration_artifacts::JointRefinementStageRuntimeProfile;

use super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts;
use super::option_refinement::{OptionRefinedCandidate, SearchTierCandidate};
use super::state::{JointRefinementOptionPricingContext, JointRefinementOptionTierPricingInputs};
use super::{JointRefinementOptionTierPricingArtifact, JointRefinementRestartOutcome};

const JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION: &str =
    "janiform.joint_refinement.resume_state.v1";
const JOINT_REFINEMENT_RESUME_STATE_FILENAME: &str = "joint-refinement.resume_state.json";
const JOINT_REFINEMENT_OPTION_PRICING_CONTEXT_FINGERPRINT_POLICY: &str =
    "joint_refinement_option_pricing_context_sha256_canonical_tiers_and_pricing_inputs.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedRestartSelectionArtifacts {
    pub(crate) best_restart_id: String,
    pub(crate) option_refinement_candidate_frontier_ids: Vec<String>,
    #[serde(default)]
    pub(crate) selected_restart_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) option_pricing_context_fingerprint: Option<String>,
    pub(crate) coarse_consistency_artifacts_by_restart_id:
        BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
    pub(crate) coarse_consistency_wall_clock_millis: u64,
}

impl PersistedRestartSelectionArtifacts {
    pub(crate) fn matches_resume_context(
        &self,
        valid_restart_ids: &BTreeSet<String>,
        selected_restart_count: usize,
        option_pricing_context_fingerprint: Option<&str>,
    ) -> bool {
        self.option_pricing_context_fingerprint.as_deref() == option_pricing_context_fingerprint
            && self.selected_restart_count >= selected_restart_count
            && !self.option_refinement_candidate_frontier_ids.is_empty()
            && self
                .option_refinement_candidate_frontier_ids
                .iter()
                .all(|restart_id| valid_restart_ids.contains(restart_id))
            && valid_restart_ids.contains(&self.best_restart_id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedSearchTierCandidate {
    pub(crate) source_rank: usize,
    pub(crate) source_dynamics_parameter_bits: Vec<u64>,
    pub(crate) search_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: JointRefinementOptionTierPricingArtifact,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

impl PersistedSearchTierCandidate {
    pub(crate) fn from_runtime(candidate: &SearchTierCandidate) -> Self {
        Self {
            source_rank: candidate.source_rank,
            source_dynamics_parameter_bits: candidate.source_dynamics_parameter_bits.clone(),
            search_tier_score: candidate.search_tier_score,
            search_tier_pricing_artifact: (*candidate.search_tier_pricing_artifact).clone(),
            restart_outcome: candidate.restart_outcome.clone(),
        }
    }

    pub(crate) fn into_runtime(self) -> SearchTierCandidate {
        SearchTierCandidate {
            source_rank: self.source_rank,
            source_dynamics_parameter_bits: self.source_dynamics_parameter_bits,
            search_tier_score: self.search_tier_score,
            search_tier_pricing_artifact: Arc::new(self.search_tier_pricing_artifact),
            restart_outcome: self.restart_outcome,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedSelectionTierCandidate {
    pub(crate) source_rank: usize,
    pub(crate) search_tier_score: f64,
    pub(crate) selection_tier_score: f64,
    pub(crate) search_tier_pricing_artifact: JointRefinementOptionTierPricingArtifact,
    pub(crate) selection_tier_pricing_artifact: JointRefinementOptionTierPricingArtifact,
    pub(crate) restart_outcome: JointRefinementRestartOutcome,
}

impl PersistedSelectionTierCandidate {
    pub(crate) fn from_runtime(candidate: &OptionRefinedCandidate) -> Self {
        Self {
            source_rank: candidate.source_rank,
            search_tier_score: candidate.search_tier_score,
            selection_tier_score: candidate.selection_tier_score,
            search_tier_pricing_artifact: (*candidate.search_tier_pricing_artifact).clone(),
            selection_tier_pricing_artifact: (*candidate.selection_tier_pricing_artifact).clone(),
            restart_outcome: candidate.restart_outcome.clone(),
        }
    }

    pub(crate) fn into_runtime(self) -> OptionRefinedCandidate {
        OptionRefinedCandidate {
            source_rank: self.source_rank,
            search_tier_score: self.search_tier_score,
            selection_tier_score: self.selection_tier_score,
            search_tier_pricing_artifact: Arc::new(self.search_tier_pricing_artifact),
            selection_tier_pricing_artifact: Arc::new(self.selection_tier_pricing_artifact),
            restart_outcome: self.restart_outcome,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedSelectedOptionRefinement {
    pub(crate) restart_id: String,
    pub(crate) search_tier_pricing_artifact: JointRefinementOptionTierPricingArtifact,
    pub(crate) selection_tier_pricing_artifact: JointRefinementOptionTierPricingArtifact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct JointRefinementResumeState {
    schema_version: String,
    run_id: String,
    stage_seed: u64,
    restart_outcomes_by_id: BTreeMap<String, JointRefinementRestartOutcome>,
    restart_selection: Option<PersistedRestartSelectionArtifacts>,
    #[serde(default)]
    search_tier_candidates_by_option_context:
        BTreeMap<String, BTreeMap<String, PersistedSearchTierCandidate>>,
    #[serde(default)]
    selection_tier_candidates_by_option_context:
        BTreeMap<String, BTreeMap<String, PersistedSelectionTierCandidate>>,
    #[serde(default)]
    selected_option_refinement_by_context: BTreeMap<String, PersistedSelectedOptionRefinement>,
    #[serde(default)]
    audit_tier_pricing_artifact_by_context:
        BTreeMap<String, JointRefinementOptionTierPricingArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    runtime_profile: Option<JointRefinementStageRuntimeProfile>,
}

impl JointRefinementResumeState {
    fn new(run_id: &str, stage_seed: u64) -> Self {
        Self {
            schema_version: JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION.to_string(),
            run_id: run_id.to_string(),
            stage_seed,
            restart_outcomes_by_id: BTreeMap::new(),
            restart_selection: None,
            search_tier_candidates_by_option_context: BTreeMap::new(),
            selection_tier_candidates_by_option_context: BTreeMap::new(),
            selected_option_refinement_by_context: BTreeMap::new(),
            audit_tier_pricing_artifact_by_context: BTreeMap::new(),
            runtime_profile: None,
        }
    }
}

pub(crate) struct JointRefinementResumeStore {
    path: Option<PathBuf>,
    state: JointRefinementResumeState,
}

impl JointRefinementResumeStore {
    pub(crate) fn load(
        disk_checkpoint_directory: Option<&Path>,
        run_id: &str,
        stage_seed: u64,
    ) -> Self {
        let Some(disk_checkpoint_directory) = disk_checkpoint_directory else {
            return Self {
                path: None,
                state: JointRefinementResumeState::new(run_id, stage_seed),
            };
        };

        let path = disk_checkpoint_directory.join(JOINT_REFINEMENT_RESUME_STATE_FILENAME);

        let state = load_resume_state_from_disk(path.as_path(), run_id, stage_seed)
            .unwrap_or_else(|| JointRefinementResumeState::new(run_id, stage_seed));

        Self {
            path: Some(path),
            state,
        }
    }

    pub(crate) fn restart_outcome(
        &self,
        restart_id: &str,
    ) -> Option<JointRefinementRestartOutcome> {
        self.state.restart_outcomes_by_id.get(restart_id).cloned()
    }

    pub(crate) fn restart_selection(&self) -> Option<&PersistedRestartSelectionArtifacts> {
        self.state.restart_selection.as_ref()
    }

    pub(crate) fn search_tier_candidate(
        &self,
        option_context_fingerprint: &str,
        restart_id: &str,
    ) -> Option<SearchTierCandidate> {
        self.state
            .search_tier_candidates_by_option_context
            .get(option_context_fingerprint)?
            .get(restart_id)
            .cloned()
            .map(PersistedSearchTierCandidate::into_runtime)
    }

    pub(crate) fn selection_tier_candidate(
        &self,
        option_context_fingerprint: &str,
        restart_id: &str,
    ) -> Option<OptionRefinedCandidate> {
        self.state
            .selection_tier_candidates_by_option_context
            .get(option_context_fingerprint)?
            .get(restart_id)
            .cloned()
            .map(PersistedSelectionTierCandidate::into_runtime)
    }

    pub(crate) fn selected_option_refinement(
        &self,
        option_context_fingerprint: &str,
    ) -> Option<(
        String,
        Arc<JointRefinementOptionTierPricingArtifact>,
        Arc<JointRefinementOptionTierPricingArtifact>,
    )> {
        self.state
            .selected_option_refinement_by_context
            .get(option_context_fingerprint)
            .map(|refinement| {
                // Intentional clones decouple the returned runtime artifacts from persisted store state.
                (
                    refinement.restart_id.clone(),
                    Arc::new(refinement.search_tier_pricing_artifact.clone()),
                    Arc::new(refinement.selection_tier_pricing_artifact.clone()),
                )
            })
    }

    pub(crate) fn selected_audit_tier_pricing_artifact(
        &self,
        option_context_fingerprint: &str,
    ) -> Option<Arc<JointRefinementOptionTierPricingArtifact>> {
        self.state
            .audit_tier_pricing_artifact_by_context
            .get(option_context_fingerprint)
            .cloned()
            .map(Arc::new)
    }

    pub(crate) fn record_restart_outcome(&mut self, outcome: &JointRefinementRestartOutcome) {
        self.state
            .restart_outcomes_by_id
            .insert(outcome.restart_id.clone(), outcome.clone());
        self.persist();
    }

    pub(crate) fn record_restart_selection(
        &mut self,
        selection: PersistedRestartSelectionArtifacts,
    ) {
        self.state.restart_selection = Some(selection);
        self.persist();
    }

    pub(crate) fn record_runtime_profile(
        &mut self,
        runtime_profile: JointRefinementStageRuntimeProfile,
    ) {
        self.state.runtime_profile = Some(runtime_profile);
        self.persist();
    }

    pub(crate) fn record_search_tier_candidate(
        &mut self,
        option_context_fingerprint: &str,
        candidate: &SearchTierCandidate,
    ) {
        self.state
            .search_tier_candidates_by_option_context
            .entry(option_context_fingerprint.to_string())
            .or_default()
            .insert(
                candidate.restart_outcome.restart_id.clone(),
                PersistedSearchTierCandidate::from_runtime(candidate),
            );
        self.persist();
    }

    pub(crate) fn record_selection_tier_candidate(
        &mut self,
        option_context_fingerprint: &str,
        candidate: &OptionRefinedCandidate,
    ) {
        self.state
            .selection_tier_candidates_by_option_context
            .entry(option_context_fingerprint.to_string())
            .or_default()
            .insert(
                candidate.restart_outcome.restart_id.clone(),
                PersistedSelectionTierCandidate::from_runtime(candidate),
            );
        self.persist();
    }

    pub(crate) fn record_selected_option_refinement(
        &mut self,
        option_context_fingerprint: &str,
        restart_id: &str,
        search_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
        selection_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
    ) {
        self.state.selected_option_refinement_by_context.insert(
            option_context_fingerprint.to_string(),
            PersistedSelectedOptionRefinement {
                restart_id: restart_id.to_string(),
                search_tier_pricing_artifact: search_tier_pricing_artifact.clone(),
                selection_tier_pricing_artifact: selection_tier_pricing_artifact.clone(),
            },
        );
        self.persist();
    }

    pub(crate) fn record_audit_tier_pricing_artifact(
        &mut self,
        option_context_fingerprint: &str,
        audit_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
    ) {
        self.state.audit_tier_pricing_artifact_by_context.insert(
            option_context_fingerprint.to_string(),
            audit_tier_pricing_artifact.clone(),
        );
        self.persist();
    }

    fn persist(&self) {
        let Some(path) = &self.path else {
            return;
        };

        if let Err(error) = persist_resume_state_to_disk(path.as_path(), &self.state) {
            let _ = error;
        }
    }
}

pub(crate) fn option_pricing_context_fingerprint(
    option_pricing_context: &JointRefinementOptionPricingContext,
) -> String {
    let fingerprint_payload = json!({
        "policy": JOINT_REFINEMENT_OPTION_PRICING_CONTEXT_FINGERPRINT_POLICY,
        "valuation_time_utc_nanos": option_pricing_context.valuation_time_utc_nanos,
        "boundary_input": option_pricing_context.boundary_input,
        "snapshot_maturity_intervals": option_pricing_context.snapshot_maturity_intervals,
        "snapshot_maturity_terminal_interval_indices": option_pricing_context
            .snapshot_maturity_terminal_interval_indices,
        "carry_input": option_pricing_context.carry_input,
        "option_surface_tiers": option_surface_tiers_fingerprint_payload(
            option_pricing_context,
        ),
        "tier_pricing_inputs": {
            "search": tier_pricing_inputs_fingerprint_payload(
                &option_pricing_context.tier_pricing_inputs.search,
            ),
            "selection": tier_pricing_inputs_fingerprint_payload(
                &option_pricing_context.tier_pricing_inputs.selection,
            ),
            "audit": tier_pricing_inputs_fingerprint_payload(
                &option_pricing_context.tier_pricing_inputs.audit,
            ),
        },
    });

    let payload_bytes = serde_json::to_vec(&fingerprint_payload)
        .unwrap_or_else(|error| error.to_string().into_bytes());

    let digest = Sha256::digest(payload_bytes.as_slice());
    format!("{digest:x}")
}

fn option_surface_tiers_fingerprint_payload(
    option_pricing_context: &JointRefinementOptionPricingContext,
) -> serde_json::Value {
    let tiers = &option_pricing_context.option_surface_tiers;

    json!({
        "valid_input_quote_count": tiers.valid_input_quote_count,
        "dropped_input_quote_count": tiers.dropped_input_quote_count,
        "canonical_quotes": tiers
            .canonical_quotes
            .iter()
            .map(|quote| {
                json!({
                    "source_position": quote.source_position,
                    "quote_timestamp_utc_nanos": quote.quote_timestamp_utc_nanos,
                    "expiry_timestamp_utc_nanos": quote.expiry_timestamp_utc_nanos,
                    "terminal_interval_index": quote.terminal_interval_index,
                    "maturity_interval_bits": quote.maturity_interval.to_bits(),
                    "strike_bits": quote.strike.to_bits(),
                    "payoff_type": quote.payoff_type,
                    "bid_price_bits": quote.bid_price.to_bits(),
                    "ask_price_bits": quote.ask_price.to_bits(),
                    "observed_mid_price_bits": quote.observed_mid_price.to_bits(),
                    "source_observed_mid_price_bits": quote
                        .source_observed_mid_price
                        .to_bits(),
                    "absolute_spread_bits": quote.absolute_spread.to_bits(),
                    "relative_spread_bits": quote.relative_spread.to_bits(),
                    "log_forward_moneyness_bits": quote.log_forward_moneyness.to_bits(),
                    "staleness_nanos": quote.staleness_nanos,
                    "violates_static_bounds": quote.violates_static_bounds,
                })
            })
            .collect::<Vec<_>>(),
        "search": option_surface_tier_fingerprint_payload(&tiers.search),
        "selection": option_surface_tier_fingerprint_payload(&tiers.selection),
        "audit": option_surface_tier_fingerprint_payload(&tiers.audit),
    })
}

fn option_surface_tier_fingerprint_payload(
    tier: &crate::stages::joint_refinement::option_surface_tiering::JointRefinementOptionSurfaceTier,
) -> serde_json::Value {
    json!({
        "tier_kind": tier.tier_kind,
        "maturity_count": tier.maturity_count,
        "maturity_representatives": tier
            .maturity_representatives
            .iter()
            .map(|representative| {
                json!({
                    "terminal_interval_index": representative.terminal_interval_index,
                    "maturity_interval_bits": representative.maturity_interval.to_bits(),
                    "expiry_timestamp_utc_nanos": representative.expiry_timestamp_utc_nanos,
                    "maturity_weight": representative.maturity_weight,
                    "quote_indices": representative.quote_indices,
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn tier_pricing_inputs_fingerprint_payload(
    tier_inputs: &JointRefinementOptionTierPricingInputs,
) -> serde_json::Value {
    json!({
        "runtime_interval_schedule": tier_inputs.runtime_interval_schedule,
        "interval_factors": tier_inputs.interval_factors,
        "grid_batch_pricing_request": tier_inputs.grid_batch_pricing_request,
        "spectral_collocation_node_count_override": tier_inputs
            .spectral_collocation_node_count_override,
        "observed_nodes": tier_inputs
            .observed_nodes
            .iter()
            .map(|node| {
                json!({
                    "quote_timestamp_utc_nanos": node.quote_timestamp_utc_nanos,
                    "expiry_timestamp_utc_nanos": node.expiry_timestamp_utc_nanos,
                    "strike_bits": node.strike.to_bits(),
                    "payoff_type": node.payoff_type,
                    "bid_price_bits": node.bid_price.to_bits(),
                    "ask_price_bits": node.ask_price.to_bits(),
                    "observed_mid_price_bits": node.observed_mid_price.to_bits(),
                    "maturity_weight": node.maturity_weight,
                    "location": {
                        "grid_index": node.location.grid_index,
                        "maturity_index": node.location.maturity_index,
                        "strike_index": node.location.strike_index,
                    },
                })
            })
            .collect::<Vec<_>>(),
        "maturity_interval_bits": tier_inputs
            .maturity_intervals
            .iter()
            .map(|value: &f64| value.to_bits())
            .collect::<Vec<_>>(),
        "maturity_terminal_interval_indices": tier_inputs.maturity_terminal_interval_indices,
    })
}

fn load_resume_state_from_disk(
    path: &Path,
    run_id: &str,
    stage_seed: u64,
) -> Option<JointRefinementResumeState> {
    let raw_bytes = match fs::read(path) {
        Ok(raw_bytes) => raw_bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => return None,
    };

    let resume_state: JointRefinementResumeState = match serde_json::from_slice(&raw_bytes) {
        Ok(resume_state) => resume_state,
        Err(_) => return None,
    };

    if resume_state.schema_version != JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION
        || resume_state.run_id != run_id
        || resume_state.stage_seed != stage_seed
    {
        return None;
    }

    Some(resume_state)
}

fn persist_resume_state_to_disk(
    path: &Path,
    resume_state: &JointRefinementResumeState,
) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Err(format!(
            "joint-refinement resume state path has no parent: {}",
            path.display()
        ));
    };

    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let encoded = serde_json::to_vec_pretty(resume_state).map_err(|error| error.to_string())?;

    let temporary_path = path.with_extension("tmp");

    fs::write(temporary_path.as_path(), encoded).map_err(|error| error.to_string())?;

    if path.exists() {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }

    fs::rename(temporary_path.as_path(), path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use janiform_calibration_artifacts::JointRefinementStageRuntimeProfile;

    use super::{
        JOINT_REFINEMENT_RESUME_STATE_FILENAME, JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION,
        JointRefinementResumeState, JointRefinementResumeStore, persist_resume_state_to_disk,
    };

    fn temp_directory() -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "janiform-joint-refinement-resume-test-{unique_suffix}"
        ))
    }

    #[test]
    fn joint_refinement_resume_store_loads_matching_state_and_ignores_mismatches() {
        let directory = temp_directory();
        let persisted_path = directory.join(JOINT_REFINEMENT_RESUME_STATE_FILENAME);

        let matching_state = JointRefinementResumeState::new("run-001", 17);

        persist_resume_state_to_disk(persisted_path.as_path(), &matching_state)
            .expect("matching resume state should persist");

        let loaded_store =
            JointRefinementResumeStore::load(Some(directory.as_path()), "run-001", 17);
        assert!(loaded_store.restart_selection().is_none());

        let mismatched_state = JointRefinementResumeState {
            schema_version: JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION.to_string(),
            run_id: "run-002".to_string(),
            stage_seed: 17,
            ..JointRefinementResumeState::new("run-002", 17)
        };

        persist_resume_state_to_disk(persisted_path.as_path(), &mismatched_state)
            .expect("mismatched resume state should persist");

        let ignored_store =
            JointRefinementResumeStore::load(Some(directory.as_path()), "run-001", 17);
        assert!(ignored_store.restart_selection().is_none());
        assert!(persisted_path.exists());

        let _ = fs::remove_file(persisted_path);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn restart_selection_resume_requires_matching_option_context_and_selection_width() {
        let selection = super::PersistedRestartSelectionArtifacts {
            best_restart_id: "baseline".to_string(),
            option_refinement_candidate_frontier_ids: vec![
                "baseline".to_string(),
                "halton_01".to_string(),
            ],
            selected_restart_count: 2,
            option_pricing_context_fingerprint: Some("options-a".to_string()),
            coarse_consistency_artifacts_by_restart_id: Default::default(),
            coarse_consistency_wall_clock_millis: 0,
        };
        let valid_restart_ids = ["baseline".to_string(), "halton_01".to_string()]
            .into_iter()
            .collect();

        assert!(selection.matches_resume_context(&valid_restart_ids, 2, Some("options-a")));
        assert!(!selection.matches_resume_context(&valid_restart_ids, 3, Some("options-a")));
        assert!(!selection.matches_resume_context(&valid_restart_ids, 2, Some("options-b")));
        assert!(!selection.matches_resume_context(&valid_restart_ids, 2, None));

        let filtered_selection = super::PersistedRestartSelectionArtifacts {
            selected_restart_count: 5,
            ..selection
        };

        assert!(filtered_selection.matches_resume_context(
            &valid_restart_ids,
            5,
            Some("options-a")
        ));
    }

    #[test]
    fn joint_refinement_resume_store_persists_runtime_profile() {
        let directory = temp_directory();
        let persisted_path = directory.join(JOINT_REFINEMENT_RESUME_STATE_FILENAME);

        let mut store = JointRefinementResumeStore::load(Some(directory.as_path()), "run-001", 17);
        let runtime_profile = JointRefinementStageRuntimeProfile {
            restart_optimization_wall_clock_millis: 123,
            coarse_consistency_wall_clock_millis: 0,
            final_seed_evaluation_wall_clock_millis: 0,
            final_assembly_wall_clock_millis: 0,
            objective_cache_hit_count: 1,
            objective_cache_miss_count: 2,
            deterministic_cache_hit_count: 3,
            deterministic_cache_miss_count: 4,
            seed_summary_cache_hit_count: 5,
            seed_summary_cache_miss_count: 6,
            aggregated_likelihood_context_cache_hit_count: 8,
            aggregated_likelihood_context_cache_miss_count: 9,
            option_pricing_cache_hit_count: 0,
            option_pricing_cache_miss_count: 0,
            option_pricing_run_count: 0,
            invalid_candidate_evaluation_count: 7,
            cumulative_deterministic_evaluation_millis: 89,
            cumulative_modeled_rate_millis: 10,
            cumulative_likelihood_context_resolution_millis: 11,
            cumulative_likelihood_context_5m_sample_millis: 12,
            cumulative_likelihood_context_15m_sample_millis: 13,
            cumulative_likelihood_context_tick_sample_millis: 14,
            cumulative_likelihood_context_tick_optimizer_millis: 15,
            cumulative_likelihood_context_5m_optimizer_millis: 16,
            cumulative_likelihood_context_15m_optimizer_millis: 17,
            cumulative_tick_likelihood_millis: 18,
            cumulative_5m_likelihood_millis: 19,
            cumulative_15m_likelihood_millis: 20,
            cumulative_expected_activity_counts_millis: 21,
            cumulative_deterministic_activity_shape_millis: 22,
            cumulative_deterministic_activity_level_millis: 23,
            cumulative_seed_summary_simulation_millis: 55,
            cumulative_raw_seed_metrics_millis: 24,
            cumulative_loss_build_millis: 25,
            cumulative_option_pricing_millis: 0,
        };

        store.record_runtime_profile(runtime_profile.clone());

        let persisted_bytes =
            fs::read(persisted_path.as_path()).expect("runtime resume state should persist");
        let persisted_state: JointRefinementResumeState =
            serde_json::from_slice(persisted_bytes.as_slice())
                .expect("runtime resume state should deserialize");

        assert_eq!(persisted_state.runtime_profile, Some(runtime_profile));

        let _ = fs::remove_file(persisted_path);
        let _ = fs::remove_dir_all(directory);
    }
}
