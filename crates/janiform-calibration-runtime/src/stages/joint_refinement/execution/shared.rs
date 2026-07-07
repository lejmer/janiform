use std::time::Instant;

use chrono::{DateTime, Utc};
use janiform_foundations::low_discrepancy::van_der_corput;
use janiform_market_time::calendar::{
    CalendarRuleMetadataArtifact, ExchangeCalendarArtifact, ExchangeCalendarRegistryArtifact,
    RegularSessionArtifact, SessionWindowArtifact,
};

use janiform_market_time::calendar::ExchangeCalendarRegistry;

use super::state::JointRefinementContext;

pub(crate) fn ordered_parameter_bits(parameter_vector: &[f64]) -> Vec<u64> {
    parameter_vector
        .iter()
        .map(|value| value.to_bits())
        .collect::<Vec<_>>()
}

pub(crate) fn planned_candidate_work_item_labels<'a>(
    candidate_ids: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    candidate_ids.into_iter().map(str::to_string).collect()
}

pub(crate) fn elapsed_millis(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn deduplicate_candidate_seeds(
    candidate_seeds: Vec<(String, Vec<f64>)>,
) -> Vec<(String, Vec<f64>)> {
    let mut deduplicated_candidates = Vec::new();
    let mut seen_parameter_bits = Vec::new();

    for (candidate_id, candidate_vector) in candidate_seeds {
        let parameter_bits = ordered_parameter_bits(candidate_vector.as_slice());

        if seen_parameter_bits
            .iter()
            .any(|seen_bits: &Vec<u64>| seen_bits == &parameter_bits)
        {
            continue;
        }

        seen_parameter_bits.push(parameter_bits);
        deduplicated_candidates.push((candidate_id, candidate_vector));
    }

    deduplicated_candidates
}

pub(crate) fn halton_parameter_vector(
    halton_index: u64,
    lower_bounds: &[f64],
    upper_bounds: &[f64],
) -> Vec<f64> {
    const HALTON_BASES: [u64; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];

    lower_bounds
        .iter()
        .zip(upper_bounds.iter())
        .enumerate()
        .map(|(dimension_index, (lower_bound, upper_bound))| {
            let halton_coordinate = van_der_corput(
                halton_index,
                HALTON_BASES[dimension_index % HALTON_BASES.len()],
            );
            lower_bound + (upper_bound - lower_bound) * halton_coordinate
        })
        .collect()
}

pub(crate) fn build_halton_candidate_id(position: usize) -> String {
    format!("halton_{position:02}")
}

pub(crate) fn with_joint_refinement_thread_pool<T, F>(
    context: &JointRefinementContext,
    operation: F,
) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match context.stage_thread_pool.as_ref() {
        Some(thread_pool) => thread_pool.install(operation),
        None => operation(),
    }
}

pub(crate) fn joint_refinement_parallel_worker_count(context: &JointRefinementContext) -> usize {
    context
        .stage_thread_pool
        .as_ref()
        .map(|thread_pool| thread_pool.current_num_threads())
        .unwrap_or(1)
}

pub(crate) fn joint_refinement_has_parallel_worker_capacity(
    context: &JointRefinementContext,
) -> bool {
    joint_refinement_parallel_worker_count(context) > 1
}

pub(crate) fn build_exchange_calendar_registry_artifact(
    calendar_registry: &ExchangeCalendarRegistry,
    exchange_code: &str,
    artifact_anchor_time_utc: DateTime<Utc>,
) -> Option<ExchangeCalendarRegistryArtifact> {
    let definition = calendar_registry.get_required(exchange_code).ok()?;

    let regular_sessions = definition
        .regular_sessions()
        .iter()
        .map(|regular_session| RegularSessionArtifact {
            weekday: regular_session.weekday(),
            windows: regular_session
                .windows()
                .iter()
                .map(|window| SessionWindowArtifact {
                    label: window.label().to_string(),
                    open_time: window.open_time().to_string(),
                    close_time: window.close_time().to_string(),
                })
                .collect(),
        })
        .collect();

    Some(ExchangeCalendarRegistryArtifact {
        schema_version: "calendar.registry.v1".to_string(),
        artifact_date: artifact_anchor_time_utc.date_naive().to_string(),
        exchanges: vec![ExchangeCalendarArtifact {
            schema_version: definition.schema_version().to_string(),
            exchange_code: definition.exchange_metadata().exchange_code().to_string(),
            exchange_name: definition.exchange_metadata().exchange_name().to_string(),
            timezone_name: Some(definition.timezone_name().to_string()),
            rule_metadata: CalendarRuleMetadataArtifact {
                holiday_calendar_code: definition
                    .rule_metadata()
                    .holiday_calendar_code()
                    .to_string(),
                early_close_rule_set: definition
                    .rule_metadata()
                    .early_close_rule_set()
                    .map(str::to_string),
            },
            regular_sessions,
            holiday_dates: Vec::new(),
            early_close_sessions: Vec::new(),
        }],
    })
}
