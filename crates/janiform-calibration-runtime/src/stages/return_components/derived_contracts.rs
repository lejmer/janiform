use janiform_foundations::numeric::strictly_positive_machine_value;

use super::constants::RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS;

const RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_HOURS_PER_WEEK: usize = 7 * 24;

pub(super) fn return_components_stage_positive_domain_floor() -> f64 {
    strictly_positive_machine_value()
}

pub(super) const fn return_components_stage_surprise_intensity_weekly_profile_bucket_count() -> usize
{
    RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_HOURS_PER_WEEK
        / RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_RESOLUTION_HOURS
}

pub(super) const fn return_components_stage_surprise_intensity_weekly_profile_bucket_count_for_resolution(
    profile_resolution_hours: usize,
) -> Option<usize> {
    if profile_resolution_hours == 0
        || !RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_HOURS_PER_WEEK
            .is_multiple_of(profile_resolution_hours)
    {
        return None;
    }

    Some(
        RETURN_COMPONENTS_STAGE_SURPRISE_INTENSITY_PROFILE_HOURS_PER_WEEK
            / profile_resolution_hours,
    )
}
