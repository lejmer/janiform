use std::sync::Arc;
use std::sync::mpsc::Sender;

/// Snapshot of bounded work-pool progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicWorkPoolStatus {
    /// Number of jobs handed to workers.
    pub scheduled_work_count: usize,
    /// Number of worker results received by the scheduler.
    pub completed_work_count: usize,
    /// Number of jobs currently running.
    pub in_flight_work_count: usize,
}

impl DynamicWorkPoolStatus {
    const fn empty() -> Self {
        Self {
            scheduled_work_count: 0,
            completed_work_count: 0,
            in_flight_work_count: 0,
        }
    }
}

/// Final bounded work-pool progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DynamicWorkPoolSummary {
    /// Number of jobs handed to workers.
    pub scheduled_work_count: usize,
    /// Number of worker results received by the scheduler.
    pub completed_work_count: usize,
}

/// Inputs for a dynamic worker pool.
pub struct DynamicWorkPoolInput<'scope, State, Job, Output, Next, Execute, Record, Stop> {
    state: &'scope mut State,
    worker_capacity: usize,
    next_job: Next,
    execute_job: Arc<Execute>,
    record_result: Record,
    should_stop_scheduling: Stop,
    _job: std::marker::PhantomData<Job>,
    _output: std::marker::PhantomData<Output>,
}

impl<'scope, State, Job, Output, Next, Execute, Record, Stop>
    DynamicWorkPoolInput<'scope, State, Job, Output, Next, Execute, Record, Stop>
{
    /// Build a dynamic work-pool input contract.
    pub fn new(
        state: &'scope mut State,
        worker_capacity: usize,
        next_job: Next,
        execute_job: Execute,
        record_result: Record,
        should_stop_scheduling: Stop,
    ) -> Self {
        Self {
            state,
            worker_capacity,
            next_job,
            execute_job: Arc::new(execute_job),
            record_result,
            should_stop_scheduling,
            _job: std::marker::PhantomData,
            _output: std::marker::PhantomData,
        }
    }
}

/// Run a dynamic worker pool.
pub fn run_dynamic_work_pool<'scope, State, Job, Output, Next, Execute, Record, Stop>(
    thread_pool: Option<&'scope rayon::ThreadPool>,
    input: DynamicWorkPoolInput<'scope, State, Job, Output, Next, Execute, Record, Stop>,
) -> DynamicWorkPoolSummary
where
    State: Send,
    Job: Send + 'scope,
    Output: Send + 'scope,
    Next: FnMut(&mut State, DynamicWorkPoolStatus) -> Option<Job> + Send,
    Execute: Fn(Job) -> Output + Send + Sync + 'scope,
    Record: FnMut(&mut State, Output, DynamicWorkPoolStatus) + Send,
    Stop: for<'state> Fn(&'state State, DynamicWorkPoolStatus) -> bool + Send,
{
    match thread_pool {
        Some(thread_pool) => thread_pool.scope(|scope| run_scoped_dynamic_work_pool(scope, input)),
        None => rayon::scope(|scope| run_scoped_dynamic_work_pool(scope, input)),
    }
}

fn run_scoped_dynamic_work_pool<'scope, State, Job, Output, Next, Execute, Record, Stop>(
    scope: &rayon::Scope<'scope>,
    mut input: DynamicWorkPoolInput<'scope, State, Job, Output, Next, Execute, Record, Stop>,
) -> DynamicWorkPoolSummary
where
    Job: Send + 'scope,
    Output: Send + 'scope,
    Next: FnMut(&mut State, DynamicWorkPoolStatus) -> Option<Job>,
    Execute: Fn(Job) -> Output + Send + Sync + 'scope,
    Record: FnMut(&mut State, Output, DynamicWorkPoolStatus),
    Stop: for<'state> Fn(&'state State, DynamicWorkPoolStatus) -> bool,
{
    let worker_capacity = input.worker_capacity.max(1);
    let (result_sender, result_receiver) = std::sync::mpsc::channel::<Output>();

    let mut status = DynamicWorkPoolStatus::empty();

    fill_worker_slots(
        scope,
        &mut input,
        &result_sender,
        worker_capacity,
        &mut status,
    );

    while status.in_flight_work_count > 0 {
        let Ok(output) = result_receiver.recv() else {
            break;
        };

        status.in_flight_work_count = status.in_flight_work_count.saturating_sub(1);
        status.completed_work_count = status.completed_work_count.saturating_add(1);

        (input.record_result)(input.state, output, status);

        fill_worker_slots(
            scope,
            &mut input,
            &result_sender,
            worker_capacity,
            &mut status,
        );
    }

    DynamicWorkPoolSummary {
        scheduled_work_count: status.scheduled_work_count,
        completed_work_count: status.completed_work_count,
    }
}

