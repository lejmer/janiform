use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use janiform_calibration_artifacts::SurpriseNewsDeterministicIntensityProfile;
use janiform_foundations::time::duration_nanos_to_hours;

use super::constants::{
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY,
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY,
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
};
use super::derived_contracts::{
    return_components_stage_positive_domain_floor,
    return_components_stage_surprise_intensity_weekly_profile_bucket_count,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct FittedSurpriseNewsArrivalLayer {
    pub open_baseline_intensity_per_hour: f64,
    pub closed_baseline_intensity_per_hour: f64,
    pub deterministic_intensity_profile: SurpriseNewsDeterministicIntensityProfile,
}

pub(super) fn fit_surprise_news_arrival_layer(
    window_start_utc_nanos: i64,
    window_end_utc_nanos: i64,
    selected_surprise_episode_timestamps_utc_nanos: &[i64],
    selected_closed_gap_count: usize,
    source_window_open_exposure_hours: f64,
    source_window_closed_exposure_hours: f64,
) -> FittedSurpriseNewsArrivalLayer {
    let effective_open_event_count = selected_surprise_episode_timestamps_utc_nanos
        .iter()
        .filter(|&&timestamp_utc_nanos| {
            timestamp_utc_nanos >= window_start_utc_nanos
                && timestamp_utc_nanos < window_end_utc_nanos
        })
        .count();

    let open_exposure_hours =
        source_window_open_exposure_hours.max(return_components_stage_positive_domain_floor());
    let closed_exposure_hours =
        source_window_closed_exposure_hours.max(return_components_stage_positive_domain_floor());

    if effective_open_event_count == 0 && selected_closed_gap_count == 0 {
        return FittedSurpriseNewsArrivalLayer {
            open_baseline_intensity_per_hour: 0.0,
            closed_baseline_intensity_per_hour: 0.0,
            deterministic_intensity_profile: unit_weekly_profile(
                return_components_stage_surprise_intensity_weekly_profile_bucket_count(),
            ),
        };
    }

    FittedSurpriseNewsArrivalLayer {
        open_baseline_intensity_per_hour: effective_open_event_count as f64 / open_exposure_hours,
        closed_baseline_intensity_per_hour: selected_closed_gap_count as f64
            / closed_exposure_hours,
        deterministic_intensity_profile: fit_surprise_news_deterministic_intensity_profile(
            window_start_utc_nanos,
            window_end_utc_nanos,
            selected_surprise_episode_timestamps_utc_nanos,
        ),
    }
}

pub(super) fn fit_surprise_news_deterministic_intensity_profile(
    window_start_utc_nanos: i64,
    window_end_utc_nanos: i64,
    selected_surprise_episode_timestamps_utc_nanos: &[i64],
) -> SurpriseNewsDeterministicIntensityProfile {
    let bucket_count = return_components_stage_surprise_intensity_weekly_profile_bucket_count();
    if window_end_utc_nanos <= window_start_utc_nanos {
        return unit_weekly_profile(bucket_count);
    }

    let exposure_hours_by_bucket =
        exposure_hours_by_weekly_bucket(window_start_utc_nanos, window_end_utc_nanos, bucket_count);

    let total_exposure_hours = exposure_hours_by_bucket.iter().sum::<f64>();
    if total_exposure_hours <= 0.0 {
        return unit_weekly_profile(bucket_count);
    }

    let mut event_count_by_bucket = vec![0.0; bucket_count];

    for &timestamp_utc_nanos in selected_surprise_episode_timestamps_utc_nanos {
        if timestamp_utc_nanos < window_start_utc_nanos
            || timestamp_utc_nanos >= window_end_utc_nanos
        {
            continue;
        }

        let bucket_index = weekly_hour_bucket_index(timestamp_utc_nanos);

        event_count_by_bucket[bucket_index] += 1.0;
    }

    let total_event_count = event_count_by_bucket.iter().sum::<f64>();
    if total_event_count <= 0.0 {
        return unit_weekly_profile(bucket_count);
    }

    let baseline_intensity = total_event_count
        / total_exposure_hours.max(return_components_stage_positive_domain_floor());
    if !baseline_intensity.is_finite() || baseline_intensity <= 0.0 {
        return unit_weekly_profile(bucket_count);
    }

    let shrinkage_event_mass = (total_event_count / bucket_count as f64).max(1.0);

    let mut weekly_hour_utc_multipliers = Vec::with_capacity(bucket_count);

    for bucket_index in 0..bucket_count {
        let bucket_exposure_hours = exposure_hours_by_bucket[bucket_index]
            .max(return_components_stage_positive_domain_floor());

        let bucket_expected_count = baseline_intensity * bucket_exposure_hours;

        let bucket_multiplier = (event_count_by_bucket[bucket_index] + shrinkage_event_mass)
            / (bucket_expected_count + shrinkage_event_mass)
                .max(return_components_stage_positive_domain_floor());

        weekly_hour_utc_multipliers.push(
            if bucket_multiplier.is_finite() && bucket_multiplier > 0.0 {
                bucket_multiplier
            } else {
                1.0
            },
        );
    }

    let weighted_mean_multiplier = weekly_hour_utc_multipliers
        .iter()
        .zip(exposure_hours_by_bucket.iter())
        .map(|(multiplier, exposure_hours)| multiplier * exposure_hours)
        .sum::<f64>()
        / total_exposure_hours.max(return_components_stage_positive_domain_floor());
    if weighted_mean_multiplier.is_finite() && weighted_mean_multiplier > 0.0 {
        for multiplier in &mut weekly_hour_utc_multipliers {
            *multiplier /= weighted_mean_multiplier;
        }
    }

    SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers,
        profile_resolution_hours:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
        fit_policy: RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY.to_string(),
        normalization_policy:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY.to_string(),
    }
}

