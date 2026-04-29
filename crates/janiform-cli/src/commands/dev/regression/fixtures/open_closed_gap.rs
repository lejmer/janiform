use std::path::Path;

use janiform::news::NewsArrivalComponent;
use janiform::verification::{
    NewsSemanticsScenarioClass, OpenClosedGapDeterministicReplayMetadata,
    OpenClosedGapIssueTraceabilityMetadata, OpenClosedGapLeakageRegressionCase,
    OpenClosedGapLeakageRegressionEventInput,
};
use serde_json::{Map, Value};

use super::shared::{
    array_field, finite_f64_field, finite_non_negative_f64_field, malformed_artifact,
    non_empty_string_field, object_field, optional_finite_f64_field, root_object,
    string_array_field, timestamp_field, u64_field,
};
use crate::error::CliError;

pub(crate) fn extract_gap_behavior_cases(
    path: &Path,
    document: &Value,
) -> Result<Vec<OpenClosedGapLeakageRegressionCase>, CliError> {
    let cases = array_field(
        root_object(document, "open/closed gap behavior fixture", path)?,
        "cases",
        "gap behavior fixture",
        "open/closed gap behavior fixture",
        path,
    )?;

    let mut parsed_cases = Vec::with_capacity(cases.len());

    for (case_position, raw_case) in cases.iter().enumerate() {
        let context = format!("gap_behavior_cases[{case_position}]");
        let case = raw_case.as_object().ok_or_else(|| {
            malformed_artifact(
                "open/closed gap behavior fixture",
                path,
                format!("{context} must be a JSON object"),
            )
        })?;
        let initial_state = object_field(
            case,
            "initial_accumulator_state",
            &context,
            "open/closed gap behavior fixture",
            path,
        )?;

        parsed_cases.push(OpenClosedGapLeakageRegressionCase {
            fixture_id: non_empty_string_field(
                case,
                "fixture_id",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            exchange_code: non_empty_string_field(
                case,
                "exchange_code",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            scenario_class: scenario_class(
                &non_empty_string_field(
                    case,
                    "scenario_class",
                    &context,
                    "open/closed gap behavior fixture",
                    path,
                )?,
                path,
            )?,
            deterministic_replay_metadata: deterministic_replay_metadata(
                object_field(
                    case,
                    "deterministic_replay_metadata",
                    &context,
                    "open/closed gap behavior fixture",
                    path,
                )?,
                &format!("{context}.deterministic_replay_metadata"),
                path,
            )?,
            issue_traceability: issue_traceability(
                object_field(
                    case,
                    "issue_traceability",
                    &context,
                    "open/closed gap behavior fixture",
                    path,
                )?,
                &format!("{context}.issue_traceability"),
                path,
            )?,
            initial_j_t: finite_f64_field(
                initial_state,
                "j_t",
                &format!("{context}.initial_accumulator_state"),
                "open/closed gap behavior fixture",
                path,
            )?,
            initial_m_t: finite_f64_field(
                initial_state,
                "m_t",
                &format!("{context}.initial_accumulator_state"),
                "open/closed gap behavior fixture",
                path,
            )?,
            initial_g_t: finite_f64_field(
                initial_state,
                "g_t",
                &format!("{context}.initial_accumulator_state"),
                "open/closed gap behavior fixture",
                path,
            )?,
            injected_events: injected_events(
                array_field(
                    case,
                    "injected_events",
                    &context,
                    "open/closed gap behavior fixture",
                    path,
                )?,
                &context,
                path,
            )?,
        });
    }

    Ok(parsed_cases)
}

fn injected_events(
    events: &[Value],
    case_context: &str,
    path: &Path,
) -> Result<Vec<OpenClosedGapLeakageRegressionEventInput>, CliError> {
    let mut parsed_events = Vec::with_capacity(events.len());

    for (event_position, raw_event) in events.iter().enumerate() {
        let context = format!("{case_context}.injected_events[{event_position}]");
        let event = raw_event.as_object().ok_or_else(|| {
            malformed_artifact(
                "open/closed gap behavior fixture",
                path,
                format!("{context} must be a JSON object"),
            )
        })?;

        parsed_events.push(OpenClosedGapLeakageRegressionEventInput {
            event_id: non_empty_string_field(
                event,
                "event_id",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            event_timestamp_utc: timestamp_field(
                event,
                "timestamp_utc",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            news_arrival_component: news_arrival_component(
                &non_empty_string_field(
                    event,
                    "news_arrival_component",
                    &context,
                    "open/closed gap behavior fixture",
                    path,
                )?,
                path,
            )?,
            shared_severity_mark: finite_non_negative_f64_field(
                event,
                "shared_severity_mark",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            coupled_return_jump_mark: finite_f64_field(
                event,
                "coupled_return_jump_mark",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            observed_activity_coupling_j_t_increment_override: optional_finite_f64_field(
                event,
                "observed_activity_coupling_j_t_increment_override",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            observed_open_jump_m_t_increment_override: optional_finite_f64_field(
                event,
                "observed_open_jump_m_t_increment_override",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            observed_closed_gap_g_t_increment_override: optional_finite_f64_field(
                event,
                "observed_closed_gap_g_t_increment_override",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
            observed_trading_return_increment_override: optional_finite_f64_field(
                event,
                "observed_trading_return_increment_override",
                &context,
                "open/closed gap behavior fixture",
                path,
            )?,
        });
    }

    Ok(parsed_events)
}

fn deterministic_replay_metadata(
    metadata: &Map<String, Value>,
    context: &str,
    path: &Path,
) -> Result<OpenClosedGapDeterministicReplayMetadata, CliError> {
    Ok(OpenClosedGapDeterministicReplayMetadata {
        replay_baseline_id: non_empty_string_field(
            metadata,
            "replay_baseline_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        event_calendar_binding_id: non_empty_string_field(
            metadata,
            "event_calendar_binding_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        jump_observation_binding_id: non_empty_string_field(
            metadata,
            "jump_observation_binding_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        gap_observation_binding_id: non_empty_string_field(
            metadata,
            "gap_observation_binding_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        candidate_selection_policy_id: non_empty_string_field(
            metadata,
            "candidate_selection_policy_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        candidate_selection_rank: u64_field(
            metadata,
            "candidate_selection_rank",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
    })
}

fn issue_traceability(
    metadata: &Map<String, Value>,
    context: &str,
    path: &Path,
) -> Result<OpenClosedGapIssueTraceabilityMetadata, CliError> {
    Ok(OpenClosedGapIssueTraceabilityMetadata {
        issue_id: non_empty_string_field(
            metadata,
            "issue_id",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
        semantic_focus_tags: string_array_field(
            metadata,
            "semantic_focus_tags",
            context,
            "open/closed gap behavior fixture",
            path,
        )?,
    })
}

fn scenario_class(raw_value: &str, path: &Path) -> Result<NewsSemanticsScenarioClass, CliError> {
    match raw_value {
        "known_only" => Ok(NewsSemanticsScenarioClass::KnownOnly),
        "surprise_only" => Ok(NewsSemanticsScenarioClass::SurpriseOnly),
        "mixed_news" => Ok(NewsSemanticsScenarioClass::MixedNews),
        _ => Err(malformed_artifact(
            "open/closed gap behavior fixture",
            path,
            format!(
                "scenario_class must be one of 'known_only', 'surprise_only', or 'mixed_news', found `{raw_value}`"
            ),
        )),
    }
}

fn news_arrival_component(raw_value: &str, path: &Path) -> Result<NewsArrivalComponent, CliError> {
    match raw_value {
        "known" => Ok(NewsArrivalComponent::Known),
        "surprise" => Ok(NewsArrivalComponent::Surprise),
        _ => Err(malformed_artifact(
            "open/closed gap behavior fixture",
            path,
            format!(
                "news_arrival_component must be either 'known' or 'surprise', found `{raw_value}`"
            ),
        )),
    }
}
