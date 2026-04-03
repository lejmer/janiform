use rayon::{ThreadPool, ThreadPoolBuilder};

pub(crate) fn resolved_pricing_cpu_count(cpu_count: Option<usize>) -> usize {
    cpu_count.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from))
}

pub(crate) fn build_pricing_thread_pool(cpu_count: Option<usize>) -> Option<ThreadPool> {
    let resolved_cpu_count = resolved_pricing_cpu_count(cpu_count);

    if resolved_cpu_count <= 1 {
        return None;
    }

    ThreadPoolBuilder::new()
        .num_threads(resolved_cpu_count)
        .build()
        .ok()
}

pub(crate) fn install_pricing_parallelism<T: Send>(
    thread_pool: Option<&ThreadPool>,
    body: impl FnOnce() -> T + Send,
) -> T {
    match thread_pool {
        Some(thread_pool) => thread_pool.install(body),
        None => body(),
    }
}
