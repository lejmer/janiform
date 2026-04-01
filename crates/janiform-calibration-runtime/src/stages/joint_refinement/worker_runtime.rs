use std::env;
use std::fs;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Condvar, Mutex};

use sysinfo::{Pid, ProcessesToUpdate, System};

use super::execution::JointRefinementSeedSummary;
use super::summary_simulation::{
    CloseSessionRequest, InitializeSessionRequest, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION,
    JointRefinementSummarySimulationSessionContext, JointRefinementSummarySimulationSessionState,
    JointRefinementSummarySimulationWorkerRequest, JointRefinementSummarySimulationWorkerResponse,
    JointRefinementWorkerRequestedOutput, RunCandidateRequest, RunCandidateResponse,
    initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
    simulate_joint_refinement_seed_summary_in_process, write_joint_refinement_worker_frame,
};
use janiform_calibration_artifacts::JointRefinementStageFittedParameters;

const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV: &str =
    "JANIFORM_JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_EXECUTABLE_ENV: &str =
    "JANIFORM_JOINT_REFINEMENT_WORKER_EXECUTABLE";

const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_IN_PROCESS: &str = "in_process";
const JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_PROCESS_WORKER: &str = "process_worker";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum JointRefinementSeedSummaryAcquisitionError {
    SimulationFailure { error_message: String },
    InfrastructureFailure { error_message: String },
}

#[derive(Debug)]
pub(crate) struct JointRefinementSummarySimulationScheduler {
    cpu_count: usize,
    backend: JointRefinementSummarySimulationBackend,
    session_context: JointRefinementSummarySimulationSessionContext,
    session_state: JointRefinementSummarySimulationSessionState,
    state: Mutex<JointRefinementSummarySimulationSchedulerState>,
    condvar: Condvar,
}

#[derive(Debug, Clone)]
enum JointRefinementSummarySimulationBackend {
    InProcess,
    ProcessWorker { worker_executable: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JointRefinementSummarySimulationBackendPreference {
    Auto,
    InProcess,
    ProcessWorker,
}

#[derive(Debug)]
struct JointRefinementSummarySimulationSchedulerState {
    active_job_count: usize,
    total_worker_count: usize,
    idle_workers: Vec<PersistentSummarySimulationWorker>,
    worker_baseline_resident_bytes: u64,
    worker_initialized_session_resident_bytes: u64,
}

impl JointRefinementSummarySimulationScheduler {
    pub(crate) fn new(
        cpu_count: usize,
        session_context: &JointRefinementSummarySimulationSessionContext,
    ) -> Result<Self, String> {
        let session_state =
            initialize_joint_refinement_summary_simulation_session(session_context)?;

        let backend = resolve_summary_simulation_backend()?;

        let (
            idle_workers,
            total_worker_count,
            worker_baseline_resident_bytes,
            worker_initialized_session_resident_bytes,
        ) = match &backend {
            JointRefinementSummarySimulationBackend::InProcess => (Vec::new(), 0, 0, 0),
            JointRefinementSummarySimulationBackend::ProcessWorker { worker_executable } => {
                let (
                    initial_worker,
                    worker_baseline_resident_bytes,
                    worker_initialized_resident_bytes,
                ) = spawn_initialized_worker(worker_executable.as_path(), session_context)?;
                (
                    vec![initial_worker],
                    1,
                    worker_baseline_resident_bytes,
                    worker_initialized_resident_bytes
                        .saturating_sub(worker_baseline_resident_bytes),
                )
            }
        };

        Ok(Self {
            cpu_count: cpu_count.max(1),
            backend,
            session_context: session_context.clone(),
            session_state,
            state: Mutex::new(JointRefinementSummarySimulationSchedulerState {
                active_job_count: 0,
                total_worker_count,
                idle_workers,
                worker_baseline_resident_bytes,
                worker_initialized_session_resident_bytes,
            }),
            condvar: Condvar::new(),
        })
    }

    pub(crate) fn run_seed_summary_simulation(
        &self,
        fitted_parameters: &JointRefinementStageFittedParameters,
        requested_seed: u64,
    ) -> Result<JointRefinementSeedSummary, JointRefinementSeedSummaryAcquisitionError> {
        if matches!(
            &self.backend,
            JointRefinementSummarySimulationBackend::InProcess
        ) {
            return simulate_joint_refinement_seed_summary_in_process(
                &self.session_state,
                fitted_parameters,
                requested_seed,
            )
            .map(|(summary, _)| summary)
            .map_err(|error_message| {
                JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message }
            });
        }

        let request = RunCandidateRequest {
            schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                .to_string(),
            fitted_parameters: fitted_parameters.clone(),
            requested_seed,
            requested_output: JointRefinementWorkerRequestedOutput::ExactSummaryOnly,
        };

        let response = self.run_candidate_request(&request)?;
        match response {
            RunCandidateResponse::ExactSummaryOnly { seed_summary } => Ok(seed_summary.summary),
            RunCandidateResponse::ExactSummaryWithDiagnostics { seed_summary, .. } => {
                Ok(seed_summary.summary)
            }
            RunCandidateResponse::SimulationFailure { error_message } => {
                Err(JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message })
            }
            RunCandidateResponse::InfrastructureFailure { error_message } => Err(
                JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure { error_message },
            ),
        }
    }

