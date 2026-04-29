//! Shared parallel execution helpers.

/// Return the CPU count available to local parallel work.
pub fn available_cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

/// Resolve an optional user CPU budget to a strictly positive CPU count.
pub fn resolved_cpu_count(cpu_count: Option<usize>) -> usize {
    cpu_count.unwrap_or_else(available_cpu_count).max(1)
}
