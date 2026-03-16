use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use super::{CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION, CalibrationStage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CalibrationStageDependency {
    pub stage: CalibrationStage,
    pub depends_on: CalibrationStage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CalibrationStageGraph {
    #[serde(default)]
    pub dependencies: Vec<CalibrationStageDependency>,
}

impl CalibrationStageGraph {
    pub const fn new(dependencies: Vec<CalibrationStageDependency>) -> Self {
        Self { dependencies }
    }

    pub fn resolve_execution_order(
        &self,
        selected_stages: &[CalibrationStage],
    ) -> Result<Vec<CalibrationStage>, CalibrationStageGraphError> {
        if selected_stages.is_empty() {
            return Err(CalibrationStageGraphError::MissingSelectedStages);
        }

        let mut stage_positions = BTreeMap::new();
        for (position, stage) in selected_stages.iter().copied().enumerate() {
            if stage_positions.insert(stage, position).is_some() {
                return Err(CalibrationStageGraphError::DuplicateSelectedStage { stage });
            }
        }

        let stage_count = selected_stages.len();
        let mut indegree_by_position = vec![0usize; stage_count];
        let mut adjacency_by_position = vec![Vec::<usize>::new(); stage_count];
        let mut seen_dependencies = BTreeSet::new();

        for dependency in &self.dependencies {
            let Some(&stage_position) = stage_positions.get(&dependency.stage) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "stage is not selected in run_spec.selected_stages",
                });
            };
            let Some(&depends_on_position) = stage_positions.get(&dependency.depends_on) else {
                return Err(CalibrationStageGraphError::UnsupportedDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                    reason: "depends_on stage is not selected in run_spec.selected_stages",
                });
            };

            if dependency.stage == dependency.depends_on {
                return Err(CalibrationStageGraphError::SelfDependency {
                    stage: dependency.stage,
                });
            }

            if !seen_dependencies.insert(*dependency) {
                return Err(CalibrationStageGraphError::DuplicateDependency {
                    stage: dependency.stage,
                    depends_on: dependency.depends_on,
                });
            }

            indegree_by_position[stage_position] =
                indegree_by_position[stage_position].saturating_add(1);

            adjacency_by_position[depends_on_position].push(stage_position);
        }

        for dependencies in &mut adjacency_by_position {
            dependencies.sort_unstable();
        }

        let mut ready_queue = VecDeque::new();
        for (position, indegree) in indegree_by_position.iter().copied().enumerate() {
            if indegree == 0 {
                ready_queue.push_back(position);
            }
        }

        let mut execution_positions = Vec::with_capacity(stage_count);
        while let Some(current_position) = ready_queue.pop_front() {
            execution_positions.push(current_position);
            for dependent_position in adjacency_by_position[current_position].iter().copied() {
                if indegree_by_position[dependent_position] == 0 {
                    continue;
                }

                indegree_by_position[dependent_position] -= 1;

                if indegree_by_position[dependent_position] == 0 {
                    ready_queue.push_back(dependent_position);
                }
            }
        }

        if execution_positions.len() != stage_count {
            let cyclic_stages = indegree_by_position
                .iter()
                .copied()
                .enumerate()
                .filter_map(|(position, indegree)| {
                    if indegree > 0 {
                        Some(selected_stages[position])
                    } else {
                        None
                    }
                })
                .collect();

            return Err(CalibrationStageGraphError::CyclicDependency {
                stages: cyclic_stages,
            });
        }

        let execution_order = execution_positions
            .iter()
            .copied()
            .map(|position| selected_stages[position])
            .collect();
        Ok(execution_order)
    }

    pub const fn schema_version() -> &'static str {
        CALIBRATION_STAGE_GRAPH_SCHEMA_VERSION
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationStageGraphError {
    MissingSelectedStages,
    DuplicateSelectedStage {
        stage: CalibrationStage,
    },
    UnsupportedDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
        reason: &'static str,
    },
    SelfDependency {
        stage: CalibrationStage,
    },
    DuplicateDependency {
        stage: CalibrationStage,
        depends_on: CalibrationStage,
    },
    CyclicDependency {
        stages: Vec<CalibrationStage>,
    },
}

impl Display for CalibrationStageGraphError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSelectedStages => write!(
                formatter,
                "stage graph validation requires at least one selected stage"
            ),
            Self::DuplicateSelectedStage { stage } => write!(
                formatter,
                "run_spec.selected_stages contains duplicate stage `{stage:?}`"
            ),
            Self::UnsupportedDependency {
                stage,
                depends_on,
                reason,
            } => write!(
                formatter,
                "unsupported stage dependency `{stage:?}` -> `{depends_on:?}`: {reason}"
            ),
            Self::SelfDependency { stage } => write!(
                formatter,
                "stage graph contains self dependency for stage `{stage:?}`"
            ),
            Self::DuplicateDependency { stage, depends_on } => write!(
                formatter,
                "stage graph contains duplicate dependency `{stage:?}` -> `{depends_on:?}`"
            ),
            Self::CyclicDependency { stages } => write!(
                formatter,
                "stage graph contains cyclic dependency involving stages: {stages:?}"
            ),
        }
    }
}

impl Error for CalibrationStageGraphError {}