    pub(crate) fn run_candidate_request(
        &self,
        request: &RunCandidateRequest,
    ) -> Result<RunCandidateResponse, JointRefinementSeedSummaryAcquisitionError> {
        let dynamic_memory_estimate =
            super::summary_simulation::estimate_summary_simulation_memory(
                &self.session_state,
                &request.fitted_parameters,
                request.requested_seed,
            )
            .map_err(|error_message| {
                JointRefinementSeedSummaryAcquisitionError::SimulationFailure { error_message }
            })?;

        let mut worker =
            self.acquire_worker(dynamic_memory_estimate.estimated_total_retained_bytes as u64)?;
        let run_result = worker.run_candidate(request);

        let reusable = matches!(
            &run_result,
            Ok(RunCandidateResponse::ExactSummaryOnly { .. })
                | Ok(RunCandidateResponse::ExactSummaryWithDiagnostics { .. })
                | Ok(RunCandidateResponse::SimulationFailure { .. })
        );

        self.release_worker(worker, reusable);

        run_result
    }

    fn acquire_worker(
        &self,
        estimated_dynamic_retained_bytes: u64,
    ) -> Result<PersistentSummarySimulationWorker, JointRefinementSeedSummaryAcquisitionError> {
        let mut state_guard = self.state.lock().map_err(|_| {
            JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                error_message: "summary simulation scheduler mutex is poisoned".to_string(),
            }
        })?;

        loop {
            if state_guard.active_job_count < self.cpu_count
                && let Some(worker) = state_guard.idle_workers.pop()
            {
                state_guard.active_job_count += 1;
                return Ok(worker);
            }

            let available_memory_bytes =
                current_available_memory_bytes().map_err(|error_message| {
                    JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                        error_message,
                    }
                })?;
            let worker_spawn_required_bytes = state_guard
                .worker_baseline_resident_bytes
                .saturating_add(state_guard.worker_initialized_session_resident_bytes)
                .saturating_add(estimated_dynamic_retained_bytes);

            if state_guard.active_job_count < self.cpu_count
                && state_guard.total_worker_count < self.cpu_count
                && available_memory_bytes >= worker_spawn_required_bytes
            {
                let JointRefinementSummarySimulationBackend::ProcessWorker { worker_executable } =
                    &self.backend
                else {
                    return Err(
                        JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                            error_message:
                                "summary simulation worker acquisition requested for in-process backend"
                                    .to_string(),
                        },
                    );
                };

