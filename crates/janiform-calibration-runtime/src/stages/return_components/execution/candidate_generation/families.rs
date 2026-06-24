use super::super::diagnostics::round_candidate_value;
use janiform_model_kernels::market_pressure::MarketPressureParameters;

use crate::stages::return_components::activity_coupling_fit;
use crate::stages::return_components::continuous_layer_fit;
use crate::stages::return_components::contracts::{
    ReturnComponentsActivityCouplingCandidateFamily,
    ReturnComponentsReturnJumpMarkScaleCandidateFamily, ReturnComponentsStageCandidateFamilyCounts,
    ReturnComponentsStageCandidateGenerationInput,
    ReturnComponentsSurpriseNewsArrivalCandidateFamily,
};
use crate::stages::return_components::known_arrival_fit;
use crate::stages::return_components::mark_layer_fit;
use crate::stages::return_components::surprise_intensity_fit;

#[derive(Debug, Clone)]
pub(super) struct ReturnComponentsStageCandidateFamilySets {
    pub(super) fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    pub(super) fitted_known_arrival_layer: known_arrival_fit::FittedKnownNewsArrivalLayer,
    pub(super) fitted_surprise_arrival_layer:
        Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily>,
    pub(super) fitted_return_jump_mark_layer:
        Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily>,
    pub(super) fitted_activity_coupling_layer: Vec<ReturnComponentsActivityCouplingCandidateFamily>,
    pub(super) fitted_market_pressure_layer: Vec<MarketPressureParameters>,
}

pub(super) fn build_candidate_family_sets(
    candidate_generation_input: &ReturnComponentsStageCandidateGenerationInput,
    fitted_continuous_return_layer: continuous_layer_fit::FittedContinuousReturnLayer,
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
    fitted_market_pressure_parameters: &MarketPressureParameters,
) -> ReturnComponentsStageCandidateFamilySets {
    ReturnComponentsStageCandidateFamilySets {
        fitted_continuous_return_layer,
        fitted_known_arrival_layer: known_arrival_fit::fit_known_news_arrival_layer(
            &candidate_generation_input.known_news_calendar,
        ),
        fitted_surprise_arrival_layer: build_surprise_arrival_families(
            fitted_surprise_arrival_layer,
        ),
        fitted_return_jump_mark_layer: build_return_jump_mark_scale_families(
            fitted_return_jump_mark_layer,
        ),
        fitted_activity_coupling_layer: build_activity_coupling_families(
            fitted_activity_coupling_layer,
        ),
        fitted_market_pressure_layer: build_market_pressure_families(
            fitted_market_pressure_parameters,
        ),
    }
}

pub(super) fn build_candidate_family_counts(
    candidate_family_sets: &ReturnComponentsStageCandidateFamilySets,
) -> ReturnComponentsStageCandidateFamilyCounts {
    ReturnComponentsStageCandidateFamilyCounts {
        nig_candidate_count: candidate_family_sets
            .fitted_continuous_return_layer
            .candidates
            .len(),
        known_news_arrival_family_count: 1,
        surprise_news_arrival_family_count: candidate_family_sets
            .fitted_surprise_arrival_layer
            .len(),
        return_jump_mark_scale_family_count: candidate_family_sets
            .fitted_return_jump_mark_layer
            .len(),
        activity_coupling_family_count: candidate_family_sets.fitted_activity_coupling_layer.len(),
        market_pressure_family_count: candidate_family_sets.fitted_market_pressure_layer.len(),
    }
}

fn build_surprise_arrival_families(
    fitted_surprise_arrival_layer: &surprise_intensity_fit::FittedSurpriseNewsArrivalLayer,
) -> Vec<ReturnComponentsSurpriseNewsArrivalCandidateFamily> {
    // Candidate families own their intensity profile because they are serialized independently.
    let weekly_utc_profile_multipliers = fitted_surprise_arrival_layer
        .deterministic_intensity_profile
        .weekly_utc_profile_multipliers
        .clone();

    let fitted_family = ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        open_baseline_intensity: round_candidate_value(
            fitted_surprise_arrival_layer.open_baseline_intensity_per_hour,
        ),
        closed_baseline_intensity: round_candidate_value(
            fitted_surprise_arrival_layer.closed_baseline_intensity_per_hour,
        ),
        weekly_utc_profile_multipliers,
        profile_resolution_hours: fitted_surprise_arrival_layer
            .deterministic_intensity_profile
            .profile_resolution_hours,
    };

    let homogeneous_family = homogeneous_surprise_arrival_family(&fitted_family);
    let neutral_family = neutral_surprise_arrival_family(&fitted_family);

    let mut families = Vec::new();
    push_unique_candidate_family_variant(&mut families, fitted_family);
    push_unique_candidate_family_variant(&mut families, homogeneous_family);
    push_unique_candidate_family_variant(&mut families, neutral_family);

    families
}

fn build_return_jump_mark_scale_families(
    fitted_return_jump_mark_layer: &mark_layer_fit::FittedReturnJumpMarkLayer,
) -> Vec<ReturnComponentsReturnJumpMarkScaleCandidateFamily> {
    let fitted_family = ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        known_return_jump_drift: fitted_return_jump_mark_layer.known_return_jump_drift,
        known_return_jump_volatility: fitted_return_jump_mark_layer.known_return_jump_volatility,
        surprise_return_jump_drift: fitted_return_jump_mark_layer.surprise_return_jump_drift,
        surprise_return_jump_volatility: fitted_return_jump_mark_layer
            .surprise_return_jump_volatility,
    };

    let mut families = Vec::new();
    push_unique_candidate_family_variant(&mut families, fitted_family);
    push_unique_candidate_family_variant(&mut families, neutral_return_jump_mark_family());

    if known_jump_component_is_active(&fitted_family)
        && surprise_jump_component_is_active(&fitted_family)
    {
        push_unique_candidate_family_variant(
            &mut families,
            known_only_return_jump_mark_family(fitted_family),
        );
        push_unique_candidate_family_variant(
            &mut families,
            surprise_only_return_jump_mark_family(fitted_family),
        );
    }

    families
}

