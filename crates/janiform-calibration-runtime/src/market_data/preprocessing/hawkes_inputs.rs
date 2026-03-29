use super::contracts::MarketDataTranslationPreprocessingError;
use super::known_news_samples::open_session_known_news_activity_share;
use super::shared::{
    CalibrationHawkesDirectionalLabelDiagnostics, CalibrationHawkesObservedSourceRegime,
    CalibrationHawkesStageCandidateInput, HawkesStructuredAlphaParameterization,
    HawkesStructuredRhoParameterization, HawkesStructuredRhoSelfDominancePolicy,
    MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS, MarketState, OpenTickRecord, ResolvedKnownNewsRecord,
    ResolvedTickRecord, arithmetic_mean, clamp_to_open_unit_interval, clamp_to_unit_interval,
    coefficient_of_variation, logistic, observation_window_hours, round_positive_to_domain,
    round_to, smallest_positive_value, strictly_positive_machine_value, unique_sorted_values,
};
use janiform_model_kernels::hawkes::{HawkesKernelLoadings, stationary_intensity_multipliers};

pub(super) fn build_hawkes_candidate_inputs(
    resolved_ticks: &[ResolvedTickRecord],
    open_price_ticks: &[OpenTickRecord],
    retained_known_news: &[ResolvedKnownNewsRecord],
    session_open_tick_count: usize,
    session_closed_tick_count: usize,
    open_session_buy_tick_count: usize,
    open_session_sell_tick_count: usize,
    directional_label_diagnostics: Option<CalibrationHawkesDirectionalLabelDiagnostics>,
) -> Result<Vec<CalibrationHawkesStageCandidateInput>, MarketDataTranslationPreprocessingError> {
    if let Some(missing_sign_tick) = resolved_ticks
        .iter()
        .find(|resolved_tick| resolved_tick.trade_sign.is_none())
    {
        return Err(
            MarketDataTranslationPreprocessingError::HawkesRequiresResolvedTradeSigns {
                source_row_number: missing_sign_tick.source_row_number,
            },
        );
    }

    let directional_label_diagnostics = directional_label_diagnostics.ok_or(
        MarketDataTranslationPreprocessingError::HawkesRequiresDirectionalLabelDiagnostics,
    )?;

    let tick_count = resolved_ticks.len();

    let observation_duration_hours = observation_window_hours(
        resolved_ticks.first().map(|tick| tick.timestamp_utc_nanos),
        resolved_ticks.last().map(|tick| tick.timestamp_utc_nanos),
    );
    let (open_session_duration_hours, open_interarrival_seconds) =
        open_session_transition_metrics(open_price_ticks);

    let mean_interarrival_seconds =
        arithmetic_mean(open_interarrival_seconds.as_slice()).unwrap_or(60.0);

    let interarrival_coefficient_of_variation =
        coefficient_of_variation(open_interarrival_seconds.as_slice());

    let short_gap_threshold_seconds = (mean_interarrival_seconds * 0.5).max(
        smallest_positive_value(open_interarrival_seconds.as_slice())
            .unwrap_or(strictly_positive_machine_value()),
    );

    let short_gap_ratio = if open_interarrival_seconds.is_empty() {
        0.0
    } else {
        open_interarrival_seconds
            .iter()
            .filter(|duration_seconds| **duration_seconds <= short_gap_threshold_seconds)
            .count() as f64
            / open_interarrival_seconds.len() as f64
    };

    let open_known_news_timestamps: Vec<i64> = retained_known_news
        .iter()
        .filter(|record| record.market_state == MarketState::Open)
        .map(|record| record.event_timestamp_utc_nanos)
        .collect();

    let open_known_news_activity_share = open_session_known_news_activity_share(
        open_price_ticks,
        open_known_news_timestamps.as_slice(),
    );

    let source_regime = CalibrationHawkesObservedSourceRegime {
        observation_duration_hours: round_to(observation_duration_hours, 6),
        open_session_duration_hours: round_to(open_session_duration_hours, 6),
        total_tick_count: tick_count,
        open_session_tick_count: session_open_tick_count,
        closed_session_tick_count: session_closed_tick_count,
        open_session_buy_tick_count,
        open_session_sell_tick_count,
        open_session_mean_interarrival_seconds: round_to(
            mean_interarrival_seconds.max(strictly_positive_machine_value()),
            6,
        ),
        open_session_interarrival_coefficient_of_variation: round_to(
            interarrival_coefficient_of_variation,
            6,
        ),
        open_session_short_gap_threshold_seconds: round_to(short_gap_threshold_seconds, 6),
        open_session_short_gap_ratio: round_to(short_gap_ratio, 6),
        known_news_open_event_count: open_known_news_timestamps.len(),
        open_session_known_news_activity_share: round_to(open_known_news_activity_share, 6),
    };

    let open_session_rate_per_hour = session_open_tick_count as f64
        / open_session_duration_hours.max(strictly_positive_machine_value());
    let open_session_rate_per_second = open_session_rate_per_hour / 3600.0;

    let poisson_short_gap_ratio =
        1.0 - f64::exp(-open_session_rate_per_second * short_gap_threshold_seconds);

    let clustering_excess = (short_gap_ratio - poisson_short_gap_ratio).max(0.0);

    let clustering_center = clamp_to_open_unit_interval(
        0.15 + 0.45 * clustering_excess
            + 0.20 * (interarrival_coefficient_of_variation - 1.0).max(0.0),
    );

    let beta_center = (60.0 / mean_interarrival_seconds.max(strictly_positive_machine_value()))
        .max(strictly_positive_machine_value());

    let open_sell_share =
        open_session_sell_tick_count as f64 / session_open_tick_count.max(1) as f64;

    let asymmetry_fraction = clamp_to_unit_interval((open_sell_share - 0.5).abs() * 1.6);

    let news_rate_per_hour = open_known_news_timestamps.len() as f64
        / open_session_duration_hours.max(strictly_positive_machine_value());

    let branching_values = derive_branching_ratio_candidates(clustering_center);
    let beta_values = derive_positive_scale_candidates(beta_center);

    let cross_shares = [0.10, 0.22];
    let lambda_scales = [0.5, 1.0, 2.0];
    let rho_scales = [0.25, 1.0, 4.0];

    let mut candidate_inputs: Vec<CalibrationHawkesStageCandidateInput> = Vec::new();
    let mut candidate_index: usize = 0;

    for branching_ratio in branching_values {
        for beta in beta_values.iter().copied() {
            for cross_share in cross_shares {
                let structured_alpha = build_structured_alpha_candidate(
                    branching_ratio,
                    beta,
                    cross_share,
                    asymmetry_fraction,
                );
                let matched_lambda_bar = matched_lambda_bar_for_observed_open_rate(
                    open_session_rate_per_hour,
                    beta,
                    &structured_alpha,
                );

                for lambda_scale in lambda_scales {
                    let lambda_bar =
                        (matched_lambda_bar * lambda_scale).max(strictly_positive_machine_value());

                    for rho_scale in rho_scales {
                        let structured_rho = build_structured_rho_candidate(
                            beta,
                            clamp_to_open_unit_interval(cross_share * 1.4),
                            clamp_to_open_unit_interval(
                                asymmetry_fraction * 1.15 + strictly_positive_machine_value(),
                            ),
                            open_session_rate_per_hour,
                            news_rate_per_hour,
                            open_known_news_activity_share,
                            rho_scale,
                        );

                        candidate_inputs.push(CalibrationHawkesStageCandidateInput {
                            candidate_id: format!("market_data_fit_{candidate_index:03}"),
                            lambda_bar: round_positive_to_domain(lambda_bar, 12),
                            beta: round_positive_to_domain(beta, 12),
                            structured_alpha,
                            structured_rho,
                            self_dominance_policy: Some(HawkesStructuredRhoSelfDominancePolicy {
                                minimum_self_to_cross_ratio: 1.0,
                            }),
                            stability_numeric_tolerance: 0.0,
                            source_regime: source_regime.clone(),
                            directional_label_diagnostics: directional_label_diagnostics.clone(),
                        });

                        candidate_index += 1;
                    }
                }
            }
        }
    }

    Ok(candidate_inputs)
}

