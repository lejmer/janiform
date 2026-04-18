use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::coarse_consistency::JointRefinementCoarseConsistencyArtifacts;
use super::option_refinement::{OptionRefinedCandidate, SearchTierCandidate};
use super::{
    JointRefinementCoarseConsistencySummary, JointRefinementOptionTierPricingArtifact,
    JointRefinementRestartOutcome,
};

const JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION: &str =
    "janiform.joint_refinement.resume_state.v1";
const JOINT_REFINEMENT_RESUME_STATE_FILENAME: &str = "joint-refinement.resume_state.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PersistedRestartSelectionArtifacts {
    pub(crate) best_restart_id: String,
    pub(crate) option_refinement_candidate_frontier_ids: Vec<String>,
    pub(crate) coarse_consistency_artifacts_by_restart_id:
        BTreeMap<String, JointRefinementCoarseConsistencyArtifacts>,
    pub(crate) selection_coarse_consistency_by_restart_id:
        BTreeMap<String, JointRefinementCoarseConsistencySummary>,
    pub(crate) coarse_consistency_wall_clock_millis: u64,
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
struct JointRefinementResumeState {
    schema_version: String,
    run_id: String,
    stage_seed: u64,
    restart_outcomes_by_id: BTreeMap<String, JointRefinementRestartOutcome>,
    restart_selection: Option<PersistedRestartSelectionArtifacts>,
    search_tier_candidates_by_restart_id: BTreeMap<String, PersistedSearchTierCandidate>,
    selection_tier_candidates_by_restart_id: BTreeMap<String, PersistedSelectionTierCandidate>,
    selected_search_tier_pricing_artifact: Option<JointRefinementOptionTierPricingArtifact>,
    selected_selection_tier_pricing_artifact: Option<JointRefinementOptionTierPricingArtifact>,
    selected_audit_tier_pricing_artifact: Option<JointRefinementOptionTierPricingArtifact>,
    selected_restart_id: Option<String>,
}

impl JointRefinementResumeState {
    fn new(run_id: &str, stage_seed: u64) -> Self {
        Self {
            schema_version: JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION.to_string(),
            run_id: run_id.to_string(),
            stage_seed,
            restart_outcomes_by_id: BTreeMap::new(),
            restart_selection: None,
            search_tier_candidates_by_restart_id: BTreeMap::new(),
            selection_tier_candidates_by_restart_id: BTreeMap::new(),
            selected_search_tier_pricing_artifact: None,
            selected_selection_tier_pricing_artifact: None,
            selected_audit_tier_pricing_artifact: None,
            selected_restart_id: None,
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

    pub(crate) fn restart_outcome(&self, restart_id: &str) -> Option<JointRefinementRestartOutcome> {
        self.state.restart_outcomes_by_id.get(restart_id).cloned()
    }

    pub(crate) fn restart_selection(&self) -> Option<&PersistedRestartSelectionArtifacts> {
        self.state.restart_selection.as_ref()
    }

    pub(crate) fn search_tier_candidate(&self, restart_id: &str) -> Option<SearchTierCandidate> {
        self.state
            .search_tier_candidates_by_restart_id
            .get(restart_id)
            .cloned()
            .map(PersistedSearchTierCandidate::into_runtime)
    }

    pub(crate) fn selection_tier_candidate(
        &self,
        restart_id: &str,
    ) -> Option<OptionRefinedCandidate> {
        self.state
            .selection_tier_candidates_by_restart_id
            .get(restart_id)
            .cloned()
            .map(PersistedSelectionTierCandidate::into_runtime)
    }

    pub(crate) fn selected_restart_id(&self) -> Option<&str> {
        self.state.selected_restart_id.as_deref()
    }

    pub(crate) fn selected_search_tier_pricing_artifact(
        &self,
    ) -> Option<Arc<JointRefinementOptionTierPricingArtifact>> {
        self.state
            .selected_search_tier_pricing_artifact
            .clone()
            .map(Arc::new)
    }

    pub(crate) fn selected_selection_tier_pricing_artifact(
        &self,
    ) -> Option<Arc<JointRefinementOptionTierPricingArtifact>> {
        self.state
            .selected_selection_tier_pricing_artifact
            .clone()
            .map(Arc::new)
    }

    pub(crate) fn selected_audit_tier_pricing_artifact(
        &self,
    ) -> Option<Arc<JointRefinementOptionTierPricingArtifact>> {
        self.state
            .selected_audit_tier_pricing_artifact
            .clone()
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

    pub(crate) fn record_search_tier_candidate(&mut self, candidate: &SearchTierCandidate) {
        self.state.search_tier_candidates_by_restart_id.insert(
            candidate.restart_outcome.restart_id.clone(),
            PersistedSearchTierCandidate::from_runtime(candidate),
        );
        self.persist();
    }

    pub(crate) fn record_selection_tier_candidate(&mut self, candidate: &OptionRefinedCandidate) {
        self.state.selection_tier_candidates_by_restart_id.insert(
            candidate.restart_outcome.restart_id.clone(),
            PersistedSelectionTierCandidate::from_runtime(candidate),
        );
        self.persist();
    }

    pub(crate) fn record_selected_option_refinement(
        &mut self,
        restart_id: &str,
        search_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
        selection_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
    ) {
        self.state.selected_restart_id = Some(restart_id.to_string());
        self.state.selected_search_tier_pricing_artifact = Some(search_tier_pricing_artifact.clone());
        self.state.selected_selection_tier_pricing_artifact =
            Some(selection_tier_pricing_artifact.clone());
        self.persist();
    }

    pub(crate) fn record_audit_tier_pricing_artifact(
        &mut self,
        audit_tier_pricing_artifact: &JointRefinementOptionTierPricingArtifact,
    ) {
        self.state.selected_audit_tier_pricing_artifact =
            Some(audit_tier_pricing_artifact.clone());
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

    let encoded =
        serde_json::to_vec_pretty(resume_state).map_err(|error| error.to_string())?;

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

    use super::{
        JointRefinementResumeState, JointRefinementResumeStore,
        JOINT_REFINEMENT_RESUME_STATE_FILENAME,
        JOINT_REFINEMENT_RESUME_STATE_SCHEMA_VERSION, persist_resume_state_to_disk,
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
        assert!(ignored_store.selected_restart_id().is_none());
        assert!(persisted_path.exists());

        let _ = fs::remove_file(persisted_path);
        let _ = fs::remove_dir_all(directory);
    }
}