pub(super) fn profile_mean_absolute_error(
    left: &SurpriseNewsDeterministicIntensityProfile,
    right: &SurpriseNewsDeterministicIntensityProfile,
) -> f64 {
    if left.weekly_hour_utc_multipliers.len() != right.weekly_hour_utc_multipliers.len()
        || left.profile_resolution_hours != right.profile_resolution_hours
    {
        return f64::INFINITY;
    }

    left.weekly_hour_utc_multipliers
        .iter()
        .zip(right.weekly_hour_utc_multipliers.iter())
        .map(|(left_multiplier, right_multiplier)| (left_multiplier - right_multiplier).abs())
        .sum::<f64>()
        / left.weekly_hour_utc_multipliers.len().max(1) as f64
}

fn unit_weekly_profile(bucket_count: usize) -> SurpriseNewsDeterministicIntensityProfile {
    SurpriseNewsDeterministicIntensityProfile {
        weekly_hour_utc_multipliers: vec![1.0; bucket_count],
        profile_resolution_hours:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS,
        fit_policy: RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_FIT_POLICY.to_string(),
        normalization_policy:
            RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_NORMALIZATION_POLICY.to_string(),
    }
}

fn exposure_hours_by_weekly_bucket(
    window_start_utc_nanos: i64,
    window_end_utc_nanos: i64,
    bucket_count: usize,
) -> Vec<f64> {
    let mut exposure_hours_by_bucket = vec![0.0; bucket_count];
    let mut current = DateTime::<Utc>::from_timestamp_nanos(window_start_utc_nanos);
    let window_end = DateTime::<Utc>::from_timestamp_nanos(window_end_utc_nanos);

    while current < window_end {
        let hour_start = current
            .with_minute(0)
            .and_then(|value| value.with_second(0))
            .and_then(|value| value.with_nanosecond(0))
            .unwrap_or(current);

        let next_hour_boundary = if current == hour_start {
            current + Duration::hours(1)
        } else {
            hour_start + Duration::hours(1)
        };

        let segment_end = next_hour_boundary.min(window_end);

        let segment_duration_hours = duration_nanos_to_hours(
            segment_end
                .timestamp_nanos_opt()
                .unwrap_or(window_end_utc_nanos)
                - current
                    .timestamp_nanos_opt()
                    .unwrap_or(window_start_utc_nanos),
        );

        let bucket_index = weekly_hour_bucket_index(
            current
                .timestamp_nanos_opt()
                .unwrap_or(window_start_utc_nanos),
        );

        exposure_hours_by_bucket[bucket_index] += segment_duration_hours.max(0.0);

        current = segment_end;
    }

    exposure_hours_by_bucket
}

