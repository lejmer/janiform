use std::io::{self, BufReader, BufWriter};

use janiform_calibration_runtime::{
    CloseSessionResponse, InitializeSessionResponse,
    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION,
    JointRefinementSummarySimulationSessionState, JointRefinementSummarySimulationWorkerRequest,
    JointRefinementSummarySimulationWorkerResponse, current_process_resident_bytes,
    initialize_joint_refinement_summary_simulation_session, read_joint_refinement_worker_frame,
    run_joint_refinement_candidate_in_session, write_joint_refinement_worker_frame,
};

fn main() -> Result<(), String> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut writer = BufWriter::new(stdout.lock());
    let mut session_state: Option<JointRefinementSummarySimulationSessionState> = None;

    while let Some(worker_request) = read_joint_refinement_worker_frame::<
        JointRefinementSummarySimulationWorkerRequest,
    >(&mut reader)?
    {
        let worker_response = match worker_request {
            JointRefinementSummarySimulationWorkerRequest::InitializeSession(request) => {
                if request.schema_version
                    != JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                {
                    JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                        schema_version:
                            JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                                .to_string(),
                        response: Box::new(InitializeSessionResponse::InfrastructureFailure {
                            error_message: format!(
                                "unexpected worker request schema version: {}",
                                request.schema_version
                            ),
                        }),
                    }
                } else {
                    let worker_baseline_resident_bytes = current_process_resident_bytes()?;
                    match initialize_joint_refinement_summary_simulation_session(
                        &request.session_context,
                    ) {
                        Ok(initialized_session_state) => {
                            session_state = Some(initialized_session_state);
                            let worker_initialized_resident_bytes =
                                current_process_resident_bytes()?;
                            JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                                schema_version:
                                    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                                        .to_string(),
                                response: Box::new(InitializeSessionResponse::Success {
                                    worker_baseline_resident_bytes,
                                    worker_initialized_resident_bytes,
                                }),
                            }
                        }
                        Err(error_message) => {
                            JointRefinementSummarySimulationWorkerResponse::InitializeSession {
                                schema_version:
                                    JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                                        .to_string(),
                                response: Box::new(InitializeSessionResponse::InfrastructureFailure {
                                    error_message,
                                }),
                            }
                        }
                    }
                }
            }
            JointRefinementSummarySimulationWorkerRequest::RunCandidate(request) => {
                let response = match &session_state {
                    Some(initialized_session_state) => {
                        run_joint_refinement_candidate_in_session(initialized_session_state, &request)
                    }
                    None => janiform_calibration_runtime::RunCandidateResponse::InfrastructureFailure {
                        error_message:
                            "joint refinement worker received run-candidate before initialize-session"
                                .to_string(),
                    },
                };

                JointRefinementSummarySimulationWorkerResponse::RunCandidate {
                    schema_version:
                        JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                            .to_string(),
                    response: Box::new(response),
                }
            }
            JointRefinementSummarySimulationWorkerRequest::CloseSession(request) => {
                if request.schema_version
                    != JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                {
                    JointRefinementSummarySimulationWorkerResponse::CloseSession {
                        schema_version:
                            JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                                .to_string(),
                        response: Box::new(CloseSessionResponse { closed: false }),
                    }
                } else {
                    let close_response =
                        JointRefinementSummarySimulationWorkerResponse::CloseSession {
                            schema_version:
                                JOINT_REFINEMENT_SUMMARY_SIMULATION_WORKER_PROTOCOL_SCHEMA_VERSION
                                    .to_string(),
                            response: Box::new(CloseSessionResponse { closed: true }),
                        };
                    write_joint_refinement_worker_frame(&mut writer, &close_response)?;
                    break;
                }
            }
        };

        write_joint_refinement_worker_frame(&mut writer, &worker_response)?;
    }

    Ok(())
}