fn fill_worker_slots<'scope, State, Job, Output, Next, Execute, Record, Stop>(
    scope: &rayon::Scope<'scope>,
    input: &mut DynamicWorkPoolInput<'scope, State, Job, Output, Next, Execute, Record, Stop>,
    result_sender: &Sender<Output>,
    worker_capacity: usize,
    status: &mut DynamicWorkPoolStatus,
) where
    Job: Send + 'scope,
    Output: Send + 'scope,
    Next: FnMut(&mut State, DynamicWorkPoolStatus) -> Option<Job>,
    Execute: Fn(Job) -> Output + Send + Sync + 'scope,
    Record: FnMut(&mut State, Output, DynamicWorkPoolStatus),
    Stop: for<'state> Fn(&'state State, DynamicWorkPoolStatus) -> bool,
{
    while status.in_flight_work_count < worker_capacity
        && !(input.should_stop_scheduling)(input.state, *status)
    {
        let Some(job) = (input.next_job)(input.state, *status) else {
            break;
        };

        status.scheduled_work_count = status.scheduled_work_count.saturating_add(1);
        status.in_flight_work_count = status.in_flight_work_count.saturating_add(1);

        let execute_job = Arc::clone(&input.execute_job);

        // Each worker owns a sender handle so completed work can stream back independently.
        let result_sender = result_sender.clone();

        scope.spawn(move |_| {
            let output = execute_job(job);
            let _ = result_sender.send(output);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{DynamicWorkPoolInput, DynamicWorkPoolStatus};

    #[derive(Debug)]
    struct TestState {
        next_work: usize,
        total_work: usize,
        completed_work: Vec<usize>,
    }

    fn never_stop_test_work_pool(_state: &TestState, _status: DynamicWorkPoolStatus) -> bool {
        false
    }

    fn stop_test_work_pool_after_three_scheduled(
        _state: &TestState,
        status: DynamicWorkPoolStatus,
    ) -> bool {
        status.scheduled_work_count >= 3
    }

    #[test]
    fn dynamic_work_pool_refills_slots_as_results_complete() {
        let mut state = TestState {
            next_work: 0,
            total_work: 5,
            completed_work: Vec::new(),
        };

        let summary = super::run_dynamic_work_pool(
            None,
            DynamicWorkPoolInput::new(
                &mut state,
                2,
                |state: &mut TestState, _status| {
                    if state.next_work >= state.total_work {
                        return None;
                    }

                    let work = state.next_work;
                    state.next_work = state.next_work.saturating_add(1);

                    Some(work)
                },
                |work| work,
                |state: &mut TestState, output, _status| state.completed_work.push(output),
                never_stop_test_work_pool,
            ),
        );

        state.completed_work.sort_unstable();

        assert_eq!(summary.scheduled_work_count, 5);
        assert_eq!(summary.completed_work_count, 5);
        assert_eq!(state.completed_work, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn dynamic_work_pool_stops_scheduling_and_drains_in_flight_work() {
        let mut state = TestState {
            next_work: 0,
            total_work: 5,
            completed_work: Vec::new(),
        };

        let summary = super::run_dynamic_work_pool(
            None,
            DynamicWorkPoolInput::new(
                &mut state,
                2,
                |state: &mut TestState, _status| {
                    if state.next_work >= state.total_work {
                        return None;
                    }

                    let work = state.next_work;
                    state.next_work = state.next_work.saturating_add(1);

                    Some(work)
                },
                |work| work,
                |state: &mut TestState, output, _status| state.completed_work.push(output),
                stop_test_work_pool_after_three_scheduled,
            ),
        );

        state.completed_work.sort_unstable();

        assert_eq!(summary.scheduled_work_count, 3);
        assert_eq!(summary.completed_work_count, 3);
        assert_eq!(state.completed_work, vec![0, 1, 2]);
    }
}