                match spawn_initialized_worker(worker_executable.as_path(), &self.session_context) {
                    Ok((worker, baseline, initialized)) => {
                        state_guard.active_job_count += 1;
                        state_guard.total_worker_count += 1;
                        state_guard.worker_baseline_resident_bytes =
                            state_guard.worker_baseline_resident_bytes.max(baseline);
                        state_guard.worker_initialized_session_resident_bytes = state_guard
                            .worker_initialized_session_resident_bytes
                            .max(initialized.saturating_sub(baseline));

                        return Ok(worker);
                    }
                    Err(error_message) => {
                        if state_guard.total_worker_count == 0 {
                            return Err(
                                JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                                    error_message,
                                },
                            );
                        }
                    }
                }
            }

            state_guard = self.condvar.wait(state_guard).map_err(|_| {
                JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                    error_message: "summary simulation scheduler wait mutex is poisoned"
                        .to_string(),
                }
            })?;
        }
    }

    fn release_worker(&self, worker: PersistentSummarySimulationWorker, reusable: bool) {
        if let Ok(mut state_guard) = self.state.lock() {
            state_guard.active_job_count = state_guard.active_job_count.saturating_sub(1);

            if reusable {
                state_guard.idle_workers.push(worker);
            } else {
                state_guard.total_worker_count = state_guard.total_worker_count.saturating_sub(1);
            }

            self.condvar.notify_all();
        }
    }
}

#[derive(Debug)]
struct PersistentSummarySimulationWorker {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    stderr: BufReader<ChildStderr>,
}

impl PersistentSummarySimulationWorker {
    fn spawn(worker_executable: &Path) -> Result<Self, String> {
        let mut child = Command::new(worker_executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| format!("failed to spawn summary simulation worker: {source}"))?;

        let Some(stdin) = child.stdin.take() else {
            return Err("summary simulation worker stdin is unavailable".to_string());
        };
        let Some(stdout) = child.stdout.take() else {
            return Err("summary simulation worker stdout is unavailable".to_string());
        };
        let Some(stderr) = child.stderr.take() else {
            return Err("summary simulation worker stderr is unavailable".to_string());
        };

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            stderr: BufReader::new(stderr),
        })
    }

    fn initialize_session(
        &mut self,
        session_context: &JointRefinementSummarySimulationSessionContext,
    ) -> Result<(u64, u64), String> {
        let worker_response = self.send_request(
            &JointRefinementSummarySimulationWorkerRequest::InitializeSession(
                InitializeSessionRequest {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                    session_context: session_context.clone(),
                },
            ),
            "summary simulation worker returned malformed initialize-session output",
        )?;

        match worker_response {
            JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                schema_version,
                response,
            } => {
                if schema_version
                    != JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                {
                    return Err(format!(
                        "unexpected summary simulation worker initialize-session schema version: {schema_version}"
                    ));
                }

                match response {
                    InitializeSessionResponse::Success {
                        worker_baseline_resident_bytes,
                        worker_initialized_resident_bytes,
                    } => Ok((
                        worker_baseline_resident_bytes,
                        worker_initialized_resident_bytes,
                    )),
                    InitializeSessionResponse::InfrastructureFailure { error_message } => {
                        Err(error_message)
                    }
                }
            }
            _ => Err(
                "summary simulation worker returned a non-initialize-session response for initialize-session"
                    .to_string(),
            ),
        }
    }

    fn run_candidate(
        &mut self,
        request: &RunCandidateRequest,
    ) -> Result<RunCandidateResponse, JointRefinementSeedSummaryAcquisitionError> {
        let worker_response = self
            .send_request(
                &JointRefinementSummarySimulationWorkerRequest::RunCandidate(request.clone()),
                "summary simulation worker returned malformed run-candidate output",
            )
            .map_err(|error_message| {
                JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure { error_message }
            })?;

        match worker_response {
            JointRefinementSummarySimulationWorkerResponse::RunCandidate {
                schema_version,
                response,
            } => {
                if schema_version
                    != JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                {
                    return Err(
                        JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                            error_message: format!(
                                "unexpected summary simulation worker run-candidate schema version: {schema_version}"
                            ),
                        },
                    );
                }

                Ok(response)
            }
            _ => Err(
                JointRefinementSeedSummaryAcquisitionError::InfrastructureFailure {
                    error_message:
                        "summary simulation worker returned a non-run-candidate response for run-candidate"
                            .to_string(),
                },
            ),
        }
    }

    fn close_session(&mut self) {
        let close_result = self.send_request(
            &JointRefinementSummarySimulationWorkerRequest::CloseSession(CloseSessionRequest {
                schema_version: JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                    .to_string(),
            }),
            "summary simulation worker returned malformed close-session output",
        );

        if let Ok(JointRefinementSummarySimulationWorkerResponse::CloseSession {
            schema_version,
            ..
        }) = close_result
            && schema_version == JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
        {
            return;
        }
    }

    fn send_request(
        &mut self,
        worker_request: &JointRefinementSummarySimulationWorkerRequest,
        malformed_output_context: &str,
    ) -> Result<JointRefinementSummarySimulationWorkerResponse, String> {
        write_joint_refinement_worker_frame(&mut self.stdin, worker_request).map_err(|source| {
            format!("failed to write summary simulation worker request: {source}")
        })?;

        read_joint_refinement_worker_frame(&mut self.stdout)
            .map_err(|source| {
                self.decorate_worker_protocol_error(format!("{malformed_output_context}: {source}"))
            })?
            .ok_or_else(|| {
                self.decorate_worker_protocol_error(format!(
                    "{malformed_output_context}: worker produced no response"
                ))
            })
    }

    fn decorate_worker_protocol_error(&mut self, fallback_message: String) -> String {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let mut stderr_message = String::new();
                let _ = self.stderr.read_to_string(&mut stderr_message);

                format!(
                    "summary simulation worker exited abnormally: status={} stderr={}",
                    status,
                    stderr_message.trim(),
                )
            }
            Ok(None) | Err(_) => fallback_message,
        }
    }
}