pub(super) fn build_structured_alpha_candidate(
    branching_ratio: f64,
    beta: f64,
    cross_share: f64,
    asymmetry_fraction: f64,
) -> HawkesStructuredAlphaParameterization {
    let row_sum_center = branching_ratio * beta;
    let asymmetry_mass = row_sum_center * asymmetry_fraction;
    let buy_row_sum = (row_sum_center - asymmetry_mass / 2.0).max(row_sum_center * 0.45);

    HawkesStructuredAlphaParameterization {
        a_self: round_positive_to_domain(
            (buy_row_sum * (1.0 - cross_share)).max(strictly_positive_machine_value()),
            12,
        ),
        a_cross: round_to((buy_row_sum * cross_share).max(0.0), 12),
        delta_self: round_to((asymmetry_mass * (1.0 - cross_share)).max(0.0), 12),
        delta_cross: round_to((asymmetry_mass * cross_share).max(0.0), 12),
    }
}

pub(super) fn build_structured_rho_candidate(
    beta: f64,
    cross_share: f64,
    asymmetry_fraction: f64,
    open_session_rate_per_hour: f64,
    news_rate_per_hour: f64,
    known_news_activity_share: f64,
    rho_scale: f64,
) -> HawkesStructuredRhoParameterization {
    let base_row_sum = if news_rate_per_hour <= 0.0 || known_news_activity_share <= 0.0 {
        0.0
    } else {
        let target_total_rho_mass =
            2.0 * beta * open_session_rate_per_hour * known_news_activity_share
                / news_rate_per_hour.max(strictly_positive_machine_value());
        (target_total_rho_mass / 2.0).max(0.0) * rho_scale
    };

    let asymmetry_mass = base_row_sum * clamp_to_unit_interval(asymmetry_fraction);
    let buy_row_sum = (base_row_sum - asymmetry_mass / 2.0).max(base_row_sum * 0.45);

    HawkesStructuredRhoParameterization {
        r_self: round_positive_to_domain(
            (buy_row_sum * (1.0 - cross_share)).max(strictly_positive_machine_value()),
            12,
        ),
        r_cross: round_to((buy_row_sum * cross_share).max(0.0), 12),
        delta_self_r: round_to((asymmetry_mass * (1.0 - cross_share)).max(0.0), 12),
        delta_cross_r: round_to((asymmetry_mass * cross_share).max(0.0), 12),
    }
}

