use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use janiform_foundations::cancellation::CancellationToken;

/// Inputs for a finite batch of independent work that can emit progress.
pub struct StreamingWorkPoolInput<'scope, State, Job, Execute, RecordProgress, RecordOutput> {
    state: &'scope mut State,
    cancellation_token: CancellationToken,
    jobs: Vec<Job>,
    worker_capacity: usize,
    execute_job: Execute,
    record_progress: RecordProgress,
    record_output: RecordOutput,
}

impl<'scope, State, Job, Execute, RecordProgress, RecordOutput>
    StreamingWorkPoolInput<'scope, State, Job, Execute, RecordProgress, RecordOutput>
{
    /// Build a streaming work-pool input contract.
    pub fn new(
        state: &'scope mut State,
        cancellation_token: CancellationToken,
        jobs: Vec<Job>,
        worker_capacity: usize,
        execute_job: Execute,
        record_progress: RecordProgress,
        record_output: RecordOutput,
    ) -> Self {
        Self {
            state,
            cancellation_token,
            jobs,
            worker_capacity,
            execute_job,
            record_progress,
            record_output,
        }
    }
}

/// Worker-side context for reporting progress from a streaming job.
pub struct StreamingWorkContext<Progress, Output> {
    worker_index: usize,
    job_position: usize,
    cancellation_token: CancellationToken,
    sender: Sender<StreamingWorkPoolMessage<Progress, Output>>,
}

impl<Progress, Output> StreamingWorkContext<Progress, Output> {
    /// Stable worker lane assigned to the running job.
    pub const fn worker_index(&self) -> usize {
        self.worker_index
    }

    /// Original job position in the submitted batch.
    pub const fn job_position(&self) -> usize {
        self.job_position
    }

    /// Cooperative cancellation token for the running job.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.shared_handle()
    }

    /// Send a progress update to the caller.
    pub fn send_progress(&self, progress: Progress) -> bool {
        if self.cancellation_token.is_cancelled() {
            return false;
        }

        self.sender
            .send(StreamingWorkPoolMessage::Progress(StreamingWorkProgress {
                worker_index: self.worker_index,
                job_position: self.job_position,
                progress,
            }))
            .is_ok()
    }
}

/// Progress emitted by a streaming worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingWorkProgress<Progress> {
    /// Stable worker lane that emitted the progress update.
    pub worker_index: usize,
    /// Original job position in the submitted batch.
    pub job_position: usize,
    /// Caller-owned progress payload.
    pub progress: Progress,
}

/// Output emitted when a streaming worker completes a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamingWorkOutput<Output> {
    /// Stable worker lane that completed the job.
    pub worker_index: usize,
    /// Original job position in the submitted batch.
    pub job_position: usize,
    /// Caller-owned output payload.
    pub output: Output,
}

/// Final streaming work-pool progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamingWorkPoolSummary {
    /// Number of jobs submitted to the work pool.
    pub scheduled_work_count: usize,
    /// Number of completed job outputs received by the caller.
    pub completed_work_count: usize,
}

enum StreamingWorkPoolMessage<Progress, Output> {
    Progress(StreamingWorkProgress<Progress>),
    Completed(StreamingWorkOutput<Output>),
}

/// Run a finite batch of independent streaming jobs on a bounded worker pool.
pub fn run_streaming_work_pool<
    'scope,
    State,
    Job,
    Progress,
    Output,
    Execute,
    RecordProgress,
    RecordOutput,
    Error,