impl Drop for PersistentSummarySimulationWorker {
    fn drop(&mut self) {
        self.close_session();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_initialized_worker(
    worker_executable: &Path,
    session_context: &JointRefinementSummarySimulationSessionContext,
) -> Result<(PersistentSummarySimulationWorker, u64, u64), String> {
    let mut worker = PersistentSummarySimulationWorker::spawn(worker_executable)?;
    let (baseline, initialized) = worker.initialize_session(session_context)?;

    Ok((worker, baseline, initialized))
}

fn resolve_summary_simulation_backend() -> Result<JointRefinementSummarySimulationBackend, String> {
    let worker_executable = resolve_optional_worker_executable_path()?;
    let backend_preference = resolve_summary_simulation_backend_preference()?;

    resolve_backend_from_preference(backend_preference, worker_executable)
}

fn resolve_optional_worker_executable_path() -> Result<Option<PathBuf>, String> {
    if let Some(path) = env::var_os(JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_EXECUTABLE_ENV) {
        let candidate = PathBuf::from(path);

        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        return Err(format!(
            "summary simulation worker executable path does not exist: {}",
            candidate.display()
        ));
    }

    Ok(discover_worker_executable_path())
}

fn discover_worker_executable_path() -> Option<PathBuf> {
    let executable_name = worker_executable_file_name();
    let current_executable = env::current_exe().ok();
    let search_roots = candidate_search_roots(current_executable.as_deref());

    for root in search_roots {
        let candidate = root.join(executable_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn unresolved_worker_executable_error_message() -> String {
    let executable_name = worker_executable_file_name();

    match env::current_exe() {
        Ok(current_executable) => format!(
            "unable to resolve summary simulation worker executable `{}` from {}",
            executable_name,
            current_executable.display()
        ),
        Err(source) => format!(
            "unable to resolve current executable while locating summary simulation worker executable `{}`: {}",
            executable_name, source
        ),
    }
}

fn resolve_summary_simulation_backend_preference()
-> Result<JointRefinementSummarySimulationBackendPreference, String> {
    match env::var(JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_ENV) {
        Ok(value) if value == JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_IN_PROCESS => {
            Ok(JointRefinementSummarySimulationBackendPreference::InProcess)
        }
        Ok(value) if value == JOINT_REFINEMENT_SUMMARY_SIMULATION_BACKEND_PROCESS_WORKER => {
            Ok(JointRefinementSummarySimulationBackendPreference::ProcessWorker)
        }
        Ok(value) => Err(format!(
            "unsupported joint refinement summary simulation backend `{value}`"
        )),
        Err(env::VarError::NotPresent) => {
            Ok(JointRefinementSummarySimulationBackendPreference::Auto)
        }
        Err(env::VarError::NotUnicode(_)) => Err(
            "joint refinement summary simulation backend environment is not valid unicode"
                .to_string(),
        ),
    }
}

fn candidate_search_roots(current_executable: Option<&Path>) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(current_executable) = current_executable {
        if let Some(parent) = current_executable.parent() {
            push_candidate_search_root(&mut roots, parent.to_path_buf());

            if let Some(grandparent) = parent.parent() {
                push_candidate_search_root(&mut roots, grandparent.to_path_buf());
            }
        }
    }

    append_workspace_target_search_roots(&mut roots);

    roots
}

fn append_workspace_target_search_roots(roots: &mut Vec<PathBuf>) {
    if let Some(workspace_root) = workspace_root_from_manifest_directory() {
        let target_directory = workspace_root.join("target");

        for profile_directory_name in ["debug", "release"] {
            push_candidate_search_root(roots, target_directory.join(profile_directory_name));
        }
    }
}

#[cfg(test)]
fn candidate_search_roots_from_current_executable(current_executable: &Path) -> Vec<PathBuf> {
    candidate_search_roots(Some(current_executable))
}

fn push_candidate_search_root(roots: &mut Vec<PathBuf>, candidate_root: PathBuf) {
    if !candidate_root.exists() || fs::metadata(candidate_root.as_path()).is_err() {
        return;
    }

    if roots
        .iter()
        .any(|existing_root| existing_root == &candidate_root)
    {
        return;
    }

    roots.push(candidate_root);
}

fn workspace_root_from_manifest_directory() -> Option<PathBuf> {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_directory
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn worker_executable_file_name() -> &'static str {
    if cfg!(windows) {
        "janiform-joint-refinement-worker.exe"
    } else {
        "janiform-joint-refinement-worker"
    }
}

fn resolve_backend_from_preference(
    backend_preference: JointRefinementSummarySimulationBackendPreference,
    worker_executable: Option<PathBuf>,
) -> Result<JointRefinementSummarySimulationBackend, String> {
    match backend_preference {
        JointRefinementSummarySimulationBackendPreference::Auto => Ok(match worker_executable {
            Some(worker_executable) => {
                JointRefinementSummarySimulationBackend::ProcessWorker { worker_executable }
            }
            None => JointRefinementSummarySimulationBackend::InProcess,
        }),
        JointRefinementSummarySimulationBackendPreference::InProcess => {
            Ok(JointRefinementSummarySimulationBackend::InProcess)
        }
        JointRefinementSummarySimulationBackendPreference::ProcessWorker => worker_executable
            .map(
                |worker_executable| JointRefinementSummarySimulationBackend::ProcessWorker {
                    worker_executable,
                },
            )
            .ok_or_else(unresolved_worker_executable_error_message),
    }
}

fn current_available_memory_bytes() -> Result<u64, String> {
    let mut system = System::new();
    system.refresh_memory();
    Ok(system.available_memory())
}

pub fn current_process_resident_bytes() -> Result<u64, String> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);

    let process_id = Pid::from_u32(std::process::id());
    let process = system
        .process(process_id)
        .ok_or_else(|| "unable to resolve current process in sysinfo snapshot".to_string())?;

    Ok(process.memory())
}

#[cfg(test)]
mod tests {
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        JointRefinementSummarySimulationBackend, JointRefinementSummarySimulationBackendPreference,
        candidate_search_roots_from_current_executable, resolve_backend_from_preference,
        spawn_initialized_worker, workspace_root_from_manifest_directory,
    };
    use crate::stages::joint_refinement::summary_simulation::tests::joint_refinement_summary_session_context_fixture;

    fn temporary_worker_script_path(stem: &str) -> PathBuf {
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX epoch")
            .as_nanos();
        let extension = if cfg!(windows) { "cmd" } else { "sh" };

        std::env::temp_dir().join(format!(
            "janiform-joint-refinement-worker-test-{stem}-{unique_suffix}.{extension}"
        ))
    }

    fn write_worker_script(path: &PathBuf, script_body: &str) {
        fs::write(path, script_body).expect("test worker script should be written");

        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(path)
                .expect("test worker script metadata should resolve")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions)
                .expect("test worker script permissions should update");
        }
    }

    #[test]
    fn malformed_worker_initialize_session_output_is_reported() {
        let script_path = temporary_worker_script_path("malformed");
        let script_body = if cfg!(windows) {
            "@echo off\r\npowershell -Command \"$stdout = [Console]::OpenStandardOutput(); $bytes = [byte[]](123); $stdout.Write($bytes, 0, $bytes.Length)\"\r\n"
        } else {
            "#!/bin/sh\nprintf '{'\n"
        };
        write_worker_script(&script_path, script_body);

        let error = spawn_initialized_worker(
            script_path.as_path(),
            &joint_refinement_summary_session_context_fixture(),
        )
        .expect_err("malformed worker output should fail");
        assert!(
            error.contains("malformed initialize-session output")
                || error.contains("exited abnormally"),
            "{error}"
        );
    }

    #[test]
    fn worker_initialize_session_schema_mismatch_is_reported() {
        let script_path = temporary_worker_script_path("schema-mismatch");
        let script_body = if cfg!(windows) {
            "@echo off\r\npowershell -Command \"$stdout = [Console]::OpenStandardOutput(); $payload = [byte[]](8,0,0,0,0,0,0,0,1,0,0,0); $stdout.Write($payload, 0, $payload.Length)\"\r\n"
        } else {
            "#!/bin/sh\nprintf '\\010\\000\\000\\000\\000\\000\\000\\000\\001\\000\\000\\000'\n"
        };
        write_worker_script(&script_path, script_body);

        let error = spawn_initialized_worker(
            script_path.as_path(),
            &joint_refinement_summary_session_context_fixture(),
        )
        .expect_err("schema-mismatched worker output should fail");
        assert!(
            error.contains("malformed initialize-session output")
                || error.contains("exited abnormally"),
            "{error}"
        );
    }

    #[test]
    fn candidate_search_roots_include_workspace_target_profile_directory() {
        let simulated_python_executable = if cfg!(windows) {
            Path::new(r"C:\workspace\.venv\Scripts\python.exe")
        } else {
            Path::new("/workspace/.venv/bin/python")
        };

        let workspace_root = workspace_root_from_manifest_directory()
            .expect("workspace root should resolve from the crate manifest directory");
        let expected_profile_directory = workspace_root.join("target").join("debug");
        let search_roots =
            candidate_search_roots_from_current_executable(simulated_python_executable);

        assert!(
            search_roots.contains(&expected_profile_directory),
            "workspace target profile directory should be searched for the worker executable",
        );
    }

    #[test]
    fn automatic_backend_selection_falls_back_to_in_process_when_worker_is_unavailable() {
        let backend = resolve_backend_from_preference(
            JointRefinementSummarySimulationBackendPreference::Auto,
            None,
        )
        .expect("automatic backend selection should succeed");

        assert!(matches!(
            backend,
            JointRefinementSummarySimulationBackend::InProcess
        ));
    }

    #[test]
    fn explicit_process_worker_backend_requires_a_worker_executable() {
        let error = resolve_backend_from_preference(
            JointRefinementSummarySimulationBackendPreference::ProcessWorker,
            None,
        )
        .expect_err("explicit process-worker backend should require a worker executable");

        assert!(
            error.contains("summary simulation worker executable"),
            "{error}"
        );
    }
}
