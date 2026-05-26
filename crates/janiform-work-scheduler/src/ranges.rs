use std::ops::Range;

/// Split an item count into deterministic balanced half-open ranges.
pub fn balanced_work_ranges(item_count: usize, range_count: usize) -> Vec<Range<usize>> {
    if item_count == 0 {
        return Vec::new();
    }

    let bounded_range_count = range_count.max(1).min(item_count);
    let base_range_size = item_count / bounded_range_count;
    let extra_items = item_count % bounded_range_count;

    let mut ranges = Vec::with_capacity(bounded_range_count);
    let mut next_start = 0usize;

    for range_position in 0..bounded_range_count {
        let range_size = base_range_size + usize::from(range_position < extra_items);
        let next_end = next_start + range_size;

        ranges.push(next_start..next_end);

        next_start = next_end;
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::balanced_work_ranges;

    #[test]
    fn balanced_work_ranges_spreads_remainder_over_front_ranges() {
        assert_eq!(balanced_work_ranges(10, 3), vec![0..4, 4..7, 7..10]);
    }

    #[test]
    fn balanced_work_ranges_caps_range_count_to_item_count() {
        assert_eq!(balanced_work_ranges(2, 5), vec![0..1, 1..2]);
    }

    #[test]
    fn balanced_work_ranges_returns_empty_ranges_for_empty_work() {
        assert_eq!(
            balanced_work_ranges(0, 5),
            Vec::<std::ops::Range<usize>>::new()
        );
    }
}