>(
    input: StreamingWorkPoolInput<'scope, State, Job, Execute, RecordProgress, RecordOutput>,
) -> Result<StreamingWorkPoolSummary, Error>
where
    Job: Send + 'scope,
    Progress: Send + 'scope,
    Output: Send + 'scope,
    Execute: Fn(Job, StreamingWorkContext<Progress, Output>) -> Output + Sync + 'scope,
    RecordProgress: FnMut(&mut State, StreamingWorkProgress<Progress>) -> Result<(), Error>,
    RecordOutput: FnMut(&mut State, StreamingWorkOutput<Output>) -> Result<(), Error>,
{
    let StreamingWorkPoolInput {
        state,
        cancellation_token,
        jobs,
        worker_capacity,
        execute_job,
        mut record_progress,
        mut record_output,
    } = input;

    let submitted_work_count = jobs.len();
    if submitted_work_count == 0 {
        return Ok(StreamingWorkPoolSummary {
            scheduled_work_count: 0,
            completed_work_count: 0,
        });
    }

    let worker_count = worker_capacity.max(1).min(submitted_work_count);
    let pending_jobs = Arc::new(Mutex::new(jobs.into_iter().enumerate().collect()));
    let scheduled_work_count = Arc::new(AtomicUsize::new(0));

    let (sender, receiver) = channel::<StreamingWorkPoolMessage<Progress, Output>>();

    let completed_work_count = std::thread::scope(|scope| {
        for worker_index in 0..worker_count {
            let pending_jobs = Arc::clone(&pending_jobs);
            let scheduled_work_count = Arc::clone(&scheduled_work_count);
            let cancellation_token = cancellation_token.shared_handle();

            let sender = sender.clone();
            let execute_job = &execute_job;

            scope.spawn(move || {
                run_streaming_worker(
                    worker_index,
                    pending_jobs,
                    scheduled_work_count,
                    cancellation_token,
                    sender,
                    execute_job,
                );
            });
        }

        drop(sender);

        collect_streaming_work_messages(state, receiver, &mut record_progress, &mut record_output)
    })?;

    Ok(StreamingWorkPoolSummary {
        scheduled_work_count: scheduled_work_count.load(Ordering::SeqCst),
        completed_work_count,
    })
}

fn run_streaming_worker<'scope, Job, Progress, Output, Execute>(
    worker_index: usize,
    pending_jobs: Arc<Mutex<VecDeque<(usize, Job)>>>,
    scheduled_work_count: Arc<AtomicUsize>,
    cancellation_token: CancellationToken,
    sender: Sender<StreamingWorkPoolMessage<Progress, Output>>,
    execute_job: &Execute,
) where
    Job: Send + 'scope,
    Progress: Send + 'scope,
    Output: Send + 'scope,
    Execute: Fn(Job, StreamingWorkContext<Progress, Output>) -> Output,
{
    loop {
        if cancellation_token.is_cancelled() {
            return;
        }

        let Some((job_position, job)) = pop_next_streaming_job(&pending_jobs) else {
            return;
        };

        scheduled_work_count.fetch_add(1, Ordering::SeqCst);

        let context = StreamingWorkContext {
            worker_index,
            job_position,
            cancellation_token: cancellation_token.shared_handle(),
            sender: sender.clone(),
        };

        let output = execute_job(job, context);
        if sender
            .send(StreamingWorkPoolMessage::Completed(StreamingWorkOutput {
                worker_index,
                job_position,
                output,
            }))
            .is_err()
        {
            return;
        }
    }
}

fn pop_next_streaming_job<Job>(
    pending_jobs: &Mutex<VecDeque<(usize, Job)>>,
) -> Option<(usize, Job)> {
    let Ok(mut guard) = pending_jobs.lock() else {
        return None;
    };

    guard.pop_front()
}

fn collect_streaming_work_messages<State, Progress, Output, RecordProgress, RecordOutput, Error>(
    state: &mut State,
    receiver: std::sync::mpsc::Receiver<StreamingWorkPoolMessage<Progress, Output>>,
    record_progress: &mut RecordProgress,
    record_output: &mut RecordOutput,
) -> Result<usize, Error>
where
    RecordProgress: FnMut(&mut State, StreamingWorkProgress<Progress>) -> Result<(), Error>,
    RecordOutput: FnMut(&mut State, StreamingWorkOutput<Output>) -> Result<(), Error>,
{
    let mut completed_work_count = 0usize;

    for message in receiver {
        match message {
            StreamingWorkPoolMessage::Progress(progress) => record_progress(state, progress)?,
            StreamingWorkPoolMessage::Completed(output) => {
                completed_work_count = completed_work_count.saturating_add(1);
                record_output(state, output)?;
            }
        }
    }

    Ok(completed_work_count)
}

#[cfg(test)]
mod tests {
    use super::{
        StreamingWorkContext, StreamingWorkOutput, StreamingWorkPoolInput, StreamingWorkProgress,
        run_streaming_work_pool,
    };
    use janiform_foundations::cancellation::CancellationSource;
    use janiform_foundations::cancellation::CancellationToken;

    #[derive(Debug, Default)]
    struct TestState {
        progress: Vec<(usize, usize, i32)>,
        outputs: Vec<(usize, usize, i32)>,
    }