fn build_activity_coupling_families(
    fitted_activity_coupling_layer: &activity_coupling_fit::FittedActivityCouplingLayer,
) -> Vec<ReturnComponentsActivityCouplingCandidateFamily> {
    let fitted_family = ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: fitted_activity_coupling_layer.known_activity_coupling_scale,
        surprise_activity_coupling_scale: fitted_activity_coupling_layer
            .surprise_activity_coupling_scale,
    };

    let mut families = Vec::new();
    push_unique_candidate_family_variant(&mut families, fitted_family);
    push_unique_candidate_family_variant(&mut families, neutral_activity_coupling_family());

    if known_activity_coupling_is_active(&fitted_family)
        && surprise_activity_coupling_is_active(&fitted_family)
    {
        push_unique_candidate_family_variant(
            &mut families,
            known_only_activity_coupling_family(fitted_family),
        );
        push_unique_candidate_family_variant(
            &mut families,
            surprise_only_activity_coupling_family(fitted_family),
        );
    }

    families
}

fn build_market_pressure_families(
    fitted_market_pressure_parameters: &MarketPressureParameters,
) -> Vec<MarketPressureParameters> {
    let mut families = Vec::new();

    // Candidate variants must own their pressure contracts independently of the fitted layer.
    push_unique_candidate_family_variant(&mut families, fitted_market_pressure_parameters.clone());
    push_unique_candidate_family_variant(
        &mut families,
        MarketPressureParameters::no_anchor_impact(),
    );

    families
}

fn homogeneous_surprise_arrival_family(
    fitted_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
) -> ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        open_baseline_intensity: fitted_family.open_baseline_intensity,
        closed_baseline_intensity: fitted_family.closed_baseline_intensity,
        weekly_utc_profile_multipliers: unit_profile_like(fitted_family),
        profile_resolution_hours: fitted_family.profile_resolution_hours,
    }
}

fn neutral_surprise_arrival_family(
    fitted_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
) -> ReturnComponentsSurpriseNewsArrivalCandidateFamily {
    ReturnComponentsSurpriseNewsArrivalCandidateFamily {
        open_baseline_intensity: 0.0,
        closed_baseline_intensity: 0.0,
        weekly_utc_profile_multipliers: unit_profile_like(fitted_family),
        profile_resolution_hours: fitted_family.profile_resolution_hours,
    }
}

fn unit_profile_like(
    fitted_family: &ReturnComponentsSurpriseNewsArrivalCandidateFamily,
) -> Vec<f64> {
    vec![1.0; fitted_family.weekly_utc_profile_multipliers.len()]
}

fn neutral_return_jump_mark_family() -> ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        known_return_jump_drift: 0.0,
        known_return_jump_volatility: 0.0,
        surprise_return_jump_drift: 0.0,
        surprise_return_jump_volatility: 0.0,
    }
}

fn known_only_return_jump_mark_family(
    fitted_family: ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        surprise_return_jump_drift: 0.0,
        surprise_return_jump_volatility: 0.0,
        ..fitted_family
    }
}

fn surprise_only_return_jump_mark_family(
    fitted_family: ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> ReturnComponentsReturnJumpMarkScaleCandidateFamily {
    ReturnComponentsReturnJumpMarkScaleCandidateFamily {
        known_return_jump_drift: 0.0,
        known_return_jump_volatility: 0.0,
        ..fitted_family
    }
}

fn known_jump_component_is_active(
    fitted_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> bool {
    fitted_family.known_return_jump_drift != 0.0
        || fitted_family.known_return_jump_volatility != 0.0
}

fn surprise_jump_component_is_active(
    fitted_family: &ReturnComponentsReturnJumpMarkScaleCandidateFamily,
) -> bool {
    fitted_family.surprise_return_jump_drift != 0.0
        || fitted_family.surprise_return_jump_volatility != 0.0
}

fn neutral_activity_coupling_family() -> ReturnComponentsActivityCouplingCandidateFamily {
    ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: 0.0,
        surprise_activity_coupling_scale: 0.0,
    }
}

fn known_only_activity_coupling_family(
    fitted_family: ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsActivityCouplingCandidateFamily {
    ReturnComponentsActivityCouplingCandidateFamily {
        surprise_activity_coupling_scale: 0.0,
        ..fitted_family
    }
}

fn surprise_only_activity_coupling_family(
    fitted_family: ReturnComponentsActivityCouplingCandidateFamily,
) -> ReturnComponentsActivityCouplingCandidateFamily {
    ReturnComponentsActivityCouplingCandidateFamily {
        known_activity_coupling_scale: 0.0,
        ..fitted_family
    }
}

fn known_activity_coupling_is_active(
    fitted_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> bool {
    fitted_family.known_activity_coupling_scale != 0.0
}

fn surprise_activity_coupling_is_active(
    fitted_family: &ReturnComponentsActivityCouplingCandidateFamily,
) -> bool {
    fitted_family.surprise_activity_coupling_scale != 0.0
}

fn push_unique_candidate_family_variant<T>(families: &mut Vec<T>, family: T)
where
    T: PartialEq,
{
    if !families.contains(&family) {
        families.push(family);
    }
}
