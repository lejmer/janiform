use janiform_foundations::time::format_duration_spec_nanos;

pub(super) fn simulation_interval_unit_label(interval_duration_nanos: i64) -> String {
    format_duration_spec_nanos(interval_duration_nanos)
        .unwrap_or_else(|| format!("{interval_duration_nanos} ns"))
}

pub(super) fn simulation_interval_progress_label(
    completed_interval_count: usize,
    total_interval_count: usize,
    interval_duration_nanos: i64,
) -> String {
    format!(
        "{completed_interval_count}/{total_interval_count} {} intervals",
        simulation_interval_unit_label(interval_duration_nanos)
    )
}

pub(super) fn simulation_interval_count_label(
    interval_count: usize,
    interval_duration_nanos: Option<i64>,
) -> String {
    match interval_duration_nanos {
        Some(interval_duration_nanos) => {
            let interval_unit = if interval_count == 1 {
                "interval"
            } else {
                "intervals"
            };

            format!(
                "{interval_count} {} {interval_unit}",
                simulation_interval_unit_label(interval_duration_nanos),
            )
        }
        None => interval_count.to_string(),
    }
}

pub(super) fn interval_duration_nanos_from_bounds(
    start_time_utc_nanos: i64,
    end_time_utc_nanos: i64,
    interval_count: usize,
) -> Option<i64> {
    let interval_count = i64::try_from(interval_count).ok()?;
    if interval_count <= 0 {
        return None;
    }

    let total_duration_nanos = end_time_utc_nanos.checked_sub(start_time_utc_nanos)?;
    if total_duration_nanos <= 0 || total_duration_nanos % interval_count != 0 {
        return None;
    }

    Some(total_duration_nanos / interval_count)
}
