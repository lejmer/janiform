use super::{
    DynamicWorkPoolInput, DynamicWorkPoolStatus, DynamicWorkPoolSummary, run_dynamic_work_pool,
};
use janiform_foundations::cancellation::CancellationToken;

/// Inputs for a finite batch of independent work.
pub struct StaticWorkPoolInput<'scope, State, Job, Execute, Record> {
    state: &'scope mut State,
    cancellation_token: CancellationToken,
    jobs: Vec<Job>,
    worker_capacity: usize,
    execute_job: Execute,
    record_result: Record,
}

impl<'scope, State, Job, Execute, Record> StaticWorkPoolInput<'scope, State, Job, Execute, Record> {
    /// Build a finite work-pool input contract.
    pub fn new(
        state: &'scope mut State,
        cancellation_token: CancellationToken,
        jobs: Vec<Job>,
        worker_capacity: usize,
        execute_job: Execute,
        record_result: Record,
    ) -> Self {
        Self {
            state,
            cancellation_token,
            jobs,
            worker_capacity,
            execute_job,
            record_result,
        }
    }
}

struct StaticWorkPoolState<'state, State, Job> {
    caller_state: &'state mut State,
    pending_jobs: std::vec::IntoIter<Job>,
}

fn static_work_pool_never_stops<State, Job>(
    _state: &StaticWorkPoolState<'_, State, Job>,
    _status: DynamicWorkPoolStatus,
) -> bool {
    false
}

/// Run a finite batch of independent work on a bounded worker pool.
pub fn run_static_work_pool<'scope, State, Job, Output, Execute, Record>(
    thread_pool: Option<&'scope rayon::ThreadPool>,
    input: StaticWorkPoolInput<'scope, State, Job, Execute, Record>,
) -> DynamicWorkPoolSummary
where
    State: Send,
    Job: Send + 'scope,
    Output: Send + 'scope,
    Execute: Fn(Job, CancellationToken) -> Output + Send + Sync + 'scope,
    Record: FnMut(&mut State, Output, DynamicWorkPoolStatus) + Send,
{
    let StaticWorkPoolInput {
        state,
        cancellation_token,
        jobs,
        worker_capacity,
        execute_job,
        mut record_result,
    } = input;

    let mut pool_state = StaticWorkPoolState {
        caller_state: state,
        pending_jobs: jobs.into_iter(),
    };

    run_dynamic_work_pool(
        thread_pool,
        DynamicWorkPoolInput::new(
            &mut pool_state,
            cancellation_token,
            worker_capacity,
            |pool_state: &mut StaticWorkPoolState<'_, State, Job>, _status| {
                pool_state.pending_jobs.next()
            },
            execute_job,
            move |pool_state: &mut StaticWorkPoolState<'_, State, Job>, output, status| {
                record_result(pool_state.caller_state, output, status);
            },
            static_work_pool_never_stops::<State, Job>,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::StaticWorkPoolInput;
    use janiform_foundations::cancellation::CancellationToken;

    #[test]
    fn static_work_pool_records_outputs_as_jobs_complete() {
        let mut outputs = Vec::new();

        let summary = super::run_static_work_pool(
            None,
            StaticWorkPoolInput::new(
                &mut outputs,
                CancellationToken::uncancelled(),
                vec![1, 2, 3],
                2,
                |value, _cancellation_token| value * 10,
                |outputs: &mut Vec<i32>, output, _status| outputs.push(output),
            ),
        );

        outputs.sort_unstable();

        assert_eq!(summary.scheduled_work_count, 3);
        assert_eq!(summary.completed_work_count, 3);
        assert_eq!(outputs, vec![10, 20, 30]);
    }
}
