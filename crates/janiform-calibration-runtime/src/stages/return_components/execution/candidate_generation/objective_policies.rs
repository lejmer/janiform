use std::sync::mpsc::Sender;

use crate::stages::return_components::contracts::{
    ReturnComponentsStageDiagnosticPolicy, ReturnComponentsStageObjectiveScoringPolicy,
    ReturnComponentsStageObservedSignalSummaryStatistics,
    ReturnComponentsStagePriceRangeSanityPolicy, ReturnComponentsStageProgressPhase,
    ReturnComponentsStageProgressUpdate,
};
use crate::stages::return_components::diagnostic_policy_fit;
use crate::stages::return_components::objective_scoring_policy_fit;
use crate::stages::return_components::price_range_policy_fit;
use crate::stages::return_components::progress::emit_return_components_labeled_progress;

pub(super) struct ObjectivePolicyFit {
    pub(super) diagnostic_policy: ReturnComponentsStageDiagnosticPolicy,
    pub(super) objective_scoring_policy: ReturnComponentsStageObjectiveScoringPolicy,
    pub(super) price_range_sanity_policy: ReturnComponentsStagePriceRangeSanityPolicy,
}

pub(super) fn fit_objective_policies(
    summary_statistics: ReturnComponentsStageObservedSignalSummaryStatistics,
    progress_sender: Option<&Sender<ReturnComponentsStageProgressUpdate>>,
) -> ObjectivePolicyFit {
    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        0,
        3,
        "fit diagnostic policy",
    );

    let diagnostic_policy = diagnostic_policy_fit::fit_diagnostic_policy();

    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        1,
        3,
        "fit objective scoring policy",
    );

    let objective_scoring_policy =
        objective_scoring_policy_fit::fit_objective_scoring_policy(summary_statistics);

    emit_return_components_labeled_progress(
        progress_sender,
        ReturnComponentsStageProgressPhase::ObjectivePolicyFit,
        2,
        3,
        "fit price range sanity policy",
    );

    let price_range_sanity_policy =
        price_range_policy_fit::fit_price_range_sanity_policy(summary_statistics);

    ObjectivePolicyFit {
        diagnostic_policy,
        objective_scoring_policy,
        price_range_sanity_policy,
    }
}
