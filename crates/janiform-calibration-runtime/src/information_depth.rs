pub(crate) fn support_information_depth(support_count: usize) -> usize {
    usize::try_from(support_count.max(1).ilog2())
        .unwrap_or(usize::MAX)
        .saturating_add(1)
}
