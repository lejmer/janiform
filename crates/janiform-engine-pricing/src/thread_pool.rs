use rayon::ThreadPoolBuilder;

pub(crate) fn with_pricing_thread_pool<T: Send>(
    cpu_count: Option<usize>,
    body: impl FnOnce() -> T + Send,
) -> T {
    let resolved_cpu_count =
        cpu_count.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));

    if resolved_cpu_count <= 1 {
        return body();
    }

    match ThreadPoolBuilder::new()
        .num_threads(resolved_cpu_count)
        .build()
    {
        Ok(thread_pool) => thread_pool.install(body),
        Err(_) => body(),
    }
}