    #[test]
    fn streaming_work_pool_records_progress_and_outputs() {
        let mut state = TestState::default();

        let summary = run_streaming_work_pool(StreamingWorkPoolInput::new(
            &mut state,
            CancellationToken::uncancelled(),
            vec![1, 2, 3],
            2,
            |value, context: StreamingWorkContext<i32, i32>| {
                assert!(context.worker_index() < 2);
                assert!(!context.cancellation_token().is_cancelled());
                assert!(context.send_progress(value));

                value * 10
            },
            |state: &mut TestState, progress: StreamingWorkProgress<i32>| -> Result<(), String> {
                state.progress.push((
                    progress.worker_index,
                    progress.job_position,
                    progress.progress,
                ));

                Ok(())
            },
            |state: &mut TestState, output: StreamingWorkOutput<i32>| -> Result<(), String> {
                state
                    .outputs
                    .push((output.worker_index, output.job_position, output.output));

                Ok(())
            },
        ))
        .expect("streaming work pool should complete");

        state
            .progress
            .sort_unstable_by_key(|(_, job_position, _)| *job_position);
        state
            .outputs
            .sort_unstable_by_key(|(_, job_position, _)| *job_position);

        let progress_by_job = state
            .progress
            .iter()
            .map(|(_, job_position, value)| (*job_position, *value))
            .collect::<Vec<_>>();

        let output_by_job = state
            .outputs
            .iter()
            .map(|(_, job_position, value)| (*job_position, *value))
            .collect::<Vec<_>>();

        assert_eq!(summary.scheduled_work_count, 3);
        assert_eq!(summary.completed_work_count, 3);
        assert!(
            state
                .progress
                .iter()
                .all(|(worker_index, _, _)| *worker_index < 2)
        );
        assert!(
            state
                .outputs
                .iter()
                .all(|(worker_index, _, _)| *worker_index < 2)
        );
        assert_eq!(progress_by_job, vec![(0, 1), (1, 2), (2, 3)]);
        assert_eq!(output_by_job, vec![(0, 10), (1, 20), (2, 30)]);
    }

    #[test]
    fn streaming_work_pool_accepts_empty_work() {
        let mut state = TestState::default();

        let summary = run_streaming_work_pool(StreamingWorkPoolInput::new(
            &mut state,
            CancellationToken::uncancelled(),
            Vec::<i32>::new(),
            2,
            |value, _context: StreamingWorkContext<i32, i32>| value,
            |_state: &mut TestState, _progress: StreamingWorkProgress<i32>| -> Result<(), String> {
                Ok(())
            },
            |_state: &mut TestState, _output: StreamingWorkOutput<i32>| -> Result<(), String> {
                Ok(())
            },
        ))
        .expect("empty streaming work pool should complete");

        assert_eq!(summary.scheduled_work_count, 0);
        assert_eq!(summary.completed_work_count, 0);
        assert!(state.progress.is_empty());
        assert!(state.outputs.is_empty());
    }

    #[test]
    fn streaming_work_pool_propagates_record_errors() {
        let mut state = TestState::default();

        let result = run_streaming_work_pool(StreamingWorkPoolInput::new(
            &mut state,
            CancellationToken::uncancelled(),
            vec![1],
            1,
            |value, context: StreamingWorkContext<i32, i32>| {
                assert!(context.send_progress(value));

                value
            },
            |_state: &mut TestState, _progress: StreamingWorkProgress<i32>| -> Result<(), String> {
                Err("progress failed".to_string())
            },
            |_state: &mut TestState, _output: StreamingWorkOutput<i32>| -> Result<(), String> {
                Ok(())
            },
        ));

        assert_eq!(result, Err("progress failed".to_string()));
    }

    #[test]
    fn streaming_work_pool_does_not_schedule_when_cancelled() {
        let mut state = TestState::default();

        let cancellation_source = CancellationSource::new();
        cancellation_source.cancel();

        let summary = run_streaming_work_pool(StreamingWorkPoolInput::new(
            &mut state,
            cancellation_source.token(),
            vec![1, 2, 3],
            2,
            |value, _context: StreamingWorkContext<i32, i32>| value,
            |_state: &mut TestState, _progress: StreamingWorkProgress<i32>| -> Result<(), String> {
                Ok(())
            },
            |_state: &mut TestState, _output: StreamingWorkOutput<i32>| -> Result<(), String> {
                Ok(())
            },
        ))
        .expect("cancelled streaming work pool should drain cleanly");

        assert_eq!(summary.scheduled_work_count, 0);
        assert_eq!(summary.completed_work_count, 0);
        assert_eq!(state.progress, Vec::<(usize, usize, i32)>::new());
        assert_eq!(state.outputs, Vec::<(usize, usize, i32)>::new());
    }
}