fn weekly_hour_bucket_index(timestamp_utc_nanos: i64) -> usize {
    let timestamp_utc = DateTime::<Utc>::from_timestamp_nanos(timestamp_utc_nanos);
    timestamp_utc.weekday().num_days_from_monday() as usize * 24 + timestamp_utc.hour() as usize
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::super::derived_contracts::return_components_stage_surprise_intensity_weekly_profile_bucket_count;
    use super::{
        fit_surprise_news_arrival_layer, fit_surprise_news_deterministic_intensity_profile,
    };

    #[test]
    fn fitted_surprise_profile_is_exposure_weighted_mean_one() {
        let window_start_utc_nanos = Utc
            .with_ymd_and_hms(2025, 1, 6, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap();

        let window_end_utc_nanos = Utc
            .with_ymd_and_hms(2025, 1, 13, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap();

        let fitted_profile = fit_surprise_news_deterministic_intensity_profile(
            window_start_utc_nanos,
            window_end_utc_nanos,
            &[
                Utc.with_ymd_and_hms(2025, 1, 6, 9, 0, 0)
                    .single()
                    .unwrap()
                    .timestamp_nanos_opt()
                    .unwrap(),
                Utc.with_ymd_and_hms(2025, 1, 6, 9, 30, 0)
                    .single()
                    .unwrap()
                    .timestamp_nanos_opt()
                    .unwrap(),
                Utc.with_ymd_and_hms(2025, 1, 8, 15, 0, 0)
                    .single()
                    .unwrap()
                    .timestamp_nanos_opt()
                    .unwrap(),
            ],
        );

        assert_eq!(
            fitted_profile.weekly_hour_utc_multipliers.len(),
            return_components_stage_surprise_intensity_weekly_profile_bucket_count()
        );

        let weighted_mean = fitted_profile
            .weekly_hour_utc_multipliers
            .iter()
            .sum::<f64>()
            / fitted_profile.weekly_hour_utc_multipliers.len() as f64;

        assert!((weighted_mean - 1.0).abs() <= 1.0e-12);
        assert!(
            fitted_profile.weekly_hour_utc_multipliers[9]
                > fitted_profile.weekly_hour_utc_multipliers[8]
        );
    }

    #[test]
    fn absent_surprise_evidence_collapses_to_zero_baseline_and_unit_profile() {
        let window_start_utc_nanos = Utc
            .with_ymd_and_hms(2025, 1, 6, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap();

        let window_end_utc_nanos = Utc
            .with_ymd_and_hms(2025, 1, 13, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_nanos_opt()
            .unwrap();

        let fitted_layer = fit_surprise_news_arrival_layer(
            window_start_utc_nanos,
            window_end_utc_nanos,
            &[],
            0,
            1.0,
            1.0,
        );

        assert_eq!(fitted_layer.open_baseline_intensity_per_hour, 0.0);
        assert_eq!(fitted_layer.closed_baseline_intensity_per_hour, 0.0);
        assert!(
            fitted_layer
                .deterministic_intensity_profile
                .weekly_hour_utc_multipliers
                .iter()
                .all(|&multiplier| (multiplier - 1.0).abs() <= 1.0e-12)
        );
    }
}