pub(super) fn open_session_transition_metrics(
    open_price_ticks: &[OpenTickRecord],
) -> (f64, Vec<f64>) {
    if open_price_ticks.len() < 2 {
        return (MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS, Vec::new());
    }

    let mut cumulative_seconds = 0.0;
    let mut same_session_gaps: Vec<f64> = Vec::new();

    for (previous_tick, current_tick) in
        open_price_ticks.iter().zip(open_price_ticks.iter().skip(1))
    {
        if previous_tick.session_close_utc_nanos == current_tick.session_close_utc_nanos
            && previous_tick.session_open_utc_nanos == current_tick.session_open_utc_nanos
        {
            let gap_seconds = (current_tick.timestamp_utc_nanos - previous_tick.timestamp_utc_nanos)
                as f64
                / 1_000_000_000.0;
            let clamped_gap_seconds = gap_seconds.max(0.0);

            cumulative_seconds += clamped_gap_seconds;
            same_session_gaps.push(clamped_gap_seconds);
        } else {
            let boundary_seconds = (previous_tick.session_close_utc_nanos
                - previous_tick.timestamp_utc_nanos) as f64
                / 1_000_000_000.0;

            cumulative_seconds += boundary_seconds.max(0.0);
        }
    }

    let end_padding_seconds = arithmetic_mean(same_session_gaps.as_slice()).unwrap_or(60.0);
    cumulative_seconds += end_padding_seconds.max(60.0);
    (
        (cumulative_seconds / 3600.0).max(MINIMUM_OPEN_TICK_COUNT_DURATION_HOURS),
        same_session_gaps,
    )
}

pub(super) fn matched_lambda_bar_for_observed_open_rate(
    open_session_rate_per_hour: f64,
    beta: f64,
    structured_alpha: &HawkesStructuredAlphaParameterization,
) -> f64 {
    let alpha = HawkesKernelLoadings {
        plus_plus: structured_alpha.a_self,
        plus_minus: structured_alpha.a_cross,
        minus_plus: structured_alpha.a_cross + structured_alpha.delta_cross,
        minus_minus: structured_alpha.a_self + structured_alpha.delta_self,
    };

    let Some(stationary_multipliers) = stationary_intensity_multipliers(alpha, beta) else {
        return (open_session_rate_per_hour / 2.0).max(strictly_positive_machine_value());
    };

    let inverse_sum = stationary_multipliers.total_intensity_multiplier();
    if inverse_sum <= strictly_positive_machine_value() {
        return (open_session_rate_per_hour / 2.0).max(strictly_positive_machine_value());
    }

    (open_session_rate_per_hour / inverse_sum).max(strictly_positive_machine_value())
}

pub(super) fn derive_branching_ratio_candidates(center: f64) -> Vec<f64> {
    const LOGIT_OFFSETS: [f64; 5] = [-2.0, -1.0, 0.0, 1.0, 2.0];

    let bounded_center = clamp_to_open_unit_interval(center);
    let center_logit = (bounded_center / (1.0 - bounded_center)).ln();

    unique_sorted_values(
        LOGIT_OFFSETS
            .iter()
            .map(|offset| {
                clamp_to_open_unit_interval(round_to(logistic(center_logit + offset), 12))
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

pub(super) fn derive_positive_scale_candidates(center: f64) -> Vec<f64> {
    const SCALE_MULTIPLIERS: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

    let bounded_center = center.max(strictly_positive_machine_value());

    unique_sorted_values(
        SCALE_MULTIPLIERS
            .iter()
            .map(|multiplier| round_positive_to_domain(bounded_center * multiplier, 12))
            .collect::<Vec<_>>()
            .as_slice(),
    )
}
